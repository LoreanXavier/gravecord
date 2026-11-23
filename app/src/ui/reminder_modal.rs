//! "Set Custom Reminder" (module 148216): a Date field opening a month calendar (react-datepicker, no dates
//! before today) and a Time select of 15-minute steps (module 201895, labels moment `LT`). Measured in
//! `design/dom/modals/custom-reminder*-darker-cozy.json`.

use super::{Root, color};
use crate::ui::motion::Transitions as _;
use crate::ui::motion::{PopKind, PopSide, Popout as _};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, prelude::*, px,
    rgba, svg,
};

pub(crate) struct CustomReminder {
    pub channel: Id,
    pub message: Id,
    pub date: NaiveDate,
    /// The month the calendar shows, as its first day.
    pub month: NaiveDate,
    pub time: Option<NaiveTime>,
    pub calendar_open: bool,
    pub times_open: bool,
}

impl CustomReminder {
    pub fn new(channel: Id, message: Id) -> Self {
        let today = Local::now().date_naive();
        Self {
            channel,
            message,
            date: today,
            month: today.with_day(1).unwrap_or(today),
            time: None,
            calendar_open: false,
            times_open: false,
        }
    }
}

/// moment `LT` in en-US: `9:00 AM`.
fn time_label(time: NaiveTime) -> String {
    time.format("%-I:%M %p").to_string()
}

fn add_months(first: NaiveDate, months: i32) -> NaiveDate {
    let total = first.year() * 12 + first.month0() as i32 + months;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(first)
}

/// The six weeks the calendar shows for `month`, Sunday first.
pub(crate) fn calendar_days(month: NaiveDate) -> Vec<NaiveDate> {
    let start = month - Duration::days(i64::from(month.weekday().num_days_from_sunday()));
    (0..42)
        .map(|offset| start + Duration::days(offset))
        .collect()
}

