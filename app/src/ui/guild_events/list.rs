//! The event cards of the Events modal (module 327241) and the pieces the card and the detail view share: the status
//! line, the three action buttons, the creator's avatar and the interested toggle. Measured in the `events-list-*`
//! captures.

use super::super::tooltip::{Side, Tip};
use super::super::{Root, avatar_source, color, icon};
use super::schedule::{self, Status, Tone};
use super::{CREATE_EVENTS, MANAGE_EVENTS};
use crate::net::Request;
use crate::parity::Parity;
use chrono::NaiveDateTime;
use dc_state::{
    ChannelKind, Id, User, permissions,
    scheduled_events::{
        ENTITY_EXTERNAL, ENTITY_STAGE_INSTANCE, EventUser, RESPONSE_INTERESTED,
        RESPONSE_UNINTERESTED, STATUS_ACTIVE, STATUS_CANCELED, STATUS_COMPLETED, ScheduledEvent,
    },
};
use gpui::{
    AnyElement, Context, ElementId, FontWeight, IntoElement, Stateful, div, img, prelude::*, px,
};
use serde_json::Value;

/// Module 666394 (`EventRsvpPickerModal`): state for picking series vs single-occurrence RSVP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RsvpPicker {
    pub event: Id,
    pub recurrence_id: String,
    /// True when the user is currently interested in the series (`8MPCVr` "I am not interested in..."),
    /// false when not yet interested (`WtORed` "I am interested in...").
    pub interested_in_series: bool,
    /// `false` = SERIES (`0`, `uoorxi`), `true` = RECURRENCE (`1`, `lwZCFT`). Defaults to `false`.
    pub selected_recurrence: bool,
}

/// Module 666394 (`929267.QC`): decides whether clicking Interested toggles the series directly,
/// toggles an existing recurrence-level RSVP directly, or opens the series-vs-recurrence picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RsvpAction {
    ToggleSeries,
    ToggleRecurrence(String),
    OpenPicker {
        recurrence_id: String,
        interested_in_series: bool,
    },
}

pub(super) fn decide_rsvp_action(
    is_recurring: bool,
    recurrence_status: u64,
    recurrence_id: Option<String>,
    has_recurrence_rsvp: bool,
    interested_in_series: bool,
) -> RsvpAction {
    if !is_recurring || matches!(recurrence_status, STATUS_COMPLETED | STATUS_CANCELED) {
        return RsvpAction::ToggleSeries;
    }
    let Some(rid) = recurrence_id.filter(|s| !s.is_empty()) else {
        return RsvpAction::ToggleSeries;
    };
    if has_recurrence_rsvp {
        return RsvpAction::ToggleRecurrence(rid);
    }
    RsvpAction::OpenPicker {
        recurrence_id: rid,
        interested_in_series,
    }
}

/// `rsvp` answers come back under this key, then the guild, the event and the answer (`1` put, `0` delete).
pub(super) const RSVP_KEY: &str = "guild-events-rsvp:";
/// The answers of the signed-in user to every event of a guild (`getGuildEventsForCurrentUser`).
pub(super) const MINE_KEY: &str = "guild-events-mine:";
/// The interested count of one event.
pub(super) const COUNTS_KEY: &str = "guild-events-counts:";
/// The users interested in one event.
pub(super) const USERS_KEY: &str = "guild-events-users:";

/// The tab of the detail view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Tab {
    Info,
    Interested,
}

/// The open detail view of an event.
pub(in super::super) struct Detail {
    pub event: Id,
    pub tab: Tab,
    /// The occurrence of a recurring event picked in "Events in series" (module 893110), by its start in ms.
    pub recurrence: Option<i64>,
    /// The occurrences the series list shows (`er(4, ...)`, four more per "View future events"), worked out when the
    /// view opens and on each View future events, and whether another follows within four years.
    pub series: Vec<chrono::DateTime<chrono::Utc>>,
    pub series_more: bool,
}

/// Which of the two sizes of the design system button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Sm,
    Md,
}

/// The look of an action button: secondary, or the green `active` one of a set Interested.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Look {
    Secondary,
    Connected,
}

