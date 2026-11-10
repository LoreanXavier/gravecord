//! Guild Events: the sidebar's Events row, the Events modal (module 568036) and, from its Create Event button, the
//! three-step Create Event flow (module 688218, in `create.rs`). Measured in `design/dom/modals/events-empty-darker-cozy.json`
//! and the `event-create-*` captures. The events themselves live in `dc_state::scheduled_events` (module 698441).
//!
//! Not built: the event cards of a non-empty list (module 327241). There is no capture of the card in the list, so the
//! list area of a server that has events is left empty.

mod create;
mod pickers;
mod schedule;

use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, permissions, scheduled_events::ScheduledEvent};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, prelude::*,
    px, rgba,
};
use serde_json::{Value, json};

/// `MANAGE_EVENTS`.
pub(super) const MANAGE_EVENTS: u64 = 1 << 33;
/// `CREATE_EVENTS`.
pub(super) const CREATE_EVENTS: u64 = 1 << 44;

const CREATE_KEY: &str = "guild-events-create";
#[cfg(feature = "capture")]
const DELETE_KEY: &str = "guild-events-delete:";
const LIST_KEY: &str = "guild-events:";
const NAV_KEY: &str = "guild-nav:events:";

/// The open Events modal of a server and, over it, the Create Event flow.
pub(super) struct GuildEvents {
    guild: Id,
    create: Option<create::Create>,
}

/// The dim layer behind a modal; a click on it closes the modal. `top_pad` nudges a modal that is taller than the
/// window allows (the official one sits 24px from the top and 1px nearer the bottom).
pub(super) fn layer(
    id: &'static str,
    top_pad: f32,
    modal: impl IntoElement,
    close: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
    cx: &mut Context<Root>,
) -> AnyElement {
    deferred(
        div()
            .id(id)
            .occlude()
            .absolute()
            .inset_0()
            .bg(rgba(0x000000b8))
            .flex()
            .items_center()
            .justify_center()
            .pt(px(top_pad))
            .on_click(cx.listener(move |this, _, window, cx| {
                close(this, window, cx);
                cx.notify();
            }))
            .child(modal),
    )
    .with_priority(1)
    .into_any_element()
}

/// Capture runs: one line per answer of the create and delete requests (status and event id only), appended to a file
/// in the temporary directory, so a proof run can be read back.
#[cfg(feature = "capture")]
pub(super) fn proof_log(line: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("gravecord-guild-events-proof.log"))
    {
        let _ = writeln!(file, "{line}");
    }
}

/// The modal's shadow: `0 4px 8px rgba(0,0,0,.16)`.
pub(super) fn modal_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., 0.16),
        offset: gpui::point(px(0.), px(4.)),
        blur_radius: px(8.),
        spread_radius: px(0.),
        inset: false,
    }]
}

impl Root {
    /// `G.Ay`: the upcoming events of a server that the user can see (no channel, or a channel they may view).
    fn visible_upcoming_events(&self, guild: Id) -> Vec<&ScheduledEvent> {
        let (Some(known), Some(me)) = (self.stores.guild(guild), self.stores.me()) else {
            return Vec::new();
        };
        self.stores
            .scheduled_events
            .upcoming(guild)
            .into_iter()
            .filter(|event| {
                event.channel_id.is_none_or(|id| {
                    known
                        .channels
                        .iter()
                        .find(|channel| channel.id == id)
                        .is_some_and(|channel| {
                            permissions::can_in_channel(
                                known,
                                channel,
                                me,
                                permissions::VIEW_CHANNEL,
                            )
                        })
                })
            })
            .collect()
    }

    /// The number beside the Events row and in the modal's title (`IBdqSu`): "Events" until there is one.
    fn events_title(&self, guild: Id) -> String {
        match self.visible_upcoming_events(guild).len() {
            0 => "Events".to_owned(),
            1 => "1 Event".to_owned(),
            count => format!("{count} Events"),
        }
    }

    /// The sidebar's Events row (module 616740): the label counts the upcoming events, a click opens the modal.
    pub(super) fn guild_events_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let label = self
            .guild
            .map_or_else(|| "Events".to_owned(), |guild| self.events_title(guild));
        div()
            .flex_none()
            .h(px(34.))
            .pt(px(1.))
            .pl(px(8.))
            .child(
                div()
                    .id("guild-events-row")
                    .h(px(32.))
                    .w(px(286.))
                    .pl(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .on_click(cx.listener(|this, _, _, cx| this.open_guild_events(cx)))
                    .child(icon("events", 20., "--channels-default"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--channels-default"))
                            .child(label),
                    ),
            )
            .into_any_element()
    }

    /// Opens the Events modal of the current server and asks for its events (`getGuildEventsForCurrentUser`).
    pub(super) fn open_guild_events(&mut self, cx: &mut Context<Self>) {
        let Some(guild) = self.guild else {
            return;
        };
        self.guild_events = Some(GuildEvents {
            guild,
            create: None,
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{LIST_KEY}{guild}"),
            route: format!("/guilds/{guild}/scheduled-events"),
        });
        cx.notify();
    }

    /// Escape: the open picker first, then the Create Event flow, then the Events modal.
    pub(super) fn guild_events_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.guild_events.as_ref() else {
            return false;
        };
        if key != "escape" {
            return false;
        }
        match open.create.as_ref() {
            Some(create) if create.open.is_some() => self.close_event_picker(cx),
            Some(_) => {
                if let Some(open) = self.guild_events.as_mut() {
                    open.create = None;
                }
            }
            None => self.guild_events = None,
        }
        cx.notify();
        true
    }

