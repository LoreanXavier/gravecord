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
    /// The icon (asset path, colour token) a toast names (`P0({icon, iconColor})`), instead of the forward arrow.
    pub icon: Option<(&'static str, &'static str)>,
    /// `success_O41c2-` (green edge, a check, 90px down) rather than `default_O41c2-` (the forward arrow, 40px).
    pub success: bool,
    pub until: Instant,
}

/// `InboxLastOpenedAt` of module 113367: when the inbox was last opened (Unix milliseconds).
const INBOX_LAST_OPENED_KEY: &str = "InboxLastOpenedAt";

/// `getOverdueMessageReminderCount` and `getMostRecentOverdueDueAt` (module 85109): the reminders due by `now_ms`, and
/// the latest due time among them (0 for none).
pub(crate) fn overdue_reminders<'a>(
    due_times: impl Iterator<Item = &'a str>,
    now_ms: i64,
) -> (usize, i64) {
    due_times
        .filter_map(|due| chrono::DateTime::parse_from_rfc3339(due).ok())
        .map(|due| due.timestamp_millis())
        .filter(|due| *due <= now_ms)
        .fold((0, 0), |(count, latest), due| (count + 1, latest.max(due)))
}

/// Module 113367: the inbox button keeps "N overdue reminder(s)" open while some are overdue and the latest became
/// due after the inbox was last opened.
pub(crate) fn overdue_notice_shown(count: usize, latest_due_ms: i64, last_opened_ms: i64) -> bool {
    count > 0 && latest_due_ms > last_opened_ms
}

/// moment's `humanize()` with its default thresholds in the interface locale, as the reminder labels use it.
pub(crate) fn humanize(milliseconds: i64) -> String {
    dc_intl::humanize(milliseconds, false)
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
        (
            dc_intl::tr("Tomorrow at 9:00am"),
            nine(today + Duration::days(1)),
        )
    } else {
        (dc_intl::tr("Today at 9:00am"), nine(today))
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
        (dc_intl::tr("in 30m"), now + Duration::minutes(30)),
        (dc_intl::tr("in 1h"), now + Duration::hours(1)),
        (dc_intl::tr("in 4h"), now + Duration::hours(4)),
        (morning_label, morning),
        (dc_intl::tr("Monday at 9:00am"), nine(monday)),
    ]
}