/// A design system button: 32 (sm) or 40 (md) high, its icon before its text, 4px between. An icon alone is square.
pub(super) fn action_button(
    id: impl Into<ElementId>,
    size: Size,
    look: Look,
    glyph: &'static str,
    label: Option<&str>,
    disabled: bool,
) -> Stateful<gpui::Div> {
    let (height, glyph_size, text_size, line, pad) = match size {
        Size::Sm => (32., 16., 14., 18., 11.),
        Size::Md => (40., 20., 16., 20., 15.),
    };
    let (background, hover, border, tint) = match look {
        Look::Secondary => (
            "--control-secondary-background-default",
            "--control-secondary-background-hover",
            "--control-secondary-border-default",
            "--control-secondary-text-default",
        ),
        Look::Connected => (
            "--control-connected-background-default",
            "--control-connected-background-hover",
            "--control-connected-border-default",
            "--control-connected-text-default",
        ),
    };
    div()
        .id(id)
        .h(px(height))
        .flex_none()
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .when(label.is_none(), |this| this.w(px(height)))
        .when(label.is_some(), |this| this.px(px(pad)))
        .when(!disabled, |this| {
            this.cursor_pointer().hover(|style| style.bg(color(hover)))
        })
        .when(disabled, |this| this.opacity(0.5))
        .when(!glyph.is_empty(), |this| {
            this.child(icon(glyph, glyph_size, tint))
        })
        .children(label.map(|label| {
            div()
                .text_size(px(text_size))
                .line_height(px(line))
                .font_weight(FontWeight(500.))
                .text_color(color(tint))
                .child(label.to_owned())
        }))
}

/// `canManageGuildEvent`: a manager of events, or the creator who may create them.
pub(super) fn can_manage(
    guild: &dc_state::Guild,
    me: permissions::Me,
    event: &ScheduledEvent,
) -> bool {
    permissions::can_in_guild(guild, me, permissions::ADMINISTRATOR)
        || permissions::can_in_guild(guild, me, MANAGE_EVENTS)
        || (event.creator_id == Some(me.id) && permissions::can_in_guild(guild, me, CREATE_EVENTS))
}

/// The tint of a status line's icon and text.
fn tones(tone: Tone) -> (&'static str, &'static str) {
    match tone {
        Tone::Plain => ("--icon-brand", "--text-default"),
        Tone::Soon => ("--icon-brand", "--text-brand"),
        Tone::Live => ("--text-feedback-positive", "--text-feedback-positive"),
        Tone::Over => ("--icon-muted", "--text-muted"),
        Tone::Canceled => ("--text-feedback-critical", "--text-default"),
    }
}

/// The status line: the calendar, then the words of `status_line` in 14/18 semibold.
pub(in crate::ui) fn status_row(status: &Status) -> gpui::Div {
    let (glyph, text) = tones(status.tone);
    div()
        .flex()
        .items_center()
        .h(px(20.))
        .child(icon("events", 20., glyph))
        .child(
            div()
                .ml(px(8.))
                .flex()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color(text))
                .children(status.lead.as_ref().map(|lead| {
                    div()
                        .text_color(color("--text-feedback-positive"))
                        .child(lead.clone())
                }))
                .child(status.text.clone()),
        )
}

/// The status of an event for the card (`with_end` false) or the detail (true).
pub(in crate::ui) fn event_status(
    event: &ScheduledEvent,
    now: NaiveDateTime,
    with_end: bool,
) -> Option<Status> {
    let start = schedule::local_time(&event.scheduled_start_time)?;
    let end = event
        .scheduled_end_time
        .as_deref()
        .and_then(schedule::local_time);
    let strings = schedule::time_strings(start, end, now);
    Some(schedule::status_line(
        &strings,
        event.status,
        event.entity_type == ENTITY_EXTERNAL,
        with_end,
    ))
}

/// `1 person is interested`, `2 people are interested`: the plural rule of the detail's count line.
pub(super) fn interested_sentence(count: i64) -> String {
    if count == 1 {
        "1 person is interested".to_owned()
    } else {
        format!("{count} people are interested")
    }
}

impl Root {
    /// A user of an event: the account's own, or one the client has met.
    pub(in crate::ui) fn event_user(&self, id: Id) -> Option<User> {
        self.stores
            .current_user
            .as_ref()
            .filter(|me| me.id == id)
            .or_else(|| self.stores.users.get(&id))
            .cloned()
    }

