//! Bookmarks and reminders (module 880457 for the menu entries, 243656 for the reminder times, 81612 for the
//! requests and toasts): `PUT /users/@me/saved-messages/{channel}/{message}` with `{source, due_at?}`, `DELETE`
//! to remove. The reminder submenu is measured in `design/dom/menus/reminder-submenu-darker-cozy.json` and the
//! toast, which shows under the Inbox button for five seconds, in `design/dom/toasts/reminder-removed-darker-cozy.json`.

use super::{Root, color};
use crate::net::Request;
use chrono::{Datelike, Duration, Local, TimeZone, Timelike};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px, svg,
};
use std::time::Instant;

pub(crate) const SAVED_KEY: &str = "saved-messages";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastIcon {
    Clock,
    Bookmark,
}

pub(crate) struct Toast {
    pub message: SharedString,
    pub icon: ToastIcon,
    pub until: Instant,
}

/// The app-wide toast (`toast_W6gbBa`): top centre, three seconds.
pub(crate) struct AppToast {
    pub message: String,
    /// `success_O41c2-` (green edge, a check, 90px down) rather than `default_O41c2-` (the forward arrow, 40px).
    pub success: bool,
    pub until: Instant,
}

/// moment's `humanize()` with its default thresholds, as the reminder labels use it.
pub(crate) fn humanize(milliseconds: i64) -> String {
    let seconds = (milliseconds.abs() as f64 / 1000.).round();
    let minutes = (seconds / 60.).round();
    let hours = (minutes / 60.).round();
    let days = (hours / 24.).round();
    if seconds < 45. {
        "a few seconds".to_owned()
    } else if seconds < 90. {
        "a minute".to_owned()
    } else if minutes < 45. {
        format!("{minutes} minutes")
    } else if minutes < 90. {
        "an hour".to_owned()
    } else if hours < 22. {
        format!("{hours} hours")
    } else if hours < 36. {
        "a day".to_owned()
    } else if days < 26. {
        format!("{days} days")
    } else if days < 45. {
        "a month".to_owned()
    } else if days < 320. {
        format!("{} months", (days / 30.4).round())
    } else if days < 548. {
        "a year".to_owned()
    } else {
        format!("{} years", (days / 365.).round())
    }
}

/// The five preset reminder times (`b` in module 243656) as (label, due time), now being `now`.
pub(crate) fn reminder_choices(
    now: chrono::DateTime<Local>,
) -> Vec<(&'static str, chrono::DateTime<Local>)> {
    let nine = |date: chrono::NaiveDate| {
        Local
            .from_local_datetime(&date.and_hms_opt(9, 0, 0).unwrap_or_default())
            .single()
            .unwrap_or(now)
    };
    let today = now.date_naive();
    let (morning_label, morning) = if now.hour() >= 9 {
        ("Tomorrow at 9:00am", nine(today + Duration::days(1)))
    } else {
        ("Today at 9:00am", nine(today))
    };
    // `day(1)` for Sunday, or Monday before 9:00; otherwise `day(8)`: next week's Monday.
    let weekday = now.weekday().num_days_from_sunday();
    let before_nine = nine(today) > now;
    let monday_offset = if weekday == 0 || (weekday == 1 && before_nine) {
        1
    } else {
        8
    };
    let monday = today - Duration::days(i64::from(weekday)) + Duration::days(monday_offset);
    vec![
        ("in 30m", now + Duration::minutes(30)),
        ("in 1h", now + Duration::hours(1)),
        ("in 4h", now + Duration::hours(4)),
        (morning_label, morning),
        ("Monday at 9:00am", nine(monday)),
    ]
}

impl Root {
    pub(super) fn request_saved(&mut self) {
        let _ = self.requests.send(Request::Fetch {
            key: SAVED_KEY.to_owned(),
            route: "/users/@me/saved-messages".to_owned(),
        });
    }

    /// The saved state of a message: `None` unsaved, `Some(None)` bookmarked, `Some(Some(due))` a reminder.
    pub(super) fn saved_state(&self, channel: Id, message: Id) -> Option<Option<String>> {
        self.stores.saved.get(&(channel, message)).cloned()
    }

