//! Short confirmations and errors in the bottom-right corner that disappear on their own.

use crate::icons::Icon;
use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, Frame, Margin, RichText, Stroke};

/// How long a toast stays, in seconds.
const SHOWN_FOR: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    /// When it was shown first (`egui::InputState::time`); set on the first frame.
    shown_at: Option<f64>,
}

#[derive(Debug, Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
}

impl Toasts {
    pub fn success(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Success, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Error, text.into());
    }

    fn push(&mut self, kind: ToastKind, text: String) {
        self.toasts.push(Toast {
            kind,
            text,
            shown_at: None,
        });
    }

    /// The toasts currently shown, oldest first.
    pub fn current(&self) -> &[Toast] {
        &self.toasts
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts
            .retain(|toast| toast.shown_at.is_none_or(|at| now - at < SHOWN_FOR));
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                for toast in &mut self.toasts {
                    toast.shown_at.get_or_insert(now);
                    Frame::new()
                        .fill(theme::raised())
                        .stroke(Stroke::new(1.0, theme::rim()))
                        .corner_radius(CornerRadius::same(theme::RADIUS_PANEL))
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (icon, color) = match toast.kind {
                                    ToastKind::Success => (Icon::Check, theme::text()),
                                    ToastKind::Error => (Icon::TriangleAlert, theme::alert()),
                                };
                                ui.add(icon.image(theme::ICON_SIZE, color));
                                ui.label(RichText::new(&toast.text).color(theme::text()));
                            });
                        });
                }
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
