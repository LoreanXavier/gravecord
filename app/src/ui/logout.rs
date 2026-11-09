//! Log Out: the confirmation of the settings sidebar's last entry. Confirming hands the decision to the shell, which
//! owns the login: it clears the stored token, closes the connections and shows the login screen again.

use super::Root;
use gpui::{AnyElement, Context, EventEmitter, IntoElement, div, prelude::*};

/// What the main view asks of the shell.
pub enum RootEvent {
    LogOut,
}

impl EventEmitter<RootEvent> for Root {}

impl Root {
    /// Leaves a call before the session ends under it (a dead token).
    pub fn leave_for_logout(&mut self, cx: &mut Context<Self>) {
        self.leave_voice(cx);
    }

    /// The confirm dialog (string `SUnWBB`, "Are you sure you want to logout?"); the way back is Cancel.
    pub(super) fn logout_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.logout_confirm {
            return None;
        }
        let spec = super::modal::Spec {
            id: "log-out",
            title: "Log Out".into(),
            subtitle: Some(
                div()
                    .child("Are you sure you want to logout?")
                    .into_any_element(),
            ),
            body: None,
            buttons: vec![
                super::modal::button(
                    "logout-cancel",
                    "Cancel",
                    super::modal::Tone::Secondary,
                    |this, _, _| this.logout_confirm = false,
                ),
                super::modal::button(
                    "logout-confirm",
                    "Log Out",
                    super::modal::Tone::Critical,
                    |this, _, cx| {
                        this.logout_confirm = false;
                        // A call is left first, so the voice server sees the disconnect, not a dropped socket.
                        this.leave_voice(cx);
                        cx.emit(RootEvent::LogOut);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.logout_confirm = false),
        };
        Some(super::modal::render(spec, cx))
    }
}
