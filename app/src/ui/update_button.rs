//! The title bar's update button and its confirmation. Module 596339: `AutoUpdateStore` holds the updater's state and the
//! button (`ag`) is the last item of the bar's trailing group, after Help. While the update downloads
//! (`UPDATE_AVAILABLE`) it shows the download icon in `GREEN_360`, disabled, tooltip "Downloading Update", the arrow
//! bouncing (`arrowBounce_qHpu2N`: 2px, 2s, ease-in-out); once downloaded (`UPDATE_DOWNLOADED`) it is live, tooltip
//! "Update Ready!". A click installs at once, except while connected to voice, where a confirmation first says the call
//! drops briefly ("Briefly leave voice?", "Update anyway!"). The button's box is the bar's own (`clickable_fErCLM`: 24
//! square, 6 radius, hover `--background-mod-normal`, disabled at half opacity and no hover). No golden shows it, so it
//! is built from the module and the stylesheet.

use super::modal::{self, Tone};
use super::tooltip::{Side, Tip};
use super::{Root, color};
use crate::updater::{self, State};
use gpui::{
    Animation, AnimationExt, AnyElement, Context, IntoElement, SharedString, div, ease_in_out,
    prelude::*, px, svg,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Whether the "Briefly leave voice?" question is up.
static CONFIRMING: AtomicBool = AtomicBool::new(false);

/// What the button reads from the updater's state: its tooltip and whether it is live.
pub(super) fn presentation(state: &State) -> Option<(&'static str, bool)> {
    match state {
        State::NotAvailable => None,
        State::Downloading { .. } => Some(("Downloading Update", false)),
        State::Ready { .. } => Some(("Update Ready!", true)),
    }
}

/// `arrowBounce_qHpu2N`: -2px, then 0 at the half way point, then -2px again, each leg eased.
fn bounce(progress: f32) -> f32 {
    let leg = if progress < 0.5 {
        progress * 2.
    } else {
        (1. - progress) * 2.
    };
    -2. + 2. * ease_in_out(leg)
}

impl Root {
    pub(super) fn update_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (tip, live) = presentation(&updater::state())?;
        let icon = svg()
            .path("icons/lb_download.svg")
            .size(px(18.))
            .flex_none()
            .relative()
            .text_color(color("--green-360"));
        let icon = if live {
            icon.into_any_element()
        } else {
            icon.with_animation(
                "update-arrow",
                Animation::new(Duration::from_secs(2)).repeat(),
                |icon, progress| icon.top(px(bounce(progress))),
            )
            .into_any_element()
        };
        let button = div()
            .id("update")
            .size(px(24.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .when(live, |button| {
                button
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .on_click(cx.listener(|this, _, _, cx| this.update_clicked(cx)))
            })
            .when(!live, |button| button.opacity(0.5))
            .child(icon);
        Some(
            self.with_tip("update-button", Tip::text(tip, Side::Bottom), button, cx)
                .mr(px(16.))
                .into_any_element(),
        )
    }

    /// `handleInstallDownload`.
    fn update_clicked(&mut self, cx: &mut Context<Self>) {
        if !matches!(updater::state(), State::Ready { .. }) {
            return;
        }
        if self.call.as_ref().is_some_and(|call| call.connected) {
            CONFIRMING.store(true, Ordering::Relaxed);
            cx.notify();
        } else {
            self.update_install(cx);
        }
    }

    /// `AUTO_UPDATER_QUIT_AND_INSTALL`: starts the verified setup file, then quits so it can replace the app.
    fn update_install(&mut self, cx: &mut Context<Self>) {
        CONFIRMING.store(false, Ordering::Relaxed);
        match updater::quit_and_install() {
            Ok(()) => cx.quit(),
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "the update could not be started");
                self.put_app_toast("The update could not be started.".to_owned(), false, cx);
            }
        }
    }

    /// The question asked while connected to voice.
    pub(super) fn update_dialog_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !CONFIRMING.load(Ordering::Relaxed) {
            return None;
        }
        let close =
            || -> modal::Handler { Box::new(|_, _, _| CONFIRMING.store(false, Ordering::Relaxed)) };
        Some(modal::render(
            modal::Spec {
                id: "update-confirm",
                title: "Briefly leave voice?".into(),
                subtitle: Some(
                    div()
                        .child(SharedString::from(
                            "Updating Discord while in a voice channel will cause you to leave briefly. You're probably going to update anyway but, you know, just warning you.",
                        ))
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    modal::button("update-cancel", "Cancel", Tone::Secondary, |_, _, _| {
                        CONFIRMING.store(false, Ordering::Relaxed);
                    }),
                    modal::button(
                        "update-anyway",
                        "Update anyway!",
                        Tone::Critical,
                        |this, _, cx| this.update_install(cx),
                    ),
                ],
                close: close(),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_button_follows_the_updater_state() {
        assert_eq!(presentation(&State::NotAvailable), None);
        assert_eq!(
            presentation(&State::Downloading {
                version: "1.0.0".into()
            }),
            Some(("Downloading Update", false))
        );
        assert_eq!(
            presentation(&State::Ready {
                version: "1.0.0".into()
            }),
            Some(("Update Ready!", true))
        );
    }

    #[test]
    fn the_arrow_bounces_between_minus_two_and_zero() {
        assert_eq!(bounce(0.), -2.);
        assert!((bounce(0.5) - 0.).abs() < 1e-4);
        assert!((bounce(1.) - -2.).abs() < 1e-4);
        assert!((bounce(0.25) - bounce(0.75)).abs() < 1e-4);
    }
}
