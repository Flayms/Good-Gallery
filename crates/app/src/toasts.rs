//! Short confirmations and errors in the bottom-right corner that disappear on their own.

use crate::icons::Icon;
use crate::theme;
use eframe::egui::{self, Align2, CornerRadius, Frame, Margin, RichText, Stroke};

/// How long a toast stays, in seconds; one with a button stays longer, and while the pointer is on it.
const SHOWN_FOR: f64 = 4.0;
const WITH_ACTION_FOR: f64 = 10.0;

/// A button a toast can offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastAction {
    /// Takes the last rename back (REQ-035-08).
    UndoRename,
    /// Takes the last date fix back (REQ-038-09).
    UndoDates,
}

impl ToastAction {
    fn label(self) -> &'static str {
        match self {
            Self::UndoRename | Self::UndoDates => "Undo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    pub action: Option<ToastAction>,
    /// When it was shown first (`egui::InputState::time`); set on the first frame.
    shown_at: Option<f64>,
}

#[derive(Debug, Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    /// The pointer was on the toasts last frame.
    hovered: bool,
}

impl Toasts {
    pub fn success(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Success, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(ToastKind::Error, text.into());
    }

    /// A confirmation with a button.
    pub fn success_with(&mut self, text: impl Into<String>, action: ToastAction) {
        self.push(ToastKind::Success, text.into());
        if let Some(toast) = self.toasts.last_mut() {
            toast.action = Some(action);
        }
    }

    fn push(&mut self, kind: ToastKind, text: String) {
        self.toasts.push(Toast {
            kind,
            text,
            action: None,
            shown_at: None,
        });
    }

    /// The toasts currently shown, oldest first.
    pub fn current(&self) -> &[Toast] {
        &self.toasts
    }

    /// Draws the toasts; the button clicked, if any.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ToastAction> {
        let now = ctx.input(|i| i.time);
        let hovered = self.hovered;
        self.toasts.retain(|toast| {
            let shown_for = if toast.action.is_some() {
                WITH_ACTION_FOR
            } else {
                SHOWN_FOR
            };
            (toast.action.is_some() && hovered) || toast.shown_at.is_none_or(|at| now - at < shown_for)
        });
        if self.toasts.is_empty() {
            return None;
        }
        let mut clicked = None;
        let actions = self.toasts.iter().any(|toast| toast.action.is_some());
        let area = egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
            .order(egui::Order::Tooltip)
            .interactable(actions)
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
                                if let Some(action) = toast.action
                                    && ui.button(action.label()).clicked()
                                {
                                    clicked = Some(action);
                                }
                            });
                        });
                }
            });
        self.hovered = actions && area.response.contains_pointer();
        if let Some(action) = clicked {
            self.toasts.retain(|toast| toast.action != Some(action));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        clicked
    }
}
