use crate::ThemePalette;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const DEFAULT_TOAST_DURATION: Duration = Duration::from_millis(3_500);
const DEFAULT_MAX_VISIBLE: usize = 3;
const FADE_IN_MS: f32 = 120.0;
const FADE_OUT_MS: f32 = 350.0;
const DUPLICATE_COALESCE_WINDOW: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub(crate) struct Toast {
    pub(crate) id: u64,
    pub(crate) message: String,
    pub(crate) kind: ToastKind,
    pub(crate) created_at: Instant,
    pub(crate) duration: Duration,
}

impl Toast {
    pub(crate) fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.created_at)
    }

    pub(crate) fn is_expired(&self, now: Instant) -> bool {
        self.elapsed(now) >= self.duration
    }

    pub(crate) fn remaining(&self, now: Instant) -> Duration {
        self.duration.saturating_sub(self.elapsed(now))
    }

    pub(crate) fn opacity(&self, now: Instant) -> f32 {
        let elapsed_ms = self.elapsed(now).as_secs_f32() * 1000.0;
        let total_ms = self.duration.as_secs_f32() * 1000.0;
        if total_ms <= 0.0 || elapsed_ms >= total_ms {
            return 0.0;
        }
        let fade_in = (elapsed_ms / FADE_IN_MS).clamp(0.0, 1.0);
        let remaining_ms = (total_ms - elapsed_ms).max(0.0);
        let fade_out = (remaining_ms / FADE_OUT_MS).clamp(0.0, 1.0);
        fade_in.min(fade_out)
    }
}

pub(crate) struct ToastManager {
    toasts: VecDeque<Toast>,
    next_id: u64,
    default_duration: Duration,
    max_visible: usize,
}

impl Default for ToastManager {
    fn default() -> Self {
        Self {
            toasts: VecDeque::new(),
            next_id: 1,
            default_duration: DEFAULT_TOAST_DURATION,
            max_visible: DEFAULT_MAX_VISIBLE,
        }
    }
}

impl ToastManager {
    #[allow(dead_code)]
    pub(crate) fn with_limits(default_duration: Duration, max_visible: usize) -> Self {
        Self {
            toasts: VecDeque::new(),
            next_id: 1,
            default_duration: default_duration.max(Duration::from_millis(200)),
            max_visible: max_visible.max(1),
        }
    }

    pub(crate) fn push(&mut self, kind: ToastKind, message: impl Into<String>) -> Option<u64> {
        self.push_at(kind, message, self.default_duration, Instant::now())
    }

    #[allow(dead_code)]
    pub(crate) fn push_with_duration(
        &mut self,
        kind: ToastKind,
        message: impl Into<String>,
        duration: Duration,
    ) -> Option<u64> {
        self.push_at(kind, message, duration, Instant::now())
    }

    pub(crate) fn push_at(
        &mut self,
        kind: ToastKind,
        message: impl Into<String>,
        duration: Duration,
        now: Instant,
    ) -> Option<u64> {
        let trimmed = message.into().trim().to_owned();
        if trimmed.is_empty() {
            return None;
        }
        self.prune_expired(now);

        if let Some(last) = self.toasts.back_mut()
            && last.kind == kind
            && last.message == trimmed
            && last.elapsed(now) <= DUPLICATE_COALESCE_WINDOW
        {
            last.created_at = now;
            last.duration = duration;
            return Some(last.id);
        }

        while self.toasts.len() >= self.max_visible {
            self.toasts.pop_front();
        }

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.toasts.push_back(Toast {
            id,
            message: trimmed,
            kind,
            created_at: now,
            duration,
        });
        Some(id)
    }

    pub(crate) fn info(&mut self, message: impl Into<String>) -> Option<u64> {
        self.push(ToastKind::Info, message)
    }

    pub(crate) fn success(&mut self, message: impl Into<String>) -> Option<u64> {
        self.push(ToastKind::Success, message)
    }

    pub(crate) fn warning(&mut self, message: impl Into<String>) -> Option<u64> {
        self.push(ToastKind::Warning, message)
    }

    pub(crate) fn error(&mut self, message: impl Into<String>) -> Option<u64> {
        self.push(ToastKind::Error, message)
    }

    pub(crate) fn dismiss(&mut self, id: u64) -> bool {
        if let Some(pos) = self.toasts.iter().position(|t| t.id == id) {
            self.toasts.remove(pos);
            true
        } else {
            false
        }
    }

    pub(crate) fn prune_expired(&mut self, now: Instant) {
        self.toasts.retain(|t| !t.is_expired(now));
    }

    #[allow(dead_code)]
    pub(crate) fn visible_toasts(&self) -> Vec<&Toast> {
        self.toasts.iter().take(self.max_visible).collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.toasts.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, palette: &ThemePalette) {
        self.show_at(ctx, palette, Instant::now());
    }

