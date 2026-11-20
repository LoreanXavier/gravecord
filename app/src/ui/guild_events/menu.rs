//! The "..." menu of an event (module 924037), its Add to Calendar submenu, the Cancel Event, Start Event and End
//! Event dialogs, and the requests behind them (`cancelGuildEvent`, `startEvent`, `endEvent` of module 496092).
//! Measured in `events-list-more-menu` and `event-detail-more-menu`; the submenu and the Start and End dialogs have no
//! capture and are built from the modules.

use super::super::modal::{self, Spec, Tone};
use super::super::{Root, color, icon};
use super::list::can_manage;
use super::schedule::local_time;
use crate::net::Request;
use crate::parity::Parity;
use chrono::NaiveDateTime;
use dc_state::{
    Id,
    scheduled_events::{
        ENTITY_EXTERNAL, STATUS_ACTIVE, STATUS_CANCELED, STATUS_COMPLETED, ScheduledEvent,
    },
};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, Pixels, Point, SharedString,
    Window, deferred, div, point, prelude::*, px,
};
use serde_json::{Value, json};

/// Status answers come back under this key, then the guild, the event and the status asked.
pub(super) const STATUS_KEY: &str = "guild-events-status:";
/// The answer to the Cancel Event request: the guild and the event.
pub(super) const CANCEL_KEY: &str = "guild-events-cancel:";

/// The open "..." menu: the event it is for, where it opens, and whether Add to Calendar is open.
pub(in super::super) struct EventMenu {
    pub event: Id,
    pub at: Point<Pixels>,
    pub calendar: bool,
}

/// The dialog over the Events modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Confirm {
    Cancel(Id),
    Start(Id),
    End(Id),
}

/// `encodeURIComponent`.
pub(super) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `qs.stringify` over pairs whose value is present, in order.
fn query(pairs: &[(&str, Option<String>)]) -> String {
    pairs
        .iter()
        .filter_map(|(key, value)| Some(format!("{key}={}", encode(value.as_ref()?))))
        .collect::<Vec<_>>()
        .join("&")
}