impl Root {
    pub(super) fn request_saved(&mut self) {
        let _ = self.requests.send(Request::Fetch {
            key: SAVED_KEY.to_owned(),
            route: "/users/@me/saved-messages?bookmark_ids=true".to_owned(),
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

    pub(in crate::ui) fn show_app_toast(&mut self, message: &'static str, cx: &mut Context<Self>) {
        self.put_app_toast(message.to_owned(), false, cx);
    }

    /// `showBlockSuccessToast` and the other `P0({variant: "success"})` toasts.
    pub(in crate::ui) fn show_success_toast(&mut self, message: String, cx: &mut Context<Self>) {
        self.put_app_toast(message, true, cx);
    }

    /// `P0({text, icon, iconColor})`: the default toast with its own icon.
    pub(in crate::ui) fn put_icon_toast(
        &mut self,
        message: String,
        icon: (&'static str, &'static str),
        cx: &mut Context<Self>,
    ) {
        self.put_app_toast(message, false, cx);
        if let Some(toast) = self.app_toast.as_mut() {
            toast.icon = Some(icon);
        }
    }

    pub(in crate::ui) fn put_app_toast(
        &mut self,
        message: String,
        success: bool,
        cx: &mut Context<Self>,
    ) {
        // `minToastDurationMs` (Accessibility, Notification Duration) lengthens the toast; never below the 3s default.
        let duration = std::time::Duration::from_millis(self.toast_duration_ms().max(3000));
        self.app_toast = Some(AppToast {
            message,
            icon: None,
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
    pub(super) fn app_toast_layer(&self) -> Vec<AnyElement> {
        crate::ui::motion::toasts::frames(
            self.app_toast
                .as_ref()
                .map(|toast| (toast.message.as_str(), toast.success, toast.until)),
        )
        .into_iter()
        .map(|(message, success, opacity, offset)| {
            let icon = self.app_toast.as_ref().and_then(|toast| toast.icon);
            toast_view(message, success, icon, opacity, offset)
        })
        .collect()
    }
}

/// One toast, `opacity` and `offset` being its enter or leave pose.
fn toast_view(
    message: String,
    success: bool,
    icon: Option<(&'static str, &'static str)>,
    opacity: f32,
    offset: f32,
) -> AnyElement {
    deferred(
        div()
            .absolute()
            .top(px((if success { 90. } else { 40. }) + offset))
            .opacity(opacity)
            .left(px(0.))
            .right(px(0.))
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    // A toast blocks no pointer: a web view under it steps aside (`web_frames.rs`).
                    .child(super::web_frames::layer_probe())
                    .p(px(12.))
                    .rounded(px(12.))
                    .border_1()
                    .when(!success, |this| {
                        this.border_color(color("--border-normal"))
                            .bg(color("--background-surface-highest"))
                    })
                    // `baselayer_O41c2- success`: a green edge and a 12% green wash over the base.
                    .when(success, |this| {
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
                    .when(success, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(12.))
                                .bg(color("--opacity-green-12")),
                        )
                    })
                    .relative()
                    .child({
                        let (path, token) = icon.unwrap_or(if success {
                            ("icons/toast_success.svg", "--icon-feedback-positive")
                        } else {
                            ("icons/toast_forwarded.svg", "--icon-feedback-positive")
                        });
                        svg().path(path).size(px(18.)).text_color(color(token))
                    })
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(message),
                    ),
            ),
    )
    .with_priority(3)
    .into_any_element()
}

impl Root {
    /// `Vc` of module 113367: the inbox was opened (or the notice dismissed with Escape) now.
    pub(super) fn mark_inbox_opened(&mut self) {
        crate::storage::set(
            INBOX_LAST_OPENED_KEY,
            serde_json::json!(chrono::Utc::now().timestamp_millis()),
        );
    }

    /// The overdue reminders' text when the notice shows.
    fn overdue_notice(&self) -> Option<String> {
        let now = chrono::Utc::now().timestamp_millis();
        let (count, latest) = overdue_reminders(
            self.stores.saved.values().filter_map(|due| due.as_deref()),
            now,
        );
        let opened = crate::storage::get(INBOX_LAST_OPENED_KEY)
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        overdue_notice_shown(count, latest, opened).then(|| dc_intl::t!("yBmFPA", count = count))
    }

    /// Escape while reminders are overdue records the time (the notice ends); true when it did. The key is not
    /// consumed: whatever else Escape closes still closes.
    pub(super) fn overdue_notice_escape(&mut self) -> bool {
        if self.overdue_notice().is_some() {
            self.mark_inbox_opened();
            return true;
        }
        false
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
            self.show_toast(dc_intl::tr("Bookmark removed."), ToastIcon::Bookmark, cx);
        } else {
            self.show_toast(
                dc_intl::tr("Bookmark added. For your eyes only."),
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
        self.show_toast(dc_intl::tr("Reminder added."), ToastIcon::Clock, cx);
    }

    pub(super) fn complete_reminder(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        self.stores.saved.remove(&(channel, message));
        let _ = self.requests.send(Request::Save {
            channel,
            message,
            due_at: None,
            save: false,
        });
        self.show_toast(dc_intl::tr("Reminder removed."), ToastIcon::Clock, cx);
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
            // `H4gnX9` / `Uq7Y+7`.
            let text = if left >= 0 {
                dc_intl::t!("H4gnX9", duration = humanize(left))
            } else {
                dc_intl::t!("Uq7Y+7", duration = humanize(left))
            };
            rows.push((label(text), 36.));
            rows.push((
                item("reminder-complete".into(), dc_intl::tr("Mark Complete"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = None;
                        this.submenu = None;
                        this.complete_reminder(channel, message, cx);
                    }))
                    .into_any_element(),
                34.,
            ));
            rows.push((label(dc_intl::tr("Edit Reminder").to_owned()), 36.));
        } else {
            rows.push((label(dc_intl::tr("Remind Me").to_owned()), 36.));
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
            item("reminder-custom".into(), dc_intl::tr("Custom..."))
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
        // Module 113367: the toast while it shows, else the overdue reminders (a clock in `TEXT_STRONG`).
        let (message, icon, icon_color): (SharedString, ToastIcon, &str) = match self.toast.as_ref()
        {
            Some(toast) => (toast.message.clone(), toast.icon, "--status-positive"),
            None => (
                self.overdue_notice()?.into(),
                ToastIcon::Clock,
                "--text-strong",
            ),
        };
        let inbox = self.tip_anchors.borrow().get("bar-inbox").copied()?;
        let (icon_file, size) = match icon {
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
                                // The toast blocks no pointer: a web view under it steps aside (`web_frames.rs`).
                                .child(super::web_frames::layer_probe())
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
                                            .text_color(color(icon_color)),
                                    ),
                                )
                                .child(
                                    div()
                                        .whitespace_nowrap()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(message),
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
    fn overdue_reminders_count_and_their_latest_due_time() {
        use super::{overdue_notice_shown, overdue_reminders};
        let due = [
            "2026-11-17T10:00:00+00:00",
            "2026-11-17T12:00:00+00:00",
            "2026-11-19T12:00:00+00:00",
        ];
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-18T00:00:00+00:00")
            .unwrap()
            .timestamp_millis();
        let (count, latest) = overdue_reminders(due.iter().copied(), now);
        assert_eq!(count, 2);
        let noon = chrono::DateTime::parse_from_rfc3339(due[1])
            .unwrap()
            .timestamp_millis();
        assert_eq!(latest, noon);
        // Shown until the inbox is opened after the latest one became due.
        assert!(overdue_notice_shown(count, latest, noon - 1));
        assert!(!overdue_notice_shown(count, latest, noon));
        assert!(!overdue_notice_shown(0, 0, 0));
        assert_eq!(dc_intl::t!("yBmFPA", count = 2usize), "2 overdue reminders");
    }

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
