use crate::{
    BackupImportMode, DIAGNOSTICS_REFRESH_INTERVAL, FastTube, ThemePalette, app_state::SettingsTab,
    buffer_profile_summary, copy_text_to_clipboard, diagnostics, downloads, followed_channels,
    format_diagnostics_display, history, i18n, open_external_url, settings, sponsorblock, updater,
};
use eframe::egui::{self, Color32, CornerRadius, CursorIcon, Pos2, RichText, Sense, Stroke, Vec2};
use settings::{BufferProfile, BufferSettings, HwdecMode, Theme};
use sponsorblock::{SponsorCategory, SponsorSkipMode};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BUFFER_PROFILE_HELP_SUMMARY: &str = "Escolha o perfil de cache do demuxer do libmpv. O perfil Padrão (64 MiB de avanço, 16 MiB de retorno e 30s de cache) equilibra consumo moderado de RAM com boa estabilidade.\n• Buffer menor reduz o uso de RAM, mas oferece menor folga contra oscilações de rede.\n• Buffer maior aumenta a tolerância a conexões instáveis e buscas rápidas, usando mais RAM.";

struct SettingsSearchEntry {
    tab: SettingsTab,
    section: &'static str,
    description: &'static str,
    keywords: &'static str,
}

const SETTINGS_SEARCH_INDEX: &[SettingsSearchEntry] = &[
    SettingsSearchEntry {
        tab: SettingsTab::General,
        section: "Tema e idioma da interface",
        description: "Alterne entre Escuro e Claro e escolha o idioma (Português / English).",
        keywords: "tema escuro claro aparencia dark light idioma language english portugues pt en",
    },
    SettingsSearchEntry {
        tab: SettingsTab::General,
        section: "Densidade compacta, Shorts e Mini player automático",
        description: "Ajuste colunas por linha da grade, oculte Shorts e ative o Mini player ao navegar.",
        keywords: "densidade compacta cartoes feed grade shorts ocultar mini player automatico navegacao",
    },
    SettingsSearchEntry {
        tab: SettingsTab::General,
        section: "Sistema, bandeja e inicialização com o Windows",
        description: "Configure tela ativa, minimizar ou fechar para a bandeja e iniciar junto com o Windows.",
        keywords: "energia tela ativa suspensao monitor iniciar windows autostart bandeja tray minimizar fechar hidden minimizado sistema",
    },
    SettingsSearchEntry {
        tab: SettingsTab::General,
        section: "Atualizador automático do FastTube (GitHub Releases)",
        description: "Verifique, baixe com verificação SHA-256 e instale a release mais recente do GitHub.",
        keywords: "atualizador automatico update github releases versao sha256 instalador portatil",
    },
    SettingsSearchEntry {
        tab: SettingsTab::General,
        section: "Atalhos de teclado personalizáveis",
        description: "Capture novas teclas de atalho para player e comandos ou restaure os padrões.",
        keywords: "atalhos teclado teclas keybindings personalizar capturar f1 espaco fullscreen teatro miniplayer",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Playback,
        section: "Velocidade, qualidade e passo",
        description: "Defina velocidade padrão, altura máxima, duração do salto e autoplay/loop.",
        keywords: "velocidade 1.0x 2.0x qualidade 1080p 4k 720p passo salto 5s 10s 15s 30s autoplay loop retomar",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Playback,
        section: "Áudio e legendas",
        description: "Lembrar volume, volume inicial, normalização (loudnorm) e legendas automáticas.",
        keywords: "audio volume lembrar padrao normalizacao loudnorm som legendas automaticas idioma pt en es",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Playback,
        section: "Decodificação por hardware (libmpv)",
        description: "Selecione GPU/hardware (auto-safe, d3d11va, dxva2) ou software para o player.",
        keywords: "hardware hwdec gpu placa video d3d11va dxva2 software libmpv decodificacao",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Playback,
        section: "Buffer de rede e demuxer",
        description: "Escolha Pequeno, Padrão, Grande ou configure limites personalizados em MiB e segundos.",
        keywords: "buffer ram demuxer cache pequeno padrao grande personalizado custom mib segundos",
    },
    SettingsSearchEntry {
        tab: SettingsTab::SponsorBlock,
        section: "Integração e modo de ação",
        description: "Ative o SponsorBlock, defina pular automático ou botão manual e duração mínima.",
        keywords: "sponsorblock patrocinio pular automatico botao manual marcadores toast duracao",
    },
    SettingsSearchEntry {
        tab: SettingsTab::SponsorBlock,
        section: "Categorias de segmentos",
        description: "Personalize patrocínio, autopromoção, intro, encerramento e intervalos não musicais.",
        keywords: "categorias sponsor selfpromo intro outro preview interaction filler music",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Privacy,
        section: "Conta Google e OAuth",
        description: "Conecte ou desconecte sua conta e importe o arquivo oauth-client.json.",
        keywords: "conta google login entrar sair oauth oauth-client.json credenciais json",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Privacy,
        section: "APIs comunitárias opcionais (opt-in)",
        description: "Return YouTube Dislike (contagem estimada) e DeArrow (títulos diretos), desativados por padrão.",
        keywords: "return youtube dislike ryd dislikes dearrow comunidade opt-in api privacidade",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Privacy,
        section: "Histórico, aprendizado e tópicos",
        description: "Pause o histórico ou aprendizado, silencie tópicos específicos e limpe dados locais.",
        keywords: "historico privacidade pausar recomendacoes topicos aprendidos silenciar limpar",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Storage,
        section: "Pasta de downloads e atualizador do yt-dlp",
        description: "Configure onde salvar downloads offline e verifique ou atualize o yt-dlp pelo GitHub.",
        keywords: "downloads pasta offline yt-dlp atualizar versao github bin",
    },
    SettingsSearchEntry {
        tab: SettingsTab::Storage,
        section: "Pasta e uso de cache",
        description: "Altere o diretório do yt-dlp, abra no Explorer, meça o espaço e libere caches.",
        keywords: "pasta cache diretorio explorer espaco disco yt-dlp limpar memoria visual",
    },
    SettingsSearchEntry {
        tab: SettingsTab::BackupDiagnostics,
        section: "Backup e importação",
        description: "Exporte ou analise arquivos .json com seletor nativo antes de mesclar ou substituir.",
        keywords: "backup exportar importar json restaurar mesclar substituir",
    },
    SettingsSearchEntry {
        tab: SettingsTab::BackupDiagnostics,
        section: "Diagnóstico de vídeo e persistência",
        description: "Veja codec, aceleração ativa, relatório redigido e abra arquivos de dados do app.",
        keywords: "diagnostico hwdec codec relatorio arquivos persistencia settings.json history.json followed-channels.json",
    },
];

fn divider_color(palette: ThemePalette) -> Color32 {
    palette.card_alt_stroke
}

fn card_surface_bg(palette: ThemePalette) -> Color32 {
    palette.info_box_bg
}

fn card_surface_stroke(palette: ThemePalette) -> Color32 {
    palette.card_alt_stroke
}

fn rail_surface_bg(theme: Theme) -> Color32 {
    match theme {
        Theme::Dark => Color32::from_rgb(20, 20, 23),
        Theme::Light => Color32::from_rgb(245, 246, 249),
    }
}

fn switch_active_color(palette: ThemePalette) -> Color32 {
    palette.info_accent_fg
}

fn switch_inactive_color(theme: Theme) -> Color32 {
    match theme {
        Theme::Dark => Color32::from_rgb(72, 72, 78),
        Theme::Light => Color32::from_rgb(194, 198, 206),
    }
}

fn nav_active_bg(theme: Theme) -> Color32 {
    match theme {
        Theme::Dark => Color32::from_rgb(40, 40, 44),
        Theme::Light => Color32::from_rgb(232, 235, 240),
    }
}

fn format_bytes_pt(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn paint_settings_tab_icon(
    painter: &egui::Painter,
    center: Pos2,
    tab: SettingsTab,
    color: Color32,
) {
    let stroke = Stroke::new(1.5, color);
    match tab {
        SettingsTab::General => {
            // Two horizontal slider lines with knobs
            let y1 = center.y - 3.2;
            let y2 = center.y + 3.2;
            painter.line_segment(
                [Pos2::new(center.x - 6.5, y1), Pos2::new(center.x + 6.5, y1)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(center.x - 6.5, y2), Pos2::new(center.x + 6.5, y2)],
                stroke,
            );
            painter.circle_filled(Pos2::new(center.x + 2.5, y1), 2.2, color);
            painter.circle_filled(Pos2::new(center.x - 2.5, y2), 2.2, color);
        }
        SettingsTab::Playback => {
            // Rounded rectangle frame with play triangle
            let frame = egui::Rect::from_center_size(center, Vec2::new(13.5, 10.5));
            painter.rect_stroke(
                frame,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            let tri = vec![
                Pos2::new(center.x - 2.0, center.y - 2.6),
                Pos2::new(center.x + 3.2, center.y),
                Pos2::new(center.x - 2.0, center.y + 2.6),
            ];
            painter.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
        }
        SettingsTab::SponsorBlock => {
            // Shield outline with skip check
            let pts = vec![
                Pos2::new(center.x, center.y - 6.5),
                Pos2::new(center.x + 5.5, center.y - 4.0),
                Pos2::new(center.x + 4.8, center.y + 2.2),
                Pos2::new(center.x, center.y + 6.5),
                Pos2::new(center.x - 4.8, center.y + 2.2),
                Pos2::new(center.x - 5.5, center.y - 4.0),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.2, center.y),
                    Pos2::new(center.x + 2.4, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 0.4, center.y - 2.0),
                    Pos2::new(center.x + 2.6, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 0.4, center.y + 2.0),
                    Pos2::new(center.x + 2.6, center.y),
                ],
                stroke,
            );
        }
        SettingsTab::Privacy => {
            // User head + shoulders silhouette
            painter.circle_stroke(Pos2::new(center.x, center.y - 2.6), 2.7, stroke);
            let arc = vec![
                Pos2::new(center.x - 5.2, center.y + 5.6),
                Pos2::new(center.x - 3.6, center.y + 2.4),
                Pos2::new(center.x + 3.6, center.y + 2.4),
                Pos2::new(center.x + 5.2, center.y + 5.6),
            ];
            painter.add(egui::Shape::line(arc, stroke));
        }
        SettingsTab::Storage => {
            // Drive box with indicator dot
            let outer = egui::Rect::from_center_size(center, Vec2::new(13.0, 11.0));
            painter.rect_stroke(
                outer,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y + 0.5),
                    Pos2::new(center.x + 6.0, center.y + 0.5),
                ],
                stroke,
            );
            painter.circle_filled(Pos2::new(center.x + 3.2, center.y + 3.0), 1.3, color);
        }
        SettingsTab::BackupDiagnostics => {
            // Waveform / pulse inside frame
            let box_rect = egui::Rect::from_center_size(center, Vec2::new(13.0, 12.0));
            painter.rect_stroke(
                box_rect,
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Inside,
            );
            let pulse = vec![
                Pos2::new(center.x - 4.5, center.y + 0.5),
                Pos2::new(center.x - 2.0, center.y + 0.5),
                Pos2::new(center.x - 0.5, center.y - 3.2),
                Pos2::new(center.x + 1.5, center.y + 3.2),
                Pos2::new(center.x + 3.0, center.y + 0.5),
                Pos2::new(center.x + 4.5, center.y + 0.5),
            ];
            painter.add(egui::Shape::line(pulse, stroke));
        }
    }
}

fn paint_radio_indicator(
    painter: &egui::Painter,
    center: Pos2,
    selected: bool,
    palette: ThemePalette,
) {
    let ring_color = if selected {
        palette.info_accent_fg
    } else {
        palette.snippet_fg.gamma_multiply(0.7)
    };
    painter.circle_stroke(center, 7.5, Stroke::new(1.5, ring_color));
    if selected {
        painter.circle_filled(center, 4.0, palette.info_accent_fg);
    }
}

fn settings_pill_badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(fg));
        });
}

fn settings_toggle_switch(
    ui: &mut egui::Ui,
    checked: &mut bool,
    palette: ThemePalette,
    theme: Theme,
    hover_text: Option<&str>,
) -> bool {
    let desired = Vec2::new(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if let Some(tip) = hover_text {
        resp = resp.on_hover_text(tip);
    }
    let changed = resp.clicked();
    if changed {
        *checked = !*checked;
    }
    if ui.is_rect_visible(rect) {
        let on = *checked;
        let track_bg = if on {
            switch_active_color(palette)
        } else {
            switch_inactive_color(theme)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(11), track_bg);
        if resp.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(11),
                Stroke::new(1.0, palette.channel_fg.gamma_multiply(0.35)),
                egui::StrokeKind::Outside,
            );
        }
        let thumb_r = 8.0_f32;
        let thumb_x = if on {
            rect.right() - 3.0 - thumb_r
        } else {
            rect.left() + 3.0 + thumb_r
        };
        painter.circle_filled(Pos2::new(thumb_x, rect.center().y), thumb_r, Color32::WHITE);
    }
    changed
}

fn paint_row_divider(ui: &mut egui::Ui, palette: ThemePalette) {
    let available = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(available, 1.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 0.0, divider_color(palette));
    }
}

fn settings_card<F>(
    ui: &mut egui::Ui,
    palette: ThemePalette,
    title: &str,
    badge: Option<(&str, Color32, Color32)>,
    header_action: Option<&str>,
    add_rows: F,
) -> bool
where
    F: FnOnce(&mut egui::Ui),
{
    let mut action_clicked = false;
    egui::Frame::new()
        .fill(card_surface_bg(palette))
        .stroke(Stroke::new(1.0, card_surface_stroke(palette)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(0, 0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(14.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if let Some((text, fg, bg)) = badge {
                            ui.add_space(6.0);
                            settings_pill_badge(ui, text, fg, bg);
                        }
                        if let Some(action_label) = header_action {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if crate::styled_compact_button(
                                        ui,
                                        action_label,
                                        false,
                                        &palette,
                                    )
                                    .clicked()
                                    {
                                        action_clicked = true;
                                    }
                                },
                            );
                        }
                    });
                });
            paint_row_divider(ui, palette);
            add_rows(ui);
        });
    action_clicked
}

fn settings_row_frame<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn settings_toggle_row(
    ui: &mut egui::Ui,
    palette: ThemePalette,
    theme: Theme,
    title: &str,
    description: &str,
    checked: &mut bool,
    dot_color: Option<Color32>,
    badge: Option<(&str, Color32, Color32)>,
) -> bool {
    settings_row_frame(ui, |ui| {
        let mut changed = false;
        ui.horizontal(|ui| {
            let text_w = (ui.available_width() - 60.0).max(140.0);
            ui.allocate_ui_with_layout(
                Vec2::new(text_w, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if let Some(color) = dot_color {
                            let (dot_rect, _) =
                                ui.allocate_exact_size(Vec2::new(10.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(dot_rect.center(), 4.5, color);
                            ui.add_space(4.0);
                        }
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if let Some((b_text, b_fg, b_bg)) = badge {
                            ui.add_space(6.0);
                            settings_pill_badge(ui, b_text, b_fg, b_bg);
                        }
                    });
                    if !description.is_empty() {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(description)
                                .size(12.0)
                                .color(palette.snippet_fg),
                        );
                    }
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                changed = settings_toggle_switch(ui, checked, palette, theme, None);
            });
        });
        changed
    })
}

fn settings_control_row(
    ui: &mut egui::Ui,
    palette: ThemePalette,
    title: &str,
    description: &str,
    right_min_width: f32,
    add_right: impl FnOnce(&mut egui::Ui),
) {
    settings_row_frame(ui, |ui| {
        let total_w = ui.available_width();
        if total_w >= 460.0 {
            ui.horizontal(|ui| {
                let text_w = (total_w - right_min_width - 16.0).max(150.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(text_w, 0.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if !description.is_empty() {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(description)
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                            );
                        }
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add_right);
            });
        } else {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(13.5)
                        .strong()
                        .color(palette.channel_fg),
                );
                if !description.is_empty() {
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(description)
                            .size(12.0)
                            .color(palette.snippet_fg),
                    );
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(add_right);
            });
        }
    });
}