    /// A round avatar of `size`, or an empty disc while the picture loads.
    pub(in crate::ui) fn event_avatar(&self, user: Option<&User>, size: f32) -> AnyElement {
        let image = user.and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        match image {
            Some(image) => img(image).size(px(size)).rounded_full().into_any_element(),
            None => div()
                .size(px(size))
                .rounded_full()
                .bg(color("--background-brand"))
                .into_any_element(),
        }
    }

    /// Asks for the pictures of the users an event shows, and for the creators the client has not met.
    pub(super) fn event_prepare_users(&mut self, guild: Id, events: &[Id]) {
        let mut wanted: Vec<Id> = Vec::new();
        for event in events {
            if let Some(creator) = self
                .stores
                .scheduled_events
                .get_guild_scheduled_event(*event)
                .and_then(|event| event.creator_id)
            {
                wanted.push(creator);
            }
            wanted.extend(
                self.stores
                    .scheduled_events
                    .get_users_for_guild_event(*event, None)
                    .iter()
                    .map(|user| user.user_id),
            );
        }
        let mut unknown = Vec::new();
        for id in wanted {
            match self.event_user(id) {
                Some(user) => {
                    let (key, url) = avatar_source(&user);
                    self.request_image(key, url);
                }
                None => unknown.push(id),
            }
        }
        unknown.sort_unstable();
        unknown.dedup();
        let asked: Vec<Id> = match self.guild_events.as_mut() {
            Some(open) => unknown
                .into_iter()
                .filter(|id| open.asked.insert(*id))
                .collect(),
            None => Vec::new(),
        };
        if !asked.is_empty() {
            let _ = self.requests.send(Request::GuildMembers {
                guilds: vec![guild],
                query: String::new(),
                limit: 0,
                user_ids: asked,
            });
        }
    }

