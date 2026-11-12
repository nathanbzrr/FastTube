//! Controles sobrepostos ao vídeo (barra inferior translúcida).
//!
//! Este módulo é independente do resto da interface: recebe a geometria do
//! vídeo e a telemetria atual do player e devolve a ação escolhida pelo
//! usuário. A integração com o `FastTube` e com o libmpv fica em `main.rs`.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Vec2};

/// Altura visual da barra de controles sobreposta, em pontos.
pub const OVERLAY_HEIGHT: f32 = 44.0;

/// Tempo sem atividade de ponteiro após o qual a barra se esconde durante a
/// reprodução.
pub const OVERLAY_HIDE_DELAY_SECS: f64 = 2.5;

/// Recuo padrão lateral e inferior da barra dentro da área do vídeo.
const BAR_INSET: f32 = 8.0;

/// Largura mínima recomendável da barra quando o vídeo permite.
const MIN_USABLE_BAR_WIDTH: f32 = 240.0;

/// Altura da faixa clicável de progresso no topo da barra.
const PROGRESS_CLICK_STRIP_HEIGHT: f32 = 8.0;

/// Altura visual da linha de progresso no topo da barra.
const PROGRESS_LINE_HEIGHT: f32 = 3.0;

/// Estado do player necessário para desenhar a barra.
#[derive(Clone, Debug)]
pub struct OverlayTelemetry {
    pub paused: bool,
    pub ended: bool,
    pub muted: bool,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub volume_percent: f32,
    pub has_queued: bool,
}

impl Default for OverlayTelemetry {
    fn default() -> Self {
        Self {
            paused: true,
            ended: false,
            muted: false,
            position_secs: 0.0,
            duration_secs: None,
            volume_percent: 100.0,
            has_queued: false,
        }
    }
}

/// Cores fornecidas pelo tema atual.
#[derive(Clone, Copy, Debug)]
pub struct OverlayStyle {
    pub panel: Color32,
    pub icon: Color32,
    pub text: Color32,
    pub accent: Color32,
}

/// Ação pedida por um controle da barra.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OverlayAction {
    TogglePlay,
    SeekBack10,
    SeekForward10,
    SeekTo(f64),
    ToggleMute,
    SetVolume(f32),
    Next,
    ToggleSettings,
    ToggleTheater,
    ToggleFullscreen,
    Close,
}

/// Decide se a barra sobreposta deve aparecer neste quadro.
pub fn overlay_visible(
    pointer_over_video: bool,
    paused: bool,
    ended: bool,
    seconds_since_activity: f64,
) -> bool {
    pointer_over_video || paused || ended || seconds_since_activity < OVERLAY_HIDE_DELAY_SECS
}

/// Formata segundos em `M:SS` ou `H:MM:SS`.
fn format_time(seconds: f64) -> String {
    let clamped = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    let hours = clamped / 3600;
    let minutes = (clamped % 3600) / 60;
    let secs = clamped % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// Formata o par `decorrido / total` exibido na barra (`--:--` quando sem duração).
fn format_elapsed_total(position_secs: f64, duration_secs: Option<f64>) -> String {
    let elapsed = format_time(position_secs);
    let total = match duration_secs {
        Some(d) if d.is_finite() && d > 0.0 => format_time(d),
        _ => "--:--".to_owned(),
    };
    format!("{elapsed} / {total}")
}

/// Calcula o retângulo da barra dentro de `video_rect`, nunca ultrapassando seus limites.
fn compute_bar_rect(video_rect: Rect) -> Option<Rect> {
    let vw = video_rect.width();
    let vh = video_rect.height();
    if !vw.is_finite() || !vh.is_finite() || vw <= 1.0 || vh <= 1.0 {
        return None;
    }

    // Ajusta o recuo horizontal para preservar ao menos ~240 px quando possível,
    // sem nunca sair de `video_rect`.
    let horiz_inset = if vw >= MIN_USABLE_BAR_WIDTH + 2.0 * BAR_INSET {
        BAR_INSET
    } else if vw > MIN_USABLE_BAR_WIDTH {
        (vw - MIN_USABLE_BAR_WIDTH) * 0.5
    } else {
        0.0
    };

    let vert_inset = if vh >= OVERLAY_HEIGHT + BAR_INSET {
        BAR_INSET
    } else {
        0.0
    };

    let height = OVERLAY_HEIGHT.min((vh - vert_inset).max(1.0));
    let bottom = (video_rect.max.y - vert_inset).clamp(video_rect.min.y, video_rect.max.y);
    let top = (bottom - height).max(video_rect.min.y);
    let left = (video_rect.min.x + horiz_inset).clamp(video_rect.min.x, video_rect.max.x);
    let right = (video_rect.max.x - horiz_inset).clamp(left, video_rect.max.x);

    let bar_rect =
        Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom)).intersect(video_rect);

    if bar_rect.width() <= 0.0 || bar_rect.height() <= 0.0 {
        None
    } else {
        Some(bar_rect)
    }
}