    /// Feeds the store from the server's list of a guild's events, and takes the answer to the create request.
    /// Returns whether the update is fully handled here.
    pub(super) fn guild_events_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(guild) = key
            .strip_prefix(LIST_KEY)
            .or_else(|| key.strip_prefix(NAV_KEY))
        {
            if body.is_array() {
                self.stores.scheduled_events.dispatch(
                    "FETCH_GUILD_EVENTS_FOR_GUILD",
                    &json!({"guild_id": guild, "guild_scheduled_events": body}),
                );
            }
            cx.notify();
            // The sidebar keeps its own copy of the answer of its key.
            return key.starts_with(LIST_KEY);
        }
        #[cfg(feature = "capture")]
        if let Some((guild, event)) = key
            .strip_prefix(DELETE_KEY)
            .and_then(|ids| ids.split_once(':'))
        {
            proof_log(&format!(
                "delete event={event} ok={} status={}",
                body["ok"], body["status"]
            ));
            if body["ok"].as_bool() == Some(true) {
                self.stores.scheduled_events.dispatch(
                    "GUILD_SCHEDULED_EVENT_DELETE",
                    &json!({"id": event, "guild_id": guild}),
                );
            }
            cx.notify();
            return true;
        }
        if key != CREATE_KEY {
            return false;
        }
        self.finish_create_event(body, cx);
        true
    }

    /// The text fields of the open flow, for the capture harness to type into.
    #[cfg(feature = "capture")]
    pub(super) fn guild_events_editors(&self) -> Vec<gpui::Entity<crate::editor::Editor>> {
        self.guild_events
            .as_ref()
            .and_then(|open| open.create.as_ref())
            .map(create::Create::editors)
            .unwrap_or_default()
    }

    /// The layer of the Events modal, or of the Create Event flow that replaces it.
    pub(super) fn guild_events_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(open) = self.guild_events.as_ref() else {
            return Vec::new();
        };
        if let Some(create) = open.create.as_ref() {
            return self.create_event_layer(open.guild, create, window, cx);
        }
        vec![self.events_modal(open.guild, cx)]
    }

    fn events_modal(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let title = self.events_title(guild);
        let can_create = self
            .stores
            .guild(guild)
            .zip(self.stores.me())
            .is_some_and(|(known, me)| super::guild_navigation::can_create_events(known, me));
        let can_roles =
            self.stores
                .guild(guild)
                .zip(self.stores.me())
                .is_some_and(|(known, me)| {
                    permissions::can_in_guild(known, me, permissions::MANAGE_ROLES)
                });
        let none = self.visible_upcoming_events(guild).is_empty();
        let header = div()
            .flex_none()
            .h(px(56.))
            .pt(px(16.))
            .pb(px(8.))
            .px(px(24.))
            .parity("events-header")
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .child(icon("events", 24., "--text-muted"))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .when(can_create, |row| {
                        row.child(
                            div()
                                .ml(px(16.))
                                .w(px(3.84))
                                .h(px(16.))
                                .flex()
                                .justify_center()
                                .child(div().w(px(1.)).h_full().bg(color("--border-subtle"))),
                        )
                        .child(
                            div()
                                .parity_with_border("events-create-button", 1.)
                                .id("events-create-button")
                                .ml(px(16.))
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--control-primary-border-default"))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child("Create Event")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_create_event(window, cx);
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("events-close")
                            .self_start()
                            .mt(px(3.))
                            .ml_auto()
                            .mr(px(-8.))
                            .w(px(24.))
                            .h(px(27.))
                            .cursor_pointer()
                            .child(icon("lb_close", 24., "--text-muted"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.guild_events = None;
                                cx.notify();
                            })),
                    ),
            );
        let empty = div()
            .parity("events-empty")
            .w(px(554.))
            .h(px(300.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .text_center()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("There are no upcoming events."),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Schedule an event for any planned activity in your server."),
            )
            .when(can_roles, |this| {
                this.child(
                    div()
                        .mt(px(4.))
                        .flex()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child("You can give other people permission to create events in ")
                        .child(
                            div()
                                .id("events-roles-link")
                                .cursor_pointer()
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-link"))
                                .hover(|style| style.underline())
                                .child("Server Settings > Roles")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.guild_events = None;
                                    this.open_server_settings_at(
                                        guild,
                                        super::server_settings::Page::Roles,
                                        cx,
                                    );
                                })),
                        )
                        .child("."),
                )
            });
        let modal = div()
            .parity_with_border("events-modal", 1.)
            .id("events-modal")
            .occlude()
            .w(px(604.))
            .min_h(px(402.))
            .max_h(px(802.))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(modal_shadow())
            .child(header)
            .child(
                div()
                    .parity("events-content")
                    .flex_1()
                    .min_h(px(344.))
                    .pl(px(24.))
                    .pr(px(16.))
                    .pt(px(8.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .when(none, |this| this.child(empty)),
            );
        layer(
            "events-layer",
            0.,
            modal,
            |this, _, _| this.guild_events = None,
            cx,
        )
    }
}

/// A form label: 16/20 medium with the red star of a required field.
pub(super) fn field_label(text: impl Into<SharedString>, required: bool) -> gpui::Div {
    div()
        .flex()
        .h(px(20.))
        .mb(px(8.))
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(text.into())
        .when(required, |this| {
            this.child(
                div()
                    .ml(px(4.))
                    .font_weight(FontWeight(400.))
                    .text_color(color("--text-feedback-critical"))
                    .child("*"),
            )
        })
}
