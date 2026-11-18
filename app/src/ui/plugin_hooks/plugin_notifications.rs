//! The client mod's Notifications and Notices APIs (Equicord `src/api/Notifications`, `src/api/Notices.tsx`,
//! GPL-3.0-or-later), for the plugins that use them.
//!
//! - `showNotification`: while the window has focus (`useNative` "not-focused", the default and the owner's setting),
//!   the in-app card (`NotificationComponent`): bottom right, 16px from the edges, `min(25vw, 400px)` wide and at
//!   least `10vh` high on `--background-base-low` with 6px corners; a 64px icon with 6px corners and 20px of padding
//!   and gap; the title upper case in 16px semibold `--text-strong`, the dismiss cross at its right (`--interactive-icon
//!   -default` at half opacity, full on hover); the body 8px under it at 1.4 line height; a 4px `--brand-500` bar that
//!   shrinks over the 5 s timeout and stops while hovered. A click runs the action and closes it, a right click closes
//!   it; one shows at a time, the next after it. Without focus it is a desktop notification instead, whose click runs
//!   the same action.
//! - `showNotice`: the notice bar's `GENERIC` notice (`colorDefault`, the positive status colours) with the message
//!   and its button; the button and the close cross dismiss it and the next queued notice follows.
//!
//! GAP: the Notification Log (persisted notifications and its modal), the desktop notification's icon.

use super::super::{Root, color};
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, img, px, relative,
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// `notifications.timeout`.
const TIMEOUT: Duration = Duration::from_millis(5000);
/// Desktop notifications of plugins carry a channel id from here down, so their click finds its action.
const NATIVE_BASE: Id = u64::MAX - 1_000_000;

/// What a click on a notification does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Action {
    None,
    OpenProfile(Id),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Notification {
    pub title: String,
    pub body: String,
    /// The icon's URL (`Root::images` key).
    pub icon: Option<String>,
    pub action: Action,
}

struct Shown {
    notification: Notification,
    started: Instant,
    /// When the pointer came over it (the timeout waits), and how long it has waited in all.
    paused_at: Option<Instant>,
    paused: Duration,
}

#[derive(Default)]
struct Queue {
    shown: Option<Shown>,
    waiting: VecDeque<Notification>,
    /// Desktop notifications' actions by their channel id.
    native: Vec<(Id, Action)>,
    next_native: u64,
    ticking: bool,
    /// `noticesQueue`: the message and the button's text; the first is shown.
    notices: VecDeque<(String, String)>,
    /// A desktop notification's action, run on the next frame.
    clicked: Option<Action>,
}

thread_local! {
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
}

impl Shown {
    fn elapsed(&self, now: Instant) -> Duration {
        let paused = self.paused + self.paused_at.map_or(Duration::ZERO, |at| now - at);
        now.saturating_duration_since(self.started)
            .saturating_sub(paused)
    }
}

