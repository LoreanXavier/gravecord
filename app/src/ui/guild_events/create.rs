//! The Create Event flow (module 688218): Location, Event Info and Review steps behind a stepper, the Back / Cancel /
//! Next footer, the schedule and recurrence pickers (module 383216) and the final
//! `POST /guilds/{guild}/scheduled-events` (`createGuildEvent`). Measured in the `event-create-*` captures.
//!
//! Left out: the invite the official creates after the event and its success slide, the image crop modal, and the Stage
//! channel option.

use super::super::{Root, avatar_source, color, editor_style, hsla, icon};
use super::schedule::{
    self, Frequency, ScheduleError, date_label, default_start, filter_times, frequency_options,
    iso_utc, parse_time, relative_label, time_label, time_options,
};
use super::{CREATE_EVENTS, CREATE_KEY, MANAGE_EVENTS, field_label, layer, modal_shadow, pickers};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use dc_state::{
    ChannelKind, Id, permissions,
    scheduled_events::{ENTITY_EXTERNAL, ENTITY_NONE, ENTITY_VOICE},
};
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, PathPromptOptions, ScrollHandle,
    SharedString, Subscription, Window, deferred, div, prelude::*, px,
};
use serde_json::{Value, json};

/// The field whose popover is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Field {
    StartDate,
    EndDate,
    StartTime,
    EndTime,
    Frequency,
    Channel,
}

pub(in super::super) struct Create {
    step: usize,
    entity: u64,
    channel: Option<Id>,
    location: Entity<Editor>,
    topic: Entity<Editor>,
    description: Entity<Editor>,
    start_time: Entity<Editor>,
    end_time: Entity<Editor>,
    start: NaiveDateTime,
    end: Option<NaiveDateTime>,
    frequency: Frequency,
    image: Option<String>,
    pub(super) open: Option<Field>,
    /// The 1st of the month the calendar shows.
    month: NaiveDate,
    scroll: ScrollHandle,
    list_scroll: ScrollHandle,
    /// A time field has been typed in since it opened (until then the list is not filtered).
    typed: bool,
    error: Option<String>,
    sending: bool,
    _subscriptions: Vec<Subscription>,
}

impl Create {
    #[cfg(feature = "capture")]
    pub(super) fn editors(&self) -> Vec<Entity<Editor>> {
        vec![
            self.topic.clone(),
            self.location.clone(),
            self.description.clone(),
            self.start_time.clone(),
            self.end_time.clone(),
        ]
    }

    fn external(&self) -> bool {
        self.entity == ENTITY_EXTERNAL
    }

    /// Whether the end date and time are in the form: always for an outside location, else once added.
    fn has_end(&self) -> bool {
        self.external() || self.end.is_some()
    }

    fn entity_ok(&self, cx: &App) -> bool {
        match self.entity {
            ENTITY_EXTERNAL => !self.location.read(cx).text().trim().is_empty(),
            ENTITY_VOICE => self.channel.is_some(),
            _ => false,
        }
    }

    fn schedule_error(&self, now: NaiveDateTime) -> Option<ScheduleError> {
        schedule::schedule_error(Some(self.start), self.end, self.external(), now)
    }

    fn topic_ok(&self, cx: &App) -> bool {
        !self.topic.read(cx).text().trim().is_empty()
    }

    fn step_valid(&self, step: usize, now: NaiveDateTime, cx: &App) -> bool {
        match step {
            0 => self.entity_ok(cx),
            1 => self.schedule_error(now).is_none() && self.topic_ok(cx),
            _ => true,
        }
    }

    /// Every step up to the current one is valid: the next one may open.
    fn can_advance(&self, now: NaiveDateTime, cx: &App) -> bool {
        (0..=self.step).all(|step| self.step_valid(step, now, cx))
    }