    pub(super) fn show_toast(
        &mut self,
        message: impl Into<SharedString>,
        icon: ToastIcon,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some(Toast {
            message: message.into(),
            icon,
            until: Instant::now() + std::time::Duration::from_secs(5),
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(5))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .toast
                    .as_ref()
                    .is_some_and(|toast| toast.until <= Instant::now())
                {
                    this.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn show_app_toast(&mut self, message: &'static str, cx: &mut Context<Self>) {
        self.put_app_toast(message.to_owned(), false, cx);
    }

    /// `showBlockSuccessToast` and the other `P0({variant: "success"})` toasts.
    pub(super) fn show_success_toast(&mut self, message: String, cx: &mut Context<Self>) {
        self.put_app_toast(message, true, cx);
    }

    pub(super) fn put_app_toast(&mut self, message: String, success: bool, cx: &mut Context<Self>) {
        let duration = std::time::Duration::from_millis(3000);
        self.app_toast = Some(AppToast {
            message,
            success,
            until: Instant::now() + duration,
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .app_toast
                    .as_ref()
                    .is_some_and(|toast| toast.until <= Instant::now())
                {
                    this.app_toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// `wrapper_O41c2-`: 12px padding and radius on `--background-surface-highest` with a `--border-normal` edge,
    /// an 18px icon in `--icon-feedback-positive`, 16/20 text; 40px from the top, centred.
    pub(super) fn app_toast_layer(&self) -> Option<AnyElement> {
        let toast = self.app_toast.as_ref()?;
        Some(
            deferred(
                div()
                    .absolute()
                    .top(px(if toast.success { 90. } else { 40. }))
                    .left(px(0.))
                    .right(px(0.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .p(px(12.))
                            .rounded(px(12.))
                            .border_1()
                            .when(!toast.success, |this| {
                                this.border_color(color("--border-normal"))
                                    .bg(color("--background-surface-highest"))
                            })
                            // `baselayer_O41c2- success`: a green edge and a 12% green wash over the base.
                            .when(toast.success, |this| {
                                this.border_color(color("--green-new-42"))
                                    .bg(color("--background-base-low"))
                            })
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.24),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(toast.success, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded(px(12.))
                                        .bg(color("--opacity-green-12")),
                                )
                            })
                            .relative()
                            .child(
                                svg()
                                    .path(if toast.success {
                                        "icons/toast_success.svg"
                                    } else {
                                        "icons/toast_forwarded.svg"
                                    })
                                    .size(px(18.))
                                    .text_color(color("--icon-feedback-positive")),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-strong"))
                                    .child(toast.message.clone()),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Bookmark Message / Remove Bookmark.
    pub(super) fn toggle_bookmark(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        let bookmarked = self.saved_state(channel, message).is_some();
        if bookmarked {
            self.stores.saved.remove(&(channel, message));
        } else {
            self.stores.saved.insert((channel, message), None);
        }
        let _ = self.requests.send(Request::Save {
            channel,
            message,
            due_at: None,
            save: !bookmarked,
        });
        if bookmarked {
            self.show_toast("Bookmark removed.", ToastIcon::Bookmark, cx);
        } else {
            self.show_toast(
                "Bookmark added. For your eyes only.",
                ToastIcon::Bookmark,
                cx,
            );
        }
    }

    pub(super) fn set_reminder(
        &mut self,
        channel: Id,
        message: Id,
        due: chrono::DateTime<Local>,
        cx: &mut Context<Self>,
    ) {
        let due_at = due
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        self.stores
            .saved
            .insert((channel, message), Some(due_at.clone()));
        let _ = self.requests.send(Request::Save {
            channel,
            message,
            due_at: Some(due_at),
            save: true,
        });
        self.show_toast("Reminder added.", ToastIcon::Clock, cx);
    }

    pub(super) fn complete_reminder(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        self.stores.saved.remove(&(channel, message));
        let _ = self.requests.send(Request::Save {
            channel,
            message,
            due_at: None,
            save: false,
        });
        self.show_toast("Reminder removed.", ToastIcon::Clock, cx);
    }

    /// The reminder submenu: `Remind Me` and the five times plus Custom..., or for a reminder already set, its due
    /// time with Mark Complete, then `Edit Reminder` and the times.
    pub(super) fn reminder_rows(
        &self,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let due = self
            .saved_state(channel, message)
            .flatten()
            .and_then(|due| chrono::DateTime::parse_from_rfc3339(&due).ok());
        let now = Local::now();
        let label = |text: String| {
            div()
                .min_h(px(32.))
                .p(px(8.))
                .text_size(px(14.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-muted"))
                .child(text)
                .into_any_element()
        };
        let item = |id: SharedString, text: &'static str| {
            div()
                .id(id)
                .h(px(34.))
                .p(px(8.))
                .rounded(px(2.))
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .rounded(px(4.))
                })
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let mut rows: Vec<(AnyElement, f32)> = Vec::new();
        if let Some(due) = due {
            let left = due.timestamp_millis() - now.timestamp_millis();
            let text = if left >= 0 {
                format!("Due in {}", humanize(left))
            } else {
                format!("Due {} ago", humanize(left))
            };
            rows.push((label(text), 36.));
            rows.push((
                item("reminder-complete".into(), "Mark Complete")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.submenu = None;
                        this.complete_reminder(channel, message, cx);
                    }))
                    .into_any_element(),
                34.,
            ));
            rows.push((label("Edit Reminder".to_owned()), 36.));
        } else {
            rows.push((label("Remind Me".to_owned()), 36.));
        }
        for (index, (text, time)) in reminder_choices(now).into_iter().enumerate() {
            rows.push((
                item(SharedString::from(format!("reminder-choice-{index}")), text)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.submenu = None;
                        this.set_reminder(channel, message, time, cx);
                    }))
                    .into_any_element(),
                34.,
            ));
        }
        rows.push((
            item("reminder-custom".into(), "Custom...")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.menu = None;
                    this.submenu = None;
                    this.custom_reminder =
                        Some(super::reminder_modal::CustomReminder::new(channel, message));
                    cx.notify();
                }))
                .into_any_element(),
            34.,
        ));
        rows
    }

    /// The toast under the Inbox button: a rich tooltip with its caret at the end, pointing up at the button.
    pub(super) fn toast_layer(&self) -> Option<AnyElement> {
        let toast = self.toast.as_ref()?;
        let inbox = self.tip_anchors.borrow().get("bar-inbox").copied()?;
        let (icon_file, size) = match toast.icon {
            ToastIcon::Clock => ("menu_create_reminder", 18.),
            ToastIcon::Bookmark => ("menu_remove_bookmark", 18.),
        };
        // The caret's tip sits on the button's centre, 8px in from the toast's right edge.
        let right_edge = f32::from(inbox.center().x) + 16.;
        let top = f32::from(inbox.bottom()) + 7.;
        Some(
            deferred(
                div()
                    .absolute()
                    .top(px(top))
                    .right(px(0.))
                    .left(px(0.))
                    .child(
                        div().absolute().top(px(0.)).left(px(right_edge)).child(
                            div()
                                .absolute()
                                .right(px(0.))
                                .top(px(0.))
                                .rounded(px(8.))
                                .bg(color("--background-surface-high"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .shadow(vec![gpui::BoxShadow {
                                    color: gpui::hsla(0., 0., 0., 0.24),
                                    offset: gpui::point(px(0.), px(12.)),
                                    blur_radius: px(24.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }])
                                .py(px(8.))
                                .px(px(12.))
                                .flex()
                                .gap(px(8.))
                                .items_center()
                                .child(
                                    div().w(px(20.)).flex().justify_center().child(
                                        svg()
                                            .path(format!("icons/{icon_file}.svg"))
                                            .size(px(size))
                                            .text_color(color("--status-positive")),
                                    ),
                                )
                                .child(
                                    div()
                                        .whitespace_nowrap()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(toast.message.clone()),
                                )
                                .child(
                                    svg()
                                        .path("icons/toast_caret_up.svg")
                                        .absolute()
                                        .right(px(8.))
                                        .top(px(-8.))
                                        .w(px(16.))
                                        .h(px(10.))
                                        .text_color(color("--background-surface-high")),
                                ),
                        ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{humanize, reminder_choices};
    use chrono::{Local, TimeZone};

    #[test]
    fn humanize_follows_moment() {
        assert_eq!(humanize(30_000), "a few seconds");
        assert_eq!(humanize(60_000), "a minute");
        assert_eq!(humanize(30 * 60_000), "30 minutes");
        assert_eq!(humanize(59 * 60_000), "an hour");
        assert_eq!(humanize(4 * 3_600_000), "4 hours");
        assert_eq!(humanize(-25 * 3_600_000), "a day");
    }

    #[test]
    fn morning_and_monday_follow_the_bundle() {
        // A Sunday at 03:00: today at 9, and Monday is tomorrow.
        let sunday = Local
            .with_ymd_and_hms(2026, 11, 8, 3, 0, 0)
            .single()
            .unwrap();
        let choices = reminder_choices(sunday);
        assert_eq!(choices[3].0, "Today at 9:00am");
        assert_eq!(choices[4].1.date_naive().to_string(), "2026-11-09");
        // A Tuesday afternoon: tomorrow at 9, Monday next week.
        let tuesday = Local
            .with_ymd_and_hms(2026, 11, 10, 15, 0, 0)
            .single()
            .unwrap();
        let choices = reminder_choices(tuesday);
        assert_eq!(choices[3].0, "Tomorrow at 9:00am");
        assert_eq!(choices[4].1.date_naive().to_string(), "2026-11-16");
    }
}