impl Root {
    /// `showNotification`.
    pub(in crate::ui) fn plugin_notify(
        &mut self,
        notification: Notification,
        window_focused: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = notification.icon.clone() {
            self.request_image(url.clone(), url);
        }
        if !window_focused {
            let channel = QUEUE.with(|queue| {
                let mut queue = queue.borrow_mut();
                let channel = NATIVE_BASE - queue.next_native % 1_000_000;
                queue.next_native += 1;
                queue.native.push((channel, notification.action.clone()));
                if queue.native.len() > 100 {
                    queue.native.remove(0);
                }
                channel
            });
            self.notification_runtime
                .show(crate::notifications::Notice {
                    channel,
                    message: 0,
                    title: notification.title,
                    body: notification.body,
                });
            return;
        }
        QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            if queue.shown.is_none() {
                queue.shown = Some(Shown {
                    notification,
                    started: Instant::now(),
                    paused_at: None,
                    paused: Duration::ZERO,
                });
            } else {
                queue.waiting.push_back(notification);
            }
        });
        self.plugin_notification_tick(cx);
        cx.notify();
    }

    /// The bar moves and the timeout runs while a card is up.
    fn plugin_notification_tick(&mut self, cx: &mut Context<Self>) {
        let start = QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            let start = !queue.ticking && queue.shown.is_some();
            if start {
                queue.ticking = true;
            }
            start
        });
        if !start {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let alive = this
                    .update(cx, |_, cx| {
                        let now = Instant::now();
                        let alive = QUEUE.with(|queue| {
                            let mut queue = queue.borrow_mut();
                            if queue
                                .shown
                                .as_ref()
                                .is_some_and(|shown| shown.elapsed(now) >= TIMEOUT)
                            {
                                close(&mut queue);
                            }
                            queue.ticking = queue.shown.is_some();
                            queue.ticking
                        });
                        cx.notify();
                        alive
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn plugin_notification_run(
        &mut self,
        action: Action,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if let Action::OpenProfile(user) = action {
            self.open_full_profile(user, window, cx);
        }
    }

    /// A desktop notification was clicked: true when it was a plugin's.
    pub(in crate::ui) fn plugin_notification_clicked(
        &mut self,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> bool {
        if channel < NATIVE_BASE - 1_000_000 {
            return false;
        }
        let action = QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            let index = queue.native.iter().position(|(id, _)| *id == channel)?;
            Some(queue.native.remove(index).1)
        });
        // The window is not at hand here: the next frame runs it (`plugin_notification_layer`).
        if let Some(action) = action {
            QUEUE.with(|queue| queue.borrow_mut().clicked = Some(action));
            cx.notify();
        }
        true
    }

    /// `NotificationComponent`, while one is up.
    pub(in crate::ui) fn plugin_notification_layer(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(action) = QUEUE.with(|queue| queue.borrow_mut().clicked.take()) {
            cx.defer_in(window, move |this, window, cx| {
                this.plugin_notification_run(action, window, cx)
            });
        }
        let (notification, progress) = QUEUE.with(|queue| {
            let queue = queue.borrow();
            let shown = queue.shown.as_ref()?;
            let elapsed = shown.elapsed(Instant::now());
            Some((
                shown.notification.clone(),
                1. - (elapsed.as_secs_f32() / TIMEOUT.as_secs_f32()).clamp(0., 1.),
            ))
        })?;
        let (width, height) = (self.viewport.0, self.viewport.1);
        let picture = notification
            .icon
            .as_ref()
            .and_then(|url| self.images.get(url).cloned());
        let action = notification.action.clone();
        Some(
            gpui::deferred(
                div()
                    .id("plugin-notification")
                    .absolute()
                    .right(px(16.))
                    .bottom(px(16.))
                    .w(px((width * 0.25).min(400.)))
                    .min_h(px(height * 0.1))
                    .flex()
                    .flex_col()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(color("--background-base-low"))
                    .text_color(color("--text-default"))
                    .cursor_pointer()
                    .occlude()
                    .on_hover(cx.listener(|_, inside: &bool, _, cx| {
                        let now = Instant::now();
                        QUEUE.with(|queue| {
                            if let Some(shown) = queue.borrow_mut().shown.as_mut() {
                                if *inside {
                                    shown.paused_at.get_or_insert(now);
                                } else if let Some(at) = shown.paused_at.take() {
                                    shown.paused += now - at;
                                }
                            }
                        });
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        QUEUE.with(|queue| close(&mut queue.borrow_mut()));
                        this.plugin_notification_run(action.clone(), window, cx);
                        this.plugin_notification_tick(cx);
                        cx.notify();
                    }))
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, _, _, cx| {
                            QUEUE.with(|queue| close(&mut queue.borrow_mut()));
                            this.plugin_notification_tick(cx);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .p(px(20.))
                            .gap(px(20.))
                            .when_some(picture, |this, picture| {
                                this.child(img(picture).flex_none().size(px(64.)).rounded(px(6.)))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(notification.title.to_uppercase()),
                                            )
                                            .child(
                                                div()
                                                    .id("plugin-notification-close")
                                                    .flex_none()
                                                    .cursor_pointer()
                                                    .opacity(0.5)
                                                    .text_color(color("--interactive-icon-default"))
                                                    .hover(|style| {
                                                        style
                                                            .opacity(1.)
                                                            .text_color(color("--interactive-icon-hover"))
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        QUEUE.with(|queue| close(&mut queue.borrow_mut()));
                                                        this.plugin_notification_tick(cx);
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        gpui::svg()
                                                            .path("icons/plugin_notification_close.svg")
                                                            .size(px(24.)),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt(px(8.))
                                            .text_size(px(16.))
                                            .line_height(relative(1.4))
                                            .child(SharedString::from(notification.body)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .mt_auto()
                            .h(px(4.))
                            .rounded(px(5.))
                            .w(relative(progress))
                            .bg(color("--brand-500")),
                    ),
            )
            .with_priority(20)
            .into_any_element(),
        )
    }

    /// `showNotice`.
    pub(in crate::ui) fn plugin_notice(
        &mut self,
        message: String,
        button: String,
        cx: &mut Context<Self>,
    ) {
        QUEUE.with(|queue| queue.borrow_mut().notices.push_back((message, button)));
        cx.notify();
    }

    /// The `GENERIC` notice at the head of the queue, as the notice bar draws it.
    pub(in crate::ui) fn plugin_notice_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (message, button) = QUEUE.with(|queue| queue.borrow().notices.front().cloned())?;
        let text = color("--status-positive-text");
        let dismiss = |this: &mut Root, cx: &mut Context<Root>| {
            QUEUE.with(|queue| queue.borrow_mut().notices.pop_front());
            let _ = this;
            cx.notify();
        };
        Some(
            div()
                .id("plugin-notice")
                .relative()
                .flex_none()
                .w_full()
                .h(px(36.))
                .pl(px(4.))
                .pr(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .bg(color("--status-positive-background"))
                .border_t_1()
                .border_l_1()
                .border_color(color("--app-frame-border"))
                .rounded_tl(px(8.))
                .text_size(px(14.))
                .line_height(px(36.))
                .font_weight(FontWeight(500.))
                .text_color(text)
                .child(message)
                .child(
                    div()
                        .id("plugin-notice-button")
                        .ml(px(10.))
                        .h(px(24.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(text)
                        .rounded(px(3.))
                        .line_height(px(22.))
                        .cursor_pointer()
                        .hover(|style| {
                            style
                                .bg(color("--status-positive-text"))
                                .text_color(color("--status-positive-background"))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx)))
                        .child(button),
                )
                .child(
                    div()
                        .id("plugin-notice-close")
                        .absolute()
                        .top(px(0.))
                        .right(px(0.))
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx)))
                        .child(super::super::icon(
                            "mod_close",
                            18.,
                            "--status-positive-text",
                        )),
                )
                .into_any_element(),
        )
    }
}

/// The card goes; the next one waiting comes up.
fn close(queue: &mut Queue) {
    queue.shown = queue.waiting.pop_front().map(|notification| Shown {
        notification,
        started: Instant::now(),
        paused_at: None,
        paused: Duration::ZERO,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timeout_waits_while_hovered() {
        let start = Instant::now();
        let shown = Shown {
            notification: Notification {
                title: "t".into(),
                body: "b".into(),
                icon: None,
                action: Action::None,
            },
            started: start,
            paused_at: Some(start + Duration::from_secs(1)),
            paused: Duration::from_secs(2),
        };
        // Four seconds on, of which two were paused before and three are being paused now.
        assert_eq!(
            shown.elapsed(start + Duration::from_secs(4)),
            Duration::ZERO
        );
        let mut queue = Queue {
            shown: Some(shown),
            ..Queue::default()
        };
        queue.waiting.push_back(Notification {
            title: "next".into(),
            body: String::new(),
            icon: None,
            action: Action::None,
        });
        close(&mut queue);
        assert_eq!(queue.shown.as_ref().unwrap().notification.title, "next");
        close(&mut queue);
        assert!(queue.shown.is_none());
    }
}