    /// `ou`: the start moves, and an end that would now come first follows an hour after it.
    fn set_start(&mut self, start: NaiveDateTime) {
        self.start = start;
        if self.end.is_some_and(|end| end < start) {
            self.end = Some(start + Duration::hours(1));
        }
        if self.frequency == Frequency::Weekday
            && matches!(start.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
        {
            self.frequency = Frequency::None;
        }
    }

    fn set_end(&mut self, end: NaiveDateTime) {
        self.end = Some(end);
    }

    fn picked_date(&self, field: Field) -> NaiveDate {
        match field {
            Field::EndDate | Field::EndTime => self.end.unwrap_or(self.start).date(),
            _ => self.start.date(),
        }
    }
}

/// The local time. Capture runs may fix it (`GRAVECORD_NOW=2026-11-10T13:10:00`) so the date fields match a recorded frame.
fn now() -> NaiveDateTime {
    #[cfg(feature = "capture")]
    if let Some(fixed) = std::env::var("GRAVECORD_NOW")
        .ok()
        .and_then(|text| NaiveDateTime::parse_from_str(&text, "%Y-%m-%dT%H:%M:%S").ok())
    {
        return fixed;
    }
    Local::now().naive_local()
}

/// A text field: the field style of the official forms, 16px text on a 20px line (the description's is 21).
fn field_editor(
    placeholder: &str,
    line_height: f32,
    area: bool,
    limit: usize,
    cx: &mut Context<Root>,
) -> Entity<Editor> {
    cx.new(|cx| {
        let mut style = editor_style();
        style.line_height = line_height;
        style.placeholder = hsla("--input-placeholder-text-default");
        let mut editor = Editor::new(style, cx);
        if area {
            editor.set_enter_inserts_newline(true);
        } else {
            editor.set_single_line(true);
        }
        editor.set_max_utf16(limit);
        editor.set_placeholder(placeholder.to_owned());
        editor.set_reports_all_edits(true);
        editor
    })
}

/// The footer's buttons: 40px, at least 100 wide, as wide as their text otherwise.
fn footer_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    disabled: bool,
    run: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    let (background, border, text) = if primary {
        (
            color("--control-primary-background-default"),
            color("--control-primary-border-default"),
            color("--white"),
        )
    } else {
        (
            color("--control-secondary-background-default"),
            color("--border-muted"),
            color("--control-secondary-text-default"),
        )
    };
    div()
        .id(id)
        .parity(id)
        .h(px(40.))
        .min_w(px(100.))
        .px(px(15.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(text)
        .when(disabled, |this| this.opacity(0.5))
        .child(label)
        .on_click(cx.listener(move |this, _, window, cx| {
            if !disabled {
                run(this, window, cx);
            }
            cx.notify();
        }))
        .into_any_element()
}

/// A 40px field box: 1px `--input-border-default`, or the 2px active ring (drawn inside the box) when it has focus.
fn field_box(id: impl Into<gpui::ElementId>, ring: bool) -> gpui::Stateful<gpui::Div> {
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

/// A click anywhere on a text box puts the caret in its field.
fn focus_on_click(
    editor: &Entity<Editor>,
    cx: &mut Context<Root>,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let editor = editor.clone();
    cx.listener(move |_, _, window, cx| focus(&editor, window, cx))
}

fn is_focused(editor: &Entity<Editor>, window: &Window, cx: &App) -> bool {
    gpui::Focusable::focus_handle(editor.read(cx), cx).is_focused(window)
}

fn focus(editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Root>) {
    window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
}

/// Heading and subtitle of a step: 24/32 semibold over 14/18 subtle, 4px apart.
fn step_heading(title: &'static str, subtitle: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(24.))
                .line_height(px(32.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(title),
        )
        .child(
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-subtle"))
                .child(subtitle),
        )
}

/// The small primary button of the form (Upload Cover Image, Remove Image).
fn small_primary(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(32.))
        .px(px(11.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--control-primary-border-default"))
        .bg(color("--control-primary-background-default"))
        .cursor_pointer()
        .flex()
        .items_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--white"))
        .child(label)
}

impl Root {
    fn event_create_mut(&mut self) -> Option<&mut Create> {
        self.guild_events.as_mut()?.create.as_mut()
    }

    /// The voice channels an event may be set in: where the user may create events, or manage them, and connect.
    fn event_voice_channels(&self, guild: Id) -> Vec<(Id, String)> {
        let (Some(known), Some(me)) = (self.stores.guild(guild), self.stores.me()) else {
            return Vec::new();
        };
        let mut channels: Vec<_> = known
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Voice)
            .filter(|channel| {
                let base = permissions::VIEW_CHANNEL | permissions::CONNECT;
                permissions::can_in_channel(known, channel, me, CREATE_EVENTS | base)
                    || permissions::can_in_channel(known, channel, me, MANAGE_EVENTS | base)
            })
            .collect();
        channels.sort_by_key(|channel| channel.position);
        channels
            .into_iter()
            .map(|channel| (channel.id, channel.name.clone()))
            .collect()
    }

    pub(in super::super) fn open_create_event(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.guild_events.as_ref() else {
            return;
        };
        let _ = open;
        let location = field_editor("Add a location, link, or something.", 20., false, 100, cx);
        let topic = field_editor("What's your event?", 20., false, 100, cx);
        let description = field_editor(
            "Tell people a little more about your event. Markdown, new lines, and links are supported.",
            21.,
            true,
            1000,
            cx,
        );
        let start_time = field_editor("", 20., false, 20, cx);
        let end_time = field_editor("", 20., false, 20, cx);
        let mut subscriptions = Vec::new();
        for editor in [&location, &topic, &description] {
            subscriptions.push(cx.subscribe(editor, |_, _, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Edited) {
                    cx.notify();
                }
            }));
        }
        for (editor, field) in [(&start_time, Field::StartTime), (&end_time, Field::EndTime)] {
            subscriptions.push(cx.subscribe(
                editor,
                move |this, editor, event: &EditorEvent, cx| {
                    match event {
                        EditorEvent::Edited => this.event_time_typed(field, &editor, cx),
                        EditorEvent::Submit(_) => this.close_event_picker(cx),
                        EditorEvent::Cancel => {}
                    }
                    cx.notify();
                },
            ));
        }
        let start = default_start(now());
        let create = Create {
            step: 0,
            entity: ENTITY_NONE,
            channel: None,
            location,
            topic,
            description,
            start_time,
            end_time,
            start,
            end: None,
            frequency: Frequency::None,
            image: None,
            open: None,
            month: start.date().with_day(1).unwrap_or(start.date()),
            scroll: ScrollHandle::new(),
            list_scroll: ScrollHandle::new(),
            typed: false,
            error: None,
            sending: false,
            _subscriptions: subscriptions,
        };
        if let Some(open) = self.guild_events.as_mut() {
            open.create = Some(create);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// What was typed in a time field: the first typing replaces the shown time, as selecting it first would.
    fn event_time_typed(&mut self, field: Field, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let text = editor.read(cx).text().to_owned();
        let Some(create) = self.event_create_mut() else {
            return;
        };
        if create.typed {
            return;
        }
        create.typed = true;
        let shown = time_label(match field {
            Field::EndTime => create.end.unwrap_or(create.start).time(),
            _ => create.start.time(),
        });
        if let Some(rest) = text.strip_prefix(&shown) {
            editor.update(cx, |editor, _| editor.set_text(rest));
        }
        create.list_scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    fn event_goto(&mut self, step: usize, window: &mut Window, cx: &mut Context<Self>) {
        let now = now();
        let Some(create) = self
            .guild_events
            .as_ref()
            .and_then(|open| open.create.as_ref())
        else {
            return;
        };
        if step > 2 || step == create.step || (step > create.step && !create.can_advance(now, cx)) {
            return;
        }
        let topic = create.topic.clone();
        if let Some(create) = self.event_create_mut() {
            create.step = step;
            create.open = None;
            create.error = None;
            create.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        if step == 1 {
            focus(&topic, window, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn event_set_entity(&mut self, entity: u64, guild: Id, cx: &mut Context<Self>) {
        let first = self.event_voice_channels(guild).first().map(|(id, _)| *id);
        let Some(create) = self.event_create_mut() else {
            return;
        };
        create.entity = entity;
        create.channel = if entity == ENTITY_VOICE { first } else { None };
        // `scheduledEndTime` of an outside event starts two hours after the start.
        if entity == ENTITY_EXTERNAL && create.end.is_none() {
            create.end = Some(create.start + Duration::hours(2));
        }
        create.open = None;
        cx.notify();
    }

    fn open_event_picker(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let (editor, label, picked) = {
            let Some(create) = self
                .guild_events
                .as_ref()
                .and_then(|open| open.create.as_ref())
            else {
                return;
            };
            if create.open == Some(field) {
                self.close_event_picker(cx);
                return;
            }
            let (editor, time) = match field {
                Field::StartTime => (Some(create.start_time.clone()), create.start.time()),
                Field::EndTime => (
                    Some(create.end_time.clone()),
                    create.end.unwrap_or(create.start).time(),
                ),
                _ => (None, create.start.time()),
            };
            let picked = match field {
                Field::StartTime | Field::EndTime => {
                    time_options().iter().position(|option| *option == time)
                }
                Field::Frequency => frequency_options(create.start.date())
                    .iter()
                    .position(|(frequency, _)| *frequency == create.frequency),
                Field::Channel => {
                    let guild = self.guild_events.as_ref().map(|open| open.guild);
                    guild.and_then(|guild| {
                        self.event_voice_channels(guild)
                            .iter()
                            .position(|(id, _)| Some(*id) == create.channel)
                    })
                }
                Field::StartDate | Field::EndDate => None,
            };
            (editor, time_label(time), picked)
        };
        // A picker that was open applies what was typed before the next one opens.
        self.close_event_picker(cx);
        if let Some(create) = self.event_create_mut() {
            create.open = Some(field);
            create.typed = false;
            if matches!(field, Field::StartDate | Field::EndDate) {
                let date = create.picked_date(field);
                create.month = date.with_day(1).unwrap_or(date);
            } else {
                create
                    .list_scroll
                    .set_offset(gpui::point(px(0.), px(-pickers::list_offset(picked))));
            }
        }
        if let Some(editor) = editor {
            editor.update(cx, |editor, _| editor.set_text(&label));
            focus(&editor, window, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Closes the open popover; a time typed into its field is applied.
    pub(super) fn close_event_picker(&mut self, cx: &mut Context<Self>) {
        let Some(create) = self.event_create_mut() else {
            return;
        };
        let Some(field) = create.open.take() else {
            return;
        };
        let editor = match field {
            Field::StartTime => create.start_time.clone(),
            Field::EndTime => create.end_time.clone(),
            _ => {
                cx.notify();
                return;
            }
        };
        let typed = editor.read(cx).text().to_owned();
        if let Some(time) = parse_time(&typed) {
            self.event_apply_time(field, time);
        }
        cx.notify();
    }

    fn event_apply_time(&mut self, field: Field, time: NaiveTime) {
        let Some(create) = self.event_create_mut() else {
            return;
        };
        match field {
            Field::StartTime => create.set_start(create.start.date().and_time(time)),
            Field::EndTime => {
                let date = create.end.unwrap_or(create.start).date();
                create.set_end(date.and_time(time));
            }
            _ => {}
        }
    }

    fn event_apply_date(&mut self, field: Field, date: NaiveDate) {
        let Some(create) = self.event_create_mut() else {
            return;
        };
        match field {
            Field::StartDate => create.set_start(date.and_time(create.start.time())),
            Field::EndDate => {
                let time = create.end.unwrap_or(create.start).time();
                create.set_end(date.and_time(time));
            }
            _ => {}
        }
    }

    fn event_pick_image(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(uri) = paths
                    .first()
                    .and_then(|path| super::super::server_settings::data_uri(path))
            {
                let _ = this.update(cx, |this, cx| {
                    this.onboarding_keep_picture(&uri);
                    if let Some(create) = this.event_create_mut() {
                        create.image = Some(uri);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// `createGuildEvent`: the request body of module 688218, with the times in UTC.
    fn create_event_body(&self, cx: &App) -> Option<Value> {
        let create = self.guild_events.as_ref()?.create.as_ref()?;
        let mut body = json!({
            "name": create.topic.read(cx).text().trim(),
            "description": create.description.read(cx).text().trim(),
            "privacy_level": 2,
            "scheduled_start_time": iso_utc(create.start),
            "entity_type": create.entity,
            "recurrence_rule": schedule::recurrence_rule(create.frequency, create.start),
        });
        if let Some(image) = create.image.as_ref() {
            body["image"] = json!(image);
        }
        if let Some(end) = create.end {
            body["scheduled_end_time"] = json!(iso_utc(end));
        }
        if create.external() {
            body["channel_id"] = Value::Null;
            body["entity_metadata"] = json!({"location": create.location.read(cx).text().trim()});
        } else {
            body["channel_id"] = json!(create.channel?.to_string());
        }
        Some(body)
    }

    fn submit_create_event(&mut self, cx: &mut Context<Self>) {
        let now = now();
        let Some(open) = self.guild_events.as_ref() else {
            return;
        };
        let guild = open.guild;
        let Some(create) = open.create.as_ref() else {
            return;
        };
        if create.sending || !(0..=2).all(|step| create.step_valid(step, now, cx)) {
            return;
        }
        let Some(body) = self.create_event_body(cx) else {
            return;
        };
        if let Some(create) = self.event_create_mut() {
            create.sending = true;
            create.error = None;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/scheduled-events"),
            body: Some(body),
        });
        cx.notify();
    }

    /// The answer to the create request: the flow closes on success (the event itself arrives as
    /// `GUILD_SCHEDULED_EVENT_CREATE`), a refusal shows its message under the preview.
    pub(super) fn finish_create_event(&mut self, body: &Value, cx: &mut Context<Self>) {
        #[cfg(feature = "capture")]
        super::proof_log(&format!(
            "create ok={} status={} id={} guild={} message={}",
            body["ok"],
            body["status"],
            body["body"]["id"],
            body["body"]["guild_id"],
            body["body"]["message"]
        ));
        if body["ok"].as_bool() == Some(true) {
            if let Some(event) = body.get("body").filter(|event| event.is_object()) {
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_CREATE", event);
                #[cfg(feature = "capture")]
                self.delete_proof_event(event);
            }
            self.guild_events = None;
        } else if let Some(create) = self.event_create_mut() {
            create.sending = false;
            create.error = Some(
                body["body"]["message"]
                    .as_str()
                    .unwrap_or("Something went wrong. Please try again.")
                    .to_owned(),
            );
        }
        cx.notify();
    }

    /// Capture runs: with `GRAVECORD_EVENT_DELETE_AFTER_CREATE` set, the event just created is deleted again at once
    /// (`DELETE /guilds/{guild}/scheduled-events/{event}`, `deleteGuildEvent`), so a proof of the create request leaves
    /// nothing behind.
    #[cfg(feature = "capture")]
    fn delete_proof_event(&mut self, event: &Value) {
        if std::env::var_os("GRAVECORD_EVENT_DELETE_AFTER_CREATE").is_none() {
            return;
        }
        let (Some(guild), Some(id)) = (event["guild_id"].as_str(), event["id"].as_str()) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{}{guild}:{id}", super::DELETE_KEY)),
            method: reqwest::Method::DELETE,
            route: format!("/guilds/{guild}/scheduled-events/{id}"),
            body: None,
        });
    }

    /// The layers of the flow: the dim modal, and over it the open popover.
    pub(super) fn create_event_layer(
        &self,
        guild: Id,
        create: &Create,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let now = now();
        let tall = create.step == 1 && !create.sending;
        let max_height = (f32::from(window.viewport_size().height) - 46.).max(0.);
        let footer = self.event_footer(create, now, cx);
        let body = match create.step {
            0 => self.event_location_step(guild, create, window, cx),
            1 => self.event_info_step(create, now, window, cx),
            _ => self.event_review_step(guild, create, now, cx),
        };
        let modal = div()
            .parity_with_border("event-create-modal", 1.)
            .id("event-create-modal")
            .occlude()
            .w(px(482.))
            .max_h(px(max_height))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(modal_shadow())
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_shrink_1()
                    .min_h(px(0.))
                    .child(self.anchor_probe("event-scroll"))
                    .child(
                        div()
                            .id("event-create-scroll")
                            .parity("event-create-content")
                            .flex_shrink_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .track_scroll(&create.scroll)
                            .child(
                                div()
                                    .p(px(16.))
                                    .pr(px(24.))
                                    .child(self.event_stepper(create, now, cx))
                                    .child(body),
                            ),
                    )
                    .child(pickers::scrollbar(
                        create.scroll.clone(),
                        2.,
                        4.,
                        3.,
                        "--scrollbar-thin-thumb",
                    )),
            )
            .child(footer);
        let mut layers = vec![layer(
            "event-create-layer",
            if tall { 2. } else { 0. },
            modal,
            |this, _, _| {
                if let Some(open) = this.guild_events.as_mut() {
                    open.create = None;
                }
            },
            cx,
        )];
        layers.extend(self.event_popover(guild, create, now, cx));
        layers
    }

    /// `A`: three equal items with a 4px bar over a 12px label; the current one is brand coloured.
    fn event_stepper(
        &self,
        create: &Create,
        now: NaiveDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = now;
        let items: Vec<AnyElement> = ["Location", "Event Info", "Review"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                let current = index == create.step;
                let tint = if current {
                    "--text-brand"
                } else {
                    "--text-muted"
                };
                div()
                    .id(("event-step", index))
                    .flex_1()
                    .when(index > 0, |this| this.ml(px(8.)))
                    .cursor_pointer()
                    .child(div().h(px(4.)).mb(px(8.)).rounded(px(4.)).bg(color(tint)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color(tint))
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.event_goto(index, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .parity("event-stepper")
            .flex()
            .h(px(28.))
            .children(items)
            .into_any_element()
    }

    fn event_footer(
        &self,
        create: &Create,
        now: NaiveDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let step = create.step;
        let last = step == 2;
        let enabled = create.can_advance(now, cx) && !create.sending;
        div()
            .flex_none()
            .h(px(72.))
            .pt(px(16.))
            .pb(px(16.))
            .px(px(24.))
            .flex()
            .items_center()
            .justify_between()
            .bg(color("--background-surface-high"))
            .child(div().when(step > 0, |this| {
                this.child(
                    div()
                        .id("event-back")
                        .px(px(11.))
                        .cursor_pointer()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child("Back")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.event_goto(step - 1, window, cx);
                        })),
                )
            }))
            .child(
                div()
                    .parity("event-footer-buttons")
                    .flex()
                    .gap(px(8.))
                    .child(footer_button(
                        "event-cancel",
                        "Cancel",
                        false,
                        false,
                        |this, _, _| {
                            if let Some(open) = this.guild_events.as_mut() {
                                open.create = None;
                            }
                        },
                        cx,
                    ))
                    .child(footer_button(
                        "event-next",
                        if last { "Create Event" } else { "Next" },
                        true,
                        !enabled,
                        move |this, window, cx| {
                            if last {
                                this.submit_create_event(cx);
                            } else {
                                this.event_goto(step + 1, window, cx);
                            }
                        },
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Step 1 (`DC+Qm8`): where the event happens.
    fn event_location_step(
        &self,
        guild: Id,
        create: &Create,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let channels = self.event_voice_channels(guild);
        let voice_note =
            if channels.is_empty() {
                if self.stores.guild(guild).is_some_and(|known| {
                    known.channels.iter().any(|c| c.kind == ChannelKind::Voice)
                }) {
                    "You do not have the required server or channel permission."
                } else {
                    "There are no available channels."
                }
            } else {
                "Hang out with voice, video, screenshare, and Go Live."
            };
        let option = |entity: u64,
                      id: &'static str,
                      name: &'static str,
                      title: &'static str,
                      detail: &'static str,
                      disabled: bool| {
            let picked = create.entity == entity;
            div()
                .id(id)
                .parity(id)
                .relative()
                .h(px(42.))
                .pl(px(56.))
                .rounded(px(8.))
                .when(disabled, |this| this.opacity(0.5))
                .when(!disabled, |this| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.event_set_entity(entity, guild, cx);
                        }))
                })
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .child(super::super::create_channel::radio(picked)),
                )
                .child(div().absolute().left(px(32.)).top(px(0.)).child(icon(
                    name,
                    20.,
                    "--text-strong",
                )))
                .child(
                    div()
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .ml(px(-24.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(detail),
                )
        };
        let location_focused = is_focused(&create.location, window, cx);
        let channel_name = create
            .channel
            .and_then(|id| channels.iter().find(|(known, _)| *known == id))
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        div()
            .mt(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(step_heading(
                "Where is your event?",
                "So no one gets lost on where to go.",
            ))
            .child(
                div()
                    .pt(px(8.))
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(option(
                        ENTITY_VOICE,
                        "event-entity-voice",
                        "channel_type_voice",
                        "Voice Channel",
                        voice_note,
                        channels.is_empty(),
                    ))
                    .child(option(
                        ENTITY_EXTERNAL,
                        "event-entity-external",
                        "event_location",
                        "Somewhere Else",
                        "Text channel, external link, or in-person location.",
                        false,
                    )),
            )
            .when(create.external(), |this| {
                this.child(
                    div().child(field_label("Enter a location", true)).child(
                        field_box("event-location", location_focused)
                            .px(px(11.))
                            .cursor_text()
                            .on_click(focus_on_click(&create.location, cx))
                            .child(div().flex_1().min_w(px(0.)).child(create.location.clone())),
                    ),
                )
            })
            .when(create.entity == ENTITY_VOICE, |this| {
                this.child(div().child(field_label("Select a channel", true)).child(
                    self.event_select_field(
                        "event-channel",
                        Field::Channel,
                        channel_name,
                        create.open == Some(Field::Channel),
                        cx,
                    ),
                ))
            })
            .into_any_element()
    }

    /// A select: the picked option's text and a chevron; the active ring while its list is open.
    fn event_select_field(
        &self,
        key: &'static str,
        field: Field,
        text: String,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        field_box(key, open)
            .cursor_pointer()
            .child(self.anchor_probe(key))
            .child(
                div()
                    .flex_1()
                    .pl(px(11.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-strong"))
                    .child(text),
            )
            .child(chevron(open))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_event_picker(field, window, cx);
            }))
            .into_any_element()
    }

    fn event_date_field(
        &self,
        key: &'static str,
        label: &'static str,
        field: Field,
        date: NaiveDate,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w(px(212.))
            .flex_none()
            .child(field_label(label, true))
            .child(
                field_box(key, false)
                    .cursor_pointer()
                    .child(self.anchor_probe(key))
                    .child(
                        div()
                            .flex_1()
                            .pl(px(12.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(date_label(date)),
                    )
                    .child(div().pr(px(13.)).child(icon(
                        "field_calendar",
                        17.,
                        "--interactive-icon-default",
                    )))
                    .when(open, |this| {
                        this.border_color(color("--input-border-default"))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_event_picker(field, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// A time field: the shown time, or its text field while the list is open.
    fn event_time_field(
        &self,
        create: &Create,
        field: Field,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let end = field == Field::EndTime;
        let (key, label, time, editor) = if end {
            (
                "event-end-time",
                "End Time",
                create.end.unwrap_or(create.start).time(),
                &create.end_time,
            )
        } else {
            (
                "event-start-time",
                "Start Time",
                create.start.time(),
                &create.start_time,
            )
        };
        let open = create.open == Some(field);
        div()
            .w(px(212.))
            .flex_none()
            .child(field_label(label, true))
            .child(
                field_box(key, open)
                    .cursor_pointer()
                    .child(self.anchor_probe(key))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(11.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .when(open, |this| this.child(editor.clone()))
                            .when(!open, |this| this.child(time_label(time))),
                    )
                    .child(chevron(open))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_event_picker(field, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// Step 2 (`GG6vbr`): topic, schedule, frequency, description and cover image.
    fn event_info_step(
        &self,
        create: &Create,
        now: NaiveDateTime,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let topic_focused = is_focused(&create.topic, window, cx);
        let description_focused = is_focused(&create.description, window, cx);
        let end = create.end.unwrap_or(create.start);
        let options = frequency_options(create.start.date());
        let frequency = options
            .iter()
            .find(|(frequency, _)| *frequency == create.frequency)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let error = create.schedule_error(now);
        let remove_end = create.end.is_some();
        let end_button = div()
            .id(if remove_end {
                "event-remove-end"
            } else {
                "event-add-end"
            })
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-link"))
            .child(if remove_end {
                "Remove end date and time"
            } else {
                "Add end date and time"
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(create) = this.event_create_mut() {
                    create.end = if remove_end {
                        None
                    } else {
                        Some(create.start + Duration::hours(1))
                    };
                    create.open = None;
                }
                cx.notify();
            }));
        let image = create
            .image
            .as_ref()
            .and_then(|uri| self.images.get(uri).cloned());
        div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(step_heading(
                "What's your event about?",
                "Fill out the details of your event.",
            ))
            .child(
                div()
                    .child(field_label("Event Topic", true))
                    .child(
                        field_box("event-topic", topic_focused)
                            .px(px(11.))
                            .cursor_text()
                            .on_click(focus_on_click(&create.topic, cx))
                            .child(div().flex_1().min_w(px(0.)).child(create.topic.clone())),
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child(self.event_date_field(
                                "event-start-date",
                                "Start Date",
                                Field::StartDate,
                                create.start.date(),
                                create.open == Some(Field::StartDate),
                                cx,
                            ))
                            .child(self.event_time_field(create, Field::StartTime, cx)),
                    )
                    .when(create.has_end(), |this| {
                        this.child(
                            div()
                                .flex()
                                .gap(px(16.))
                                .child(self.event_date_field(
                                    "event-end-date",
                                    "End Date",
                                    Field::EndDate,
                                    end.date(),
                                    create.open == Some(Field::EndDate),
                                    cx,
                                ))
                                .child(self.event_time_field(create, Field::EndTime, cx)),
                        )
                    })
                    .when(!create.external(), |this| this.child(end_button))
                    .child(
                        div()
                            .child(field_label("Event Frequency", true))
                            .child(self.event_select_field(
                                "event-frequency",
                                Field::Frequency,
                                frequency,
                                create.open == Some(Field::Frequency),
                                cx,
                            )),
                    )
                    .children(error.map(|error| {
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-feedback-critical"))
                            .child(error.text())
                    })),
            )
            .child(
                div()
                    .child(field_label("Description", false))
                    .child(
                        div()
                            .id("event-description")
                            .relative()
                            .min_h(px(90.))
                            .cursor_text()
                            .on_click(focus_on_click(&create.description, cx))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .pl(px(10.))
                            .pr(px(10.))
                            .pt(px(12.))
                            .pb(px(12.))
                            .when(description_focused, |this| {
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
                            .child(create.description.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Cover Image"),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child("We recommend an image that\u{2019}s at least 800px wide and 320px tall."),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(8.))
                            .when_some(image, |this, image| {
                                this.child(
                                    div()
                                        .id("event-image-change")
                                        .w(px(440.))
                                        .h(px(176.))
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .cursor_pointer()
                                        .child(gpui::img(image).size_full())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.event_pick_image(cx);
                                        })),
                                )
                            })
                            .child(if create.image.is_some() {
                                small_primary("event-image-remove", "Remove Image")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(create) = this.event_create_mut() {
                                            create.image = None;
                                        }
                                        cx.notify();
                                    }))
                            } else {
                                small_primary("event-image-upload", "Upload Cover Image")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.event_pick_image(cx);
                                    }))
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Step 3 (`yBsFE3`): the event as its card will show it.
    fn event_review_step(
        &self,
        guild: Id,
        create: &Create,
        now: NaiveDateTime,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let name = create.topic.read(cx).text().trim().to_owned();
        let description = create.description.read(cx).text().trim().to_owned();
        let (place_icon, place) = if create.external() {
            (
                "event_location",
                create.location.read(cx).text().trim().to_owned(),
            )
        } else {
            let channel = create.channel.and_then(|id| {
                self.stores
                    .guild(guild)
                    .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
                    .map(|channel| channel.name.clone())
            });
            ("channel_type_voice", channel.unwrap_or_default())
        };
        let subtitle: AnyElement = if create.external() {
            div()
                .child("This event will auto start when it's time.")
                .into_any_element()
        } else {
            div()
                .flex()
                .justify_center()
                .child("When it's time, visit ")
                .child(
                    div()
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(place.clone()),
                )
                .child(" to start the event!")
                .into_any_element()
        };
        let card = div()
            .parity_with_border("event-card", 1.)
            .w_full()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-mod-subtle"))
            .p(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(20.))
                    .mb(px(12.))
                    .flex()
                    .items_center()
                    .child(icon("events", 20., "--text-brand"))
                    .child(
                        div()
                            .ml(px(8.))
                            .flex_1()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(relative_label(create.start, now)),
                    )
                    .child(match avatar {
                        Some(image) => gpui::img(image)
                            .size(px(20.))
                            .rounded_full()
                            .into_any_element(),
                        None => div()
                            .size(px(20.))
                            .rounded_full()
                            .bg(color("--background-brand"))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .ml(px(12.))
                            .h(px(20.))
                            .px(px(8.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lowest"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("event_rsvp", 12., "--text-default"))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child("0"),
                            ),
                    ),
            )
            .child(
                div()
                    .min_h(px(32.))
                    .pt(px(1.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .when(!description.is_empty(), |this| {
                        this.child(
                            div()
                                .mt(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child(description),
                        )
                    }),
            )
            .child(
                div()
                    .mt(px(16.))
                    .mb(px(12.))
                    .mx(px(-16.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .child(
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .child(icon(place_icon, 20., "--text-default"))
                    .child(
                        div()
                            .ml(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(place),
                    ),
            );
        div()
            .pt(px(24.))
            .child(card)
            .child(
                div()
                    .pt(px(24.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .child(
                        div()
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Here's a preview of your event."),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-subtle"))
                            .child(subtitle),
                    )
                    .children(create.error.as_ref().map(|error| {
                        div()
                            .mt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(SharedString::from(error.clone()))
                    })),
            )
            .into_any_element()
    }

    /// The open popover over a transparent layer that closes it: the calendar under a date field, the list under a
    /// time, frequency or channel field (above it when it would not fit in the form).
    fn event_popover(
        &self,
        guild: Id,
        create: &Create,
        now: NaiveDateTime,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let field = create.open?;
        let key = match field {
            Field::StartDate => "event-start-date",
            Field::EndDate => "event-end-date",
            Field::StartTime => "event-start-time",
            Field::EndTime => "event-end-time",
            Field::Frequency => "event-frequency",
            Field::Channel => "event-channel",
        };
        let anchors = self.tip_anchors.borrow();
        // An anchor is the field inside its 1px border; the popover sits against the outer edge.
        let bounds = anchors.get(key).copied().map(|inner| {
            gpui::Bounds::new(
                gpui::point(inner.left() - px(1.), inner.top() - px(1.)),
                gpui::size(inner.size.width + px(2.), inner.size.height + px(2.)),
            )
        })?;
        let limit = anchors
            .get("event-scroll")
            .map_or(gpui::px(f32::MAX), |scroll| scroll.bottom());
        drop(anchors);
        let (popover, top): (AnyElement, gpui::Pixels) = match field {
            Field::StartDate | Field::EndDate => {
                let (min, selected) = match field {
                    Field::EndDate => (
                        create.start.date(),
                        create.end.unwrap_or(create.start).date(),
                    ),
                    _ => (now.date(), create.start.date()),
                };
                let max = if create.frequency == Frequency::None {
                    now.date() + Duration::days(365)
                } else {
                    now.date() + Duration::days(365 * 5)
                };
                let calendar = pickers::calendar(
                    create.month,
                    selected,
                    min,
                    max,
                    move |this, date, _, _| {
                        this.event_apply_date(field, date);
                        if let Some(create) = this.event_create_mut() {
                            create.open = None;
                        }
                    },
                    |this, step, _, _| {
                        if let Some(create) = this.event_create_mut() {
                            let months =
                                create.month.year() * 12 + create.month.month0() as i32 + step;
                            let (year, month) =
                                (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                            create.month =
                                NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(create.month);
                        }
                    },
                    cx,
                );
                (calendar, bounds.bottom() + px(8.))
            }
            Field::StartTime | Field::EndTime => {
                let options = time_options();
                let typed = create.typed.then(|| match field {
                    Field::EndTime => create.end_time.read(cx).text().to_owned(),
                    _ => create.start_time.read(cx).text().to_owned(),
                });
                let shown = filter_times(&options, typed.as_deref().unwrap_or(""));
                let current = match field {
                    Field::EndTime => create.end.unwrap_or(create.start).time(),
                    _ => create.start.time(),
                };
                let picked = shown.iter().position(|time| *time == current);
                let labels = shown.iter().map(|time| time_label(*time)).collect();
                let list = pickers::dropdown(
                    f32::from(bounds.size.width),
                    labels,
                    picked,
                    &create.list_scroll,
                    move |this, index, _, cx| {
                        if let Some(time) = shown.get(index).copied() {
                            this.event_apply_time(field, time);
                        }
                        if let Some(create) = this.event_create_mut() {
                            create.open = None;
                        }
                        cx.notify();
                    },
                    cx,
                );
                (list, pickers::beside_field(bounds, 210., limit))
            }
            Field::Frequency => {
                let options = frequency_options(create.start.date());
                let picked = options
                    .iter()
                    .position(|(frequency, _)| *frequency == create.frequency);
                let labels = options.iter().map(|(_, label)| label.clone()).collect();
                let list = pickers::dropdown(
                    f32::from(bounds.size.width),
                    labels,
                    picked,
                    &create.list_scroll,
                    move |this, index, _, _| {
                        if let Some(create) = this.event_create_mut() {
                            if let Some((frequency, _)) = options.get(index) {
                                create.frequency = *frequency;
                            }
                            create.open = None;
                        }
                    },
                    cx,
                );
                (list, pickers::beside_field(bounds, 210., limit))
            }
            Field::Channel => {
                let channels = self.event_voice_channels(guild);
                let picked = channels
                    .iter()
                    .position(|(id, _)| Some(*id) == create.channel);
                let labels = channels.iter().map(|(_, name)| name.clone()).collect();
                let list = pickers::dropdown(
                    f32::from(bounds.size.width),
                    labels,
                    picked,
                    &create.list_scroll,
                    move |this, index, _, _| {
                        if let Some(create) = this.event_create_mut() {
                            create.channel = channels.get(index).map(|(id, _)| *id);
                            create.open = None;
                        }
                    },
                    cx,
                );
                (list, pickers::beside_field(bounds, 210., limit))
            }
        };
        Some(
            deferred(
                div()
                    .id("event-popover-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_event_picker(cx);
                        cx.notify();
                    }))
                    .child(div().absolute().left(bounds.left()).top(top).child(popover)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// The chevron button of a select: 36px wide, 20px icon, turned over while the list is open.
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
