//! The popovers of the Create Event flow: the dropdown list of the time and select fields (`Dropdown` of module 201895:
//! 5 rows of 40px visible, a check on the picked one) and the date calendar (module 582306, react-datepicker styled by
//! `calendarContainer_tafmh`). Measured in `event-create-time-open`, `repeat-open` and `date-open`.

use super::super::{Root, color, icon};
use chrono::{Datelike, Duration, NaiveDate};
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, ScrollHandle, SharedString,
    Window, canvas, div, fill, point, prelude::*, px, size,
};
use std::rc::Rc;

/// A scroll bar drawn over the right side of a scrolling box: `right` and `inset` are the distances from its edge and
/// from its top and bottom, `token` the thumb colour. The thumb follows `handle`'s offset.
pub(super) fn scrollbar(
    handle: ScrollHandle,
    right: f32,
    width: f32,
    inset: f32,
    token: &'static str,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let max = handle.max_offset().y;
            if max <= px(1.) {
                return;
            }
            let viewport = bounds.size.height;
            let track = viewport - px(inset * 2.);
            let thumb = (track * (viewport / (viewport + max))).max(px(32.));
            let scrolled = (-handle.offset().y / max).clamp(0., 1.);
            let top = bounds.top() + px(inset) + (track - thumb) * scrolled;
            let area = Bounds::new(
                point(bounds.right() - px(right + width), top),
                size(px(width), thumb),
            );
            window.paint_quad(fill(area, color(token)).corner_radii(px(width / 2.)));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Where a popover of `height` goes under (or, when it does not fit above `limit`, over) a field: 4px apart.
pub(super) fn beside_field(field: Bounds<Pixels>, height: f32, limit: Pixels) -> Pixels {
    let below = field.bottom() + px(4.);
    if below + px(height) > limit {
        field.top() - px(4.) - px(height)
    } else {
        below
    }
}

/// How far a list of `count` rows of 40px starts scrolled so the picked row is the last one in view.
pub(super) fn list_offset(picked: Option<usize>) -> f32 {
    picked.map_or(0., |index| (index as f32 * 40. - 160.).max(0.))
}

/// The dropdown: a 1px bordered panel on `--background-surface-higher` with 5 rows in view, the picked one checked.
pub(super) fn dropdown(
    width: f32,
    labels: Vec<String>,
    picked: Option<usize>,
    scroll: &ScrollHandle,
    pick: impl Fn(&mut Root, usize, &mut Window, &mut Context<Root>) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    let pick = Rc::new(pick);
    let rows: Vec<AnyElement> = labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| {
            let pick = pick.clone();
            div()
                .id(("event-choice", index))
                .flex_none()
                .h(px(40.))
                .pl(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-strong"))
                .child(div().flex_1().child(label))
                .when(picked == Some(index), |row| {
                    row.child(div().w(px(24.)).flex().justify_center().child(icon(
                        "select_check",
                        20.,
                        "--text-strong",
                    )))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    pick(this, index, window, cx);
                    cx.notify();
                }))
                .into_any_element()
        })
        .collect();
    div()
        .id("event-dropdown")
        .occlude()
        .relative()
        .w(px(width))
        .h(px(210.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-higher"))
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.16),
            offset: point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(
            div()
                .id("event-dropdown-list")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .pl(px(4.))
                        .pr(px(20.))
                        .py(px(4.))
                        .children(rows),
                ),
        )
        .child(scrollbar(
            scroll.clone(),
            8.,
            8.,
            8.,
            "--scrollbar-auto-thumb",
        ))
        .into_any_element()
}

/// The first of the 42 days the calendar shows for a month: the Sunday on or before the 1st.
fn grid_start(month: NaiveDate) -> NaiveDate {
    let first = month.with_day(1).unwrap_or(month);
    first - Duration::days(i64::from(first.weekday().num_days_from_sunday()))
}

