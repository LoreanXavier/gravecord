//! Schedule Message: the "+" menu's submenu of quick times, the Schedule Message modal with its date and time pickers
//! (module 896455 with the fields of module 388096 and the pickers 582306 and 201895), the composer's schedule bar
//! (`t6` in module 989161) and the request that schedules the next message instead of sending it (modules 151282 and
//! 138399). The rules, the store and the draft are `dc_state::scheduled_message`. Measured in
//! `design/dom/modals/schedule-message-*` and `design/dom/channel/schedule-set-darker-cozy.json`.
//!
//! Not built: the rows of the inbox's Scheduled list and their four buttons (module 713024 has no capture of a
//! populated list), the upsell modal that a create over the limit opens (module 853861) and the first-use coachmark.

use super::guild_events::{field_label, pickers, schedule as times};
use super::modal::{self, Tone};
use super::saved::ToastIcon;
use super::{Root, color, editor_style, hsla, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::{Request, Upload};
use crate::parity::Parity;
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use dc_state::{Id, scheduled_message as scheduled};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ScrollHandle, SharedString, Subscription,
    Window, deferred, div, prelude::*, px,
};
use serde_json::Value;

/// The key the create request answers under.
const CREATE_KEY: &str = "scheduled-create";
/// The key of the list request (`FETCH_SCHEDULED_MESSAGES`).
pub(super) const FETCH_KEY: &str = "scheduled-fetch";
/// The key prefix of a cancel (`SCHEDULED_MESSAGES_DELETE_*`); the message id follows.
#[cfg(feature = "capture")]
const DELETE_KEY: &str = "scheduled-delete:";

/// The field whose popover is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Date,
    Time,
}

/// The open modal (`896455`).
pub(super) struct Modal {
    channel: Id,
    /// The picked date and time, local.
    value: NaiveDateTime,
    open: Option<Field>,
    /// The 1st of the month the calendar shows.
    month: NaiveDate,
    time: Entity<Editor>,
    list_scroll: ScrollHandle,
    /// The time field has been typed in since it opened (until then the list is not filtered).
    typed: bool,
    _events: Subscription,
}

/// The message a create request holds, for the composer to take it back when the server refuses.
struct Pending {
    channel: Id,
    content: String,
    /// The time the message was scheduled for (the draft's), for the toast.
    when: String,
}

#[derive(Default)]
pub(super) struct State {
    pub modal: Option<Modal>,
    /// The "Schedule Message" row's submenu in the "+" menu is open.
    pub submenu: bool,
    pending: Option<Pending>,
}

/// "Oops! Limit reached." (`RLdUVh`), `3AMt7r` with the limit, Okay (`BddRzS`, the cancel text) and Manage Messages (`lv6bDa`).
fn limit_alert(max: u32) -> super::chat_notices::Alert {
    super::chat_notices::Alert::show(
        "scheduled-limit",
        dc_intl::t("RLdUVh"),
        Some(dc_intl::t!("3AMt7r", max = max).to_string()),
        Some(dc_intl::t("BddRzS").to_owned()),
        Some(dc_intl::t("lv6bDa").to_owned()),
        super::chat_notices::Then::ScheduledInbox,
    )
}

/// The local time. Capture runs may fix it (`GRAVECORD_NOW=2026-11-10T13:10:00`) so the default time matches a recorded
/// frame.
fn now() -> NaiveDateTime {
    #[cfg(feature = "capture")]
    if let Some(fixed) = std::env::var("GRAVECORD_NOW")
        .ok()
        .and_then(|text| NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S").ok())
    {
        return fixed;
    }
    chrono::Local::now().naive_local()
}

/// A 40px field box: 1px `--input-border-default`, or the 2px active ring (drawn inside the box) when it has focus.
fn field_box(id: &'static str, ring: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .h(px(40.))
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(color("--input-border-default"))
        .bg(color("--input-background-default"))
        .flex()
        .items_center()
        .when(ring, |this| {
            this.child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .rounded(px(8.))
                    .border_2()
                    .border_color(color("--input-border-active")),
            )
        })
}

/// The chevron button of the time select: 36px wide, 20px icon, turned over while the list is open.
fn chevron(open: bool) -> AnyElement {
    div()
        .w(px(36.))
        .h(px(38.))
        .pr(px(3.))
        .pt(px(2.))
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui::svg()
                .path("icons/select_chevron.svg")
                .size(px(20.))
                .text_color(color("--text-strong"))
                .when(open, |this| {
                    this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        std::f32::consts::PI,
                    )))
                }),
        )
        .into_any_element()
}