/// What the calendar links carry about an event.
pub(super) struct CalendarEvent {
    pub name: String,
    pub description: String,
    pub location: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl CalendarEvent {
    pub(super) fn of(event: &ScheduledEvent, location: String) -> Option<Self> {
        let start = local_time(&event.scheduled_start_time)?;
        Some(Self {
            name: event.name.clone(),
            description: event.description.clone().unwrap_or_default(),
            location,
            start,
            end: event
                .scheduled_end_time
                .as_deref()
                .and_then(local_time)
                .unwrap_or(start),
        })
    }

    /// `YYYYMMDDTHHmmss`.
    fn compact(time: NaiveDateTime) -> String {
        time.format("%Y%m%dT%H%M%S").to_string()
    }

    /// `YYYY-MM-DDTHH:mm:ss`.
    fn dashed(time: NaiveDateTime) -> String {
        time.format("%Y-%m-%dT%H:%M:%S").to_string()
    }

    pub(super) fn google_url(&self) -> String {
        let query = query(&[
            ("text", Some(self.name.clone())),
            (
                "dates",
                Some(format!(
                    "{}/{}",
                    Self::compact(self.start),
                    Self::compact(self.end)
                )),
            ),
            ("details", Some(self.description.clone())),
            ("action", Some("TEMPLATE".to_owned())),
            ("location", Some(self.location.clone())),
        ]);
        format!("https://calendar.google.com/calendar/render?{query}")
    }

    pub(super) fn yahoo_url(&self) -> String {
        let query = query(&[
            ("v", Some("60".to_owned())),
            ("title", Some(self.name.clone())),
            ("st", Some(Self::compact(self.start))),
            ("et", Some(Self::compact(self.end))),
            ("desc", Some(self.description.clone())),
            ("in_loc", Some(self.location.clone())),
        ]);
        format!("https://calendar.yahoo.com/?{query}")
    }

    pub(super) fn outlook_url(&self) -> String {
        let query = query(&[
            ("path", Some("/calendar/action/compose".to_owned())),
            ("rru", Some("addevent".to_owned())),
            ("startdt", Some(Self::dashed(self.start))),
            ("enddt", Some(Self::dashed(self.end))),
            ("subject", Some(self.name.clone())),
            ("body", Some(self.description.clone())),
            ("location", Some(self.location.clone())),
            ("allday", Some("false".to_owned())),
        ]);
        format!("https://outlook.live.com/calendar/0/deeplink/compose?{query}")
    }

    /// The `.ics` file: one `VEVENT` with the times as the wall clock shows them.
    pub(super) fn ics(&self, uid: &str, stamp: NaiveDateTime) -> String {
        fn escape(text: &str) -> String {
            text.replace('\\', "\\\\")
                .replace(';', "\\;")
                .replace(',', "\\,")
                .replace("\r\n", "\\n")
                .replace('\n', "\\n")
        }
        let lines = [
            "BEGIN:VCALENDAR".to_owned(),
            "VERSION:2.0".to_owned(),
            "PRODID:-//Discord//Events//EN".to_owned(),
            "BEGIN:VEVENT".to_owned(),
            format!("UID:{uid}"),
            format!("DTSTAMP:{}", Self::compact(stamp)),
            format!("DTSTART:{}", Self::compact(self.start)),
            format!("DTEND:{}", Self::compact(self.end)),
            format!("SUMMARY:{}", escape(&self.name)),
            format!("DESCRIPTION:{}", escape(&self.description)),
            format!("LOCATION:{}", escape(&self.location)),
            "END:VEVENT".to_owned(),
            "END:VCALENDAR".to_owned(),
        ];
        lines.join("\r\n") + "\r\n"
    }
}

/// `startEvent` and `endEvent`: the body of the status change.
pub(super) fn status_body(status: u64) -> Value {
    json!({"status": status})
}

/// The route of one event.
pub(super) fn event_route(guild: Id, event: Id) -> String {
    format!("/guilds/{guild}/scheduled-events/{event}")
}

/// The link `Copy Event Link` puts on the clipboard (`GUILD_EVENT_DETAILS` on the web host).
pub(super) fn event_link(guild: Id, event: Id) -> String {
    format!("https://discord.com/events/{guild}/{event}")
}

/// The rows of the menu, in the order of module 924037.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    Calendar,
    Start,
    Edit,
    End,
    Cancel,
    CopyLink,
    Report,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Self::Calendar => "Add to Calendar",
            Self::Start => "Start Event",
            Self::Edit => "Edit Event",
            Self::End => "End Event",
            Self::Cancel => "Cancel Event",
            Self::CopyLink => "Copy Event Link",
            Self::Report => "Report Event",
        }
    }
}