    pub(crate) fn show_at(&mut self, ctx: &egui::Context, palette: &ThemePalette, now: Instant) {
        self.prune_expired(now);
        if self.toasts.is_empty() {
            return;
        }

        let mut next_repaint = Duration::from_millis(250);
        for toast in self.toasts.iter().take(self.max_visible) {
            let elapsed_ms = toast.elapsed(now).as_secs_f32() * 1000.0;
            let remaining = toast.remaining(now);
            let remaining_ms = remaining.as_secs_f32() * 1000.0;
            if elapsed_ms < FADE_IN_MS || remaining_ms <= FADE_OUT_MS {
                next_repaint = next_repaint.min(Duration::from_millis(33));
            } else {
                let until_fade_out =
                    Duration::from_secs_f32(((remaining_ms - FADE_OUT_MS).max(20.0)) / 1000.0);
                next_repaint = next_repaint.min(until_fade_out);
            }
        }
        ctx.request_repaint_after(next_repaint.max(Duration::from_millis(16)));

        let visible: Vec<Toast> = self.toasts.iter().take(self.max_visible).cloned().collect();
        let mut dismissed_id: Option<u64> = None;

        egui::Area::new(egui::Id::new("fasttube_toast_stack"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Tooltip)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(340.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    for toast in &visible {
                        let alpha = toast.opacity(now);
                        if alpha <= 0.01 {
                            continue;
                        }
                        let (base_bg, base_stroke, base_fg, accent, icon) = match toast.kind {
                            ToastKind::Info => (
                                palette.card_alt_bg,
                                palette.info_box_stroke,
                                palette.header_button_fg,
                                palette.info_accent_fg,
                                "ℹ",
                            ),
                            ToastKind::Success => (
                                palette.notice_bg,
                                palette.notice_stroke,
                                palette.notice_fg,
                                palette.notice_fg,
                                "✓",
                            ),
                            ToastKind::Warning => (
                                palette.card_alt_bg,
                                palette.warning_fg,
                                palette.header_button_fg,
                                palette.warning_fg,
                                "⚠",
                            ),
                            ToastKind::Error => (
                                palette.error_bg,
                                palette.error_stroke,
                                palette.error_banner_fg,
                                palette.error_fg,
                                "✕",
                            ),
                        };
                        let bg = base_bg.gamma_multiply(alpha);
                        let stroke_color = base_stroke.gamma_multiply(alpha);
                        let fg = base_fg.gamma_multiply(alpha);
                        let accent_color = accent.gamma_multiply(alpha);

                        egui::Frame::new()
                            .fill(bg)
                            .stroke(Stroke::new(1.0, stroke_color))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(egui::Margin::symmetric(12, 9))
                            .show(ui, |ui| {
                                ui.set_min_width(220.0);
                                ui.set_max_width(320.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(
                                        RichText::new(icon).size(14.0).strong().color(accent_color),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&toast.message).size(12.5).color(fg),
                                        )
                                        .wrap(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let close_btn = egui::Button::new(
                                                RichText::new("\u{00d7}").size(14.0).color(fg),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .frame(false);
                                            if ui
                                                .add(close_btn)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .on_hover_text("Fechar aviso")
                                                .clicked()
                                            {
                                                dismissed_id = Some(toast.id);
                                            }
                                        },
                                    );
                                });
                            });
                    }
                });
            });

        if let Some(id) = dismissed_id {
            self.dismiss(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_manager_caps_visible_at_three_and_dismisses_oldest() {
        let mut manager = ToastManager::default();
        let now = Instant::now();
        let id1 = manager
            .push_at(ToastKind::Info, "Um", Duration::from_secs(4), now)
            .unwrap();
        let id2 = manager
            .push_at(ToastKind::Success, "Dois", Duration::from_secs(4), now)
            .unwrap();
        let id3 = manager
            .push_at(ToastKind::Warning, "Três", Duration::from_secs(4), now)
            .unwrap();
        let id4 = manager
            .push_at(ToastKind::Error, "Quatro", Duration::from_secs(4), now)
            .unwrap();

        assert_ne!(id1, id2);
        assert_eq!(manager.len(), 3);
        let messages: Vec<&str> = manager
            .visible_toasts()
            .into_iter()
            .map(|t| t.message.as_str())
            .collect();
        assert_eq!(messages, vec!["Dois", "Três", "Quatro"]);

        assert!(manager.dismiss(id3));
        assert!(!manager.dismiss(id1));
        assert!(manager.dismiss(id2));
        assert!(manager.dismiss(id4));
        assert!(manager.is_empty());
    }

    #[test]
    fn toast_manager_expires_and_computes_fade_opacity() {
        let mut manager = ToastManager::default();
        let start = Instant::now();
        manager
            .push_at(
                ToastKind::Success,
                "Link copiado",
                Duration::from_secs(2),
                start,
            )
            .unwrap();

        let toast = &manager.visible_toasts()[0];
        assert!(toast.opacity(start) < 0.05);
        assert!((toast.opacity(start + Duration::from_millis(500)) - 1.0).abs() < f32::EPSILON);
        let fading = toast.opacity(start + Duration::from_millis(1_850));
        assert!(fading > 0.0 && fading < 1.0);
        assert_eq!(toast.opacity(start + Duration::from_secs(2)), 0.0);

        manager.prune_expired(start + Duration::from_secs(2));
        assert!(manager.is_empty());
    }

    #[test]
    fn toast_manager_coalesces_rapid_duplicates_and_ignores_blank() {
        let mut manager = ToastManager::default();
        let start = Instant::now();
        assert!(
            manager
                .push_at(ToastKind::Info, "   ", Duration::from_secs(3), start)
                .is_none()
        );
        let id1 = manager
            .push_at(
                ToastKind::Success,
                "Adicionado à fila",
                Duration::from_secs(3),
                start,
            )
            .unwrap();
        let id2 = manager
            .push_at(
                ToastKind::Success,
                "Adicionado à fila",
                Duration::from_secs(3),
                start + Duration::from_millis(300),
            )
            .unwrap();
        assert_eq!(id1, id2);
        assert_eq!(manager.len(), 1);
    }
}