impl Root {
    /// `vK`: scheduling is offered in a private channel, or where the user may send messages.
    pub(super) fn schedule_offered(&self) -> bool {
        match self.current_channel() {
            Some(channel) => self.can_in_channel(channel, dc_state::permissions::SEND_MESSAGES),
            None => self.channel.is_some(),
        }
    }

    /// `canRecordVoiceMessage` (module 87207): the row is left out of the "+" menu in a forum channel, while files
    /// are waiting in the composer, and where the user lacks "Send Voice Messages"; a private channel always has it.
    pub(super) fn record_voice_offered(&self) -> bool {
        // `eP` in module 316576: the row needs the experiment `2026-09-desktop-voice-message-recording`.
        if !self.voice_recording_enabled() || !self.pending_files.is_empty() {
            return false;
        }
        match self.current_channel() {
            Some(channel) => {
                !channel.is_forum()
                    && self.can_in_channel(channel, dc_state::permissions::SEND_VOICE_MESSAGES)
            }
            None => self.channel.is_some(),
        }
    }

    /// `disabled: isFinalizing` in module 316576: the "+" menu keeps the row visible while the previous clip finishes
    /// encoding, but disables starting another recording until finalization completes.
    pub(super) fn record_voice_disabled(&self) -> bool {
        self.voice_recording_finalizing()
    }

    /// The time the next message of this channel is scheduled for (the draft), as ISO text.
    pub(super) fn scheduled_timestamp(&self, channel: Id) -> Option<String> {
        self.stores
            .scheduled_drafts
            .get_scheduled_message(channel)
            .map(|draft| draft.scheduled_timestamp.clone())
    }

    /// `changeScheduledMessage`: the next message of this channel goes out at `when`.
    fn change_schedule(&mut self, channel: Id, when: NaiveDateTime, cx: &mut Context<Self>) {
        self.stores.scheduled_drafts.change(
            channel,
            times::iso_utc(when),
            super::picker::now_ms() as i64,
        );
        cx.notify();
    }