fn menu_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., 0.24),
        offset: point(px(0.), px(12.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// One row: 8px of padding round an 18px label, a caret at the end of a row that opens a submenu.
fn row(
    id: impl Into<gpui::ElementId>,
    label: &str,
    danger: bool,
    caret: bool,
) -> gpui::Stateful<gpui::Div> {
    let tint = if danger {
        "--text-feedback-critical"
    } else {
        "--text-strong"
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .p(px(8.))
        .rounded(px(2.))
        .cursor_pointer()
        .hover(move |style| {
            style
                .bg(color(if danger {
                    "--background-feedback-mod-critical"
                } else {
                    "--interactive-background-hover"
                }))
                .rounded(px(4.))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color(tint))
                .child(label.to_owned()),
        )
        .when(caret, |this| {
            this.child(div().flex_none().ml(px(8.)).child(icon(
                "menu_caret_right",
                20.,
                "--icon-subtle",
            )))
        })
}

fn panel() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .p(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-higher"))
        .shadow(menu_shadow())
}

impl Root {
    pub(super) fn open_event_menu(&mut self, event: Id, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.close_all_context_menus();
        if let Some(open) = self.guild_events.as_mut() {
            open.menu = Some(EventMenu {
                event,
                at,
                calendar: false,
            });
        }
        cx.notify();
    }

    pub(in crate::ui) fn clear_event_menu(&mut self) {
        if let Some(open) = self.guild_events.as_mut() {
            open.menu = None;
        }
    }

    fn close_event_menu(&mut self, cx: &mut Context<Self>) {
        self.clear_event_menu();
        cx.notify();
    }

    /// The rows the user may use on this event, in the two groups of the menu.
    fn event_menu_items(&self, event: &ScheduledEvent) -> (Vec<Item>, Vec<Item>) {
        let manage = self
            .stores
            .guild(event.guild_id)
            .zip(self.stores.me())
            .is_some_and(|(guild, me)| can_manage(guild, me, event));
        (menu_items(event, manage), vec![Item::Report])
    }

    /// The menu and, beside it, the Add to Calendar submenu.
    pub(super) fn event_menu_layers(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(open) = self.guild_events.as_ref() else {
            return Vec::new();
        };
        let Some(menu) = open.menu.as_ref() else {
            return Vec::new();
        };
        let Some(event) = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(menu.event)
            .cloned()
        else {
            return Vec::new();
        };
        let (first, second) = self.event_menu_items(&event);
        let target = event.id;
        let item_row = |item: Item, cx: &mut Context<Self>| -> AnyElement {
            let caret = item == Item::Calendar;
            let danger = matches!(item, Item::Cancel | Item::Report | Item::End);
            let mut element = row(item.label(), item.label(), danger, caret)
                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                    if !*inside {
                        return;
                    }
                    if let Some(menu) = this
                        .guild_events
                        .as_mut()
                        .and_then(|open| open.menu.as_mut())
                        && menu.calendar != (item == Item::Calendar)
                    {
                        menu.calendar = item == Item::Calendar;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.event_menu_pick(item, target, window, cx);
                }));
            if item == Item::Calendar {
                element = element.child(self.anchor_probe("event-menu-calendar"));
            }
            element.into_any_element()
        };
        let mut body = panel()
            .parity_with_border("event-menu", 1.)
            .absolute()
            .occlude()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(first.iter().map(|item| item_row(*item, cx))),
            )
            .child(div().my(px(8.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(second.iter().map(|item| item_row(*item, cx))),
            );
        // The menu opens at the click and keeps 28px from the bottom of the window.
        let height = 2.
            + 16.
            + first
                .iter()
                .map(|item| if *item == Item::Calendar { 36. } else { 34. })
                .sum::<f32>()
            + 17.
            + 34. * second.len() as f32;
        let bottom = f32::from(window.viewport_size().height) - 28.;
        let right = f32::from(window.viewport_size().width) - 28.;
        let top = f32::from(menu.at.y).min(bottom - height).max(8.);
        let left = f32::from(menu.at.x).min(right - 160.).max(8.);
        body = body
            .left(px(left))
            .top(px(top))
            .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                let in_cal = this
                    .guild_events
                    .as_ref()
                    .and_then(|open| open.menu.as_ref())
                    .is_some_and(|menu| menu.calendar)
                    && this
                        .tip_anchors
                        .borrow()
                        .get("event-calendar-submenu")
                        .is_some_and(|bounds| bounds.contains(&event.position));
                if !in_cal && !this.keys.outside_closed_this_press {
                    this.keys.outside_closed_this_press = true;
                    this.close_event_menu(cx);
                    if event.button != gpui::MouseButton::Right {
                        cx.stop_propagation();
                    }
                }
            }));
        let mut layers = vec![
            deferred(
                div()
                    .id("event-menu-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(body),
            )
            .with_priority(2)
            .into_any_element(),
        ];
        if menu.calendar && first.contains(&Item::Calendar) {
            layers.push(self.event_calendar_submenu(left, top, target, cx));
        }
        layers
    }

    /// Add to Calendar: Google, the `.ics` file, Yahoo and Outlook, beside the row.
    fn event_calendar_submenu(
        &self,
        menu_left: f32,
        menu_top: f32,
        event: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let anchor = self
            .tip_anchors
            .borrow()
            .get(&SharedString::from("event-menu-calendar"))
            .copied();
        let (left, top) = match anchor {
            Some(bounds) => (f32::from(bounds.right()) + 8., f32::from(bounds.top()) - 8.),
            None => (menu_left + 160., menu_top + 9.),
        };
        let items = [
            ("Add to Google Calendar", 0),
            ("Download ICS file", 1),
            ("Add to Yahoo", 2),
            ("Add to Outlook", 3),
        ];
        deferred(
            div().id("event-calendar-layer").child(
                panel()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .occlude()
                    .child(self.anchor_probe("event-calendar-submenu"))
                    .children(items.map(|(label, which)| {
                        row(("event-calendar", which), label, false, false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.event_calendar_pick(event, which, cx);
                            }))
                            .into_any_element()
                    })),
            ),
        )
        .with_priority(3)
        .into_any_element()
    }

    /// The text of the "location" the calendar entry carries: the outside place, or the channel in its server.
    fn event_calendar_location(&self, event: &ScheduledEvent) -> String {
        if event.entity_type == ENTITY_EXTERNAL {
            return event.location.clone().unwrap_or_default();
        }
        let channel = event.channel_id.and_then(|id| {
            self.stores
                .guild(event.guild_id)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
                .map(|channel| channel.name.clone())
        });
        let guild = self
            .stores
            .guild(event.guild_id)
            .map(|guild| guild.name.clone());
        match (guild, channel) {
            (Some(guild), Some(channel)) => format!("Discord: {guild} server, {channel}"),
            _ => "Discord".to_owned(),
        }
    }

    fn event_calendar_pick(&mut self, event: Id, which: usize, cx: &mut Context<Self>) {
        let Some(event) = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event)
            .cloned()
        else {
            return;
        };
        let Some(entry) = CalendarEvent::of(&event, self.event_calendar_location(&event)) else {
            return;
        };
        match which {
            0 => cx.open_url(&entry.google_url()),
            2 => cx.open_url(&entry.yahoo_url()),
            3 => cx.open_url(&entry.outlook_url()),
            _ => {
                let text = entry.ics(&event.id.to_string(), chrono::Local::now().naive_local());
                if let Some(folder) = std::env::var_os("USERPROFILE")
                    .map(|home| std::path::PathBuf::from(home).join("Downloads"))
                {
                    let target = free_name(&folder, "discord-event.ics");
                    if let Err(error) = std::fs::write(&target, text) {
                        tracing::warn!("event calendar file not saved: {error}");
                    }
                }
            }
        }
        self.close_event_menu(cx);
    }

    fn event_menu_pick(
        &mut self,
        item: Item,
        event: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            Item::Calendar => {
                if let Some(menu) = self
                    .guild_events
                    .as_mut()
                    .and_then(|open| open.menu.as_mut())
                {
                    menu.calendar = !menu.calendar;
                }
                cx.notify();
                return;
            }
            Item::Start => self.set_event_confirm(Confirm::Start(event)),
            Item::End => self.set_event_confirm(Confirm::End(event)),
            Item::Cancel => self.set_event_confirm(Confirm::Cancel(event)),
            Item::Edit => self.open_edit_event(event, window, cx),
            Item::CopyLink => self.copy_event_link(event, cx),
            // Report Event (module 928658 `uA`): the report modal over the page, the menu closed first.
            Item::Report => {
                if let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) {
                    self.close_event_menu(cx);
                    self.open_report_event(guild, event, cx);
                    return;
                }
            }
        }
        self.close_event_menu(cx);
    }

    fn set_event_confirm(&mut self, confirm: Confirm) {
        if let Some(open) = self.guild_events.as_mut() {
            open.confirm = Some(confirm);
        }
    }

    /// `Copy Event Link`: the link on the clipboard and the "Link Copied!" toast.
    pub(super) fn copy_event_link(&mut self, event: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(event_link(guild, event)));
        self.show_success_toast("Link Copied!".to_owned(), cx);
        self.close_event_menu(cx);
    }

    /// The dialog of a pending Cancel Event, Start Event or End Event.
    pub(super) fn event_confirm_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.guild_events.as_ref()?.confirm?;
        let close = |this: &mut Root, _: &mut Window, _: &mut Context<Root>| {
            if let Some(open) = this.guild_events.as_mut() {
                open.confirm = None;
            }
        };
        Some(match confirm {
            Confirm::Cancel(event) => modal::render_small(
                Spec {
                    id: "event-cancel-confirm",
                    title: "Cancel Event?".into(),
                    subtitle: Some(
                        div()
                            .child("Are you sure you want to cancel this event?")
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        modal::button("event-cancel-never", "Never Mind", Tone::Secondary, close),
                        modal::button(
                            "event-cancel-yes",
                            "Cancel Event",
                            Tone::Critical,
                            move |this, _, cx| this.cancel_event(event, cx),
                        ),
                    ],
                    close: Box::new(close),
                },
                cx,
            ),
            Confirm::End(event) => modal::render_small(
                Spec {
                    id: "event-end-confirm",
                    title: "End Event".into(),
                    subtitle: Some(
                        div()
                            .child("Are you sure you want to end the event?")
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        modal::button("event-end-never", "Cancel", Tone::Secondary, close),
                        modal::button(
                            "event-end-yes",
                            "Yes, end the event",
                            Tone::Critical,
                            move |this, _, cx| this.set_event_status(event, STATUS_COMPLETED, cx),
                        ),
                    ],
                    close: Box::new(close),
                },
                cx,
            ),
            Confirm::Start(event) => {
                let found = self
                    .stores
                    .scheduled_events
                    .get_guild_scheduled_event(event)?;
                let privacy = if found.privacy_level == 1 {
                    "Public"
                } else {
                    "Private"
                };
                modal::render(
                    Spec {
                        id: "event-start-confirm",
                        title: found.name.clone().into(),
                        subtitle: Some(
                            div()
                                .flex()
                                .child("You are about to start this ")
                                .child(div().font_weight(FontWeight(700.)).child(privacy))
                                .child(" event")
                                .into_any_element(),
                        ),
                        body: None,
                        buttons: vec![modal::button(
                            "event-start-yes",
                            "Start Event",
                            Tone::Active,
                            move |this, _, cx| {
                                this.set_event_status(event, STATUS_ACTIVE, cx);
                            },
                        )],
                        close: Box::new(close),
                    },
                    cx,
                )
            }
        })
    }

    /// `startEvent` and `endEvent`: `PATCH` the status of the event.
    pub(super) fn set_event_status(&mut self, event: Id, status: u64, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{STATUS_KEY}{guild}:{event}:{status}")),
            method: reqwest::Method::PATCH,
            route: event_route(guild, event),
            body: Some(status_body(status)),
        });
        cx.notify();
    }

    /// `endEvent` of module 496092 from outside the events pages (Disconnect Actions, module 984624): the event is
    /// marked completed.
    pub(in crate::ui) fn end_guild_event(&mut self, guild: Id, event: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{STATUS_KEY}{guild}:{event}:3")),
            method: reqwest::Method::PATCH,
            route: event_route(guild, event),
            body: Some(status_body(3)),
        });
        cx.notify();
    }

    /// `deleteGuildEvent`: the event is deleted for everyone, after the confirmation.
    pub(super) fn cancel_event(&mut self, event: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{CANCEL_KEY}{guild}:{event}")),
            method: reqwest::Method::DELETE,
            route: event_route(guild, event),
            body: None,
        });
        cx.notify();
    }

    /// The answers to the requests above. Returns whether the key was one of them.
    pub(super) fn event_menu_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = body["ok"].as_bool() == Some(true);
        if let Some(ids) = key.strip_prefix(CANCEL_KEY) {
            if let (true, Some((guild, event))) = (ok, ids.split_once(':')) {
                self.stores.scheduled_events.dispatch(
                    "GUILD_SCHEDULED_EVENT_DELETE",
                    &json!({"id": event, "guild_id": guild}),
                );
                if let Some(open) = self.guild_events.as_mut() {
                    open.confirm = None;
                    if open
                        .detail
                        .as_ref()
                        .is_some_and(|detail| detail.event.to_string() == event)
                    {
                        open.detail = None;
                    }
                }
            }
            cx.notify();
            return true;
        }
        if key.starts_with(STATUS_KEY) {
            if ok && body["body"].is_object() {
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_UPDATE", &body["body"]);
                if let Some(open) = self.guild_events.as_mut() {
                    open.confirm = None;
                }
            }
            cx.notify();
            return true;
        }
        false
    }
}