fn pill_choice_button(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    palette: ThemePalette,
) -> egui::Response {
    crate::styled_pill_button(ui, label, selected, &palette)
}

fn danger_outline_button(ui: &mut egui::Ui, label: &str, palette: ThemePalette) -> egui::Response {
    crate::styled_danger_button(ui, label, &palette)
}

fn status_banner(ui: &mut egui::Ui, palette: ThemePalette, text: &str, is_error: bool) {
    let (bg, stroke, fg) = if is_error {
        (
            palette.error_bg,
            palette.error_stroke,
            palette.error_banner_fg,
        )
    } else {
        (palette.notice_bg, palette.notice_stroke, palette.notice_fg)
    };
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).size(12.5).color(fg));
        });
}

fn clickable_radio_card(
    ui: &mut egui::Ui,
    palette: ThemePalette,
    selected: bool,
    title: &str,
    description: &str,
    badge: Option<(&str, Color32, Color32)>,
) -> bool {
    let (fill, stroke) = if selected {
        (palette.profile_selected_bg, palette.profile_selected_stroke)
    } else {
        (
            palette.profile_unselected_bg,
            palette.profile_unselected_stroke,
        )
    };
    let frame_resp = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(if selected { 1.4 } else { 1.0 }, stroke))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (radio_rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                paint_radio_indicator(ui.painter(), radio_rect.center(), selected, palette);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(13.0)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        if let Some((b_text, b_fg, b_bg)) = badge {
                            ui.add_space(6.0);
                            settings_pill_badge(ui, b_text, b_fg, b_bg);
                        }
                    });
                    if !description.is_empty() {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(description)
                                .size(12.0)
                                .color(palette.snippet_fg),
                        );
                    }
                });
            });
        })
        .response;
    ui.interact(frame_resp.rect, frame_resp.id.with("click"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
}

fn theme_swatch_card(
    ui: &mut egui::Ui,
    palette: ThemePalette,
    target_theme: Theme,
    label: &str,
    selected: bool,
) -> bool {
    let (canvas_bg, card_bg, line_col, accent_col) = match target_theme {
        Theme::Dark => (
            Color32::from_rgb(15, 15, 17),
            Color32::from_rgb(30, 30, 34),
            Color32::from_rgb(86, 88, 96),
            Color32::from_rgb(62, 166, 255),
        ),
        Theme::Light => (
            Color32::from_rgb(246, 247, 250),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(185, 190, 200),
            Color32::from_rgb(6, 95, 212),
        ),
    };
    let (fill, stroke) = if selected {
        (palette.profile_selected_bg, palette.profile_selected_stroke)
    } else {
        (
            palette.profile_unselected_bg,
            palette.profile_unselected_stroke,
        )
    };
    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(if selected { 1.5 } else { 1.0 }, stroke))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(126.0);
            let (preview_rect, _) = ui.allocate_exact_size(Vec2::new(106.0, 44.0), Sense::hover());
            if ui.is_rect_visible(preview_rect) {
                let p = ui.painter();
                p.rect_filled(preview_rect, CornerRadius::same(6), canvas_bg);
                p.rect_stroke(
                    preview_rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0, palette.card_alt_stroke),
                    egui::StrokeKind::Inside,
                );
                let mini_card = egui::Rect::from_min_size(
                    Pos2::new(preview_rect.left() + 8.0, preview_rect.top() + 8.0),
                    Vec2::new(56.0, 28.0),
                );
                p.rect_filled(mini_card, CornerRadius::same(4), card_bg);
                p.circle_filled(
                    Pos2::new(preview_rect.right() - 16.0, preview_rect.top() + 14.0),
                    4.0,
                    accent_col,
                );
                p.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(mini_card.left() + 6.0, mini_card.top() + 7.0),
                        Vec2::new(34.0, 4.0),
                    ),
                    CornerRadius::same(2),
                    line_col,
                );
                p.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(mini_card.left() + 6.0, mini_card.top() + 15.0),
                        Vec2::new(22.0, 4.0),
                    ),
                    CornerRadius::same(2),
                    line_col.gamma_multiply(0.6),
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (radio_rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                paint_radio_indicator(ui.painter(), radio_rect.center(), selected, palette);
                ui.label(
                    RichText::new(label)
                        .size(12.5)
                        .strong()
                        .color(palette.channel_fg),
                );
            });
        })
        .response;
    ui.interact(resp.rect, resp.id.with("click"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
}

fn settings_search_bar(ui: &mut egui::Ui, query: &mut String, palette: ThemePalette, theme: Theme) {
    let bg = match theme {
        Theme::Dark => Color32::from_rgb(15, 15, 17),
        Theme::Light => Color32::WHITE,
    };
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, palette.card_alt_stroke))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 26.0), Sense::hover());
                if ui.is_rect_visible(icon_rect) {
                    let c = icon_rect.center();
                    let stroke = Stroke::new(1.4, palette.snippet_fg);
                    ui.painter()
                        .circle_stroke(Pos2::new(c.x - 1.5, c.y - 1.5), 4.0, stroke);
                    ui.painter().line_segment(
                        [
                            Pos2::new(c.x + 1.5, c.y + 1.5),
                            Pos2::new(c.x + 4.5, c.y + 4.5),
                        ],
                        stroke,
                    );
                }
                let clear_w = if query.is_empty() { 0.0 } else { 22.0 };
                let edit_w = (ui.available_width() - clear_w - 6.0).max(70.0);
                ui.add_sized(
                    [edit_w, 26.0],
                    egui::TextEdit::singleline(query)
                        .frame(egui::Frame::NONE)
                        .hint_text("Buscar configurações..."),
                );
                if !query.is_empty()
                    && ui
                        .add_sized(
                            [20.0, 26.0],
                            egui::Button::new(
                                RichText::new("×").size(14.0).color(palette.snippet_fg),
                            )
                            .fill(Color32::TRANSPARENT)
                            .frame(false),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Limpar busca")
                        .clicked()
                {
                    query.clear();
                }
            });
        });
}

fn stepper_u32_controls(
    ui: &mut egui::Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    step: u32,
    suffix: &str,
    palette: ThemePalette,
) -> bool {
    let mut changed = false;
    let min_val = *range.start();
    let max_val = *range.end();
    ui.horizontal(|ui| {
        if pill_choice_button(ui, &format!("-{step}"), false, palette).clicked() {
            *value = value.saturating_sub(step).max(min_val);
            changed = true;
        }
        if ui
            .add(
                egui::Slider::new(value, range)
                    .clamping(egui::SliderClamping::Always)
                    .suffix(suffix),
            )
            .changed()
        {
            changed = true;
        }
        if pill_choice_button(ui, &format!("+{step}"), false, palette).clicked() {
            *value = value.saturating_add(step).min(max_val);
            changed = true;
        }
    });
    changed
}

