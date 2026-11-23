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
const DELETE_KEY: &str = "scheduled-delete:";
/// The key prefix of Send Now (`POST /users/@me/scheduled-messages/{id}/send`, module 151282 + 859403).
const SEND_NOW_KEY: &str = "scheduled-send:";
/// The key prefix of Update (`PATCH /users/@me/scheduled-messages/{id}`, module 151282 + 859403).
const UPDATE_KEY: &str = "scheduled-update:";

/// The field whose popover is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Date,
    Time,
}

/// Extracts the local creation `NaiveDateTime` from a Discord snowflake ID (`853297.a`, epoch `2015-01-01`).
fn snowflake_created_local(id: Id) -> Option<NaiveDateTime> {
    let ms = ((id >> 22) as i64).checked_add(1_420_070_400_000)?;
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.with_timezone(&chrono::Local).naive_local())
}

/// The open modal (`896455` create schedule, or `23395` update scheduled message when `editing_id.is_some()`).
pub(super) struct Modal {
    channel: Id,
    /// When editing an existing scheduled message's timestamp (module 23395 `SCHEDULED_MESSAGE_EDIT_MODAL_KEY`).
    pub(super) editing_id: Option<Id>,
    /// Opened from the poll editor (module 50518 `ex`): Set Schedule hands the time back to the poll form.
    for_poll: bool,
    /// The poll already has a time, so the secondary button reads Clear (`onClear` in module 896455).
    clearable: bool,
    /// True while the `PATCH /users/@me/scheduled-messages/{id}` update request is in flight.
    submitting: bool,
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

/// `POST /users/@me/scheduled-messages` for a poll (`sendPollMessage` with `scheduledTimestamp`, module 180662): the
/// create body of `pr` carrying the poll under `poll`, with no content.
pub(super) fn scheduled_poll_body(channel: Id, timestamp: &str, poll: &Value) -> Value {
    let mut body = scheduled::create_body(scheduled::CreateRequest {
        channel,
        content: "",
        scheduled_timestamp: timestamp,
        flags: None,
        message_reference: None,
        allowed_mentions: None,
        sticker_ids: &[],
        attachments: Vec::new(),
    });
    body["poll"] = poll.clone();
    body
}

/// The message a create request holds, for the composer to take it back when the server refuses.
struct Pending {
    channel: Id,
    content: String,
    /// The time the message was scheduled for (the draft's), for the toast.
    when: String,
    /// A scheduled poll: the composer's own scheduled draft stays.
    poll: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub modal: Option<Modal>,
    /// The "Schedule Message" row's submenu in the "+" menu is open.
    pub submenu: bool,
    /// Non-Nitro scheduled-message limit upsell modal (module 853861 / 859403).
    pub upsell_open: bool,
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
            editing_id: None,
            for_poll: false,
            clearable: false,
            submitting: false,
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

    /// Module 50518 `ex`: the modal for the poll editor, on the poll's time (with Clear) or the default time.
    pub(super) fn open_poll_schedule_modal(
        &mut self,
        value: Option<NaiveDateTime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_schedule_modal(value, window, cx);
        if let Some(modal) = self.schedule.modal.as_mut() {
            modal.for_poll = true;
            modal.clearable = value.is_some();
        }
    }