/// `name`, or `name (1)`, `name (2)` ... until a free path.
fn free_name(folder: &std::path::Path, filename: &str) -> std::path::PathBuf {
    let (stem, extension) = match filename.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_owned(), format!(".{extension}")),
        None => (filename.to_owned(), String::new()),
    };
    let mut target = folder.join(filename);
    let mut copy = 1;
    while target.exists() {
        target = folder.join(format!("{stem} ({copy}){extension}"));
        copy += 1;
    }
    target
}

/// The first group of the menu (modules 924037 and 24661): Start only for an event that has not started or ended, End
/// only for a running event that is not external, Cancel only while the event is not running.
fn menu_items(event: &ScheduledEvent, manage: bool) -> Vec<Item> {
    let active = event.status == STATUS_ACTIVE;
    let over = matches!(event.status, STATUS_COMPLETED | STATUS_CANCELED);
    let mut items = Vec::new();
    if !active {
        items.push(Item::Calendar);
    }
    if manage && !active && !over {
        items.push(Item::Start);
    }
    if manage {
        items.push(Item::Edit);
    }
    if manage && active && event.entity_type != ENTITY_EXTERNAL {
        items.push(Item::End);
    }
    if manage && !active {
        items.push(Item::Cancel);
    }
    items.push(Item::CopyLink);
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 11)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn entry() -> CalendarEvent {
        CalendarEvent {
            name: "Fixture event".to_owned(),
            description: "A test, event.".to_owned(),
            location: "Fixture test location".to_owned(),
            start: at(14, 0),
            end: at(16, 0),
        }
    }

    fn event_of(status: u64, entity: u64) -> ScheduledEvent {
        ScheduledEvent::from_value(&json!({
            "id": "2", "guild_id": "1", "name": "x", "status": status, "entity_type": entity,
            "scheduled_start_time": "2026-11-11T14:00:00+00:00",
        }))
        .unwrap()
    }

    #[test]
    fn end_event_is_not_offered_for_external_events() {
        // Module 24661 returns nothing for an external event, active or not.
        let external = menu_items(&event_of(STATUS_ACTIVE, ENTITY_EXTERNAL), true);
        assert!(!external.contains(&Item::End));
        let voice = menu_items(&event_of(STATUS_ACTIVE, 2), true);
        assert!(voice.contains(&Item::End));
        assert!(!voice.contains(&Item::Start) && !voice.contains(&Item::Cancel));
        // Not offered to a user who cannot manage events, nor for an event that is not running.
        assert!(!menu_items(&event_of(STATUS_ACTIVE, 2), false).contains(&Item::End));
        assert!(!menu_items(&event_of(1, 2), true).contains(&Item::End));
    }

    #[test]
    fn start_event_only_before_it_runs() {
        assert!(menu_items(&event_of(1, ENTITY_EXTERNAL), true).contains(&Item::Start));
        assert!(!menu_items(&event_of(STATUS_COMPLETED, 2), true).contains(&Item::Start));
    }

    #[test]
    fn start_and_end_patch_only_the_status() {
        // Started: 2, ended: 3 (`GuildScheduledEventStatus`); the request is a PATCH of the event's own route.
        assert_eq!(status_body(2), json!({"status": 2}));
        assert_eq!(status_body(3), json!({"status": 3}));
        assert_eq!(
            event_route(1568692564637524109, 1569683390914109440),
            "/guilds/1568692564637524109/scheduled-events/1569683390914109440"
        );
        assert_eq!(STATUS_ACTIVE, 2);
        assert_eq!(STATUS_COMPLETED, 3);
    }

    #[test]
    fn the_link_is_the_web_address_of_the_event() {
        assert_eq!(event_link(5, 7), "https://discord.com/events/5/7");
    }

    #[test]
    fn calendar_links_carry_the_event_as_module_924037_builds_them() {
        let entry = entry();
        assert_eq!(
            entry.google_url(),
            "https://calendar.google.com/calendar/render?text=Fixture%20event\
&dates=20261111T140000%2F20261111T160000&details=A%20test%2C%20event.&action=TEMPLATE\
&location=Fixture%20test%20location"
        );
        assert_eq!(
            entry.yahoo_url(),
            "https://calendar.yahoo.com/?v=60&title=Fixture%20event&st=20261111T140000\
&et=20261111T160000&desc=A%20test%2C%20event.&in_loc=Fixture%20test%20location"
        );
        assert_eq!(
            entry.outlook_url(),
            "https://outlook.live.com/calendar/0/deeplink/compose?path=%2Fcalendar%2Faction%2Fcompose\
&rru=addevent&startdt=2026-11-11T14%3A00%3A00&enddt=2026-11-11T16%3A00%3A00\
&subject=Fixture%20event&body=A%20test%2C%20event.&location=Fixture%20test%20location&allday=false"
        );
    }

    #[test]
    fn the_ics_file_is_one_event() {
        let text = entry().ics("42", at(13, 10));
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        assert!(text.contains("DTSTART:20261111T140000\r\n"));
        assert!(text.contains("DESCRIPTION:A test\\, event.\r\n"));
        assert!(text.ends_with("END:VEVENT\r\nEND:VCALENDAR\r\n"));
    }
}