impl FastTube {
    pub(crate) fn poll_cache_picker(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.settings.cache_picker_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(Some(path))) => {
                self.settings.cache_picker_rx = None;
                let path_str = path.to_string_lossy().into_owned();
                self.settings.cache_dir_input = path_str.clone();
                self.settings.cache_dir_input_synced = true;
                let mut prefs = self.settings.prefs.value.clone();
                prefs.cache_dir_override = Some(path_str);
                self.apply_app_preferences(prefs);
                if self.settings.prefs.error.is_none() {
                    self.settings.cache_status = Some(format!(
                        "Pasta de cache atualizada para '{}'.",
                        path.display()
                    ));
                    self.settings.cache_disk_usage = None;
                }
                ctx.request_repaint();
            }
            Ok(Ok(None)) => {
                self.settings.cache_picker_rx = None;
            }
            Ok(Err(error)) => {
                self.settings.cache_picker_rx = None;
                self.settings.cache_status = Some(error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.settings.cache_picker_rx = None;
            }
        }
    }

    pub(crate) fn poll_download_dir_picker(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.settings.download_picker_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(Some(path))) => {
                self.settings.download_picker_rx = None;
                let path_str = path.to_string_lossy().into_owned();
                self.settings.download_dir_input = path_str.clone();
                self.settings.download_dir_input_synced = true;
                let mut prefs = self.settings.prefs.value.clone();
                prefs.download_dir_override = Some(path_str);
                self.apply_app_preferences(prefs);
                if self.settings.prefs.error.is_none() {
                    self.downloads.notice = Some(format!(
                        "{}: {}",
                        self.pick("Pasta de downloads atualizada", "Downloads folder updated"),
                        path.display()
                    ));
                }
                ctx.request_repaint();
            }
            Ok(Ok(None)) => {
                self.settings.download_picker_rx = None;
            }
            Ok(Err(error)) => {
                self.settings.download_picker_rx = None;
                self.downloads.notice = Some(error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.settings.download_picker_rx = None;
            }
        }
    }

    pub(crate) fn poll_backup_picker(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.settings.backup_picker_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok((is_save, Ok(Some(path)))) => {
                self.settings.backup_picker_rx = None;
                self.backup.path_text = path.to_string_lossy().into_owned();
                self.backup.status = Some(if is_save {
                    format!("Destino selecionado: {}", path.display())
                } else {
                    format!("Arquivo selecionado: {}", path.display())
                });
                ctx.request_repaint();
            }
            Ok((_, Ok(None))) => {
                self.settings.backup_picker_rx = None;
            }
            Ok((_, Err(error))) => {
                self.settings.backup_picker_rx = None;
                self.backup.status = Some(error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.settings.backup_picker_rx = None;
            }
        }
    }

    pub(crate) fn poll_oauth_picker(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.settings.oauth_picker_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(Some(source))) => {
                self.settings.oauth_picker_rx = None;
                match settings::oauth_client_path() {
                    Ok(dest) => match settings::install_oauth_client_file(&source, &dest) {
                        Ok(()) => {
                            self.settings.oauth_import_status = Some((
                                true,
                                format!("Credenciais OAuth salvas em '{}'.", dest.display()),
                            ));
                        }
                        Err(error) => {
                            self.settings.oauth_import_status = Some((false, error));
                        }
                    },
                    Err(error) => {
                        self.settings.oauth_import_status = Some((
                            false,
                            format!("Não foi possível localizar a pasta de configurações: {error}"),
                        ));
                    }
                }
                ctx.request_repaint();
            }
            Ok(Ok(None)) => {
                self.settings.oauth_picker_rx = None;
            }
            Ok(Err(error)) => {
                self.settings.oauth_picker_rx = None;
                self.settings.oauth_import_status = Some((false, error));
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.settings.oauth_picker_rx = None;
            }
        }
    }

    pub(crate) fn reset_settings_tab_defaults(&mut self, tab: SettingsTab, ctx: &egui::Context) {
        let defaults = settings::AppPreferences::default();
        match tab {
            SettingsTab::General => {
                self.apply_theme(Theme::default());
                self.sync_theme_visuals(ctx);
                let mut prefs = self.settings.prefs.value.clone();
                let changed_feed = prefs.hide_shorts_in_home != defaults.hide_shorts_in_home;
                prefs.language = defaults.language;
                prefs.compact_card_density = defaults.compact_card_density;
                prefs.hide_shorts_in_home = defaults.hide_shorts_in_home;
                prefs.auto_mini_player = defaults.auto_mini_player;
                prefs.keep_awake_during_playback = defaults.keep_awake_during_playback;
                prefs.minimize_to_tray = defaults.minimize_to_tray;
                prefs.close_to_tray = defaults.close_to_tray;
                prefs.start_minimized_to_tray = defaults.start_minimized_to_tray;
                prefs.auto_check_updates = defaults.auto_check_updates;
                prefs.skipped_app_update_version = defaults.skipped_app_update_version;
                prefs.keybindings = defaults.keybindings;
                self.settings.capturing_keybinding = None;
                self.apply_app_preferences(prefs);
                if changed_feed {
                    self.invalidate_feed_cache();
                }
            }
            SettingsTab::Playback => {
                self.apply_playback_preferences(settings::PlaybackPreferences::default());
                self.apply_buffer_profile(BufferProfile::Default);
                let def_buf = BufferProfile::Default.values();
                self.settings.custom_buffer_max_mib = def_buf.max_mib;
                self.settings.custom_buffer_back_mib = def_buf.back_mib;
                self.settings.custom_buffer_cache_secs = def_buf.cache_seconds;
                self.settings.custom_buffer_synced = true;

                let mut prefs = self.settings.prefs.value.clone();
                prefs.seek_step_secs = defaults.seek_step_secs;
                prefs.default_autoplay = defaults.default_autoplay;
                prefs.default_loop = defaults.default_loop;
                prefs.resume_enabled = defaults.resume_enabled;
                prefs.remember_volume = defaults.remember_volume;
                prefs.default_volume_percent = defaults.default_volume_percent;
                prefs.audio_normalization = defaults.audio_normalization;
                prefs.auto_subtitles = defaults.auto_subtitles;
                prefs.preferred_subtitle_lang = defaults.preferred_subtitle_lang;
                prefs.hwdec_mode = defaults.hwdec_mode;
                self.apply_app_preferences(prefs);
                if self.selected.is_none() {
                    self.playback.autoplay = defaults.default_autoplay;
                    self.playback.loop_video = defaults.default_loop;
                }
            }
            SettingsTab::SponsorBlock => {
                let mut prefs = self.settings.prefs.value.clone();
                prefs.sponsorblock = sponsorblock::SponsorBlockSettings::default();
                self.apply_app_preferences(prefs);
            }
            SettingsTab::Privacy => {
                let mut prefs = self.settings.prefs.value.clone();
                prefs.ryd_enabled = defaults.ryd_enabled;
                prefs.dearrow_enabled = defaults.dearrow_enabled;
                self.apply_app_preferences(prefs);
            }
            SettingsTab::Storage => {
                let mut prefs = self.settings.prefs.value.clone();
                prefs.cache_dir_override = defaults.cache_dir_override;
                prefs.download_dir_override = defaults.download_dir_override;
                self.apply_app_preferences(prefs);
                self.settings.cache_dir_input_synced = false;
                self.settings.download_dir_input_synced = false;
                self.settings.cache_disk_usage = None;
            }
            SettingsTab::BackupDiagnostics => {}
        }
    }

    pub(crate) fn settings_page_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        self.ensure_playback_settings_synchronized();
        self.settings.ensure_prefs_synchronized();
        let now = Instant::now();
        self.refresh_player_diagnostics(now);
        if self.selected.is_some() {
            ctx.request_repaint_after(DIAGNOSTICS_REFRESH_INTERVAL);
        }

        let palette = self.palette();
        let theme = self.settings.theme.value;
        let avail_w = ui.available_width();
        let wide_layout = avail_w >= 760.0;

        ui.add_space(6.0);
        if wide_layout {
            let rail_w = 246.0;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(rail_w, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.settings_left_rail_ui(ui, palette, theme);
                    },
                );
                ui.add_space(14.0);
                let content_w = ui.available_width().max(260.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(content_w, ui.available_height()),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.settings_content_area_ui(ui, ctx, palette, theme);
                    },
                );
            });
        } else {
            self.settings_top_tabs_ui(ui, palette, theme);
            ui.add_space(8.0);
            paint_row_divider(ui, palette);
            ui.add_space(8.0);
            self.settings_content_area_ui(ui, ctx, palette, theme);
        }

        self.backup_import_modal_ui(ctx);
    }

    fn settings_left_rail_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette, theme: Theme) {
        egui::Frame::new()
            .fill(rail_surface_bg(theme))
            .stroke(Stroke::new(1.0, palette.card_alt_stroke))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(10, 12))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("settings_rail_scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if crate::styled_pill_button(ui, "← Voltar", false, &palette).clicked() {
                            self.ui.settings_open = false;
                            self.sync_player_window_visibility();
                        }

                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Configurações")
                                .size(19.0)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.add_space(10.0);
                        settings_search_bar(ui, &mut self.settings.search_query, palette, theme);
                        ui.add_space(10.0);
                        paint_row_divider(ui, palette);
                        ui.add_space(8.0);

                        for tab in SettingsTab::ALL {
                            let active = self.settings.active_tab == tab
                                && self.settings.search_query.trim().is_empty();
                            let has_alert = match tab {
                                SettingsTab::Playback => {
                                    self.has_pending_buffer_restart()
                                        || self.has_pending_hwdec_restart()
                                }
                                SettingsTab::BackupDiagnostics => !self.message.is_empty(),
                                _ => false,
                            };
                            let text_color = if active {
                                palette.channel_fg
                            } else {
                                palette.snippet_fg
                            };
                            let icon_color = if active {
                                palette.info_accent_fg
                            } else {
                                palette.snippet_fg
                            };
                            let row_w = ui.available_width();
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::new(row_w, 38.0), Sense::click());
                            let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                            if ui.is_rect_visible(rect) {
                                let bg = if active {
                                    nav_active_bg(theme)
                                } else if resp.hovered() {
                                    nav_active_bg(theme).gamma_multiply(0.6)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                if bg != Color32::TRANSPARENT {
                                    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
                                }
                                if active {
                                    let indicator = egui::Rect::from_min_size(
                                        Pos2::new(rect.left() + 2.0, rect.top() + 8.0),
                                        Vec2::new(3.0, rect.height() - 16.0),
                                    );
                                    ui.painter().rect_filled(
                                        indicator,
                                        CornerRadius::same(2),
                                        palette.info_accent_fg,
                                    );
                                }
                                paint_settings_tab_icon(
                                    ui.painter(),
                                    Pos2::new(rect.left() + 18.0, rect.center().y),
                                    tab,
                                    icon_color,
                                );
                                let mut label_text = tab.label_pt().to_owned();
                                if tab == SettingsTab::SponsorBlock
                                    && self.settings.prefs.value.sponsorblock.enabled
                                {
                                    let count = self
                                        .settings
                                        .prefs
                                        .value
                                        .sponsorblock
                                        .categories
                                        .active_count();
                                    label_text = format!("SponsorBlock ({count})");
                                }
                                ui.painter().text(
                                    Pos2::new(rect.left() + 34.0, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    label_text,
                                    egui::FontId::proportional(13.0),
                                    text_color,
                                );
                                if has_alert {
                                    ui.painter().circle_filled(
                                        Pos2::new(rect.right() - 10.0, rect.center().y),
                                        3.5,
                                        palette.warning_fg,
                                    );
                                }
                            }
                            if resp.clicked() {
                                self.settings.active_tab = tab;
                                self.settings.search_query.clear();
                                self.settings.confirm_reset_tab = None;
                            }
                            ui.add_space(2.0);
                        }

                        if self.selected.is_some() {
                            ui.add_space(10.0);
                            paint_row_divider(ui, palette);
                            ui.add_space(10.0);
                            egui::Frame::new()
                                .fill(palette.info_box_bg)
                                .stroke(Stroke::new(1.0, palette.info_box_stroke))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        RichText::new("Vídeo ativo em 2º plano")
                                            .size(11.5)
                                            .strong()
                                            .color(palette.info_accent_fg),
                                    );
                                    ui.add_space(4.0);
                                    if pill_choice_button(ui, "Voltar ao vídeo", true, palette)
                                        .clicked()
                                    {
                                        self.ui.settings_open = false;
                                        self.sync_player_window_visibility();
                                    }
                                });
                        }
                    });
            });
    }

    fn settings_top_tabs_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette, theme: Theme) {
        ui.horizontal_wrapped(|ui| {
            if crate::styled_pill_button(ui, "← Voltar", false, &palette).clicked() {
                self.ui.settings_open = false;
                self.sync_player_window_visibility();
            }
            ui.add_space(6.0);
            for tab in SettingsTab::ALL {
                let selected =
                    self.settings.active_tab == tab && self.settings.search_query.trim().is_empty();
                if pill_choice_button(ui, tab.label_pt(), selected, palette).clicked() {
                    self.settings.active_tab = tab;
                    self.settings.search_query.clear();
                    self.settings.confirm_reset_tab = None;
                }
            }
        });
        ui.add_space(8.0);
        settings_search_bar(ui, &mut self.settings.search_query, palette, theme);
    }

    fn settings_content_area_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        theme: Theme,
    ) {
        egui::ScrollArea::vertical()
            .id_salt("settings_content_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let max_w = ui.available_width().min(800.0);
                ui.set_max_width(max_w);

                if self.selected.is_some() {
                    egui::Frame::new()
                        .fill(palette.info_box_bg)
                        .stroke(Stroke::new(1.0, palette.info_box_stroke))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(
                                        "O vídeo continua em reprodução em segundo plano enquanto esta página está aberta.",
                                    )
                                    .size(12.5)
                                    .color(palette.info_accent_fg),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if pill_choice_button(
                                            ui,
                                            "Voltar ao vídeo",
                                            true,
                                            palette,
                                        )
                                        .clicked()
                                        {
                                            self.ui.settings_open = false;
                                            self.sync_player_window_visibility();
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(12.0);
                }

                let trimmed_query = self.settings.search_query.trim().to_owned();
                if !trimmed_query.is_empty() {
                    self.settings_search_results_ui(ui, &trimmed_query, palette);
                    ui.add_space(24.0);
                    return;
                }

                let tab = self.settings.active_tab;
                ui.horizontal(|ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                    paint_settings_tab_icon(
                        ui.painter(),
                        icon_rect.center(),
                        tab,
                        palette.info_accent_fg,
                    );
                    ui.label(
                        RichText::new(tab.label_pt())
                            .size(12.5)
                            .strong()
                            .color(palette.info_accent_fg),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(tab.label_pt())
                                .size(21.0)
                                .strong()
                                .color(palette.channel_fg),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(tab.subtitle_pt())
                                .size(13.0)
                                .color(palette.snippet_fg),
                        );
                    });
                    if matches!(
                        tab,
                        SettingsTab::General
                            | SettingsTab::Playback
                            | SettingsTab::SponsorBlock
                            | SettingsTab::Privacy
                            | SettingsTab::Storage
                    ) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.settings.confirm_reset_tab == Some(tab) {
                                if danger_outline_button(ui, "Confirmar", palette).clicked() {
                                    self.reset_settings_tab_defaults(tab, ctx);
                                    self.settings.confirm_reset_tab = None;
                                }
                                if pill_choice_button(ui, "Cancelar", false, palette).clicked() {
                                    self.settings.confirm_reset_tab = None;
                                }
                            } else if pill_choice_button(ui, "Restaurar padrões", false, palette)
                                .clicked()
                            {
                                self.settings.confirm_reset_tab = Some(tab);
                            }
                        });
                    }
                });
                ui.add_space(12.0);
                paint_row_divider(ui, palette);
                ui.add_space(16.0);

                if let Some(error) = self.settings.prefs.error.clone() {
                    status_banner(ui, palette, &error, true);
                    ui.add_space(12.0);
                }

                match tab {
                    SettingsTab::General => {
                        self.settings_tab_general_ui(ui, ctx, palette, theme);
                    }
                    SettingsTab::Playback => {
                        self.settings_tab_playback_ui(ui, palette, theme);
                    }
                    SettingsTab::SponsorBlock => {
                        self.settings_tab_sponsorblock_ui(ui, ctx, palette, theme);
                    }
                    SettingsTab::Privacy => {
                        self.settings_tab_privacy_ui(ui, ctx, palette, theme);
                    }
                    SettingsTab::Storage => {
                        self.settings_tab_storage_ui(ui, ctx, palette);
                    }
                    SettingsTab::BackupDiagnostics => {
                        self.settings_tab_backup_diagnostics_ui(ui, ctx, palette);
                    }
                }
                ui.add_space(24.0);
            });
    }

    fn settings_search_results_ui(
        &mut self,
        ui: &mut egui::Ui,
        query: &str,
        palette: ThemePalette,
    ) {
        let lower_query = query.to_ascii_lowercase();
        let terms: Vec<&str> = lower_query.split_whitespace().collect();
        let matches: Vec<&SettingsSearchEntry> = SETTINGS_SEARCH_INDEX
            .iter()
            .filter(|entry| {
                let hay = format!(
                    "{} {} {} {}",
                    entry.tab.label_pt(),
                    entry.section,
                    entry.description,
                    entry.keywords
                )
                .to_ascii_lowercase();
                terms.iter().all(|term| hay.contains(term))
            })
            .collect();

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Resultados para \"{query}\""))
                    .size(20.0)
                    .strong()
                    .color(palette.channel_fg),
            );
            ui.add_space(6.0);
            settings_pill_badge(
                ui,
                &format!("{} encontrado(s)", matches.len()),
                palette.info_accent_fg,
                palette.queue_next_bg,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill_choice_button(ui, "Limpar busca", false, palette).clicked() {
                    self.settings.search_query.clear();
                }
            });
        });
        ui.add_space(12.0);
        paint_row_divider(ui, palette);
        ui.add_space(14.0);

        if matches.is_empty() {
            settings_card(
                ui,
                palette,
                "Nenhuma configuração encontrada",
                None,
                None,
                |ui| {
                    settings_row_frame(ui, |ui| {
                        ui.label(
                        RichText::new(
                            "Tente termos como \"idioma\", \"volume\", \"legendas\", \"buffer\", \"oauth\", \"downloads\", \"yt-dlp\" ou \"tema\".",
                        )
                        .size(12.5)
                        .color(palette.snippet_fg),
                    );
                        ui.add_space(8.0);
                        if pill_choice_button(ui, "Voltar às abas", false, palette).clicked() {
                            self.settings.search_query.clear();
                        }
                    });
                },
            );
            return;
        }

        for entry in matches {
            egui::Frame::new()
                .fill(card_surface_bg(palette))
                .stroke(Stroke::new(1.0, card_surface_stroke(palette)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let text_w = (ui.available_width() - 130.0).max(160.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(text_w, 0.0),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new(entry.section)
                                            .size(14.0)
                                            .strong()
                                            .color(palette.channel_fg),
                                    );
                                    ui.add_space(6.0);
                                    settings_pill_badge(
                                        ui,
                                        entry.tab.label_pt(),
                                        palette.info_accent_fg,
                                        palette.queue_next_bg,
                                    );
                                });
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(entry.description)
                                        .size(12.0)
                                        .color(palette.snippet_fg),
                                );
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if pill_choice_button(ui, "Abrir aba →", true, palette).clicked() {
                                self.settings.active_tab = entry.tab;
                                self.settings.search_query.clear();
                                self.settings.confirm_reset_tab = None;
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }
    }

    fn settings_tab_general_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        theme: Theme,
    ) {
        let pt = self.ui_pt();
        settings_card(
            ui,
            palette,
            if pt {
                "Aparência e feed"
            } else {
                "Appearance & feed"
            },
            None,
            None,
            |ui| {
                settings_control_row(
                    ui,
                    palette,
                    if pt {
                        "Tema da interface"
                    } else {
                        "Interface theme"
                    },
                    if pt {
                        "Aplicado imediatamente aos painéis e cartões; a superfície do vídeo permanece escura."
                    } else {
                        "Applied immediately to panels and cards; video surface stays dark."
                    },
                    275.0,
                    |ui| {
                        let mut selected_theme = self.settings.theme.value;
                        if theme_swatch_card(
                            ui,
                            palette,
                            Theme::Dark,
                            if pt {
                                "Escuro (padrão)"
                            } else {
                                "Dark (default)"
                            },
                            selected_theme == Theme::Dark,
                        ) {
                            selected_theme = Theme::Dark;
                        }
                        ui.add_space(6.0);
                        if theme_swatch_card(
                            ui,
                            palette,
                            Theme::Light,
                            if pt { "Claro" } else { "Light" },
                            selected_theme == Theme::Light,
                        ) {
                            selected_theme = Theme::Light;
                        }
                        if selected_theme != self.settings.theme.value {
                            self.apply_theme(selected_theme);
                            self.sync_theme_visuals(ctx);
                        }
                    },
                );
                paint_row_divider(ui, palette);

                let mut prefs = self.settings.prefs.value.clone();
                let mut changed = false;
                let mut invalidate = false;

                settings_control_row(
                    ui,
                    palette,
                    if pt {
                        "Idioma da interface"
                    } else {
                        "Interface language"
                    },
                    if pt {
                        "Alterne entre Português (Brasil) e Inglês imediatamente."
                    } else {
                        "Switch immediately between Portuguese (Brazil) and English."
                    },
                    240.0,
                    |ui| {
                        for lang in i18n::UiLanguage::ALL {
                            if pill_choice_button(ui, lang.label(), prefs.language == lang, palette)
                                .clicked()
                                && prefs.language != lang
                            {
                                prefs.language = lang;
                                changed = true;
                            }
                        }
                    },
                );
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Densidade compacta dos cartões"
                    } else {
                        "Compact card density"
                    },
                    if pt {
                        "Exibe mais colunas por linha e reduz o espaçamento na grade da página inicial."
                    } else {
                        "Show more columns per row and tighten spacing on the home grid."
                    },
                    &mut prefs.compact_card_density,
                    None,
                    None,
                ) {
                    changed = true;
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Ocultar Shorts na página inicial"
                    } else {
                        "Hide Shorts on Home"
                    },
                    if pt {
                        "Remove a estante e os cartões de Shorts da aba Início (a seção Shorts continua acessível)."
                    } else {
                        "Remove the Shorts shelf and cards from Home (the Shorts section remains accessible)."
                    },
                    &mut prefs.hide_shorts_in_home,
                    None,
                    None,
                ) {
                    changed = true;
                    invalidate = true;
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Auto Mini player ao navegar"
                    } else {
                        "Auto Mini player while browsing"
                    },
                    if pt {
                        "Fixa o vídeo no miniplayer interno no canto inferior direito ao voltar para o menu ou outra aba (sem minimizar o programa)."
                    } else {
                        "Dock the video into FastTube's bottom-right in-app miniplayer when navigating to the menu or other non-video tabs."
                    },
                    &mut prefs.auto_mini_player,
                    None,
                    None,
                ) {
                    changed = true;
                }

                if changed {
                    self.apply_app_preferences(prefs);
                    if invalidate {
                        self.invalidate_feed_cache();
                    }
                }
            },
        );
        if let Some(error) = &self.settings.theme.error {
            ui.add_space(8.0);
            status_banner(ui, palette, error, true);
        }

        ui.add_space(16.0);
        self.desktop.autostart.ensure_loaded(ctx);
        self.desktop.autostart.poll();

        settings_card(
            ui,
            palette,
            if pt {
                "Sistema, energia e bandeja"
            } else {
                "System, power & tray"
            },
            None,
            None,
            |ui| {
                let mut prefs = self.settings.prefs.value.clone();
                let mut prefs_changed = false;
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Manter tela ativa durante a reprodução"
                    } else {
                        "Keep screen awake during playback"
                    },
                    if pt {
                        "Evita suspensão do sistema ou desligamento do monitor enquanto um vídeo está em reprodução."
                    } else {
                        "Prevents system sleep or display turn-off while a video is actively playing."
                    },
                    &mut prefs.keep_awake_during_playback,
                    None,
                    None,
                ) {
                    prefs_changed = true;
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Minimizar para a bandeja do sistema"
                    } else {
                        "Minimize to system tray"
                    },
                    if pt {
                        "Ao minimizar a janela principal, oculta o FastTube na área de notificação."
                    } else {
                        "When minimizing the main window, hide FastTube to the notification area."
                    },
                    &mut prefs.minimize_to_tray,
                    None,
                    None,
                ) {
                    prefs_changed = true;
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Fechar janela para a bandeja"
                    } else {
                        "Close window to tray"
                    },
                    if pt {
                        "Ao clicar em fechar (X), mantém o FastTube em segundo plano na bandeja até escolher Sair."
                    } else {
                        "Clicking close (X) keeps FastTube running in the tray until you choose Quit."
                    },
                    &mut prefs.close_to_tray,
                    None,
                    None,
                ) {
                    prefs_changed = true;
                }
                paint_row_divider(ui, palette);

                let unsupported = matches!(
                    self.desktop.autostart.state,
                    crate::desktop::AutostartUiState::Unsupported
                );
                let loading = matches!(
                    self.desktop.autostart.state,
                    crate::desktop::AutostartUiState::Loading
                );
                let mut autostart_enabled = self.desktop.autostart.state.is_enabled();
                let autostart_hidden = self.desktop.autostart.state.is_hidden();
                let autostart_desc = match self.desktop.autostart.state {
                    crate::desktop::AutostartUiState::Loading => {
                        if pt {
                            "Verificando configuração de inicialização no Windows..."
                        } else {
                            "Checking Windows startup configuration..."
                        }
                    }
                    crate::desktop::AutostartUiState::Unsupported => {
                        if pt {
                            "Inicialização automática indisponível neste ambiente."
                        } else {
                            "Automatic startup is unavailable in this environment."
                        }
                    }
                    crate::desktop::AutostartUiState::NeedsApproval => {
                        if pt {
                            "Requer aprovação nas configurações de inicialização do Windows."
                        } else {
                            "Requires approval in Windows Startup Apps settings."
                        }
                    }
                    crate::desktop::AutostartUiState::Disabled
                    | crate::desktop::AutostartUiState::Enabled { .. } => {
                        if pt {
                            "Inicia o FastTube automaticamente ao entrar na sessão do Windows."
                        } else {
                            "Starts FastTube automatically when signing in to Windows."
                        }
                    }
                };
                let autostart_badge = match self.desktop.autostart.state {
                    crate::desktop::AutostartUiState::Enabled { hidden: true } => Some((
                        if pt {
                            "Ativo (bandeja)"
                        } else {
                            "Active (tray)"
                        },
                        palette.notice_fg,
                        palette.notice_bg,
                    )),
                    crate::desktop::AutostartUiState::Enabled { hidden: false } => Some((
                        if pt { "Ativo" } else { "Active" },
                        palette.notice_fg,
                        palette.notice_bg,
                    )),
                    crate::desktop::AutostartUiState::NeedsApproval => Some((
                        if pt {
                            "Aprovação no Windows"
                        } else {
                            "Needs Windows approval"
                        },
                        palette.warning_fg,
                        palette.error_bg,
                    )),
                    _ => None,
                };
                ui.add_enabled_ui(
                    !self.desktop.autostart.busy && !unsupported && !loading,
                    |ui| {
                        if settings_toggle_row(
                            ui,
                            palette,
                            theme,
                            if pt {
                                "Iniciar com o Windows"
                            } else {
                                "Start with Windows"
                            },
                            autostart_desc,
                            &mut autostart_enabled,
                            None,
                            autostart_badge,
                        ) {
                            self.desktop.autostart.set_enabled(
                                autostart_enabled,
                                prefs.start_minimized_to_tray,
                                ctx,
                            );
                        }
                    },
                );
                paint_row_divider(ui, palette);

                let syncing_badge = (autostart_enabled
                    && autostart_hidden != prefs.start_minimized_to_tray)
                    .then_some((
                        if pt { "Sincronizando" } else { "Syncing" },
                        palette.info_accent_fg,
                        palette.queue_next_bg,
                    ));
                let start_min_toggled = settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Iniciar minimizado na bandeja ao entrar no Windows"
                    } else {
                        "Start minimized to tray on Windows startup"
                    },
                    if pt {
                        "Quando o início automático estiver ativo, abre o FastTube diretamente oculto na bandeja."
                    } else {
                        "When automatic startup is enabled, launches FastTube hidden in the system tray."
                    },
                    &mut prefs.start_minimized_to_tray,
                    None,
                    syncing_badge,
                );
                if start_min_toggled {
                    prefs_changed = true;
                    if autostart_enabled && !self.desktop.autostart.busy && !unsupported && !loading
                    {
                        self.desktop.autostart.set_enabled(
                            true,
                            prefs.start_minimized_to_tray,
                            ctx,
                        );
                    }
                }

                if prefs_changed {
                    self.apply_app_preferences(prefs);
                }

                if let Some(err) = &self.desktop.autostart.error {
                    settings_row_frame(ui, |ui| {
                        status_banner(ui, palette, err, true);
                    });
                }
            },
        );

        ui.add_space(16.0);
        let app_update_busy = self.settings.app_update_rx.is_some();
        let app_badge = if let Some(check) = &self.settings.app_update_check {
            if check.update_available {
                Some((
                    format!("v{} -> v{}", check.current_version, check.latest_version),
                    palette.info_accent_fg,
                    palette.queue_next_bg,
                ))
            } else {
                Some((
                    format!("v{}", check.current_version),
                    palette.notice_fg,
                    palette.notice_bg,
                ))
            }
        } else {
            Some((
                format!("v{}", updater::CURRENT_APP_VERSION),
                palette.snippet_fg,
                palette.queue_next_bg,
            ))
        };
        let install_kind = updater::current_install_kind();
        settings_card(
            ui,
            palette,
            if pt {
                "Atualizador automático (GitHub Releases)"
            } else {
                "Auto-updater (GitHub Releases)"
            },
            app_badge
                .as_ref()
                .map(|(text, fg, bg)| (text.as_str(), *fg, *bg)),
            None,
            |ui| {
                let mut prefs = self.settings.prefs.value.clone();
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    if pt {
                        "Verificar atualizações automaticamente ao iniciar"
                    } else {
                        "Check for updates automatically at startup"
                    },
                    if pt {
                        "Consulta a release mais recente do repositório nathanbzrr/FastTube no GitHub ao abrir o app."
                    } else {
                        "Checks the latest release from nathanbzrr/FastTube on GitHub when launching the app."
                    },
                    &mut prefs.auto_check_updates,
                    None,
                    Some((
                        install_kind.label(pt),
                        palette.snippet_fg,
                        palette.queue_next_bg,
                    )),
                ) {
                    self.apply_app_preferences(prefs);
                }
                paint_row_divider(ui, palette);

                let check_snapshot = self.settings.app_update_check.clone();
                let control_desc = if let Some(check) = &check_snapshot {
                    if check.update_available {
                        if pt {
                            format!(
                                "Nova versão v{} disponível ({}). Integridade validada por SHA-256 antes de reiniciar.",
                                check.latest_version, check.asset_name
                            )
                        } else {
                            format!(
                                "New version v{} available ({}). Verified with SHA-256 before restarting.",
                                check.latest_version, check.asset_name
                            )
                        }
                    } else if pt {
                        format!(
                            "Você já está na versão mais recente (v{}).",
                            check.current_version
                        )
                    } else {
                        format!(
                            "You are on the latest version (v{}).",
                            check.current_version
                        )
                    }
                } else if pt {
                    "Verifica se há uma nova versão no GitHub e aplica a atualização automaticamente.".to_owned()
                } else {
                    "Checks GitHub for a newer release and applies the update automatically."
                        .to_owned()
                };

                settings_control_row(
                    ui,
                    palette,
                    if pt {
                        "Versão do FastTube"
                    } else {
                        "FastTube version"
                    },
                    &control_desc,
                    300.0,
                    |ui| {
                        ui.add_enabled_ui(!app_update_busy, |ui| {
                            if pill_choice_button(
                                ui,
                                if app_update_busy {
                                    if pt { "Processando..." } else { "Working..." }
                                } else if pt {
                                    "Verificar atualizações"
                                } else {
                                    "Check updates"
                                },
                                false,
                                palette,
                            )
                            .clicked()
                            {
                                self.start_app_update_check(false, ctx);
                            }

                            if let Some(check) = &check_snapshot
                                && check.update_available
                            {
                                if pill_choice_button(
                                    ui,
                                    if pt {
                                        "Baixar e instalar"
                                    } else {
                                        "Download & install"
                                    },
                                    true,
                                    palette,
                                )
                                .clicked()
                                {
                                    self.start_app_update_download(ctx);
                                }
                                if pill_choice_button(ui, "GitHub", false, palette).clicked() {
                                    open_external_url(&check.release_url);
                                }
                            }
                        });
                    },
                );

                if let Some(check) = &check_snapshot
                    && check.update_available
                {
                    let is_skipped = self
                        .settings
                        .prefs
                        .value
                        .skipped_app_update_version
                        .as_deref()
                        == Some(check.latest_version.as_str());
                    if is_skipped {
                        settings_row_frame(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if pt {
                                        format!(
                                            "Aviso da versão v{} silenciado.",
                                            check.latest_version
                                        )
                                    } else {
                                        format!("Banner for v{} is muted.", check.latest_version)
                                    })
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                                );
                                if pill_choice_button(
                                    ui,
                                    if pt { "Mostrar novamente" } else { "Un-mute" },
                                    false,
                                    palette,
                                )
                                .clicked()
                                {
                                    let mut prefs = self.settings.prefs.value.clone();
                                    prefs.skipped_app_update_version = None;
                                    self.apply_app_preferences(prefs);
                                    self.settings.app_update_banner_dismissed = false;
                                }
                            });
                        });
                    }
                    if !check.release_notes.trim().is_empty() {
                        paint_row_divider(ui, palette);
                        settings_row_frame(ui, |ui| {
                            ui.label(
                                RichText::new(if pt {
                                    format!("Notas da versão ({}):", check.tag_name)
                                } else {
                                    format!("Release notes ({}):", check.tag_name)
                                })
                                .size(12.0)
                                .strong()
                                .color(palette.channel_fg),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(&check.release_notes)
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                            );
                        });
                    }
                }

                if let Some((ok, status)) = &self.settings.app_update_status {
                    settings_row_frame(ui, |ui| {
                        status_banner(ui, palette, status, !*ok);
                    });
                }
            },
        );

        ui.add_space(16.0);
        if let Some(slot_id) = self.settings.capturing_keybinding {
            let captured = ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    return Some(None);
                }
                let ctrl = i.modifiers.ctrl || i.modifiers.command;
                let alt = i.modifiers.alt;
                let shift = i.modifiers.shift;
                for ev in &i.events {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } = ev
                        && let Some(formatted) =
                            settings::Keybindings::format_egui_key(ctrl, alt, shift, *key)
                    {
                        return Some(Some(formatted));
                    }
                }
                None
            });
            if let Some(maybe_binding) = captured {
                if let Some(binding) = maybe_binding {
                    let mut prefs = self.settings.prefs.value.clone();
                    match slot_id {
                        "play_pause" => prefs.keybindings.play_pause = binding,
                        "seek_back" => prefs.keybindings.seek_back = binding,
                        "seek_forward" => prefs.keybindings.seek_forward = binding,
                        "mute" => prefs.keybindings.mute = binding,
                        "theater" => prefs.keybindings.theater = binding,
                        "fullscreen" => prefs.keybindings.fullscreen = binding,
                        "mini_player" => prefs.keybindings.mini_player = binding,
                        "audio_only" => prefs.keybindings.audio_only = binding,
                        "screenshot" => prefs.keybindings.screenshot = binding,
                        "ab_loop_a" => prefs.keybindings.ab_loop_a = binding,
                        "ab_loop_b" => prefs.keybindings.ab_loop_b = binding,
                        "ab_loop_clear" => prefs.keybindings.ab_loop_clear = binding,
                        "command_palette" => prefs.keybindings.command_palette = binding,
                        _ => {}
                    }
                    self.apply_app_preferences(prefs);
                }
                self.settings.capturing_keybinding = None;
            }
        }

        let kb = self.settings.prefs.value.keybindings.clone();
        let step_secs = self.seek_step_secs();
        let shortcuts_open = settings_card(
            ui,
            palette,
            if pt {
                "Atalhos de teclado personalizáveis"
            } else {
                "Customizable keyboard shortcuts"
            },
            None,
            Some(if pt {
                "Guia completa (F1)"
            } else {
                "Full guide (F1)"
            }),
            |ui| {
                settings_row_frame(ui, |ui| {
                    let seek_back_desc = if pt {
                        format!("Voltar {step_secs} segundos")
                    } else {
                        format!("Seek back {step_secs}s")
                    };
                    let seek_fwd_desc = if pt {
                        format!("Avançar {step_secs} segundos")
                    } else {
                        format!("Seek forward {step_secs}s")
                    };
                    let rows: [(&'static str, &str, &str); 13] = [
                        (
                            "play_pause",
                            if pt {
                                "Reproduzir / pausar"
                            } else {
                                "Play / pause"
                            },
                            &kb.play_pause,
                        ),
                        ("seek_back", &seek_back_desc, &kb.seek_back),
                        ("seek_forward", &seek_fwd_desc, &kb.seek_forward),
                        (
                            "mute",
                            if pt { "Silenciar áudio" } else { "Mute audio" },
                            &kb.mute,
                        ),
                        (
                            "theater",
                            if pt { "Modo teatro" } else { "Theater mode" },
                            &kb.theater,
                        ),
                        (
                            "fullscreen",
                            if pt { "Tela cheia" } else { "Fullscreen" },
                            &kb.fullscreen,
                        ),
                        ("mini_player", "Mini player", &kb.mini_player),
                        (
                            "audio_only",
                            if pt {
                                "Modo somente áudio"
                            } else {
                                "Audio-only mode"
                            },
                            &kb.audio_only,
                        ),
                        (
                            "screenshot",
                            if pt {
                                "Capturar frame (PNG)"
                            } else {
                                "Screenshot frame (PNG)"
                            },
                            &kb.screenshot,
                        ),
                        (
                            "ab_loop_a",
                            if pt {
                                "Marcar ponto A do loop"
                            } else {
                                "Set A-B loop start"
                            },
                            &kb.ab_loop_a,
                        ),
                        (
                            "ab_loop_b",
                            if pt {
                                "Marcar ponto B do loop"
                            } else {
                                "Set A-B loop end"
                            },
                            &kb.ab_loop_b,
                        ),
                        (
                            "ab_loop_clear",
                            if pt {
                                "Limpar loop A-B"
                            } else {
                                "Clear A-B loop"
                            },
                            &kb.ab_loop_clear,
                        ),
                        (
                            "command_palette",
                            if pt {
                                "Paleta de comandos"
                            } else {
                                "Command palette"
                            },
                            &kb.command_palette,
                        ),
                    ];
                    for (slot, label, current_val) in rows {
                        let capturing_this = self.settings.capturing_keybinding == Some(slot);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).size(12.5).color(palette.channel_fg));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let btn_text = if capturing_this {
                                        if pt {
                                            "Pressione tecla (Esc cancela)..."
                                        } else {
                                            "Press key (Esc cancels)..."
                                        }
                                    } else {
                                        current_val
                                    };
                                    if pill_choice_button(ui, btn_text, capturing_this, palette)
                                        .clicked()
                                    {
                                        self.settings.capturing_keybinding =
                                            if capturing_this { None } else { Some(slot) };
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(6.0);
                    if pill_choice_button(
                        ui,
                        if pt {
                            "Restaurar atalhos padrão"
                        } else {
                            "Reset default shortcuts"
                        },
                        false,
                        palette,
                    )
                    .clicked()
                    {
                        let mut prefs = self.settings.prefs.value.clone();
                        prefs.keybindings = settings::Keybindings::default();
                        self.settings.capturing_keybinding = None;
                        self.apply_app_preferences(prefs);
                    }
                });
            },
        );
        if shortcuts_open {
            self.ui.shortcuts_open = true;
        }
    }

    fn settings_tab_playback_ui(&mut self, ui: &mut egui::Ui, palette: ThemePalette, theme: Theme) {
        let mut playback_prefs = self.settings.playback.value;
        let mut app_prefs = self.settings.prefs.value.clone();
        let mut app_prefs_changed = false;

        settings_card(
            ui,
            palette,
            "Controles e preferências padrão",
            None,
            None,
            |ui| {
                settings_control_row(
                    ui,
                    palette,
                    "Velocidade padrão",
                    "Aplicada automaticamente ao abrir novos vídeos.",
                    310.0,
                    |ui| {
                        let speed_presets: [(u8, &str); 5] = [
                            (8, "2.00×"),
                            (6, "1.50×"),
                            (5, "1.25×"),
                            (4, "1.00×"),
                            (3, "0.75×"),
                        ];
                        let is_preset = speed_presets
                            .iter()
                            .any(|(q, _)| *q == playback_prefs.speed_quarters);
                        egui::ComboBox::from_id_salt("settings_default_speed")
                            .width(84.0)
                            .selected_text(if is_preset {
                                "Outras".to_owned()
                            } else {
                                format!("{:.2}×", f64::from(playback_prefs.speed_quarters) / 4.0)
                            })
                            .show_ui(ui, |ui| {
                                for speed in 1..=8 {
                                    ui.selectable_value(
                                        &mut playback_prefs.speed_quarters,
                                        speed,
                                        format!("{:.2}×", f64::from(speed) / 4.0),
                                    );
                                }
                            });
                        for (q, label) in speed_presets {
                            if pill_choice_button(
                                ui,
                                label,
                                playback_prefs.speed_quarters == q,
                                palette,
                            )
                            .clicked()
                            {
                                playback_prefs.speed_quarters = q;
                            }
                        }
                    },
                );
                paint_row_divider(ui, palette);

                settings_control_row(
                    ui,
                    palette,
                    "Qualidade máxima padrão",
                    "Se não houver formato dentro do limite, o player escolhe a melhor opção compatível.",
                    320.0,
                    |ui| {
                        let quality_presets: [(Option<u32>, &str); 4] = [
                            (Some(2160), "2160p"),
                            (Some(1440), "1440p"),
                            (Some(720), "720p"),
                            (None, "Auto (1080p)"),
                        ];
                        let is_preset = quality_presets
                            .iter()
                            .any(|(h, _)| *h == playback_prefs.max_height);
                        egui::ComboBox::from_id_salt("settings_default_quality")
                            .width(92.0)
                            .selected_text(if is_preset {
                                "Outras".to_owned()
                            } else {
                                playback_prefs
                                    .max_height
                                    .map(|h| format!("{h}p"))
                                    .unwrap_or_else(|| "Auto".into())
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut playback_prefs.max_height,
                                    None,
                                    "Automática (até 1080p)",
                                );
                                for height in [360, 480, 720, 1080, 1440, 2160] {
                                    ui.selectable_value(
                                        &mut playback_prefs.max_height,
                                        Some(height),
                                        format!("Até {height}p"),
                                    );
                                }
                            });
                        for (height, label) in quality_presets {
                            if pill_choice_button(
                                ui,
                                label,
                                playback_prefs.max_height == height,
                                palette,
                            )
                            .clicked()
                            {
                                playback_prefs.max_height = height;
                            }
                        }
                    },
                );
                paint_row_divider(ui, palette);

                settings_control_row(
                    ui,
                    palette,
                    "Passo de avanço e retrocesso",
                    "Duração usada nos botões do overlay e nos atalhos ← / → e J / L.",
                    210.0,
                    |ui| {
                        for secs in [30_u32, 15, 10, 5] {
                            let label = format!("{secs}s");
                            if pill_choice_button(
                                ui,
                                &label,
                                app_prefs.seek_step_secs == secs,
                                palette,
                            )
                            .clicked()
                            {
                                app_prefs.seek_step_secs = secs;
                                app_prefs_changed = true;
                            }
                        }
                    },
                );
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Reprodução automática padrão",
                    "Avança automaticamente para o próximo vídeo da fila ou recomendação ao finalizar.",
                    &mut app_prefs.default_autoplay,
                    None,
                    None,
                ) {
                    app_prefs_changed = true;
                    if self.selected.is_none() {
                        self.playback.autoplay = app_prefs.default_autoplay;
                    }
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Repetir vídeo por padrão (loop)",
                    "Mantém a repetição contínua ativada ao iniciar novos vídeos.",
                    &mut app_prefs.default_loop,
                    None,
                    None,
                ) {
                    app_prefs_changed = true;
                    if self.selected.is_none() {
                        self.playback.loop_video = app_prefs.default_loop;
                    }
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Retomar reprodução de onde parou",
                    "Usa o progresso salvo no histórico local para continuar vídeos já iniciados.",
                    &mut app_prefs.resume_enabled,
                    None,
                    None,
                ) {
                    app_prefs_changed = true;
                }
            },
        );

        if playback_prefs != self.settings.playback.value {
            self.apply_playback_preferences(playback_prefs);
        }
        if let Some(error) = &self.settings.playback.error {
            ui.add_space(8.0);
            status_banner(ui, palette, error, true);
        }

        ui.add_space(16.0);
        settings_card(ui, palette, "Áudio e legendas", None, None, |ui| {
            if settings_toggle_row(
                ui,
                palette,
                theme,
                "Lembrar último volume entre sessões",
                "Salva automaticamente o nível de volume ao fechar o vídeo ou sair do aplicativo.",
                &mut app_prefs.remember_volume,
                None,
                None,
            ) {
                app_prefs_changed = true;
            }
            paint_row_divider(ui, palette);

            let vol_title = if app_prefs.remember_volume {
                "Último volume salvo"
            } else {
                "Volume inicial padrão"
            };
            let vol_desc = if app_prefs.remember_volume {
                "Usado ao abrir o próximo vídeo caso o volume ainda não tenha sido alterado nesta sessão."
            } else {
                "Todos os vídeos iniciarão exatamente neste nível de volume."
            };
            settings_control_row(ui, palette, vol_title, vol_desc, 270.0, |ui| {
                let mut vol = u32::from(app_prefs.default_volume_percent.min(100));
                if stepper_u32_controls(ui, &mut vol, 0..=100, 5, "%", palette) {
                    app_prefs.default_volume_percent = vol.min(100) as u8;
                    app_prefs_changed = true;
                }
            });
            paint_row_divider(ui, palette);

            if settings_toggle_row(
                ui,
                palette,
                theme,
                "Normalização de áudio (loudnorm)",
                "Aplica filtro EBU R128 em tempo real no libmpv para equilibrar vídeos baixos e altos.",
                &mut app_prefs.audio_normalization,
                None,
                None,
            ) {
                app_prefs_changed = true;
            }
            paint_row_divider(ui, palette);

            if settings_toggle_row(
                ui,
                palette,
                theme,
                "Ativar legendas automaticamente",
                "Seleciona automaticamente a faixa mais próxima do idioma preferido quando o vídeo carrega.",
                &mut app_prefs.auto_subtitles,
                None,
                None,
            ) {
                app_prefs_changed = true;
            }
            paint_row_divider(ui, palette);

            ui.add_enabled_ui(app_prefs.auto_subtitles, |ui| {
                settings_control_row(
                    ui,
                    palette,
                    "Idioma preferido das legendas",
                    "Código ISO (ex.: pt, pt-BR, en, es). Legendas manuais têm prioridade sobre automáticas.",
                    260.0,
                    |ui| {
                        ui.add_sized(
                            [66.0, 28.0],
                            egui::TextEdit::singleline(&mut app_prefs.preferred_subtitle_lang)
                                .char_limit(16)
                                .hint_text("pt"),
                        );
                        if app_prefs.preferred_subtitle_lang != self.settings.prefs.value.preferred_subtitle_lang
                        {
                            app_prefs_changed = true;
                        }
                        for lang in ["es", "en", "pt-BR", "pt"] {
                            let active = app_prefs
                                .preferred_subtitle_lang
                                .eq_ignore_ascii_case(lang);
                            if pill_choice_button(ui, lang, active, palette).clicked() {
                                app_prefs.preferred_subtitle_lang = lang.to_owned();
                                app_prefs_changed = true;
                            }
                        }
                    },
                );
            });
        });

        ui.add_space(16.0);
        let hwdec_pending = self.has_pending_hwdec_restart();
        let hwdec_badge = if hwdec_pending {
            Some(("Reinício pendente", palette.warning_fg, palette.error_bg))
        } else {
            None
        };
        settings_card(
            ui,
            palette,
            "Decodificação por hardware (libmpv)",
            hwdec_badge,
            None,
            |ui| {
                settings_row_frame(ui, |ui| {
                    let mut chosen_hwdec = app_prefs.hwdec_mode;
                    for mode in HwdecMode::ALL {
                        let is_selected = app_prefs.hwdec_mode == mode;
                        let recommended_badge = (mode == HwdecMode::AutoSafe).then_some((
                            "Recomendado",
                            palette.notice_fg,
                            palette.notice_bg,
                        ));
                        if clickable_radio_card(
                            ui,
                            palette,
                            is_selected,
                            mode.label_pt(),
                            mode.description_pt(),
                            recommended_badge,
                        ) && !is_selected
                        {
                            chosen_hwdec = mode;
                        }
                        ui.add_space(6.0);
                    }
                    if chosen_hwdec != app_prefs.hwdec_mode {
                        app_prefs.hwdec_mode = chosen_hwdec;
                        app_prefs_changed = true;
                    }
                    if let Some(active) = self.playback.active_hwdec
                        && active != app_prefs.hwdec_mode
                    {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "⚠ O player em execução foi iniciado com '{}' e a opção selecionada agora é '{}'.",
                                    active.as_mpv_value(),
                                    app_prefs.hwdec_mode.as_mpv_value()
                                ))
                                .size(12.0)
                                .color(palette.warning_fg),
                            );
                            if self.selected.is_some()
                                && pill_choice_button(
                                    ui,
                                    "Aplicar agora e recarregar vídeo",
                                    true,
                                    palette,
                                )
                                .clicked()
                            {
                                self.settings.pending_reload_video = true;
                            }
                        });
                    }
                });
            },
        );

        if app_prefs_changed {
            self.apply_app_preferences(app_prefs);
        }

        ui.add_space(16.0);
        if !self.settings.custom_buffer_synced {
            let v = self.settings.buffer.profile.values();
            self.settings.custom_buffer_max_mib = v.max_mib;
            self.settings.custom_buffer_back_mib = v.back_mib;
            self.settings.custom_buffer_cache_secs = v.cache_seconds;
            self.settings.custom_buffer_synced = true;
        }
        let buffer_pending = self.has_pending_buffer_restart();
        let buffer_badge = if buffer_pending {
            Some(("Reinício pendente", palette.warning_fg, palette.error_bg))
        } else {
            None
        };
        settings_card(
            ui,
            palette,
            "Buffer de rede e demuxer (libmpv)",
            buffer_badge,
            None,
            |ui| {
                settings_row_frame(ui, |ui| {
                    ui.label(
                        RichText::new(BUFFER_PROFILE_HELP_SUMMARY)
                            .size(12.5)
                            .color(palette.snippet_fg),
                    );
                    ui.add_space(10.0);

                    let profiles = [
                        (
                            BufferProfile::Small,
                            "Pequeno — 32 MiB máx / 8 MiB retorno / 15s",
                            "Menor uso de memória RAM; recomendado para máquinas com pouca memória e conexão estável.",
                            None,
                        ),
                        (
                            BufferProfile::Default,
                            "Padrão — 64 MiB máx / 16 MiB retorno / 30s",
                            "Equilíbrio padrão do FastTube entre baixo consumo de memória e fluidez na reprodução.",
                            Some(("Recomendado", palette.notice_fg, palette.notice_bg)),
                        ),
                        (
                            BufferProfile::Large,
                            "Grande — 128 MiB máx / 32 MiB retorno / 60s",
                            "Maior folga de cache para conexões instáveis ou vídeos longos, com maior consumo de RAM.",
                            None,
                        ),
                    ];
                    let mut chosen_profile = None;
                    for (profile, title, desc, badge) in profiles {
                        let is_selected = self.settings.buffer.profile == profile;
                        if clickable_radio_card(ui, palette, is_selected, title, desc, badge)
                            && !is_selected
                        {
                            chosen_profile = Some(profile);
                            let v = profile.values();
                            self.settings.custom_buffer_max_mib = v.max_mib;
                            self.settings.custom_buffer_back_mib = v.back_mib;
                            self.settings.custom_buffer_cache_secs = v.cache_seconds;
                        }
                        ui.add_space(6.0);
                    }

                    let is_custom =
                        matches!(self.settings.buffer.profile, BufferProfile::Custom(_));
                    let custom_title = format!(
                        "Personalizado — {} MiB máx / {} MiB retorno / {}s",
                        self.settings.custom_buffer_max_mib,
                        self.settings.custom_buffer_back_mib,
                        self.settings.custom_buffer_cache_secs
                    );
                    if clickable_radio_card(
                        ui,
                        palette,
                        is_custom,
                        &custom_title,
                        "Ajuste manual dos limites do demuxer (1–1024 MiB e 1–3600s).",
                        None,
                    ) && !is_custom
                    {
                        chosen_profile = Some(BufferProfile::Custom(BufferSettings {
                            max_mib: self.settings.custom_buffer_max_mib.clamp(1, 1024),
                            back_mib: self
                                .settings
                                .custom_buffer_back_mib
                                .min(self.settings.custom_buffer_max_mib.clamp(1, 1024)),
                            cache_seconds: self.settings.custom_buffer_cache_secs.clamp(1, 3600),
                        }));
                    }

                    if is_custom {
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(palette.card_alt_bg)
                            .stroke(Stroke::new(1.0, palette.card_alt_stroke))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(12, 10))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Avanço máximo (MiB):")
                                            .size(12.5)
                                            .color(palette.channel_fg),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            stepper_u32_controls(
                                                ui,
                                                &mut self.settings.custom_buffer_max_mib,
                                                1..=1024,
                                                16,
                                                " MiB",
                                                palette,
                                            );
                                        },
                                    );
                                });
                                let max_mib = self.settings.custom_buffer_max_mib.clamp(1, 1024);
                                self.settings.custom_buffer_max_mib = max_mib;
                                if self.settings.custom_buffer_back_mib > max_mib {
                                    self.settings.custom_buffer_back_mib = max_mib;
                                }
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Retorno máximo (MiB):")
                                            .size(12.5)
                                            .color(palette.channel_fg),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            stepper_u32_controls(
                                                ui,
                                                &mut self.settings.custom_buffer_back_mib,
                                                0..=max_mib,
                                                4,
                                                " MiB",
                                                palette,
                                            );
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Tempo de cache (s):")
                                            .size(12.5)
                                            .color(palette.channel_fg),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            stepper_u32_controls(
                                                ui,
                                                &mut self.settings.custom_buffer_cache_secs,
                                                1..=3600,
                                                15,
                                                "s",
                                                palette,
                                            );
                                        },
                                    );
                                });
                                ui.add_space(8.0);
                                let edited_custom = BufferSettings {
                                    max_mib: self.settings.custom_buffer_max_mib,
                                    back_mib: self.settings.custom_buffer_back_mib,
                                    cache_seconds: self.settings.custom_buffer_cache_secs,
                                };
                                let dirty_custom =
                                    self.settings.buffer.profile.values() != edited_custom;
                                ui.horizontal_wrapped(|ui| {
                                    ui.add_enabled_ui(dirty_custom, |ui| {
                                        if pill_choice_button(
                                            ui,
                                            "Aplicar buffer personalizado",
                                            dirty_custom,
                                            palette,
                                        )
                                        .clicked()
                                        {
                                            chosen_profile =
                                                Some(BufferProfile::Custom(edited_custom));
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "RAM máx estimada ~{} MiB",
                                            edited_custom
                                                .max_mib
                                                .saturating_add(edited_custom.back_mib)
                                        ))
                                        .size(11.5)
                                        .color(palette.snippet_fg),
                                    );
                                });
                            });
                    }

                    if let Some(profile) = chosen_profile {
                        self.apply_buffer_profile(profile);
                    }

                    if let Some(active) = self.playback.active_buffer
                        && active.values() != self.settings.buffer.profile.values()
                    {
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "⚠ O player em execução foi iniciado com {} e o perfil selecionado agora é {}.",
                                    buffer_profile_summary(active),
                                    buffer_profile_summary(self.settings.buffer.profile)
                                ))
                                .size(12.0)
                                .color(palette.warning_fg),
                            );
                            if self.selected.is_some()
                                && pill_choice_button(
                                    ui,
                                    "Aplicar agora e recarregar vídeo",
                                    true,
                                    palette,
                                )
                                .clicked()
                            {
                                self.settings.pending_reload_video = true;
                            }
                        });
                    }
                });
            },
        );
        if let Some(notice) = &self.settings.buffer.notice {
            ui.add_space(8.0);
            status_banner(ui, palette, notice, false);
        }
        if let Some(error) = &self.settings.buffer.error {
            ui.add_space(8.0);
            status_banner(ui, palette, error, true);
        }
    }

    fn settings_tab_sponsorblock_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        theme: Theme,
    ) {
        let mut prefs = self.settings.prefs.value.clone();
        let mut changed = false;
        let master_badge = if prefs.sponsorblock.enabled {
            Some(("Ativo", palette.notice_fg, palette.notice_bg))
        } else {
            Some(("Desativado", palette.snippet_fg, palette.header_button_bg))
        };

        settings_card(
            ui,
            palette,
            "Integração SponsorBlock",
            master_badge,
            None,
            |ui| {
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Ativar SponsorBlock",
                    "Busca segmentos marcados pela comunidade em sponsor.ajay.app ao abrir vídeos do YouTube.",
                    &mut prefs.sponsorblock.enabled,
                    None,
                    None,
                ) {
                    changed = true;
                }
                paint_row_divider(ui, palette);

                ui.add_enabled_ui(prefs.sponsorblock.enabled, |ui| {
                    settings_control_row(
                        ui,
                        palette,
                        "Modo de ação",
                        prefs.sponsorblock.skip_mode.description_pt(),
                        320.0,
                        |ui| {
                            for mode in SponsorSkipMode::ALL.into_iter().rev() {
                                let short = match mode {
                                    SponsorSkipMode::AutoSkip => "Pular auto",
                                    SponsorSkipMode::ManualButton => "Botão manual",
                                    SponsorSkipMode::HighlightOnly => "Apenas destacar",
                                };
                                if pill_choice_button(
                                    ui,
                                    short,
                                    prefs.sponsorblock.skip_mode == mode,
                                    palette,
                                )
                                .on_hover_text(mode.label_pt())
                                .clicked()
                                {
                                    prefs.sponsorblock.skip_mode = mode;
                                    changed = true;
                                }
                            }
                        },
                    );
                    paint_row_divider(ui, palette);

                    settings_control_row(
                        ui,
                        palette,
                        "Duração mínima do segmento",
                        "Segmentos menores que este limite serão ignorados para evitar cortes bruscos.",
                        280.0,
                        |ui| {
                            ui.horizontal(|ui| {
                                if pill_choice_button(ui, "-0.5s", false, palette).clicked() {
                                    prefs.sponsorblock.min_duration_secs =
                                        (prefs.sponsorblock.min_duration_secs - 0.5).clamp(0.0, 30.0);
                                    changed = true;
                                }
                                let resp = ui.add(
                                    egui::Slider::new(
                                        &mut prefs.sponsorblock.min_duration_secs,
                                        0.0..=30.0,
                                    )
                                    .step_by(0.5)
                                    .suffix("s"),
                                );
                                if resp.changed() {
                                    changed = true;
                                }
                                if pill_choice_button(ui, "+0.5s", false, palette).clicked() {
                                    prefs.sponsorblock.min_duration_secs =
                                        (prefs.sponsorblock.min_duration_secs + 0.5).clamp(0.0, 30.0);
                                    changed = true;
                                }
                            });
                        },
                    );
                    paint_row_divider(ui, palette);

                    if settings_toggle_row(
                        ui,
                        palette,
                        theme,
                        "Mostrar marcadores na barra de progresso",
                        "Exibe trechos coloridos na timeline do player com tooltip da categoria.",
                        &mut prefs.sponsorblock.show_timeline_markers,
                        None,
                        None,
                    ) {
                        changed = true;
                    }
                    paint_row_divider(ui, palette);

                    if settings_toggle_row(
                        ui,
                        palette,
                        theme,
                        "Mostrar notificação ao pular segmento",
                        "Exibe um aviso discreto sobre o vídeo indicando a categoria pulada.",
                        &mut prefs.sponsorblock.show_skip_toast,
                        None,
                        None,
                    ) {
                        changed = true;
                    }
                });
            },
        );

        ui.add_space(16.0);
        let enabled_count = prefs.sponsorblock.categories.active_count();
        let total_count = SponsorCategory::ALL.len();
        let count_text = format!("{enabled_count} de {total_count} ativas");
        let bulk_label = if enabled_count < total_count {
            "Ativar todas"
        } else {
            "Desativar todas"
        };

        let bulk_clicked = settings_card(
            ui,
            palette,
            "Categorias de segmentos",
            Some((&count_text, palette.info_accent_fg, palette.queue_next_bg)),
            Some(bulk_label),
            |ui| {
                ui.add_enabled_ui(prefs.sponsorblock.enabled, |ui| {
                    settings_row_frame(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Predefinição rápida:")
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                            );
                            if pill_choice_button(ui, "Apenas recomendadas", false, palette)
                                .clicked()
                            {
                                for cat in SponsorCategory::ALL {
                                    prefs.sponsorblock.categories.set(cat, cat.recommended());
                                }
                                changed = true;
                            }
                        });
                    });
                    for category in SponsorCategory::ALL {
                        paint_row_divider(ui, palette);
                        let mut enabled = prefs.sponsorblock.categories.is_enabled(category);
                        let badge = category.recommended().then_some((
                            "Recomendada",
                            palette.notice_fg,
                            palette.notice_bg,
                        ));
                        let toggled = settings_toggle_row(
                            ui,
                            palette,
                            theme,
                            category.label_pt(),
                            category.description_pt(),
                            &mut enabled,
                            Some(category.color()),
                            badge,
                        );
                        if toggled {
                            prefs.sponsorblock.categories.set(category, enabled);
                            changed = true;
                        }
                    }
                });
            },
        );
        if bulk_clicked {
            let enable_all = enabled_count < total_count;
            prefs.sponsorblock.categories.set_all(enable_all);
            changed = true;
        }

        if changed {
            let was_enabled = self.settings.prefs.value.sponsorblock.enabled;
            self.apply_app_preferences(prefs);
            if !was_enabled
                && self.settings.prefs.value.sponsorblock.enabled
                && let Some(video) = self.selected.clone()
            {
                self.start_sponsorblock_fetch(&video.url, ctx);
            }
        }

        if let Some(error) = &self.watch.sponsorblock.error {
            ui.add_space(10.0);
            status_banner(
                ui,
                palette,
                &format!("Última consulta SponsorBlock falhou: {error}"),
                true,
            );
        }
    }

    fn settings_tab_privacy_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
        theme: Theme,
    ) {
        let oauth_path = settings::oauth_client_path().ok();
        let oauth_exists = oauth_path.as_ref().is_some_and(|p| p.is_file());

        settings_card(ui, palette, "Conta Google", None, None, |ui| {
            settings_row_frame(ui, |ui| {
                if self.auth.authenticated {
                    ui.horizontal(|ui| {
                        let avatar_url = self.auth.my_channel_avatar_url.clone();
                        let label = self
                            .auth
                            .my_channel_title
                            .clone()
                            .unwrap_or_else(|| "Conta Google conectada".to_owned());
                        let _ = self.paint_avatar_or_initials(
                            ui,
                            ctx,
                            avatar_url.as_deref(),
                            &label,
                            40.0,
                            false,
                        );
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&label)
                                        .size(14.0)
                                        .strong()
                                        .color(palette.channel_fg),
                                );
                                settings_pill_badge(
                                    ui,
                                    "OAuth ativo",
                                    palette.notice_fg,
                                    palette.notice_bg,
                                );
                            });
                            ui.label(
                                RichText::new(
                                    "Usada apenas para carregar suas inscrições, playlists e avaliar/comentar vídeos.",
                                )
                                .size(12.0)
                                .color(palette.snippet_fg),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if pill_choice_button(ui, "Sair da conta", false, palette).clicked() {
                                self.sign_out_and_reload_home(ctx);
                            }
                        });
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Nenhuma conta Google conectada")
                                    .size(14.0)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                            ui.label(
                                RichText::new(
                                    "Você pode navegar e assistir vídeos sem conta. Faça login apenas se quiser sincronizar inscrições e playlists.",
                                )
                                .size(12.0)
                                .color(palette.snippet_fg),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if pill_choice_button(ui, "Entrar com Google", true, palette).clicked()
                            {
                                self.start_reauth(ctx);
                            }
                        });
                    });
                }
            });
            paint_row_divider(ui, palette);

            settings_row_frame(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Credenciais OAuth (oauth-client.json)")
                                    .size(13.5)
                                    .strong()
                                    .color(palette.channel_fg),
                            );
                            ui.add_space(6.0);
                            if oauth_exists {
                                settings_pill_badge(
                                    ui,
                                    "Configurado",
                                    palette.notice_fg,
                                    palette.notice_bg,
                                );
                            } else {
                                settings_pill_badge(
                                    ui,
                                    "Não encontrado",
                                    palette.warning_fg,
                                    palette.error_bg,
                                );
                            }
                        });
                        ui.add_space(2.0);
                        let desc = oauth_path
                            .as_ref()
                            .map(|p| format!("Local esperado: {}", p.display()))
                            .unwrap_or_else(|| {
                                "Importe o JSON do cliente OAuth Desktop gerado no Google Cloud."
                                    .to_owned()
                            });
                        ui.label(RichText::new(desc).size(12.0).color(palette.snippet_fg));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(path) = &oauth_path
                            && pill_choice_button(ui, "Abrir pasta", false, palette).clicked()
                        {
                            let target = path.parent().unwrap_or(path);
                            if let Err(err) = settings::open_in_explorer(target) {
                                self.settings.oauth_import_status = Some((false, err));
                            }
                        }
                        let picking = self.settings.oauth_picker_rx.is_some();
                        ui.add_enabled_ui(!picking, |ui| {
                            if pill_choice_button(
                                ui,
                                if picking {
                                    "Abrindo..."
                                } else {
                                    "Importar JSON..."
                                },
                                false,
                                palette,
                            )
                            .clicked()
                            {
                                let initial = oauth_path.clone();
                                let (tx, rx) = mpsc::channel();
                                self.settings.oauth_picker_rx = Some(rx);
                                let ctx_clone = ctx.clone();
                                std::thread::spawn(move || {
                                    let res = settings::pick_json_file_dialog(
                                        false,
                                        "Selecionar oauth-client.json",
                                        initial.as_deref(),
                                    );
                                    let _ = tx.send(res);
                                    ctx_clone.request_repaint();
                                });
                            }
                        });
                    });
                });
                if let Some((ok, status)) = &self.settings.oauth_import_status {
                    ui.add_space(8.0);
                    status_banner(ui, palette, status, !ok);
                }
            });
        });

        ui.add_space(16.0);
        self.ensure_feed_cache();
        let mut prefs = self.settings.prefs.value.clone();
        let mut changed = false;
        let mut invalidate = false;

        settings_card(
            ui,
            palette,
            "Histórico e recomendações locais",
            None,
            None,
            |ui| {
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Pausar histórico de exibição",
                    "Impede que novos vídeos assistidos ou progressos sejam registrados localmente.",
                    &mut prefs.pause_watch_history,
                    None,
                    None,
                ) {
                    changed = true;
                }
                paint_row_divider(ui, palette);

                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Pausar aprendizado de recomendações",
                    "As recomendações da página inicial deixam de considerar seu histórico e fila local.",
                    &mut prefs.pause_recommendations,
                    None,
                    None,
                ) {
                    changed = true;
                    invalidate = true;
                }
                paint_row_divider(ui, palette);

                settings_row_frame(ui, |ui| {
                    ui.label(
                        RichText::new("Tópicos aprendidos localmente")
                            .size(13.0)
                            .strong()
                            .color(palette.channel_fg),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(
                            "Clique em × para silenciar um tópico das recomendações locais.",
                        )
                        .size(11.5)
                        .color(palette.snippet_fg),
                    );
                    ui.add_space(6.0);
                    let topics = self.feed.cache.profile.top_topics(8);
                    if prefs.pause_recommendations {
                        ui.label(
                            RichText::new("Aprendizado pausado no momento.")
                                .size(12.0)
                                .color(palette.snippet_fg),
                        );
                    } else if topics.is_empty() {
                        ui.label(
                            RichText::new("Ainda aprendendo a partir dos seus vídeos assistidos.")
                                .size(12.0)
                                .color(palette.snippet_fg),
                        );
                    } else {
                        let mut mute_candidate: Option<String> = None;
                        ui.horizontal_wrapped(|ui| {
                            for topic in &topics {
                                let btn = egui::Button::new(
                                    RichText::new(format!("{topic}  ×"))
                                        .size(11.5)
                                        .strong()
                                        .color(palette.info_accent_fg),
                                )
                                .fill(palette.queue_next_bg)
                                .stroke(Stroke::new(
                                    1.0,
                                    palette.info_accent_fg.gamma_multiply(0.25),
                                ))
                                .corner_radius(CornerRadius::same(12))
                                .min_size(Vec2::new(0.0, 24.0));
                                if ui
                                    .add(btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Silenciar tópico das recomendações")
                                    .clicked()
                                {
                                    mute_candidate = Some(topic.to_ascii_lowercase());
                                }
                            }
                        });
                        if let Some(topic) = mute_candidate
                            && !prefs.muted_topics.contains(&topic)
                        {
                            prefs.muted_topics.push(topic);
                            changed = true;
                            invalidate = true;
                        }
                    }

                    if !prefs.muted_topics.is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Tópicos silenciados:")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            let mut restore_topic: Option<String> = None;
                            for muted in &prefs.muted_topics {
                                let btn = egui::Button::new(
                                    RichText::new(format!("+ {muted}"))
                                        .size(11.5)
                                        .color(palette.header_button_fg),
                                )
                                .fill(palette.header_button_bg)
                                .stroke(Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(CornerRadius::same(12))
                                .min_size(Vec2::new(0.0, 24.0));
                                if ui
                                    .add(btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Restaurar tópico")
                                    .clicked()
                                {
                                    restore_topic = Some(muted.clone());
                                }
                            }
                            if let Some(topic) = restore_topic {
                                prefs.muted_topics.retain(|t| t != &topic);
                                changed = true;
                                invalidate = true;
                            }
                            if pill_choice_button(ui, "Restaurar todos", false, palette).clicked() {
                                prefs.muted_topics.clear();
                                changed = true;
                                invalidate = true;
                            }
                        });
                    }

                    if !prefs.muted_channels.is_empty() {
                        ui.add_space(10.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Canais silenciados:")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            let mut restore_channel: Option<String> = None;
                            for muted in &prefs.muted_channels {
                                let btn = egui::Button::new(
                                    RichText::new(format!("+ {muted}"))
                                        .size(11.5)
                                        .color(palette.header_button_fg),
                                )
                                .fill(palette.header_button_bg)
                                .stroke(Stroke::new(1.0, palette.card_alt_stroke))
                                .corner_radius(CornerRadius::same(12))
                                .min_size(Vec2::new(0.0, 24.0));
                                if ui
                                    .add(btn)
                                    .on_hover_cursor(CursorIcon::PointingHand)
                                    .on_hover_text("Restaurar canal nas recomendações")
                                    .clicked()
                                {
                                    restore_channel = Some(muted.clone());
                                }
                            }
                            if let Some(ch) = restore_channel {
                                prefs.muted_channels.retain(|c| c != &ch);
                                changed = true;
                                invalidate = true;
                            }
                            if pill_choice_button(ui, "Restaurar canais", false, palette).clicked()
                            {
                                prefs.muted_channels.clear();
                                changed = true;
                                invalidate = true;
                            }
                        });
                    }
                });
                paint_row_divider(ui, palette);

                let history_count = self.history.data.entries().len();
                let history_desc = format!(
                    "{history_count} vídeo(s) salvos neste computador. Esta ação não pode ser desfeita."
                );
                settings_control_row(
                    ui,
                    palette,
                    "Limpar histórico local",
                    &history_desc,
                    220.0,
                    |ui| {
                        if self.settings.confirm_clear_history {
                            if pill_choice_button(ui, "Cancelar", false, palette).clicked() {
                                self.settings.confirm_clear_history = false;
                            }
                            if danger_outline_button(ui, "Confirmar limpeza", palette).clicked() {
                                self.clear_history();
                                self.settings.confirm_clear_history = false;
                            }
                        } else if pill_choice_button(ui, "Limpar histórico", false, palette)
                            .clicked()
                        {
                            self.settings.confirm_clear_history = true;
                        }
                    },
                );
            },
        );

        ui.add_space(16.0);
        let pt = self.ui_pt();
        let community_title = if pt {
            "APIs comunitárias (opt-in)"
        } else {
            "Community APIs (opt-in)"
        };
        let community_badge = if prefs.ryd_enabled || prefs.dearrow_enabled {
            if pt { "Ativo" } else { "Enabled" }
        } else if pt {
            "Desligado"
        } else {
            "Off"
        };
        let theme = self.settings.theme.value;
        settings_card(
            ui,
            palette,
            community_title,
            Some((
                community_badge,
                if prefs.ryd_enabled || prefs.dearrow_enabled {
                    palette.info_accent_fg
                } else {
                    palette.snippet_fg
                },
                palette.queue_next_bg,
            )),
            None,
            |ui| {
                let mut ryd = prefs.ryd_enabled;
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "Return YouTube Dislike",
                    if pt {
                        "Exibe contagem estimada de dislikes no player via returnyoutubedislikeapi.com. Nenhuma requisição é feita quando desativado."
                    } else {
                        "Shows estimated dislike counts in the watch view via returnyoutubedislikeapi.com. Zero requests are made when disabled."
                    },
                    &mut ryd,
                    None,
                    None,
                ) {
                    prefs.ryd_enabled = ryd;
                    changed = true;
                }
                paint_row_divider(ui, palette);
                let mut dearrow = prefs.dearrow_enabled;
                if settings_toggle_row(
                    ui,
                    palette,
                    theme,
                    "DeArrow (SponsorBlock)",
                    if pt {
                        "Sugere títulos e miniaturas comunitárias menos sensacionalistas via sponsor.ajay.app. Nenhuma requisição é feita quando desativado."
                    } else {
                        "Fetches community crowd-sourced titles via sponsor.ajay.app (with Original toggle). Zero requests are made when disabled."
                    },
                    &mut dearrow,
                    None,
                    None,
                ) {
                    prefs.dearrow_enabled = dearrow;
                    changed = true;
                }
            },
        );

        if changed {
            self.apply_app_preferences(prefs);
            if invalidate {
                self.invalidate_feed_cache();
            }
        }
        if let Some(notice) = &self.history.notice {
            ui.add_space(8.0);
            status_banner(ui, palette, notice, false);
        }
        if let Some(error) = &self.history.error {
            ui.add_space(8.0);
            status_banner(ui, palette, error, true);
        }
    }

    fn settings_tab_storage_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        let pt = self.ui_pt();
        if !self.settings.download_dir_input_synced {
            self.settings.download_dir_input = self
                .settings
                .prefs
                .value
                .download_dir_override
                .clone()
                .unwrap_or_else(|| {
                    downloads::DownloadManager::default_download_dir()
                        .to_string_lossy()
                        .into_owned()
                });
            self.settings.download_dir_input_synced = true;
        }
        let effective_download_dir = downloads::DownloadManager::effective_download_dir(
            self.settings.prefs.value.download_dir_override.as_deref(),
        );

        settings_card(
            ui,
            palette,
            if pt {
                "Pasta de downloads offline"
            } else {
                "Offline downloads folder"
            },
            None,
            None,
            |ui| {
                settings_row_frame(ui, |ui| {
                    ui.label(
                        RichText::new(if pt {
                            "Define onde o gerenciador de downloads salva arquivos MP4/M4A/Opus/MP3."
                        } else {
                            "Sets where the download manager saves offline MP4/M4A/Opus/MP3 files."
                        })
                        .size(12.5)
                        .color(palette.snippet_fg),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let edit_w = (ui.available_width() - 110.0).max(160.0);
                        ui.add_sized(
                            [edit_w, 30.0],
                            egui::TextEdit::singleline(&mut self.settings.download_dir_input)
                                .hint_text(if pt {
                                    "Caminho absoluto da pasta de downloads"
                                } else {
                                    "Absolute path to downloads folder"
                                }),
                        );
                        let picking = self.settings.download_picker_rx.is_some();
                        ui.add_enabled_ui(!picking, |ui| {
                            if pill_choice_button(
                                ui,
                                if picking {
                                    if pt { "Abrindo..." } else { "Opening..." }
                                } else if pt {
                                    "Procurar..."
                                } else {
                                    "Browse..."
                                },
                                false,
                                palette,
                            )
                            .clicked()
                            {
                                let initial =
                                    PathBuf::from(self.settings.download_dir_input.trim());
                                let (tx, rx) = mpsc::channel();
                                self.settings.download_picker_rx = Some(rx);
                                let ctx_clone = ctx.clone();
                                std::thread::spawn(move || {
                                    let res = settings::pick_folder_dialog(Some(&initial));
                                    let _ = tx.send(res);
                                    ctx_clone.request_repaint();
                                });
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if pill_choice_button(
                            ui,
                            if pt { "Aplicar pasta" } else { "Apply folder" },
                            true,
                            palette,
                        )
                        .clicked()
                        {
                            let candidate = self.settings.download_dir_input.trim().to_owned();
                            if candidate.is_empty() || !PathBuf::from(&candidate).is_absolute() {
                                self.downloads.notice = Some(if pt {
                                    "Informe um caminho absoluto válido ou clique em Restaurar padrão."
                                        .into()
                                } else {
                                    "Enter a valid absolute path or click Restore default.".into()
                                });
                            } else {
                                let mut prefs = self.settings.prefs.value.clone();
                                prefs.download_dir_override = Some(candidate);
                                self.apply_app_preferences(prefs);
                                if self.settings.prefs.error.is_none() {
                                    self.downloads.notice = Some(if pt {
                                        "Pasta de downloads atualizada.".into()
                                    } else {
                                        "Downloads folder updated.".into()
                                    });
                                }
                            }
                        }
                        if pill_choice_button(
                            ui,
                            if pt { "Restaurar padrão" } else { "Restore default" },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            let mut prefs = self.settings.prefs.value.clone();
                            prefs.download_dir_override = None;
                            self.apply_app_preferences(prefs);
                            self.settings.download_dir_input_synced = false;
                            if self.settings.prefs.error.is_none() {
                                self.downloads.notice = Some(if pt {
                                    "Pasta de downloads restaurada para o padrão.".into()
                                } else {
                                    "Downloads folder restored to default.".into()
                                });
                            }
                        }
                        if pill_choice_button(
                            ui,
                            if pt { "Abrir no Explorer" } else { "Open in Explorer" },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            let _ = std::fs::create_dir_all(&effective_download_dir);
                            if let Err(err) = settings::open_in_explorer(&effective_download_dir) {
                                self.downloads.notice = Some(err);
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "{}: {}",
                            if pt {
                                "Pasta efetiva"
                            } else {
                                "Effective folder"
                            },
                            effective_download_dir.display()
                        ))
                        .size(11.5)
                        .color(palette.snippet_fg),
                    );
                    if let Some(notice) = &self.downloads.notice {
                        ui.add_space(6.0);
                        status_banner(ui, palette, notice, false);
                    }
                });
            },
        );

        ui.add_space(16.0);
        let ytdlp_busy = self.settings.ytdlp_rx.is_some();
        let ytdlp_badge_info = self.settings.ytdlp_check.as_ref().map(|info| {
            let label = if info.update_available {
                format!(
                    "{} -> {}",
                    info.current_version.as_deref().unwrap_or("?"),
                    info.latest_version
                )
            } else {
                info.latest_version.clone()
            };
            (label, info.update_available)
        });
        settings_card(
            ui,
            palette,
            if pt {
                "Atualizador do yt-dlp"
            } else {
                "yt-dlp Updater"
            },
            ytdlp_badge_info.as_ref().map(|(lbl, upd)| {
                (
                    lbl.as_str(),
                    if *upd {
                        palette.info_accent_fg
                    } else {
                        palette.snippet_fg
                    },
                    palette.queue_next_bg,
                )
            }),
            None,
            |ui| {
                settings_control_row(
                    ui,
                    palette,
                    if pt {
                        "Binário yt-dlp (GitHub Releases)"
                    } else {
                        "yt-dlp binary (GitHub Releases)"
                    },
                    if pt {
                        "Verifica a versão mais recente no repositório oficial do yt-dlp e atualiza o executável localmente."
                    } else {
                        "Checks the latest official yt-dlp release on GitHub and atomically updates the local executable."
                    },
                    260.0,
                    |ui| {
                        ui.add_enabled_ui(!ytdlp_busy, |ui| {
                            if pill_choice_button(
                                ui,
                                if ytdlp_busy {
                                    if pt { "Consultando..." } else { "Checking..." }
                                } else if pt {
                                    "Verificar atualizações"
                                } else {
                                    "Check updates"
                                },
                                false,
                                palette,
                            )
                            .clicked()
                            {
                                self.start_ytdlp_check(ctx);
                            }
                            if let Some(info) = self.settings.ytdlp_check.clone()
                                && info.update_available
                                && pill_choice_button(
                                    ui,
                                    if pt {
                                        "Baixar / Atualizar"
                                    } else {
                                        "Download / Update"
                                    },
                                    true,
                                    palette,
                                )
                                .clicked()
                            {
                                self.start_ytdlp_update(
                                    info.download_url,
                                    info.install_path,
                                    info.latest_version,
                                    ctx,
                                );
                            }
                        });
                    },
                );
                if let Some((ok, status)) = &self.settings.ytdlp_status {
                    ui.add_space(6.0);
                    status_banner(ui, palette, status, !*ok);
                }
            },
        );

        ui.add_space(16.0);
        if !self.settings.cache_dir_input_synced {
            self.settings.cache_dir_input = self
                .settings
                .prefs
                .value
                .cache_dir_override
                .clone()
                .unwrap_or_else(|| {
                    settings::default_cache_dir()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
            self.settings.cache_dir_input_synced = true;
        }

        let effective_base =
            settings::effective_cache_dir(self.settings.prefs.value.cache_dir_override.as_deref())
                .ok();
        let ytdlp_subdir = effective_base.as_deref().map(settings::yt_dlp_cache_subdir);

        if let Some(target_dir) = ytdlp_subdir.as_deref() {
            let needs_measure = match &self.settings.cache_disk_usage {
                Some((cached_dir, _, measured_at)) => {
                    cached_dir != target_dir || measured_at.elapsed() > Duration::from_secs(15)
                }
                None => true,
            };
            if needs_measure {
                let usage = settings::measure_dir_usage(target_dir);
                self.settings.cache_disk_usage =
                    Some((target_dir.to_path_buf(), usage, Instant::now()));
            }
        }

        settings_card(ui, palette, "Local da pasta de cache", None, None, |ui| {
            settings_row_frame(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Define onde o yt-dlp armazena caches de extração e metadados temporários.",
                    )
                    .size(12.5)
                    .color(palette.snippet_fg),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let edit_w = (ui.available_width() - 110.0).max(160.0);
                    ui.add_sized(
                        [edit_w, 30.0],
                        egui::TextEdit::singleline(&mut self.settings.cache_dir_input)
                            .hint_text("Caminho absoluto da pasta de cache"),
                    );
                    let picking = self.settings.cache_picker_rx.is_some();
                    ui.add_enabled_ui(!picking, |ui| {
                        if pill_choice_button(
                            ui,
                            if picking { "Abrindo..." } else { "Procurar..." },
                            false,
                            palette,
                        )
                        .clicked()
                        {
                            let initial = PathBuf::from(self.settings.cache_dir_input.trim());
                            let (tx, rx) = mpsc::channel();
                            self.settings.cache_picker_rx = Some(rx);
                            let ctx_clone = ctx.clone();
                            std::thread::spawn(move || {
                                let res = settings::pick_folder_dialog(Some(&initial));
                                let _ = tx.send(res);
                                ctx_clone.request_repaint();
                            });
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if pill_choice_button(ui, "Aplicar pasta", true, palette).clicked() {
                        let candidate = self.settings.cache_dir_input.trim().to_owned();
                        if candidate.is_empty() {
                            self.settings.cache_status = Some(
                                "Informe um caminho absoluto ou clique em Restaurar padrão.".into(),
                            );
                        } else {
                            let mut prefs = self.settings.prefs.value.clone();
                            prefs.cache_dir_override = Some(candidate);
                            self.apply_app_preferences(prefs);
                            if self.settings.prefs.error.is_none() {
                                self.settings.cache_status =
                                    Some("Diretório de cache atualizado.".into());
                                self.settings.cache_disk_usage = None;
                            }
                        }
                    }
                    if pill_choice_button(ui, "Restaurar padrão", false, palette).clicked() {
                        let mut prefs = self.settings.prefs.value.clone();
                        prefs.cache_dir_override = None;
                        self.apply_app_preferences(prefs);
                        self.settings.cache_dir_input_synced = false;
                        self.settings.cache_disk_usage = None;
                        if self.settings.prefs.error.is_none() {
                            self.settings.cache_status =
                                Some("Diretório de cache restaurado para o padrão.".into());
                        }
                    }
                    if let Some(dir) = ytdlp_subdir.as_deref().or(effective_base.as_deref())
                        && pill_choice_button(ui, "Abrir no Explorer", false, palette).clicked()
                        && let Err(err) = settings::open_in_explorer(dir)
                    {
                        self.settings.cache_status = Some(err);
                    }
                });
                if let Some(subdir) = &ytdlp_subdir {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("Pasta efetiva do yt-dlp: {}", subdir.display()))
                            .size(11.5)
                            .color(palette.snippet_fg),
                    );
                }
            });
        });

        ui.add_space(16.0);
        let usage_text = self
            .settings
            .cache_disk_usage
            .as_ref()
            .map(|(_, u, _)| format!("{} ({} arquivos)", format_bytes_pt(u.bytes), u.files))
            .unwrap_or_else(|| "Indisponível".into());
        settings_card(
            ui,
            palette,
            "Uso de espaço e limpeza",
            Some((&usage_text, palette.info_accent_fg, palette.queue_next_bg)),
            None,
            |ui| {
                settings_control_row(
                    ui,
                    palette,
                    "Cache em disco do yt-dlp",
                    "Remove apenas os arquivos dentro da subpasta yt-dlp configurada.",
                    190.0,
                    |ui| {
                        if pill_choice_button(ui, "Recalcular", false, palette).clicked() {
                            self.settings.cache_disk_usage = None;
                        }
                        if pill_choice_button(ui, "Limpar cache do yt-dlp", false, palette)
                            .clicked()
                        {
                            if let Some(base) = &effective_base {
                                match settings::clear_yt_dlp_cache_dir(base) {
                                    Ok(removed) => {
                                        self.settings.cache_disk_usage = None;
                                        self.settings.cache_status = Some(format!(
                                            "Cache do yt-dlp limpo ({removed} arquivo(s) removido(s))."
                                        ));
                                    }
                                    Err(err) => {
                                        self.settings.cache_status =
                                            Some(format!("Falha ao limpar cache do yt-dlp: {err}"));
                                    }
                                }
                            } else {
                                self.settings.cache_status = Some(
                                    "Não foi possível determinar a pasta de cache efetiva.".into(),
                                );
                            }
                        }
                    },
                );
                paint_row_divider(ui, palette);

                let mem_desc = format!(
                    "Em memória agora: {} vídeos no feed, {} metadados carregados, {} itens na fila.",
                    self.feed.videos.len(),
                    self.video_metadata.len(),
                    self.queue.data.len()
                );
                settings_control_row(
                    ui,
                    palette,
                    "Caches visuais e de sessão em memória",
                    &mem_desc,
                    190.0,
                    |ui| {
                        if pill_choice_button(ui, "Liberar caches visuais", false, palette)
                            .clicked()
                        {
                            self.clear_visual_caches(ctx);
                        }
                    },
                );
            },
        );

        if let Some(status) = &self.settings.cache_status {
            ui.add_space(8.0);
            status_banner(ui, palette, status, false);
        }
        if let Some(notice) = &self.settings.buffer.notice {
            ui.add_space(8.0);
            status_banner(ui, palette, notice, false);
        }
    }

    fn settings_tab_backup_diagnostics_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        palette: ThemePalette,
    ) {
        settings_card(ui, palette, "Backup de dados locais", None, None, |ui| {
            settings_row_frame(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Inclui biblioteca, marcadores, fila, histórico, preferências e SponsorBlock. Sessões OAuth, tokens e cookies ficam excluídos.",
                    )
                    .size(12.5)
                    .color(palette.snippet_fg),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Arquivo:").size(13.0));
                    let picking = self.settings.backup_picker_rx.is_some();
                    let edit_w = (ui.available_width() - 255.0).max(140.0);
                    ui.add_sized(
                        [edit_w, 30.0],
                        egui::TextEdit::singleline(&mut self.backup.path_text)
                            .hint_text("Caminho do arquivo .json"),
                    );
                    ui.add_enabled_ui(!picking, |ui| {
                        if pill_choice_button(ui, "Escolher...", false, palette).clicked() {
                            let initial = PathBuf::from(self.backup.path_text.trim());
                            let (tx, rx) = mpsc::channel();
                            self.settings.backup_picker_rx = Some(rx);
                            let ctx_clone = ctx.clone();
                            std::thread::spawn(move || {
                                let res = settings::pick_json_file_dialog(
                                    false,
                                    "Selecionar arquivo de backup",
                                    Some(&initial),
                                );
                                let _ = tx.send((false, res));
                                ctx_clone.request_repaint();
                            });
                        }
                        if pill_choice_button(ui, "Salvar em...", false, palette).clicked() {
                            let initial = PathBuf::from(self.backup.path_text.trim());
                            let (tx, rx) = mpsc::channel();
                            self.settings.backup_picker_rx = Some(rx);
                            let ctx_clone = ctx.clone();
                            std::thread::spawn(move || {
                                let res = settings::pick_json_file_dialog(
                                    true,
                                    "Salvar backup do FastTube",
                                    Some(&initial),
                                );
                                let _ = tx.send((true, res));
                                ctx_clone.request_repaint();
                            });
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if pill_choice_button(ui, "Exportar", true, palette).clicked() {
                        self.export_user_data();
                    }
                    if pill_choice_button(ui, "Analisar importação", false, palette)
                        .on_hover_text(
                            "Valida o arquivo e mostra uma prévia antes de alterar qualquer dado.",
                        )
                        .clicked()
                    {
                        self.analyze_import();
                    }
                    let backup_path = PathBuf::from(self.backup.path_text.trim());
                    if !self.backup.path_text.trim().is_empty()
                        && pill_choice_button(ui, "Abrir pasta", false, palette).clicked()
                        && let Err(err) = settings::open_in_explorer(&backup_path)
                    {
                        self.backup.status = Some(err);
                    }
                });
                if let Some(status) = &self.backup.status {
                    ui.add_space(8.0);
                    ui.label(RichText::new(status).size(12.5).color(palette.snippet_fg));
                }
            });
        });

        ui.add_space(16.0);
        if self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
        let diag_display =
            format_diagnostics_display(&self.playback.diagnostics, self.selected.is_some());
        let (status_badge_text, status_color, status_bg) = if self.selected.is_none() {
            (
                "Inativo",
                palette.diag_inactive_fg,
                palette.header_button_bg,
            )
        } else if self
            .playback
            .diagnostics
            .effective_decoding
            .is_hardware_active()
        {
            ("Hardware ativo", palette.diag_hw_fg, palette.notice_bg)
        } else if self.playback.diagnostics.effective_decoding.is_software() {
            ("Software", palette.diag_sw_fg, palette.error_bg)
        } else {
            (
                "Desconhecido",
                palette.diag_unknown_fg,
                palette.queue_next_bg,
            )
        };

        settings_card(
            ui,
            palette,
            "Diagnóstico de decodificação de vídeo (libmpv)",
            Some((status_badge_text, status_color, status_bg)),
            None,
            |ui| {
                settings_row_frame(ui, |ui| {
                    ui.label(
                        RichText::new(&diag_display.status_line)
                            .size(13.0)
                            .strong()
                            .color(status_color),
                    );
                    ui.add_space(8.0);
                    let meta_val = diag_display.metadata_line.clone().unwrap_or_else(|| {
                        if self.selected.is_some() {
                            "Metadados da faixa (video-codec / tamanho): indisponíveis no momento"
                                .to_owned()
                        } else {
                            "Metadados da faixa (video-codec / tamanho): inativo (sem vídeo aberto)"
                                .to_owned()
                        }
                    });
                    egui::Grid::new("settings_hwdec_diag_grid")
                        .num_columns(2)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Solicitado")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            ui.label(RichText::new(&diag_display.configured_line).size(12.0));
                            ui.end_row();

                            ui.label(
                                RichText::new("Em execução")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            ui.label(RichText::new(&diag_display.effective_line).size(12.0));
                            ui.end_row();

                            ui.label(
                                RichText::new("Faixa / codec")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            ui.label(RichText::new(&meta_val).size(12.0));
                            ui.end_row();

                            ui.label(
                                RichText::new("Resumo")
                                    .size(12.0)
                                    .strong()
                                    .color(palette.snippet_fg),
                            );
                            ui.label(
                                RichText::new(&diag_display.summary_line)
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                            );
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(diag_display.note_line)
                            .size(11.5)
                            .color(palette.snippet_fg),
                    );
                    if self.has_pending_hwdec_restart() && self.selected.is_some() {
                        ui.add_space(8.0);
                        if pill_choice_button(
                            ui,
                            "Aplicar decodificação pendente e recarregar vídeo",
                            true,
                            palette,
                        )
                        .clicked()
                        {
                            self.settings.pending_reload_video = true;
                        }
                    }
                });
            },
        );

        if !self.message.is_empty() {
            ui.add_space(16.0);
            let kind = diagnostics::classify_failure(&self.message);
            let guidance =
                diagnostics::guidance(kind, self.auth.authenticated, self.selected.is_some());
            let category = match kind {
                diagnostics::FailureKind::Login => "Login",
                diagnostics::FailureKind::Quota => "Cota da API",
                diagnostics::FailureKind::Connection => "Conexão",
                diagnostics::FailureKind::Playback => "Reprodução/dependências",
                diagnostics::FailureKind::Unknown => "Falha não classificada",
            };
            settings_card(
                ui,
                palette,
                &format!("Diagnóstico da sessão: {category}"),
                None,
                None,
                |ui| {
                    settings_row_frame(ui, |ui| {
                        ui.label(RichText::new(guidance.summary).size(12.5));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Próximos passos").size(12.5).strong());
                        for step in guidance.steps {
                            ui.label(
                                RichText::new(format!("• {step}"))
                                    .size(12.0)
                                    .color(palette.snippet_fg),
                            );
                        }
                        ui.add_space(8.0);
                        if pill_choice_button(ui, "Copiar relatório redigido", false, palette)
                            .clicked()
                        {
                            let report = diagnostics::safe_report(
                                kind,
                                self.auth.authenticated,
                                self.selected.is_some(),
                            );
                            self.message = match copy_text_to_clipboard(ui.ctx(), &report) {
                                Ok(()) => {
                                    "Relatório de diagnóstico copiado para a área de transferência."
                                        .into()
                                }
                                Err(error) => error,
                            };
                        }
                    });
                },
            );
        }

        let files: Vec<(&str, Option<PathBuf>)> = vec![
            (
                "Preferências gerais",
                self.settings
                    .prefs
                    .path
                    .clone()
                    .or_else(|| settings::app_preferences_path().ok()),
            ),
            (
                "Buffer do player",
                self.settings
                    .buffer
                    .path
                    .clone()
                    .or_else(|| settings::settings_path().ok()),
            ),
            (
                "Reprodução padrão",
                self.settings
                    .playback
                    .path
                    .clone()
                    .or_else(|| settings::playback_path().ok()),
            ),
            (
                "Tema da interface",
                self.settings
                    .theme
                    .path
                    .clone()
                    .or_else(|| settings::theme_path().ok()),
            ),
            (
                "Histórico local",
                self.history
                    .path
                    .clone()
                    .or_else(|| history::history_path().ok()),
            ),
            (
                "Biblioteca local",
                self.library.path.clone().or_else(|| {
                    settings::settings_path()
                        .ok()
                        .map(|p| p.with_file_name("library.json"))
                }),
            ),
            (
                "Fila de reprodução",
                self.queue.path.clone().or_else(|| {
                    history::history_path()
                        .ok()
                        .map(|p| p.with_file_name("queue.json"))
                }),
            ),
            (
                "Filtros do feed",
                self.feed_preferences.path.clone().or_else(|| {
                    history::history_path()
                        .ok()
                        .map(|p| p.with_file_name("feed-preferences.json"))
                }),
            ),
            (
                "Canais seguidos (locais)",
                self.followed_channels
                    .path
                    .clone()
                    .or_else(|| followed_channels::path().ok()),
            ),
            ("Cliente OAuth", settings::oauth_client_path().ok()),
        ];

        let resolved_files: Vec<(&str, PathBuf)> = files
            .into_iter()
            .filter_map(|(label, maybe_path)| maybe_path.map(|p| (label, p)))
            .collect();

        if !resolved_files.is_empty() {
            ui.add_space(16.0);
            settings_card(ui, palette, "Arquivos de persistência", None, None, |ui| {
                for (idx, (label, path)) in resolved_files.iter().enumerate() {
                    if idx > 0 {
                        paint_row_divider(ui, palette);
                    }
                    let exists = path.is_file();
                    let dot_color = if exists {
                        palette.watched_status_fg
                    } else {
                        palette.snippet_fg.gamma_multiply(0.5)
                    };
                    settings_row_frame(ui, |ui| {
                        ui.horizontal(|ui| {
                            let info_w = (ui.available_width() - 145.0).max(150.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(info_w, 0.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    ui.horizontal(|ui| {
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(10.0, 14.0),
                                            Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            3.5,
                                            dot_color,
                                        );
                                        ui.label(
                                            RichText::new(*label)
                                                .size(12.5)
                                                .strong()
                                                .color(palette.channel_fg),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(path.display().to_string())
                                            .size(11.0)
                                            .color(palette.snippet_fg),
                                    );
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if pill_choice_button(ui, "Copiar", false, palette).clicked() {
                                        let path_text = path.display().to_string();
                                        self.settings.storage_action_status =
                                            match copy_text_to_clipboard(ui.ctx(), &path_text) {
                                                Ok(()) => Some((
                                                    true,
                                                    format!("Caminho copiado: {path_text}"),
                                                )),
                                                Err(err) => Some((false, err)),
                                            };
                                    }
                                    if pill_choice_button(ui, "Abrir", false, palette).clicked()
                                        && let Err(err) = settings::open_in_explorer(path)
                                    {
                                        self.settings.storage_action_status = Some((false, err));
                                    }
                                },
                            );
                        });
                    });
                }
            });
        }
        if let Some((ok, status)) = &self.settings.storage_action_status {
            ui.add_space(8.0);
            status_banner(ui, palette, status, !ok);
        }
    }

    pub(crate) fn backup_import_modal_ui(&mut self, ctx: &egui::Context) {
        if !self.backup.confirm_import {
            return;
        }
        let palette = self.palette();
        let preview = self.backup.preview.clone();
        egui::Window::new("Prévia da importação")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .default_width(480.0)
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .fill(palette.card_bg)
                    .stroke(Stroke::new(1.0, palette.card_alt_stroke))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ctx, |ui| {
                let Some(preview) = preview.as_ref() else {
                    ui.label(
                        RichText::new("A prévia não está disponível.")
                            .size(13.0)
                            .color(palette.snippet_fg),
                    );
                    ui.add_space(10.0);
                    if pill_choice_button(ui, "Fechar", false, palette).clicked() {
                        self.backup.confirm_import = false;
                    }
                    return;
                };
                ui.label(
                    RichText::new(
                        preview
                            .path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("backup selecionado"),
                    )
                    .size(15.0)
                    .strong()
                    .color(palette.channel_fg),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    settings_pill_badge(
                        ui,
                        &format!("{} na biblioteca", preview.library_items),
                        palette.info_accent_fg,
                        palette.queue_next_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} favoritos", preview.favorites),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} Assistir depois", preview.watch_later),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} coleções", preview.collections),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} marcadores", preview.bookmarks),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} na fila", preview.queue_items),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} no histórico", preview.history_entries),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                    settings_pill_badge(
                        ui,
                        &format!("{} canais seguidos", preview.followed_channels),
                        palette.channel_fg,
                        palette.header_button_bg,
                    );
                });

                ui.add_space(12.0);
                paint_row_divider(ui, palette);
                ui.add_space(10.0);

                ui.label(
                    RichText::new("Como aplicar")
                        .size(13.5)
                        .strong()
                        .color(palette.channel_fg),
                );
                ui.add_space(6.0);
                if clickable_radio_card(
                    ui,
                    palette,
                    self.backup.import_mode == BackupImportMode::Merge,
                    "Mesclar com os dados locais (recomendado)",
                    "Adiciona itens ausentes sem apagar os atuais. A biblioteca local prevalece em conflitos, o histórico mantém o maior progresso e a fila respeita o limite de 50.",
                    None,
                ) {
                    self.backup.import_mode = BackupImportMode::Merge;
                }
                ui.add_space(6.0);
                if clickable_radio_card(
                    ui,
                    palette,
                    self.backup.import_mode == BackupImportMode::Replace,
                    "Substituir todos os dados",
                    "Remove a visão atual de biblioteca, fila, histórico e preferências e aplica somente o backup.",
                    None,
                ) {
                    self.backup.import_mode = BackupImportMode::Replace;
                }

                ui.add_space(10.0);
                let preview_note = match self.backup.import_mode {
                    BackupImportMode::Merge => {
                        let mut msg = format!(
                            "Prévia: {} itens novos, {} conflitos mantidos e {} excedentes do limite.",
                            preview.merge_added, preview.merge_conflicts, preview.merge_skipped
                        );
                        if preview.history_progress_updates > 0 {
                            msg.push_str(&format!(
                                " {} entradas do histórico terão progresso atualizado.",
                                preview.history_progress_updates
                            ));
                        }
                        msg
                    }
                    BackupImportMode::Replace => {
                        "As preferências do arquivo também substituirão tema, velocidade, qualidade e buffer."
                            .to_owned()
                    }
                };
                status_banner(ui, palette, &preview_note, false);
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Antes da gravação, o FastTube cria um ponto de restauração. Se a operação falhar ou for interrompida, os arquivos anteriores serão recuperados.",
                    )
                    .size(11.5)
                    .color(palette.snippet_fg),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if pill_choice_button(ui, "Aplicar backup", true, palette).clicked() {
                        self.backup.confirm_import = false;
                        self.import_user_data();
                    }
                    if pill_choice_button(ui, "Cancelar", false, palette).clicked() {
                        self.backup.confirm_import = false;
                        self.backup.preview = None;
                        self.backup.pending_import = None;
                        self.backup.status =
                            Some("Importação cancelada; dados atuais preservados.".into());
                    }
                });
            });
    }

    #[cfg(test)]
    pub(crate) fn settings_panel_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.settings_page_ui(ui, &ctx);
    }

    #[cfg(test)]
    pub(crate) fn settings_window_ui(&mut self, ctx: &egui::Context) {
        self.sync_theme_visuals(ctx);
        if !self.ui.settings_open {
            self.sync_player_window_visibility();
            return;
        }
        let mut open = self.ui.settings_open;
        let window_width = (ctx.content_rect().width() - 32.0).clamp(320.0, 960.0);
        egui::Window::new("⚙ Configurações do FastTube")
            .open(&mut open)
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .default_width(window_width)
            .show(ctx, |ui| {
                self.settings_page_ui(ui, ctx);
            });
        self.ui.settings_open = open;
        if !self.ui.settings_open && self.selected.is_none() {
            self.reset_decoding_diagnostics();
        }
        self.sync_player_window_visibility();
    }
}