    /// Module 23395 (`SCHEDULED_MESSAGE_EDIT_MODAL_KEY`): opens Update Scheduled Message pre-filled with the
    /// scheduled message's current send timestamp (`sendAtTimestamp.substring(0, 200)`).
    pub(super) fn open_update_scheduled_modal(
        &mut self,
        id: Id,
        send_at_timestamp: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let truncated: String = send_at_timestamp.chars().take(200).collect();
        let value =
            scheduled::to_local(&truncated).unwrap_or_else(|| scheduled::default_time(now()));
        let time = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            style.placeholder = hsla("--input-placeholder-text-default");
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
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
            channel: 0,
            editing_id: Some(id),
            for_poll: false,
            clearable: false,
            submitting: false,
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

    /// Set Schedule (module 896455) or Update (module 23395): validates against `time_error` (with the 30-day
    /// snowflake cap when editing an existing scheduled message).
    fn set_schedule(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.schedule.modal.as_ref() else {
            return;
        };
        if modal.submitting {
            return;
        }
        let (channel, editing_id, value) = (modal.channel, modal.editing_id, modal.value);
        if scheduled::time_error(value, now(), editing_id.and_then(snowflake_created_local))
            .is_some()
        {
            return;
        }
        if let Some(id) = editing_id {
            if let Some(modal) = self.schedule.modal.as_mut() {
                modal.submitting = true;
            }
            let iso = times::iso_utc(value);
            self.update_scheduled_message(id, Some(&iso), None, cx);
            return;
        }
        if self
            .schedule
            .modal
            .as_ref()
            .is_some_and(|modal| modal.for_poll)
        {
            if let Some(form) = self.poll.as_mut() {
                form.scheduled = Some(value);
            }
            self.schedule.modal = None;
            cx.notify();
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
        let is_update = modal.editing_id.is_some();
        let error = scheduled::time_error(
            modal.value,
            current,
            modal.editing_id.and_then(snowflake_created_local),
        );
        let time_open = modal.open == Some(Field::Time);
        let date_field = div()
            .flex_1()
            .min_w(px(0.))
            .child(field_label(dc_intl::t("pSZKvM"), true))
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
            .child(field_label(dc_intl::t("GOmEb8"), true))
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
            if is_update {
                "schedule-update"
            } else {
                "schedule-set"
            },
            if is_update {
                dc_intl::t("VZANAD")
            } else {
                "Set Schedule"
            },
            if error.is_some() || modal.submitting {
                Tone::PrimaryDisabled
            } else {
                Tone::Primary
            },
            |this, _, cx| this.set_schedule(cx),
        );
        let clearable = modal.clearable;
        let spec = modal::Spec {
            id: if is_update {
                "update-scheduled-message"
            } else {
                "schedule-message"
            },
            title: if is_update {
                dc_intl::t("jbdHj3").into()
            } else {
                "Schedule Message".into()
            },
            subtitle: None,
            body: Some(body),
            buttons: vec![
                // `onClear` replaces Cancel with Clear (`VkKicb`), which clears the time and closes.
                modal::button(
                    "schedule-cancel",
                    if clearable {
                        dc_intl::t("VkKicb")
                    } else {
                        dc_intl::t("ETE/oC")
                    },
                    Tone::Secondary,
                    move |this, _, _| {
                        if clearable && let Some(form) = this.poll.as_mut() {
                            form.scheduled = None;
                        }
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
                    scheduled::latest(current, modal.editing_id.and_then(snowflake_created_local))
                        .date(),
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
            poll: false,
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

    /// Posts a poll for later: the create request answers under the same key as a scheduled message.
    pub(super) fn send_scheduled_poll(&mut self, channel: Id, timestamp: String, poll: &Value) {
        self.schedule.pending = Some(Pending {
            channel,
            content: String::new(),
            when: timestamp.clone(),
            poll: true,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: "/users/@me/scheduled-messages".to_owned(),
            body: Some(scheduled_poll_body(channel, &timestamp, poll)),
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
            key if key.starts_with(DELETE_KEY) => {
                let id = key[DELETE_KEY.len()..].parse::<Id>().unwrap_or_default();
                if body["ok"].as_bool() == Some(true) {
                    self.stores.scheduled_messages.removal_success(id);
                    // `Re` of module 859403: `JF/LWn` "Successfully canceled the message."
                    self.show_toast(dc_intl::t("JF/LWn").to_owned(), ToastIcon::Clock, cx);
                } else {
                    self.stores.scheduled_messages.removal_failure(id);
                    let err = body["body"]["message"]
                        .as_str()
                        .unwrap_or("Something went wrong.");
                    self.put_app_toast(dc_intl::t!("sUvyW3", error = err).to_string(), false, cx);
                }
                #[cfg(feature = "capture")]
                {
                    proof_log(&format!(
                        "delete ok={} status={}",
                        body["ok"], body["status"]
                    ));
                    // The proof ends with a list: nothing may be left.
                    self.fetch_scheduled_messages();
                }
            }
            key if key.starts_with(SEND_NOW_KEY) => {
                let id = key[SEND_NOW_KEY.len()..].parse::<Id>().unwrap_or_default();
                if body["ok"].as_bool() == Some(true) {
                    self.stores.scheduled_messages.removal_success(id);
                    // `Ki` of module 859403: `BHCm/d` "Message sent."
                    self.show_toast(dc_intl::t("BHCm/d").to_owned(), ToastIcon::Clock, cx);
                } else {
                    self.stores.scheduled_messages.removal_failure(id);
                    let err = body["body"]["message"]
                        .as_str()
                        .unwrap_or("Something went wrong.");
                    self.put_app_toast(dc_intl::t!("uy++C+", error = err).to_string(), false, cx);
                }
            }
            key if key.starts_with(UPDATE_KEY) => {
                if body["ok"].as_bool() == Some(true) {
                    if let Some(updated) =
                        scheduled::ScheduledMessageSend::from_value(&body["body"])
                    {
                        self.stores.scheduled_messages.put(updated);
                    }
                    if self
                        .schedule
                        .modal
                        .as_ref()
                        .and_then(|modal| modal.editing_id)
                        .is_some()
                    {
                        self.schedule.modal = None;
                    }
                    // `kb` of module 859403: `MXsMRk` "Scheduled message updated"
                    self.show_toast(dc_intl::t("MXsMRk").to_owned(), ToastIcon::Clock, cx);
                } else {
                    if let Some(modal) = self.schedule.modal.as_mut() {
                        modal.submitting = false;
                    }
                    let err = body["body"]["message"]
                        .as_str()
                        .unwrap_or("Something went wrong.");
                    self.put_app_toast(dc_intl::t!("slM6In", error = err).to_string(), false, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Sends a scheduled message immediately (`POST /users/@me/scheduled-messages/{id}/send`, module 151282 + 859403).
    pub(super) fn send_now_scheduled_message(&mut self, id: Id, cx: &mut Context<Self>) {
        self.stores.scheduled_messages.removal_start(id);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{SEND_NOW_KEY}{id}")),
            method: reqwest::Method::POST,
            route: format!("/users/@me/scheduled-messages/{id}/send"),
            body: None,
        });
        cx.notify();
    }

    /// Cancels a scheduled message (`DELETE /users/@me/scheduled-messages/{id}`, module 151282 + 859403).
    pub(super) fn cancel_scheduled_message(&mut self, id: Id, cx: &mut Context<Self>) {
        self.stores.scheduled_messages.removal_start(id);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{DELETE_KEY}{id}")),
            method: reqwest::Method::DELETE,
            route: format!("/users/@me/scheduled-messages/{id}"),
            body: None,
        });
        cx.notify();
    }

    /// Updates a scheduled message (`PATCH /users/@me/scheduled-messages/{id}`, module 151282 `Eg` + module 23395).
    /// Omits `content` when only updating `scheduled_timestamp` (module 23395) so message text is not clobbered.
    pub(super) fn update_scheduled_message(
        &mut self,
        id: Id,
        scheduled_timestamp: Option<&str>,
        content: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{UPDATE_KEY}{id}")),
            method: reqwest::Method::PATCH,
            route: format!("/users/@me/scheduled-messages/{id}"),
            body: Some(update_scheduled_payload(scheduled_timestamp, content)),
        });
        cx.notify();
    }

    /// Non-Nitro scheduled-message limit upsell modal (module 853861 / 859403 `vh`).
    pub(super) fn schedule_upsell_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.schedule.upsell_open {
            return None;
        }
        let title = dc_intl::t!("GNoaxo", premiumMax = 25).to_string();
        let body = dc_intl::t!("YnsakI", premiumMax = 25).to_string();
        Some(
            deferred(
                div()
                    .id("schedule-upsell-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(gpui::rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.schedule.upsell_open = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("schedule-upsell-modal")
                            .w(px(440.))
                            .rounded(px(12.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(14.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(icon("nitro_wheel", 24., "--icon-strong"))
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(gpui::rgb(0x5865f2))
                                            .text_size(px(10.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child("BETA"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_center()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-muted"))
                                    .child(body),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .mt(px(6.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("schedule-upsell-nitro")
                                            .w_full()
                                            .h(px(40.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.schedule.upsell_open = false;
                                                this.open_nitro(cx);
                                            }))
                                            .child(dc_intl::t("PcTCB7")),
                                    )
                                    .child(
                                        div()
                                            .id("schedule-upsell-ok")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.schedule.upsell_open = false;
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("BddRzS")),
                                    ),
                            ),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `schedule-upsell [open|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_schedule_upsell(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        self.schedule.upsell_open = mode.trim() != "close";
        cx.notify();
        if self.schedule.upsell_open {
            "schedule-upsell: open".into()
        } else {
            "schedule-upsell: closed".into()
        }
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
                if !pending.poll {
                    self.stores.scheduled_drafts.clear(pending.channel);
                }
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
        // `vh` of module 859403: over the limit with Nitro -> "Oops! Limit reached." alert;
        // without Nitro (`upgradable`) -> upsell modal 853861 ("Get up to 25 scheduled messages").
        if body["body"]["code"].as_u64() == Some(scheduled::TOO_MANY_SCHEDULED_MESSAGES) {
            let (max, upgradable) = scheduled::limit(self.stores.account.premium_type == Some(2));
            if upgradable {
                self.schedule.upsell_open = true;
            } else {
                self.chat_extra.alert = Some(limit_alert(max));
            }
            cx.notify();
            return;
        }
        let error = body["body"]["message"]
            .as_str()
            .unwrap_or("Something went wrong. Please try again.");
        self.put_app_toast(dc_intl::t!("PsJmUe", error = error).to_string(), false, cx);
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

/// Builds the `PATCH /users/@me/scheduled-messages/{id}` JSON body (module 151282 `Eg` / module 23395): omits
/// absent fields so editing only the timestamp never clobbers `content`.
pub(super) fn update_scheduled_payload(
    scheduled_timestamp: Option<&str>,
    content: Option<&str>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(ts) = scheduled_timestamp {
        map.insert("scheduled_timestamp".to_owned(), serde_json::json!(ts));
    }
    if let Some(body) = content {
        map.insert("content".to_owned(), serde_json::json!(body));
    }
    serde_json::Value::Object(map)
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

    #[test]
    fn update_scheduled_payload_omits_absent_fields_and_keys_match_23395() {
        let ts_only = super::update_scheduled_payload(Some("2026-11-22T12:00:00Z"), None);
        assert_eq!(
            ts_only,
            serde_json::json!({"scheduled_timestamp": "2026-11-22T12:00:00Z"})
        );
        let both = super::update_scheduled_payload(Some("2026-11-22T12:00:00Z"), Some("hello"));
        assert_eq!(
            both,
            serde_json::json!({"scheduled_timestamp": "2026-11-22T12:00:00Z", "content": "hello"})
        );
        assert_eq!(dc_intl::t("jbdHj3"), "Update Scheduled Message");
        assert_eq!(dc_intl::t("VZANAD"), "Update");
        assert_eq!(dc_intl::t("SBcdAN"), "Edit schedule time");
        assert_eq!(dc_intl::t("JLba51"), "Send now");
    }
}

#[cfg(test)]
mod poll_tests {
    /// Module 180662 sends a scheduled poll as a scheduled message: empty content, the time, and the poll under `poll`.
    #[test]
    fn a_scheduled_poll_is_a_scheduled_message_with_the_poll() {
        let poll = serde_json::json!({ "question": { "text": "Lunch?" }, "duration": 24 });
        let body = super::scheduled_poll_body(42, "2026-11-24T12:00:00.000Z", &poll);
        assert_eq!(body["channel_id"], "42");
        assert_eq!(body["content"], "");
        assert_eq!(body["scheduled_timestamp"], "2026-11-24T12:00:00.000Z");
        assert_eq!(body["poll"], poll);
    }
}