impl Root {
    pub(super) fn custom_reminder_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.custom_reminder.as_ref()?;
        let field = |id: &'static str, focused: bool| {
            div()
                .id(id)
                .w(px(211.))
                .h(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(if focused {
                    color("--input-border-active")
                } else {
                    color("--input-border-default")
                })
                .rule(crate::ui::motion::Rule::TextField)
                .bg(color("--input-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
        };
        let label = |text: &'static str| {
            div()
                .h(px(20.))
                .mb(px(8.))
                .flex()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .child(
                    div()
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(text),
                )
                .child(
                    div()
                        .text_color(color("--text-feedback-critical"))
                        .child("*"),
                )
        };
        let date_text = state.date.format("%b %-d, %Y").to_string();
        let time_text = state.time.map(time_label);
        let date_field = field("reminder-date", state.calendar_open)
            .px(px(12.))
            .gap(px(8.))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.custom_reminder.as_mut() {
                    state.calendar_open = !state.calendar_open;
                    state.times_open = false;
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(date_text),
            )
            .child(
                svg()
                    .path("icons/field_calendar.svg")
                    .size(px(18.))
                    .text_color(color("--text-default")),
            );
        let time_field = field("reminder-time", state.times_open)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(state) = this.custom_reminder.as_mut() {
                    state.times_open = !state.times_open;
                    state.calendar_open = false;
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .pl(px(12.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(if time_text.is_some() {
                        color("--text-default")
                    } else {
                        color("--input-placeholder-text-default")
                    })
                    .child(time_text.unwrap_or_else(|| "Select...".to_owned())),
            )
            .child(
                div().w(px(36.)).pr(px(4.)).flex().justify_center().child(
                    svg()
                        .path("icons/select_chevron.svg")
                        .size(px(20.))
                        .text_color(color("--text-strong"))
                        .when(state.times_open, |this| {
                            this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                std::f32::consts::PI,
                            )))
                        }),
                ),
            );
        let body = div()
            .relative()
            .flex()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(label("Date"))
                    .child(date_field),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(label("Time"))
                    .child(time_field),
            )
            .when(state.calendar_open, |this| {
                this.child(self.reminder_calendar(cx))
            })
            .when(state.times_open, |this| this.child(self.reminder_times(cx)))
            .into_any_element();
        let ready = state.time.is_some();
        Some(super::modal::render(
            super::modal::Spec {
                id: "custom-reminder-modal",
                title: "Set Custom Reminder".into(),
                subtitle: Some(div().into_any_element()),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "custom-reminder-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| this.custom_reminder = None,
                    ),
                    super::modal::button(
                        "custom-reminder-save",
                        "Save",
                        super::modal::Tone::Primary,
                        move |this, _, cx| {
                            let Some(state) = this.custom_reminder.take() else {
                                return;
                            };
                            let Some(time) = state.time.filter(|_| ready) else {
                                this.custom_reminder = Some(state);
                                return;
                            };
                            if let Some(due) = Local
                                .from_local_datetime(&state.date.and_time(time))
                                .single()
                            {
                                this.set_reminder(state.channel, state.message, due, cx);
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.custom_reminder = None),
            },
            cx,
        ))
    }

    /// The calendar above the Date field: 322 wide on `--background-surface-higher`, 20px padding, a 13px semibold
    /// month title over a rule, weekday initials, and six weeks of 40px bordered days.
    fn reminder_calendar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.custom_reminder.as_ref() else {
            return div().into_any_element();
        };
        let today = Local::now().date_naive();
        let month = state.month;
        let selected = state.date;
        let can_go_back = month > today.with_day(1).unwrap_or(today);
        let nav = |id: &'static str, back: bool| {
            div()
                .id(id)
                .absolute()
                .top(px(0.))
                .when(back, |this| this.left(px(0.)))
                .when(!back, |this| this.right(px(0.)))
                .size(px(18.))
                .rounded(px(2.))
                .border_1()
                .border_color(rgba(0x121214ff))
                .bg(color("--background-mod-normal"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path("icons/menu_caret.svg")
                        .size(px(14.))
                        .text_color(color("--text-strong"))
                        .when(back, |this| {
                            this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                std::f32::consts::PI,
                            )))
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this.custom_reminder.as_mut() {
                        state.month = add_months(state.month, if back { -1 } else { 1 });
                    }
                    cx.notify();
                }))
        };
        let weeks: Vec<AnyElement> = calendar_days(month)
            .chunks(7)
            .map(|week| {
                div()
                    .flex()
                    .children(week.iter().map(|day| {
                        let day = *day;
                        let available = day >= today;
                        let outside = day.month() != month.month();
                        let chosen = day == selected;
                        div()
                            .id(SharedString::from(format!("day-{day}")))
                            .relative()
                            .size(px(40.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.8))
                            .font_weight(FontWeight(if chosen {
                                700.
                            } else if outside || !available {
                                500.
                            } else {
                                400.
                            }))
                            .text_color(if outside || !available {
                                gpui::hsla(0., 0., 0.98, 0.3)
                            } else {
                                color("--text-strong").into()
                            })
                            .when(!available, |this| this.bg(rgba(0x2c2d32ff)))
                            .when(available, |this| {
                                this.cursor_pointer()
                                    .hover(|style| style.bg(color("--background-mod-normal")))
                                    .rule(crate::ui::motion::Rule::ListBoxItem)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(state) = this.custom_reminder.as_mut() {
                                            state.date = day;
                                            state.calendar_open = false;
                                        }
                                        cx.notify();
                                    }))
                            })
                            .child(day.day().to_string())
                            .when(chosen, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .bottom(px(0.))
                                        .left(px(0.))
                                        .right(px(0.))
                                        .h(px(3.))
                                        .bg(color("--brand-500")),
                                )
                            })
                    }))
                    .into_any_element()
            })
            .collect();
        deferred(
            div()
                .id("reminder-calendar-popover")
                .popout("reminder-calendar", PopKind::Translate, PopSide::Top)
                .absolute()
                .left(px(0.))
                .bottom(px(48.))
                .w(px(322.))
                .p(px(20.))
                .rounded(px(5.))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .occlude()
                .on_click(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.custom_reminder.as_mut() {
                        state.calendar_open = false;
                        cx.notify();
                    }
                }))
                .child(
                    div()
                        .relative()
                        .h(px(41.))
                        .mb(px(10.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .justify_center()
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(month.format("%B %Y").to_string().to_uppercase())
                        .when(can_go_back, |this| this.child(nav("calendar-back", true)))
                        .child(nav("calendar-next", false)),
                )
                .child(
                    div().flex().mb(px(10.)).children(
                        ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
                            .into_iter()
                            .map(|name| {
                                div()
                                    .w(px(40.))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .line_height(px(13.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(gpui::hsla(0., 0., 0.98, 0.6))
                                    .child(name.to_uppercase())
                            }),
                    ),
                )
                .children(weeks),
        )
        .with_priority(3)
        .into_any_element()
    }

    /// The Time list: every 15 minutes from midnight, 40px rows in a 208px scrolling box under the field.
    fn reminder_times(&self, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.custom_reminder.as_ref().and_then(|state| state.time);
        let times =
            (0..96).filter_map(|step| NaiveTime::from_hms_opt(step / 4, (step % 4) * 15, 0));
        deferred(
            div()
                .id("reminder-times-popover")
                .absolute()
                .left(px(219.))
                .top(px(72.))
                .w(px(211.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .occlude()
                .on_click(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    if let Some(state) = this.custom_reminder.as_mut() {
                        state.times_open = false;
                        cx.notify();
                    }
                }))
                .p(px(4.))
                .child(
                    div()
                        .id("reminder-times")
                        .h(px(200.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .children(times.map(|time| {
                            let selected = chosen == Some(time);
                            div()
                                .id(SharedString::from(format!("time-{time}")))
                                .flex_none()
                                .h(px(40.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-strong"))
                                .when(selected, |this| {
                                    this.bg(color("--interactive-background-selected"))
                                })
                                .hover(|style| style.bg(color("--interactive-background-hover")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(state) = this.custom_reminder.as_mut() {
                                        state.time = Some(time);
                                        state.times_open = false;
                                    }
                                    cx.notify();
                                }))
                                .child(time_label(time))
                        })),
                ),
        )
        .with_priority(3)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{add_months, calendar_days, time_label};
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn calendar_starts_on_sunday_with_six_weeks() {
        let november = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let days = calendar_days(november);
        assert_eq!(days.len(), 42);
        assert_eq!(days[0], november);
        assert_eq!(days[29].to_string(), "2026-11-30");
        assert_eq!(add_months(november, 2).to_string(), "2027-01-01");
        assert_eq!(add_months(november, -11).to_string(), "2025-12-01");
        assert_eq!(
            time_label(NaiveTime::from_hms_opt(0, 15, 0).unwrap()),
            "12:15 AM"
        );
    }
}