    /// `e0`: opens the modal on `value`, or on the default time.
    pub(super) fn open_schedule_modal(
        &mut self,
        value: Option<NaiveDateTime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(channel) = self.channel else {
            return;
        };
        let value = value.unwrap_or_else(|| scheduled::default_time(now()));
        let time = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            style.placeholder = hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            // Enter applies the typed time and keeps the field's text.
            editor.set_keeps_text(true);
            editor.set_max_utf16(20);
            editor.set_reports_all_edits(true);
            editor
        });
        let events = cx.subscribe(&time, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => this.schedule_time_typed(&editor, cx),
                EditorEvent::Submit(_) => this.close_schedule_popover(cx),
                EditorEvent::Cancel => this.schedule_escape(cx),
            }
            cx.notify();
        });
        self.schedule.submenu = false;
        self.schedule.modal = Some(Modal {
            channel,
            value,
            open: None,
            month: value.date().with_day(1).unwrap_or(value.date()),
            time,
            list_scroll: ScrollHandle::new(),
            typed: false,
            _events: events,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The bar was clicked: the modal on the scheduled time (`isEditing`).
    fn edit_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        let value = self
            .scheduled_timestamp(channel)
            .and_then(|iso| scheduled::to_local(&iso));
        self.open_schedule_modal(value, window, cx);
    }

    /// Escape: the open popover first, then the modal.
    pub(super) fn schedule_escape(&mut self, cx: &mut Context<Self>) {
        match self.schedule.modal.as_mut() {
            Some(modal) if modal.open.is_some() => {
                modal.open = None;
                modal.typed = false;
            }
            _ => self.schedule.modal = None,
        }
        cx.notify();
    }

    /// What was typed in the time field: the first typing replaces the shown time, as selecting it first would.
    fn schedule_time_typed(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).text().to_owned();
        let Some(modal) = self.schedule.modal.as_mut() else {
            return;
        };
        if modal.typed || modal.open != Some(Field::Time) {
            return;
        }
        modal.typed = true;
        let shown = times::time_label(modal.value.time());
        if let Some(rest) = text.strip_prefix(&shown) {
            editor.update(cx, |editor, _| editor.set_text(rest));
        }
        modal.list_scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    /// Opens or closes the popover of a field. A time picker that was open applies what was typed first.
    fn toggle_schedule_field(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let Some(modal) = self.schedule.modal.as_ref() else {
            return;
        };
        if modal.open == Some(field) {
            self.close_schedule_popover(cx);
            window.focus(&self.focus, cx);
            return;
        }
        self.close_schedule_popover(cx);
        let Some(modal) = self.schedule.modal.as_mut() else {
            return;
        };
        modal.open = Some(field);
        modal.typed = false;
        match field {
            Field::Date => {
                modal.month = modal.value.date().with_day(1).unwrap_or(modal.value.date());
                window.focus(&self.focus, cx);
            }
            Field::Time => {
                let picked = times::time_options()
                    .iter()
                    .position(|option| *option == modal.value.time());
                modal
                    .list_scroll
                    .set_offset(gpui::point(px(0.), px(-pickers::list_offset(picked))));
                let editor = modal.time.clone();
                let label = times::time_label(modal.value.time());
                editor.update(cx, |editor, _| editor.set_text(&label));
                window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
            }
        }
        cx.notify();
    }

    /// Closes the open popover; a time typed into its field is applied.
    fn close_schedule_popover(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.schedule.modal.as_mut() else {
            return;
        };
        let Some(field) = modal.open.take() else {
            return;
        };
        if field == Field::Time {
            let typed = modal.time.read(cx).text().to_owned();
            if let Some(time) = times::parse_time(&typed) {
                modal.value = modal.value.date().and_time(time);
            }
        }
        modal.typed = false;
        cx.notify();
    }

    /// Set Schedule: the draft takes the picked time and the modal closes.
    fn set_schedule(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.schedule.modal.as_ref() else {
            return;
        };
        let (channel, value) = (modal.channel, modal.value);
        if scheduled::time_error(value, now(), None).is_some() {
            return;
        }
        self.change_schedule(channel, value, cx);
        self.schedule.modal = None;
        cx.notify();
    }

    /// A quick time of the submenu: the next message goes out then.
    fn schedule_quick(&mut self, when: NaiveDateTime, cx: &mut Context<Self>) {
        if let Some(channel) = self.channel {
            self.change_schedule(channel, when, cx);
        }
        self.plus_menu = false;
        self.schedule.submenu = false;
        cx.notify();
    }

    /// The submenu of the "Schedule Message" row (`s` in module 970244): the quick times, a rule, "Custom...".
    pub(super) fn schedule_submenu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.schedule.submenu || self.schedule.modal.is_some() {
            return None;
        }
        let anchor = self
            .tip_anchors
            .borrow()
            .get("plus-schedule-row")
            .copied()?;
        let row = |id: SharedString, label: &'static str| {
            div()
                .id(id)
                .h(px(36.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|this| this.bg(color("--background-mod-subtle")))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(label)
        };
        let mut rows: Vec<AnyElement> = scheduled::quick_times(now())
            .into_iter()
            .map(|(label, when)| {
                row(SharedString::from(format!("schedule-quick-{label}")), label)
                    .on_click(cx.listener(move |this, _, _, cx| this.schedule_quick(when, cx)))
                    .into_any_element()
            })
            .collect();
        rows.push(
            div()
                .my(px(4.))
                .h(px(1.))
                .bg(color("--border-subtle"))
                .into_any_element(),
        );
        rows.push(
            row("schedule-custom".into(), "Custom...")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.plus_menu = false;
                    this.open_schedule_modal(None, window, cx);
                }))
                .into_any_element(),
        );
        // Four rows of 36px, the rule (1px between 4px margins), 8px padding and the 1px border; kept inside the window.
        let height = 2. + 16. + 4. * 36. + 9.;
        let top = (anchor.top() - px(9.))
            .min(window.viewport_size().height - px(8. + height))
            .max(px(8.));
        Some(
            div()
                .id("schedule-submenu")
                .occlude()
                .absolute()
                .left(anchor.right() + px(13.))
                .top(top)
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
                .children(rows)
                .into_any_element(),
        )
    }

    /// The layers of the modal: the dim modal, and over it the open popover.
    pub(super) fn schedule_modal_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(modal) = self.schedule.modal.as_ref() else {
            return Vec::new();
        };
        let current = now();
        let error = scheduled::time_error(modal.value, current, None);
        let time_open = modal.open == Some(Field::Time);
        let date_field = div()
            .flex_1()
            .min_w(px(0.))
            .child(field_label("Date", true))
            .child(
                field_box("schedule-date", false)
                    .parity_with_border("schedule-date-field", 1.)
                    .cursor_pointer()
                    .child(self.anchor_probe("schedule-date"))
                    .child(
                        div()
                            .flex_1()
                            .pl(px(12.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(times::date_label(modal.value.date())),
                    )
                    .child(div().pr(px(12.)).child(icon(
                        "field_calendar",
                        18.,
                        "--interactive-icon-default",
                    )))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_schedule_field(Field::Date, window, cx);
                    })),
            );
        let time_field = div()
            .flex_1()
            .min_w(px(0.))
            .child(field_label("Time", true))
            .child(
                field_box("schedule-time", time_open)
                    .parity_with_border("schedule-time-field", 1.)
                    .cursor_pointer()
                    .child(self.anchor_probe("schedule-time"))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(12.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .when(time_open, |this| this.child(modal.time.clone()))
                            .when(!time_open, |this| {
                                this.child(times::time_label(modal.value.time()))
                            }),
                    )
                    .child(chevron(time_open))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_schedule_field(Field::Time, window, cx);
                    })),
            )
            .children(error.map(|error| {
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error.text())
            }));
        let body = div()
            .h(px(84.))
            .flex()
            .gap(px(16.))
            .child(date_field)
            .child(time_field)
            .into_any_element();
        let set = modal::button(
            "schedule-set",
            "Set Schedule",
            if error.is_some() {
                Tone::PrimaryDisabled
            } else {
                Tone::Primary
            },
            |this, _, cx| this.set_schedule(cx),
        );
        let spec = modal::Spec {
            id: "schedule-message",
            title: "Schedule Message".into(),
            subtitle: None,
            body: Some(body),
            buttons: vec![
                modal::button(
                    "schedule-cancel",
                    "Cancel",
                    Tone::Secondary,
                    |this, _, _| {
                        this.schedule.modal = None;
                    },
                ),
                set,
            ],
            close: Box::new(|this, _, _| this.schedule.modal = None),
        };
        let mut layers = vec![modal::render(spec, cx)];
        layers.extend(self.schedule_popover(modal, current, window, cx));
        layers
    }

    /// The popover of the open field: the calendar under the date, the list under (or over) the time.
    fn schedule_popover(
        &self,
        modal: &Modal,
        current: NaiveDateTime,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let field = modal.open?;
        let key = match field {
            Field::Date => "schedule-date",
            Field::Time => "schedule-time",
        };
        let bounds = self.tip_anchors.borrow().get(key).copied().map(|inner| {
            gpui::Bounds::new(
                gpui::point(inner.left() - px(1.), inner.top() - px(1.)),
                gpui::size(inner.size.width + px(2.), inner.size.height + px(2.)),
            )
        })?;
        let viewport = window.viewport_size().height;
        let (popover, top): (AnyElement, gpui::Pixels) = match field {
            Field::Date => {
                const HEIGHT: f32 = 364.8;
                let calendar = pickers::calendar(
                    modal.month,
                    modal.value.date(),
                    scheduled::earliest(current).date(),
                    scheduled::latest(current, None).date(),
                    |this, date, _, _| {
                        if let Some(modal) = this.schedule.modal.as_mut() {
                            modal.value = date.and_time(modal.value.time());
                            modal.open = None;
                        }
                    },
                    |this, step, _, _| {
                        if let Some(modal) = this.schedule.modal.as_mut() {
                            let months =
                                modal.month.year() * 12 + modal.month.month0() as i32 + step;
                            let (year, month) =
                                (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                            modal.month =
                                NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(modal.month);
                        }
                    },
                    cx,
                );
                // The popper keeps the calendar inside the window, 3px from its bottom edge.
                let below = bounds.bottom() + px(8.);
                (calendar, below.min(viewport - px(HEIGHT + 3.)))
            }
            Field::Time => {
                let options = times::time_options();
                let typed = modal.typed.then(|| modal.time.read(cx).text().to_owned());
                let shown = times::filter_times(&options, typed.as_deref().unwrap_or(""));
                let picked = shown.iter().position(|time| *time == modal.value.time());
                let labels = shown.iter().map(|time| times::time_label(*time)).collect();
                let list = pickers::dropdown(
                    f32::from(bounds.size.width),
                    labels,
                    picked,
                    &modal.list_scroll,
                    move |this, index, _, cx| {
                        if let (Some(modal), Some(time)) =
                            (this.schedule.modal.as_mut(), shown.get(index).copied())
                        {
                            modal.value = modal.value.date().and_time(time);
                            modal.open = None;
                            modal.typed = false;
                        }
                        cx.notify();
                    },
                    cx,
                );
                (list, pickers::beside_field(bounds, 210., viewport))
            }
        };
        Some(
            deferred(
                div()
                    .id("schedule-popover-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_schedule_popover(cx);
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }))
                    .child(div().absolute().left(bounds.left()).top(top).child(popover)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The bar above the text field while the channel has a scheduled time (`t6`): the line is a button that opens the
    /// modal on that time, the cross takes the schedule back.
    pub(super) fn schedule_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        let when = scheduled::to_local(&self.scheduled_timestamp(channel)?)?;
        let label = scheduled::bar_label(when);
        Some(
            div()
                .h(px(36.))
                .pt(px(3.))
                .when(self.replying.is_none(), |this| this.rounded_t(px(8.)))
                .bg(color("--background-surface-higher"))
                .child(
                    div()
                        .id("schedule-bar")
                        .parity("schedule-bar")
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .ml(px(16.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(label),
                        )
                        .child(
                            div()
                                .id("schedule-bar-close")
                                .mr(px(4.))
                                .w(px(40.))
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(icon("reply_close", 16., "--interactive-text-default"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.stores.scheduled_drafts.clear(channel);
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.edit_schedule(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    /// Enter with a schedule set: the message is scheduled instead of sent (`_sendMessage` with
    /// `scheduledTimestamp`). The draft stays until the server has created it.
    pub(super) fn send_scheduled(
        &mut self,
        channel: Id,
        timestamp: String,
        content: String,
        files: Vec<Upload>,
        reply: Option<(Option<u64>, u64, bool)>,
        sticker_ids: Vec<u64>,
    ) {
        self.schedule.pending = Some(Pending {
            channel,
            content: content.clone(),
            when: timestamp.clone(),
        });
        let _ = self.requests.send(Request::Schedule {
            channel,
            content,
            files,
            reply,
            sticker_ids,
            timestamp,
        });
    }

    /// `GET /users/@me/scheduled-messages` (`sy`), asked for when the inbox's Scheduled tab opens.
    pub(super) fn fetch_scheduled_messages(&mut self) {
        self.stores.scheduled_messages.fetch_start();
        let _ = self.requests.send(Request::Rest {
            key: Some(FETCH_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: "/users/@me/scheduled-messages".to_owned(),
            body: None,
        });
    }

    /// The answers to the scheduled-message requests. True when the key was one of them.
    pub(super) fn schedule_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            CREATE_KEY => self.schedule_created(body, cx),
            FETCH_KEY => {
                if body["ok"].as_bool() == Some(true) {
                    let listed = body["body"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(scheduled::ScheduledMessageSend::from_value)
                        .collect();
                    self.stores.scheduled_messages.fetch_success(listed);
                } else {
                    self.stores.scheduled_messages.fetch_failure();
                }
                #[cfg(feature = "capture")]
                proof_log(&format!(
                    "fetch ok={} status={} count={}",
                    body["ok"],
                    body["status"],
                    body["body"].as_array().map_or(0, Vec::len)
                ));
            }
            #[cfg(feature = "capture")]
            key if key.starts_with(DELETE_KEY) => {
                let id = key[DELETE_KEY.len()..].parse::<Id>().unwrap_or_default();
                if body["ok"].as_bool() == Some(true) {
                    self.stores.scheduled_messages.removal_success(id);
                } else {
                    self.stores.scheduled_messages.removal_failure(id);
                }
                proof_log(&format!(
                    "delete ok={} status={}",
                    body["ok"], body["status"]
                ));
                // The proof ends with a list: nothing may be left.
                self.fetch_scheduled_messages();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The answer to the create request (`SCHEDULED_MESSAGES_CREATE_SUCCESS` or `_FAILURE`).
    fn schedule_created(&mut self, body: &Value, cx: &mut Context<Self>) {
        let pending = self.schedule.pending.take();
        #[cfg(feature = "capture")]
        proof_log(&format!(
            "create ok={} status={} code={}",
            body["ok"], body["status"], body["body"]["code"]
        ));
        if body["ok"].as_bool() == Some(true) {
            let created = scheduled::ScheduledMessageSend::from_value(&body["body"]);
            if let Some(pending) = pending.as_ref() {
                // Creating takes the draft away: the bar goes, the next message is sent at once again.
                self.stores.scheduled_drafts.clear(pending.channel);
                if let Some(when) = scheduled::to_local(&pending.when) {
                    self.show_toast(scheduled::scheduled_toast(when), ToastIcon::Clock, cx);
                }
            }
            if let Some(created) = created {
                #[cfg(feature = "capture")]
                self.cancel_proof_message(created.scheduled_message_id);
                self.stores.scheduled_messages.put(created);
            }
            return;
        }
        // The message goes back to the composer, as a failed send keeps its draft.
        if let Some(pending) = pending {
            let empty = self.composer.read(cx).text().is_empty();
            if empty && Some(pending.channel) == self.channel {
                self.composer
                    .update(cx, |composer, _| composer.set_text(&pending.content));
            }
        }
        // `vh` of module 859403: over the limit with Nitro, "Oops! Limit reached." with Manage Messages. GAP: without
        // Nitro the official client opens the upsell modal 853861, which is not built; the toast below stands in.
        if body["body"]["code"].as_u64() == Some(scheduled::TOO_MANY_SCHEDULED_MESSAGES) {
            let (max, upgradable) = scheduled::limit(self.stores.account.premium_type == Some(2));
            if !upgradable {
                self.chat_extra.alert = Some(limit_alert(max));
                cx.notify();
                return;
            }
        }
        let error = body["body"]["message"]
            .as_str()
            .unwrap_or("Something went wrong. Please try again.");
        self.put_app_toast(format!("Failed to schedule message. {error}"), false, cx);
    }

    /// Capture runs: with `GRAVECORD_SCHEDULE_DELETE_AFTER_CREATE` set, the message just scheduled is cancelled at once
    /// (`DELETE /users/@me/scheduled-messages/{id}`, `deleteScheduledMessage`) and the list is read back, so a proof of
    /// the create request leaves nothing behind. The cancel is the official one for a scheduled message.
    #[cfg(feature = "capture")]
    fn cancel_proof_message(&mut self, id: Id) {
        if std::env::var_os("GRAVECORD_SCHEDULE_DELETE_AFTER_CREATE").is_none() {
            return;
        }
        self.stores.scheduled_messages.removal_start(id);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{DELETE_KEY}{id}")),
            method: reqwest::Method::DELETE,
            route: format!("/users/@me/scheduled-messages/{id}"),
            body: None,
        });
    }

    /// The text field of the open modal, for the capture harness to type into.
    #[cfg(feature = "capture")]
    pub(super) fn schedule_editors(&self) -> Vec<Entity<Editor>> {
        self.schedule
            .modal
            .as_ref()
            .filter(|modal| modal.open == Some(Field::Time))
            .map(|modal| modal.time.clone())
            .into_iter()
            .collect()
    }
}

/// Capture runs: one line per answer of the create, cancel and list requests (status and counts only), appended to a
/// file in the temporary directory, so a proof run can be read back.
#[cfg(feature = "capture")]
fn proof_log(line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("gravecord-scheduled-proof.log"))
    {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod limit_tests {
    /// `vh` of module 859403: with Nitro the limit alert names the limit, Okay and Manage Messages.
    #[test]
    fn the_limit_alert_names_the_limit_and_manages_messages() {
        let (max, upgradable) = dc_state::scheduled_message::limit(true);
        assert!(!upgradable);
        let alert = super::limit_alert(max);
        assert_eq!(alert.title, "Oops! Limit reached.");
        let body = &alert.subtitle.as_ref().unwrap()[0].0;
        assert!(body.contains("25 scheduled messages"), "{body}");
        let labels: Vec<&str> = alert.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Okay", "Manage Messages"]);
        assert!(matches!(
            alert.buttons[1].then,
            super::super::chat_notices::Then::ScheduledInbox
        ));
    }
}