    /// The answers that complete the list: the count of every event, and what the user said to them.
    pub(super) fn event_fetch_details(&mut self, guild: Id, events: &[Id]) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{MINE_KEY}{guild}"),
            route: format!("/users/@me/scheduled-events?guild_ids={guild}"),
        });
        for event in events {
            self.event_fetch_counts(guild, *event);
        }
    }

    pub(in crate::ui) fn event_fetch_counts(&mut self, guild: Id, event: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{COUNTS_KEY}{guild}:{event}"),
            route: format!("/guilds/{guild}/scheduled-events/{event}/users/counts"),
        });
    }

    pub(super) fn event_fetch_users(&mut self, guild: Id, event: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{USERS_KEY}{guild}:{event}"),
            route: format!(
                "/guilds/{guild}/scheduled-events/{event}/users?limit=100&with_member=true"
            ),
        });
    }

    /// Takes the answers of the keys above into the store. Returns whether the key was one of them.
    pub(super) fn event_details_fetched(&mut self, key: &str, body: &Value) -> bool {
        if key.starts_with(MINE_KEY) {
            if body.is_array() {
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_RSVPS_FETCH_SUCESS", body);
            }
            return true;
        }
        if let Some(ids) = key.strip_prefix(COUNTS_KEY) {
            if let Some((_, event)) = ids.split_once(':') {
                let mut counts = body.clone();
                counts["event_id"] = Value::String(event.to_owned());
                self.stores
                    .scheduled_events
                    .dispatch("GUILD_SCHEDULED_EVENT_USER_COUNTS_FETCH_SUCCESS", &counts);
            }
            return true;
        }
        if let Some(ids) = key.strip_prefix(USERS_KEY) {
            let Some((guild, event)) = ids.split_once(':') else {
                return true;
            };
            let list = body
                .as_array()
                .or_else(|| body["users"].as_array())
                .cloned()
                .unwrap_or_default();
            let mut answers = Vec::new();
            for mut entry in list {
                if let Some(user) = User::from_value(&entry["user"]) {
                    self.stores.users.entry(user.id).or_insert(user);
                }
                entry["guild_scheduled_event_id"] = Value::String(event.to_owned());
                answers.push(entry);
            }
            self.stores.scheduled_events.dispatch(
                "GUILD_SCHEDULED_EVENT_USERS_FETCH_SUCCESS",
                &Value::Array(answers),
            );
            if let (Ok(guild), Ok(event)) = (guild.parse::<Id>(), event.parse::<Id>()) {
                self.event_prepare_users(guild, &[event]);
            }
            return true;
        }
        false
    }

    /// Whether the user is interested in the event (`isInterestedInEventRecurrence`).
    pub(in crate::ui) fn event_interested(&self, event: Id) -> bool {
        self.stores.me().is_some_and(|me| {
            self.stores
                .scheduled_events
                .is_interested_in_event_recurrence(event, None, me.id)
        })
    }

    /// `updateRsvp` of a series or recurrence (`666394` / `929267.QC`): if the event is recurring
    /// and a recurrence is selected without an existing recurrence-level RSVP, opens the RSVP picker
    /// modal; otherwise sends `PUT` / `DELETE /guilds/{guild}/scheduled-events/{event}[/{recurrence}]/users/@me`.
    pub(in crate::ui) fn toggle_event_interest(&mut self, event: Id, cx: &mut Context<Self>) {
        let Some(me) = self.stores.me() else {
            return;
        };
        let known = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event)
            .cloned();
        let is_recurring = known.as_ref().is_some_and(|e| !e.recurrence_rule.is_null());
        let recurrence_id = self
            .guild_events
            .as_ref()
            .and_then(|open| open.detail.as_ref())
            .filter(|d| d.event == event)
            .and_then(|d| d.recurrence)
            .map(|ms| ((ms - 1_420_070_400_000) << 22).to_string())
            .or_else(|| {
                known
                    .as_ref()
                    .filter(|e| !e.recurrence_rule.is_null())
                    .and_then(|e| e.start())
                    .map(|dt| ((dt.timestamp_millis() - 1_420_070_400_000) << 22).to_string())
            });
        let status = known.as_ref().map_or(0, |e| e.status);
        let has_recurrence_rsvp = recurrence_id.as_deref().is_some_and(|rid| {
            self.stores
                .scheduled_events
                .get_rsvp(event, Some(rid), me.id)
                .is_some()
        });
        let interested_in_series = self
            .stores
            .scheduled_events
            .get_rsvp(event, None, me.id)
            .is_some_and(|r| r.response == RESPONSE_INTERESTED);

        match decide_rsvp_action(
            is_recurring,
            status,
            recurrence_id,
            has_recurrence_rsvp,
            interested_in_series,
        ) {
            RsvpAction::ToggleSeries => self.toggle_event_interest_target(event, None, cx),
            RsvpAction::ToggleRecurrence(rid) => {
                self.toggle_event_interest_target(event, Some(rid), cx)
            }
            RsvpAction::OpenPicker {
                recurrence_id,
                interested_in_series,
            } => {
                if let Some(open) = self.guild_events.as_mut() {
                    open.rsvp_picker = Some(RsvpPicker {
                        event,
                        recurrence_id,
                        interested_in_series,
                        selected_recurrence: false,
                    });
                }
                cx.notify();
            }
        }
    }

    pub(super) fn toggle_event_interest_target(
        &mut self,
        event: Id,
        recurrence_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (Some(guild), Some(me)) = (
            self.guild_events.as_ref().map(|open| open.guild),
            self.stores.me(),
        ) else {
            return;
        };
        let rid = recurrence_id.as_deref();
        let interested = self
            .stores
            .scheduled_events
            .is_interested_in_event_recurrence(event, rid, me.id);
        let response_value = if rid.is_some() && interested {
            RESPONSE_UNINTERESTED
        } else {
            RESPONSE_INTERESTED
        };
        let (method, body, answer) = if interested && rid.is_none() {
            self.stores
                .scheduled_events
                .remove_rsvp(event, None, me.id, RESPONSE_INTERESTED);
            (reqwest::Method::DELETE, None, 0)
        } else if interested && rid.is_some() {
            // Opt out of a single occurrence while remaining interested in the series (`response: 0`),
            // or delete the recurrence-level positive RSVP if the user was not interested in the series.
            let in_series = self
                .stores
                .scheduled_events
                .get_rsvp(event, None, me.id)
                .is_some_and(|r| r.response == RESPONSE_INTERESTED);
            if in_series {
                self.stores.scheduled_events.add_rsvp(
                    event,
                    rid,
                    EventUser {
                        user_id: me.id,
                        response: RESPONSE_UNINTERESTED,
                        nick: None,
                    },
                );
                (
                    reqwest::Method::PUT,
                    Some(serde_json::json!({"response": RESPONSE_UNINTERESTED})),
                    1,
                )
            } else {
                self.stores
                    .scheduled_events
                    .remove_rsvp(event, rid, me.id, RESPONSE_INTERESTED);
                (reqwest::Method::DELETE, None, 0)
            }
        } else {
            self.stores.scheduled_events.add_rsvp(
                event,
                rid,
                EventUser {
                    user_id: me.id,
                    response: response_value,
                    nick: None,
                },
            );
            (
                reqwest::Method::PUT,
                Some(serde_json::json!({"response": response_value})),
                1,
            )
        };
        let route = match rid {
            Some(rid) => format!("/guilds/{guild}/scheduled-events/{event}/{rid}/users/@me"),
            None => format!("/guilds/{guild}/scheduled-events/{event}/users/@me"),
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{RSVP_KEY}{guild}:{event}:{answer}")),
            method,
            route,
            body,
        });
        cx.notify();
    }

    /// Module 666394 (`EventRsvpPickerModal`): 440px dialog asking whether the RSVP applies to
    /// `"All events in this series"` (`uoorxi`, 0) or `"Only this event"` (`lwZCFT`, 1).
    pub(super) fn event_rsvp_picker_layer(
        &self,
        picker: &RsvpPicker,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = if picker.interested_in_series {
            dc_intl::t("8MPCVr")
        } else {
            dc_intl::t("WtORed")
        };
        let selected_recurrence = picker.selected_recurrence;
        let event = picker.event;
        let recurrence_id = picker.recurrence_id.clone();

        let radio_option = |id: &'static str, label: &'static str, is_recurrence: bool| {
            let active = selected_recurrence == is_recurrence;
            div()
                .id(id)
                .w_full()
                .px(px(12.))
                .py(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if active {
                    "--background-brand"
                } else {
                    "--border-subtle"
                }))
                .bg(color(if active {
                    "--background-mod-subtle"
                } else {
                    "--background-surface-higher"
                }))
                .hover(|s| s.bg(color("--background-mod-subtle")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(color(if active {
                            "--background-brand"
                        } else {
                            "--interactive-icon-default"
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(active, |dot| {
                            dot.child(
                                div()
                                    .size(px(10.))
                                    .rounded_full()
                                    .bg(color("--background-brand")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(p) = this
                        .guild_events
                        .as_mut()
                        .and_then(|open| open.rsvp_picker.as_mut())
                    {
                        p.selected_recurrence = is_recurrence;
                    }
                    cx.notify();
                }))
        };

        let card = div()
            .parity("event-rsvp-picker-modal")
            .id("event-rsvp-picker-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(super::modal_shadow())
            .overflow_hidden()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(24.))
                    .pt(px(24.))
                    .pb(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .id("event-rsvp-picker-close")
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(icon("lb_close", 20., "--text-muted"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.guild_events.as_mut() {
                                    open.rsvp_picker = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(radio_option(
                        "event-rsvp-series",
                        dc_intl::t("uoorxi"),
                        false,
                    ))
                    .child(radio_option(
                        "event-rsvp-recurrence",
                        dc_intl::t("lwZCFT"),
                        true,
                    )),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("event-rsvp-cancel")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("ETE/oC"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(open) = this.guild_events.as_mut() {
                                    open.rsvp_picker = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("event-rsvp-ok")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("TyCVIq"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let target_rec = if selected_recurrence {
                                    Some(recurrence_id.clone())
                                } else {
                                    None
                                };
                                if let Some(open) = this.guild_events.as_mut() {
                                    open.rsvp_picker = None;
                                }
                                this.toggle_event_interest_target(event, target_rec, cx);
                            })),
                    ),
            );

        super::layer(
            "event-rsvp-picker-layer",
            0.,
            card,
            |this, _, _| {
                if let Some(open) = this.guild_events.as_mut() {
                    open.rsvp_picker = None;
                }
            },
            cx,
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_event_rsvp_picker(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.eq_ignore_ascii_case("close") {
            self.guild_events = None;
            cx.notify();
            return "event-rsvp-picker: closed".to_owned();
        }
        let guild = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let uninterested = arg.eq_ignore_ascii_case("uninterested");
        let open = self.guild_events.get_or_insert_with(|| super::GuildEvents {
            guild,
            create: None,
            detail: None,
            menu: None,
            confirm: None,
            rsvp_picker: None,
            asked: std::collections::HashSet::new(),
        });
        open.rsvp_picker = Some(RsvpPicker {
            event: 99_666_394,
            recurrence_id: "1300000000000000000".to_owned(),
            interested_in_series: uninterested,
            selected_recurrence: false,
        });
        cx.notify();
        format!("event-rsvp-picker: open uninterested={uninterested}")
    }

    /// The answer to an interested request: a refusal undoes what was shown.
    pub(super) fn event_rsvp_answered(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        let Some(ids) = key.strip_prefix(RSVP_KEY) else {
            return;
        };
        if body["ok"].as_bool() == Some(true) {
            return;
        }
        let mut parts = ids.split(':');
        let (Some(_guild), Some(event), Some(answer), Some(me)) = (
            parts.next(),
            parts.next().and_then(|event| event.parse::<Id>().ok()),
            parts.next(),
            self.stores.me(),
        ) else {
            return;
        };
        if answer == "1" {
            self.stores
                .scheduled_events
                .remove_rsvp(event, None, me.id, RESPONSE_INTERESTED);
        } else {
            self.stores.scheduled_events.add_rsvp(
                event,
                None,
                EventUser {
                    user_id: me.id,
                    response: RESPONSE_INTERESTED,
                    nick: None,
                },
            );
        }
        cx.notify();
    }

    /// The Interested button of the card and the detail: green with a check when set, else secondary with a bell.
    /// A finished event shows "Event is over", disabled.
    pub(super) fn interested_button(
        &self,
        id: &'static str,
        event: &ScheduledEvent,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = event.id;
        if matches!(event.status, STATUS_COMPLETED | STATUS_CANCELED) {
            return action_button(id, size, Look::Secondary, "", Some("Event is over"), true)
                .into_any_element();
        }
        let interested = self.event_interested(target);
        action_button(
            id,
            size,
            if interested {
                Look::Connected
            } else {
                Look::Secondary
            },
            if interested { "event_check" } else { "bell" },
            Some("Interested"),
            false,
        )
        .parity_with_border(id, 1.)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_event_interest(target, cx);
        }))
        .into_any_element()
    }

    /// The "..." button; the menu it opens sits at the click.
    pub(super) fn event_more_button(
        &self,
        id: &'static str,
        event: Id,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        action_button(id, size, Look::Secondary, "more_horizontal", None, false)
            .parity_with_border(id, 1.)
            .on_click(cx.listener(move |this, click: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                this.open_event_menu(event, click.position(), cx);
            }))
            .into_any_element()
    }

    pub(super) fn event_copy_link_button(
        &self,
        id: &'static str,
        event: Id,
        size: Size,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        action_button(
            id,
            size,
            Look::Secondary,
            "share_link",
            Some("Copy Link"),
            false,
        )
        .parity_with_border(id, 1.)
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.copy_event_link(event, cx);
        }))
        .into_any_element()
    }

    /// The place line of an event: its icon and name, an outside location or the channel.
    pub(in crate::ui) fn event_place(&self, event: &ScheduledEvent) -> (&'static str, String) {
        if event.entity_type == ENTITY_EXTERNAL {
            return ("event_location", event.location.clone().unwrap_or_default());
        }
        let channel = event.channel_id.and_then(|id| {
            self.stores
                .guild(event.guild_id)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
                .map(|channel| channel.name.clone())
        });
        (
            if event.entity_type == ENTITY_STAGE_INSTANCE {
                "stage_channel"
            } else {
                "channel_type_voice"
            },
            channel.unwrap_or_default(),
        )
    }

    /// One card of the list (`card__wH91U`).
    pub(super) fn event_card(
        &self,
        event: &ScheduledEvent,
        now: NaiveDateTime,
        first: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = event.id;
        let status = event_status(event, now, false);
        let creator = event
            .creator_id
            .and_then(|creator| self.event_user(creator));
        let count = self.stores.scheduled_events.get_user_count(id, None);
        let (place_icon, place) = self.event_place(event);
        let joined_this_event = event.status == STATUS_ACTIVE
            && event.channel_id.is_some()
            && self
                .call
                .as_ref()
                .is_some_and(|c| Some(c.channel) == event.channel_id);
        let mut card = div()
            .id(("event-card", id))
            .w(px(554.))
            .flex_none()
            .rounded(px(8.))
            .border_1()
            .border_color(if joined_this_event {
                color("--status-positive-background")
            } else {
                color("--border-subtle")
            })
            .bg(color("--background-surface-higher"))
            .hover(|style| style.bg(color("--background-surface-highest")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open_event_detail(id, cx)));
        if first {
            card = card.parity_with_border("events-card", 1.);
        }
        let mut status_line = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(20.))
            .children(status.as_ref().map(status_row))
            .child(div().flex_1())
            .child(self.event_avatar(creator.as_ref(), 20.))
            .child(
                div()
                    .ml(px(8.))
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
                            .child(count.to_string()),
                    ),
            );
        if first {
            status_line = status_line.parity("events-card-status");
        }
        // Module 567550 `S`: a channel the viewer may enter is a link (`channelContainerEnabled__wH91U`: underlined,
        // `--interactive-text-active` on hover) that joins it (`onJoinClick` of module 482857, given while the viewer
        // may connect); a private one tells event managers who can see the event (tooltip on the icon).
        let place_channel = event.channel_id.and_then(|id| {
            self.stores
                .guild(event.guild_id)
                .and_then(|known| known.channels.iter().find(|channel| channel.id == id))
        });
        // Events take place in voice channels and Stages, the only channels `onJoinClick` enters.
        let joinable = place_channel.filter(|channel| {
            matches!(channel.kind, ChannelKind::Voice | ChannelKind::Other(13))
                && self.my_permission(event.guild_id, Some(channel), permissions::CONNECT)
        });
        let private_note = joinable.is_some_and(|channel| {
            let manager = self
                .stores
                .me()
                .zip(self.stores.guild(event.guild_id))
                .is_some_and(|(me, known)| can_manage(known, me, event));
            manager
                && self
                    .stores
                    .guild(event.guild_id)
                    .is_some_and(|known| permissions::is_private(known, channel))
        });
        let join_target = joinable.map(|channel| (event.guild_id, channel.id));
        // A pending member (membership screening, `H` of module 51758) who may not connect: the row still answers a
        // click with the screening (`Ze`), under "Missing permission to connect." (`channelContainerDisabled__wH91U`).
        let screening_only = joinable.is_none()
            && place_channel.is_some()
            && self
                .stores
                .guild(event.guild_id)
                .is_some_and(permissions::pending_gate);
        let place_icon_element = div().child(icon(place_icon, 20., "--text-default"));
        let place_icon_element = if private_note {
            self.with_tip(
                format!("event-card-private-tip-{id}"),
                Tip::text(dc_intl::t("48WXaW"), Side::Top),
                place_icon_element,
                cx,
            )
            .into_any_element()
        } else {
            place_icon_element.into_any_element()
        };
        let place_row = div()
            .id(("event-card-place", id))
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w(px(0.))
            .text_color(color("--text-default"))
            .when_some(join_target, |this, (guild, channel)| {
                this.cursor_pointer()
                    .hover(|style| {
                        style
                            .text_color(color("--interactive-text-active"))
                            .underline()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.request_join_voice(guild, channel, cx);
                    }))
            })
            .when(screening_only, |this| {
                this.cursor_not_allowed()
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.open_screening(cx);
                    }))
            })
            .when(event.entity_type == ENTITY_EXTERNAL, |this| {
                this.on_click(|_, _, cx| cx.stop_propagation())
            })
            .child(place_icon_element)
            .child(
                div()
                    .truncate()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .child(place),
            );
        let place_row = if screening_only {
            self.with_tip(
                format!("event-card-connect-tip-{id}"),
                Tip::text(dc_intl::t("nHjY9C"), Side::Top),
                place_row,
                cx,
            )
            .into_any_element()
        } else {
            place_row.into_any_element()
        };
        // Module 567550 `G`: active voice/Stage event speaker grid (up to 5 avatars + `+N Speaker(s)` `185ggO`).
        let speakers_grid = (event.status == STATUS_ACTIVE)
            .then_some(event.channel_id)
            .flatten()
            .and_then(|cid| {
                let mut speakers: Vec<Id> = self
                    .stores
                    .voice_members(event.guild_id, cid)
                    .into_iter()
                    .filter(|vs| event.entity_type != ENTITY_STAGE_INSTANCE || !vs.suppress)
                    .map(|vs| vs.user_id)
                    .collect();
                if speakers.is_empty() {
                    return None;
                }
                let total = speakers.len();
                speakers.truncate(5);
                let overflow = total.saturating_sub(speakers.len());
                let mut cells: Vec<AnyElement> = speakers
                    .into_iter()
                    .map(|uid| {
                        let user = self.event_user(uid);
                        let name = user
                            .as_ref()
                            .map(|u| u.display_name().to_owned())
                            .unwrap_or_else(|| uid.to_string());
                        div()
                            .w(px(164.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(self.event_avatar(user.as_ref(), 20.))
                            .child(
                                div()
                                    .max_w(px(120.))
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(name),
                            )
                            .into_any_element()
                    })
                    .collect();
                if overflow > 0 {
                    let overflow_label =
                        dc_intl::format("185ggO", &[("count", dc_intl::Arg::Num(overflow as f64))]);
                    cells.push(
                        div()
                            .w(px(164.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(20.))
                                    .rounded_full()
                                    .bg(color("--background-base-lowest"))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("mic", 12., "--text-default")),
                            )
                            .child(
                                div()
                                    .max_w(px(120.))
                                    .truncate()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(overflow_label),
                            )
                            .into_any_element(),
                    );
                }
                Some(
                    div()
                        .mt(px(16.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(cells),
                )
            });
        let footer = div()
            .mt(px(12.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_between()
            .child(place_row)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.event_more_button("event-card-more", id, Size::Sm, cx))
                    .child(self.event_copy_link_button("event-card-copy", id, Size::Sm, cx))
                    .child(self.interested_button("event-card-interested", event, Size::Sm, cx)),
            );
        card.child(
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .child(status_line)
                .child(
                    div()
                        .mt(px(12.))
                        .child(
                            div()
                                .h(px(24.))
                                .truncate()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(event.name.clone()),
                        )
                        .children(
                            event
                                .description
                                .clone()
                                .filter(|text| !text.is_empty())
                                .map(|text| {
                                    div()
                                        .mt(px(8.))
                                        .line_clamp(2)
                                        .text_ellipsis()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(text)
                                }),
                        ),
                )
                .children(speakers_grid)
                .child(
                    div()
                        .mt(px(16.))
                        .mx(px(-16.))
                        .h(px(1.))
                        .bg(color("--border-subtle")),
                )
                .child(footer),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsvp_picker_decision_matches_module_666394() {
        assert_eq!(
            decide_rsvp_action(false, 1, Some("123".into()), false, false),
            RsvpAction::ToggleSeries
        );
        assert_eq!(
            decide_rsvp_action(true, STATUS_CANCELED, Some("123".into()), false, false),
            RsvpAction::ToggleSeries
        );
        assert_eq!(
            decide_rsvp_action(true, STATUS_COMPLETED, Some("123".into()), false, false),
            RsvpAction::ToggleSeries
        );
        assert_eq!(
            decide_rsvp_action(true, 1, Some("123".into()), true, true),
            RsvpAction::ToggleRecurrence("123".into())
        );
        assert_eq!(
            decide_rsvp_action(true, 1, Some("123".into()), false, false),
            RsvpAction::OpenPicker {
                recurrence_id: "123".into(),
                interested_in_series: false,
            }
        );
        assert_eq!(dc_intl::t("WtORed"), "I am interested in...");
        assert_eq!(dc_intl::t("8MPCVr"), "I am not interested in...");
        assert_eq!(dc_intl::t("uoorxi"), "All events in this series");
        assert_eq!(dc_intl::t("lwZCFT"), "Only this event");
    }
}