/// Converte uma fração `[0, 1]` e a duração opcional em uma ação `SeekTo`.
fn action_for_progress_click(fraction: f64, duration_secs: Option<f64>) -> Option<OverlayAction> {
    let duration = duration_secs?;
    if !duration.is_finite() || duration <= 0.0 || !fraction.is_finite() {
        return None;
    }
    let clamped = fraction.clamp(0.0, 1.0);
    Some(OverlayAction::SeekTo(clamped * duration))
}

/// Desenha um botão compacto da barra com cores do `OverlayStyle`.
fn bar_button(
    ui: &mut egui::Ui,
    label: &str,
    style: &OverlayStyle,
    accent_hover: bool,
    enabled: bool,
) -> egui::Response {
    let text_color = if enabled {
        style.icon
    } else {
        style.icon.gamma_multiply(0.4)
    };
    let button = egui::Button::new(RichText::new(label).color(text_color).size(13.0))
        .frame(true)
        .corner_radius(CornerRadius::same(4))
        .min_size(Vec2::new(24.0, 24.0));
    let response = ui.add_enabled(enabled, button);
    if enabled && accent_hover && response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CornerRadius::same(4),
            Stroke::new(1.0, style.accent),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// Desenha a barra sobreposta no rodapé de `video_rect`.
///
/// Retorna a ação acionada (se houver) e se o ponteiro está sobre a barra.
pub fn show(
    ui: &mut egui::Ui,
    video_rect: Rect,
    telemetry: &OverlayTelemetry,
    style: &OverlayStyle,
    visible: bool,
) -> (Option<OverlayAction>, bool) {
    if !visible {
        return (None, false);
    }

    let Some(bar_rect) = compute_bar_rect(video_rect) else {
        return (None, false);
    };

    let pointer_inside = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|pos| bar_rect.contains(pos));

    let prev_clip = ui.clip_rect();
    let clipped_bar = bar_rect.intersect(prev_clip);
    if clipped_bar.width() <= 0.0 || clipped_bar.height() <= 0.0 {
        return (None, pointer_inside);
    }

    let mut action_play: Option<OverlayAction> = None;
    let mut action_seek_btn: Option<OverlayAction> = None;
    let mut action_progress: Option<OverlayAction> = None;
    let mut action_mute: Option<OverlayAction> = None;
    let mut action_volume: Option<OverlayAction> = None;
    let mut action_next: Option<OverlayAction> = None;
    let mut action_settings: Option<OverlayAction> = None;
    let mut action_theater: Option<OverlayAction> = None;
    let mut action_fullscreen: Option<OverlayAction> = None;
    let mut action_close: Option<OverlayAction> = None;

    ui.scope_builder(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
        ui.set_clip_rect(clipped_bar);

        // Fundo translúcido arredondado desenhado antes dos controles.
        ui.painter().rect_filled(bar_rect, 8.0, style.panel);

        // Linha fina de progresso (3 px) no topo da barra e faixa clicável (8 px).
        let strip_height = PROGRESS_CLICK_STRIP_HEIGHT.min(bar_rect.height());
        let strip_rect = Rect::from_min_max(
            bar_rect.min,
            Pos2::new(bar_rect.max.x, bar_rect.min.y + strip_height),
        );
        let line_height = PROGRESS_LINE_HEIGHT.min(bar_rect.height());
        let line_rect = Rect::from_min_max(
            bar_rect.min,
            Pos2::new(bar_rect.max.x, bar_rect.min.y + line_height),
        );

        let track_color = style.text.gamma_multiply(0.22);
        ui.painter().rect_filled(line_rect, 2.0, track_color);

        if let Some(duration) = telemetry.duration_secs
            && duration.is_finite()
            && duration > 0.0
        {
            let ratio = (telemetry.position_secs / duration).clamp(0.0, 1.0) as f32;
            let fill_width = line_rect.width() * ratio;
            if fill_width > 0.0 {
                let fill_rect = Rect::from_min_max(
                    line_rect.min,
                    Pos2::new(line_rect.min.x + fill_width, line_rect.max.y),
                );
                ui.painter().rect_filled(fill_rect, 2.0, style.accent);
            }
        }

        let progress_id = ui.make_persistent_id("fasttube_overlay_progress_strip");
        let progress_resp = ui.interact(strip_rect, progress_id, Sense::click());
        if progress_resp.clicked()
            && strip_rect.width() > 0.0
            && let Some(pos) = progress_resp
                .interact_pointer_pos()
                .or_else(|| ui.input(|i| i.pointer.interact_pos()))
        {
            let fraction = f64::from((pos.x - strip_rect.min.x) / strip_rect.width());
            action_progress = action_for_progress_click(fraction, telemetry.duration_secs);
        }

        // Região interna dos controles, mantendo recuo horizontal e abaixo da linha de progresso.
        let controls_top = (bar_rect.min.y + PROGRESS_LINE_HEIGHT + 2.0).min(bar_rect.max.y);
        let inner_rect = Rect::from_min_max(
            Pos2::new((bar_rect.min.x + 8.0).min(bar_rect.max.x), controls_top),
            Pos2::new((bar_rect.max.x - 8.0).max(bar_rect.min.x), bar_rect.max.y),
        )
        .intersect(bar_rect);

        if inner_rect.width() > 0.0 && inner_rect.height() > 0.0 {
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
                ui.set_clip_rect(clipped_bar.intersect(inner_rect));
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                ui.spacing_mut().button_padding = Vec2::new(5.0, 2.0);
                ui.visuals_mut().override_text_color = Some(style.text);
                ui.visuals_mut().selection.bg_fill = style.accent;

                ui.horizontal_centered(|ui| {
                    let play_glyph = if telemetry.paused || telemetry.ended {
                        "▶"
                    } else {
                        "❚❚"
                    };
                    if bar_button(ui, play_glyph, style, true, true).clicked() {
                        action_play = Some(OverlayAction::TogglePlay);
                    }

                    if bar_button(ui, "−10s", style, false, true).clicked() {
                        action_seek_btn = Some(OverlayAction::SeekBack10);
                    }

                    if bar_button(ui, "+10s", style, false, true).clicked()
                        && action_seek_btn.is_none()
                    {
                        action_seek_btn = Some(OverlayAction::SeekForward10);
                    }

                    let time_label =
                        format_elapsed_total(telemetry.position_secs, telemetry.duration_secs);
                    ui.label(
                        RichText::new(time_label)
                            .color(style.text)
                            .size(11.5)
                            .monospace(),
                    );

                    let mute_label = if telemetry.muted { "Mudo" } else { "Som" };
                    if bar_button(ui, mute_label, style, false, true).clicked() {
                        action_mute = Some(OverlayAction::ToggleMute);
                    }

                    let mut volume = telemetry.volume_percent.clamp(0.0, 100.0);
                    ui.spacing_mut().slider_width = 76.0;
                    let slider = egui::Slider::new(&mut volume, 0.0..=100.0).show_value(false);
                    if ui.add(slider).changed() {
                        action_volume = Some(OverlayAction::SetVolume(volume));
                    }

                    if bar_button(ui, "⏭", style, false, telemetry.has_queued).clicked() {
                        action_next = Some(OverlayAction::Next);
                    }

                    if bar_button(ui, "⚙", style, false, true).clicked() {
                        action_settings = Some(OverlayAction::ToggleSettings);
                    }

                    if bar_button(ui, "▭", style, false, true).clicked() {
                        action_theater = Some(OverlayAction::ToggleTheater);
                    }

                    if bar_button(ui, "⛶", style, false, true).clicked() {
                        action_fullscreen = Some(OverlayAction::ToggleFullscreen);
                    }

                    if bar_button(ui, "✕", style, false, true).clicked() {
                        action_close = Some(OverlayAction::Close);
                    }
                });
            });
        }
    });

    // Prioridade determinística: play/pause, seek buttons, progress click,
    // mute, volume, next, settings, theater, fullscreen, close.
    let action = action_play
        .or(action_seek_btn)
        .or(action_progress)
        .or(action_mute)
        .or(action_volume)
        .or(action_next)
        .or(action_settings)
        .or(action_theater)
        .or(action_fullscreen)
        .or(action_close);

    (action, pointer_inside)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_style() -> OverlayStyle {
        OverlayStyle {
            panel: Color32::from_rgba_unmultiplied(20, 20, 24, 210),
            icon: Color32::from_rgb(240, 240, 245),
            text: Color32::from_rgb(220, 220, 230),
            accent: Color32::from_rgb(230, 57, 70),
        }
    }

    #[test]
    fn overlay_visible_truth_table() {
        // Ponteiro sobre o vídeo.
        assert!(overlay_visible(true, false, false, 10.0));
        // Pausado.
        assert!(overlay_visible(false, true, false, 10.0));
        // Finalizado.
        assert!(overlay_visible(false, false, true, 10.0));
        // Atividade recente (< OVERLAY_HIDE_DELAY_SECS).
        assert!(overlay_visible(false, false, false, 0.0));
        assert!(overlay_visible(
            false,
            false,
            false,
            OVERLAY_HIDE_DELAY_SECS - 0.01
        ));
        // Nenhum dos critérios ativos.
        assert!(!overlay_visible(
            false,
            false,
            false,
            OVERLAY_HIDE_DELAY_SECS
        ));
        assert!(!overlay_visible(false, false, false, 15.0));
    }

    #[test]
    fn format_time_values_and_none_duration() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.0), "0:59");
        assert_eq!(format_time(60.0), "1:00");
        assert_eq!(format_time(3599.0), "59:59");
        assert_eq!(format_time(3600.0), "1:00:00");
        assert_eq!(format_time(3661.0), "1:01:01");

        assert_eq!(format_elapsed_total(0.0, None), "0:00 / --:--");
        assert_eq!(format_elapsed_total(65.0, Some(3600.0)), "1:05 / 1:00:00");
    }

    #[test]
    fn headless_smoke_test_and_invisible_short_circuit() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(640.0, 360.0));
        let telemetry = OverlayTelemetry {
            paused: false,
            ended: false,
            muted: false,
            position_secs: 42.0,
            duration_secs: Some(120.0),
            volume_percent: 80.0,
            has_queued: true,
        };
        let style = sample_style();

        let mut visible_out = None;
        let mut hidden_out = None;

        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            visible_out = Some(show(ui, rect, &telemetry, &style, true));
            hidden_out = Some(show(ui, rect, &telemetry, &style, false));
        });
        output.textures_delta.clear();

        assert_eq!(visible_out, Some((None, false)));
        assert_eq!(hidden_out, Some((None, false)));
    }

    #[test]
    fn progress_click_helper_and_synthetic_play_click() {
        assert_eq!(
            action_for_progress_click(0.25, Some(200.0)),
            Some(OverlayAction::SeekTo(50.0))
        );
        assert_eq!(
            action_for_progress_click(1.5, Some(100.0)),
            Some(OverlayAction::SeekTo(100.0))
        );
        assert_eq!(action_for_progress_click(0.5, None), None);
        assert_eq!(action_for_progress_click(0.5, Some(0.0)), None);

        // Clique sintético no botão Play (primeiro controle à esquerda da barra).
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(640.0, 360.0));
        let telemetry = OverlayTelemetry::default();
        let style = sample_style();
        let bar_rect = compute_bar_rect(rect).expect("bar_rect válido");
        let play_click_pos = Pos2::new(bar_rect.min.x + 20.0, bar_rect.center().y + 2.0);

        let ctx = egui::Context::default();
        // Primeiro quadro posiciona os widgets e registra os retângulos interativos.
        let mut raw_move = egui::RawInput::default();
        raw_move
            .events
            .push(egui::Event::PointerMoved(play_click_pos));
        let mut output = ctx.run_ui(raw_move, |ui| {
            let _ = show(ui, rect, &telemetry, &style, true);
        });
        output.textures_delta.clear();

        // Segundo quadro pressiona e solta o botão primário sobre o botão Play.
        let mut raw_click = egui::RawInput::default();
        raw_click
            .events
            .push(egui::Event::PointerMoved(play_click_pos));
        raw_click.events.push(egui::Event::PointerButton {
            pos: play_click_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        raw_click.events.push(egui::Event::PointerButton {
            pos: play_click_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });

        let mut click_result = (None, false);
        let mut output = ctx.run_ui(raw_click, |ui| {
            click_result = show(ui, rect, &telemetry, &style, true);
        });
        output.textures_delta.clear();

        assert_eq!(click_result, (Some(OverlayAction::TogglePlay), true));
    }
}