/// The calendar popover for the month `month` (its 1st): 322px wide with 20px padding, the month name and the
/// next/previous buttons over a 7x6 grid of 40px days. Days outside `min..=max` are disabled, days outside the month are
/// dimmed but can be picked, today (`selected`) is bold with a brand bar under it.
#[allow(clippy::too_many_arguments)]
pub(super) fn calendar(
    month: NaiveDate,
    selected: NaiveDate,
    min: NaiveDate,
    max: NaiveDate,
    pick: impl Fn(&mut Root, NaiveDate, &mut Window, &mut Context<Root>) + 'static,
    step_month: impl Fn(&mut Root, i32, &mut Window, &mut Context<Root>) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    let pick = Rc::new(pick);
    let step_month = Rc::new(step_month);
    let start = grid_start(month);
    let has_previous = month > min.with_day(1).unwrap_or(min);
    let has_next = month.with_day(1).unwrap_or(month) < max.with_day(1).unwrap_or(max);
    let dim = gpui::rgba(0xfbfbfb4d);
    let nav = |id: &'static str, forward: bool, show: bool| {
        div()
            .id(id)
            .absolute()
            .top(px(0.))
            .when(forward, |this| this.right(px(0.)))
            .when(!forward, |this| this.left(px(0.)))
            .size(px(18.))
            .when(show, |this| {
                this.rounded(px(2.))
                    .border_1()
                    .border_color(gpui::rgb(0x121214))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(
                        gpui::svg()
                            .path("icons/select_chevron.svg")
                            .size(px(12.))
                            .text_color(color("--text-strong"))
                            .with_transformation(gpui::Transformation::rotate(gpui::radians(
                                if forward {
                                    -std::f32::consts::FRAC_PI_2
                                } else {
                                    std::f32::consts::FRAC_PI_2
                                },
                            ))),
                    )
                    .on_click(cx.listener({
                        let step_month = step_month.clone();
                        move |this, _, window, cx| {
                            step_month(this, if forward { 1 } else { -1 }, window, cx);
                            cx.notify();
                        }
                    }))
            })
    };
    let weeks: Vec<AnyElement> = (0..6)
        .map(|week| {
            let cells: Vec<AnyElement> = (0..7)
                .map(|weekday| {
                    let date = start + Duration::days(week * 7 + weekday);
                    let disabled = date < min || date > max;
                    let outside = date.month() != month.month();
                    let today = date == selected;
                    let pick = pick.clone();
                    div()
                        .id(("event-day", (week * 7 + weekday) as usize))
                        .relative()
                        .flex_none()
                        .w(px(40.))
                        .h(px(40.))
                        .border_t_1()
                        .border_l_1()
                        .border_color(color("--border-subtle"))
                        .text_size(px(12.8))
                        .line_height(px(40.))
                        .text_center()
                        .font_weight(FontWeight(if today {
                            700.
                        } else if disabled || outside {
                            500.
                        } else {
                            400.
                        }))
                        .text_color(if disabled || outside {
                            dim
                        } else {
                            color("--text-strong")
                        })
                        // Measured: rgb(44, 45, 50), no token carries it.
                        .when(disabled, |cell| cell.bg(gpui::rgb(0x2c2d32)))
                        .when(!disabled, |cell| {
                            cell.cursor_pointer().on_click(cx.listener(
                                move |this, _, window, cx| {
                                    pick(this, date, window, cx);
                                    cx.notify();
                                },
                            ))
                        })
                        .child(SharedString::from(date.day().to_string()))
                        .when(today, |cell| {
                            cell.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(px(3.))
                                    .bg(color("--background-brand")),
                            )
                        })
                        .into_any_element()
                })
                .collect();
            div().flex().h(px(40.)).children(cells).into_any_element()
        })
        .collect();
    let names: Vec<AnyElement> = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
        .into_iter()
        .map(|name| {
            div()
                .w(px(40.))
                .flex_none()
                .text_center()
                .child(name)
                .into_any_element()
        })
        .collect();
    div()
        .id("event-calendar")
        .occlude()
        .w(px(322.))
        .p(px(20.))
        .rounded(px(4.8))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-higher"))
        .shadow(vec![
            gpui::BoxShadow {
                color: gpui::hsla(228. / 360., 0.07, 0.13, 0.6),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
            gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.2),
                offset: point(px(0.), px(2.)),
                blur_radius: px(10.),
                spread_radius: px(0.),
                inset: false,
            },
        ])
        .child(
            div()
                .relative()
                .w(px(280.))
                .child(nav("event-calendar-previous", false, has_previous))
                .child(nav("event-calendar-next", true, has_next))
                .child(
                    div()
                        .h(px(40.))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .text_center()
                        .child(SharedString::from(
                            month.format("%B %Y").to_string().to_uppercase(),
                        )),
                )
                // The header's rule: 1px at 0.2 of the darkest background, 20px under the month name.
                .child(div().h(px(1.)).mb(px(10.)).bg(gpui::rgba(0x12121433)))
                .child(
                    div()
                        .flex()
                        .h(px(13.8))
                        .mb(px(10.))
                        .text_size(px(12.8))
                        .line_height(px(12.8))
                        .font_weight(FontWeight(500.))
                        .text_color(gpui::rgba(0xfbfbfb99))
                        .children(names),
                )
                .child(div().h(px(8.)))
                .child(
                    div()
                        .relative()
                        .h(px(240.))
                        .flex()
                        .flex_col()
                        .children(weeks)
                        // The grid's right and bottom edges, 1px rgb(18, 18, 20).
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(gpui::rgb(0x121214)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(1.))
                                .bg(gpui::rgb(0x121214)),
                        ),
                ),
        )
        .into_any_element()
}
