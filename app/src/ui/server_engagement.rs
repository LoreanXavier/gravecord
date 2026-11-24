//! Server Settings > Engagement (module 989433 `As`, saved by `Aa`): System Messages, Activity Feed, Default
//! Notification Settings, the inactive channel and timeout, and the Server Widget switch. Edits stay local until the
//! notice's Save: `PATCH /guilds/{id}` for the server's fields and `PATCH /guilds/{id}/widget` for the widget.
//! Measured in `design/dom/settings/guild-engagement-darker-cozy.json`. With the widget on, the official page also
//! shows its Invite Channel, JSON API and premade widget; those are not drawn yet.

use super::{Root, color};
use crate::net::Request;
use dc_state::{ChannelKind, Id};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, deferred, div, prelude::*,
    px,
};
use serde_json::Value;

pub(super) const ENGAGEMENT_PREFIX: &str = "guild-engagement:";
const WIDGET_PREFIX: &str = "guild-engagement:widget:";
const SAVED_PREFIX: &str = "guild-engagement:saved:";

/// `ogj` in module 652215: a set flag turns the system message off.
const SUPPRESS_JOIN: u64 = 1;
const SUPPRESS_BOOSTS: u64 = 2;
const SUPPRESS_TIPS: u64 = 4;
const SUPPRESS_JOIN_REPLIES: u64 = 8;
const FEED_ON: &str = "ACTIVITY_FEED_ENABLED_BY_USER";
const FEED_OFF: &str = "ACTIVITY_FEED_DISABLED_BY_USER";
/// `pF`: the inactive timeouts, in seconds, with their labels.
const TIMEOUTS: [(u64, &str); 5] = [
    (60, "1 minute"),
    (300, "5 minutes"),
    (900, "15 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
];

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) struct Values {
    flags: u64,
    system_channel: Option<String>,
    afk_channel: Option<String>,
    afk_timeout: u64,
    default_notifications: u64,
    features: Vec<String>,
    widget: bool,
    widget_channel: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Select {
    System,
    AfkChannel,
    AfkTimeout,
    /// The widget's invite channel (`Ae` in module 989433).
    WidgetChannel,
}

#[derive(Default)]
pub(crate) struct EngagementState {
    saved: Option<Values>,
    now: Values,
    open_select: Option<Select>,
}

impl Values {
    fn read(guild: &Value, widget: Option<&Value>) -> Self {
        let text = |key: &str| guild[key].as_str().map(str::to_owned);
        Values {
            flags: guild["system_channel_flags"].as_u64().unwrap_or(0),
            system_channel: text("system_channel_id"),
            afk_channel: text("afk_channel_id"),
            afk_timeout: guild["afk_timeout"].as_u64().unwrap_or(300),
            default_notifications: guild["default_message_notifications"].as_u64().unwrap_or(0),
            features: guild["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|feature| feature.as_str().map(str::to_owned))
                .collect(),
            widget: widget
                .and_then(|widget| widget["enabled"].as_bool())
                .unwrap_or(false),
            widget_channel: widget
                .and_then(|widget| widget["channel_id"].as_str().map(str::to_owned)),
        }
    }

    /// `pM`: on unless the server turned it off.
    fn feed_on(&self) -> bool {
        !self.features.iter().any(|feature| feature == FEED_OFF)
    }

    /// `Aa`'s body for `saveGuild`; features only when they changed.
    fn guild_body(&self, saved: &Values) -> Value {
        let mut body = serde_json::json!({
            "system_channel_flags": self.flags,
            "system_channel_id": self.system_channel,
            "afk_channel_id": self.afk_channel,
            "afk_timeout": self.afk_timeout,
            "default_message_notifications": self.default_notifications,
        });
        if self.features != saved.features {
            body["features"] = serde_json::json!(self.features);
        }
        body
    }

    fn guild_changed(&self, saved: &Values) -> bool {
        Values {
            widget: saved.widget,
            widget_channel: saved.widget_channel.clone(),
            ..self.clone()
        } != *saved
    }

    fn widget_changed(&self, saved: &Values) -> bool {
        self.widget != saved.widget || self.widget_channel != saved.widget_channel
    }
}

impl Root {
    /// Escape: an open select closes before the page does.
    pub(super) fn close_engagement_layer(&mut self) -> bool {
        self.server_settings
            .as_mut()
            .is_some_and(|open| open.engagement.open_select.take().is_some())
    }

    pub(super) fn request_engagement(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{ENGAGEMENT_PREFIX}{guild}"),
            route: format!("/guilds/{guild}"),
        });
        let _ = self.requests.send(Request::Fetch {
            key: format!("{WIDGET_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/widget"),
        });
    }

    pub(super) fn engagement_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(SAVED_PREFIX) {
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_engagement(guild);
            }
            return;
        }
        let guild = key
            .strip_prefix(WIDGET_PREFIX)
            .or_else(|| key.strip_prefix(ENGAGEMENT_PREFIX))
            .and_then(|guild| guild.parse::<Id>().ok());
        let Some(guild) = guild else {
            return;
        };
        let (Some(server), Some(widget)) = (
            self.fetched
                .get(&format!("{ENGAGEMENT_PREFIX}{guild}"))
                .cloned(),
            self.fetched
                .get(&format!("{WIDGET_PREFIX}{guild}"))
                .cloned(),
        ) else {
            return;
        };
        let values = Values::read(&server, Some(&widget));
        if let Some(open) = self.server_settings.as_mut() {
            open.engagement.saved = Some(values.clone());
            open.engagement.now = values;
        }
        cx.notify();
    }

    pub(super) fn engagement_dirty(&self) -> bool {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.engagement) else {
            return false;
        };
        state
            .saved
            .as_ref()
            .is_some_and(|saved| *saved != state.now)
    }

    pub(super) fn reset_engagement(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self
            .server_settings
            .as_mut()
            .map(|open| &mut open.engagement)
            && let Some(saved) = state.saved.clone()
        {
            state.now = saved;
        }
        cx.notify();
    }

    pub(super) fn save_engagement(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.engagement) else {
            return;
        };
        let Some(saved) = state.saved.clone() else {
            return;
        };
        let now = state.now.clone();
        if now.guild_changed(&saved) {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}"),
                body: Some(now.guild_body(&saved)),
            });
        }
        if now.widget_changed(&saved) {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{SAVED_PREFIX}{guild}")),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/widget"),
                body: Some(
                    serde_json::json!({"enabled": now.widget, "channel_id": now.widget_channel}),
                ),
            });
        }
        if let Some(state) = self
            .server_settings
            .as_mut()
            .map(|open| &mut open.engagement)
        {
            state.saved = Some(now);
        }
        cx.notify();
    }

    fn engagement_edit(&mut self, change: impl FnOnce(&mut Values), cx: &mut Context<Self>) {
        if let Some(state) = self
            .server_settings
            .as_mut()
            .map(|open| &mut open.engagement)
        {
            change(&mut state.now);
        }
        cx.notify();
    }

    /// A label with its switch on the right (`container_VhFsFg`).
    fn engagement_switch(
        label: &'static str,
        on: bool,
        id: &'static str,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Values) + 'static,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(24.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label),
            )
            .child(
                div()
                    .id(id)
                    .cursor_pointer()
                    .child(super::create_channel::switch(on))
                    .on_click(cx.listener(move |this, _, _, cx| this.engagement_edit(&run, cx))),
            )
            .into_any_element()
    }

    /// A closed select (`select_uF`): the chosen channel with its category, or plain text, and the chevron.
    fn engagement_select_box(
        &self,
        id: &'static str,
        which: Select,
        shown: (Option<&'static str>, String, Option<String>),
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (hash, label, category) = shown;
        div()
            .id(id)
            .relative()
            .child(self.anchor_probe(id))
            .h(px(40.))
            .pl(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--input-border-default"))
            .bg(color("--input-background-default"))
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| this.cursor_pointer())
            .flex()
            .items_center()
            .gap(px(8.))
            .children(hash.map(|glyph| super::icon(glyph, 16., "--interactive-text-default")))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(label),
            )
            .children(category.map(|category| {
                div()
                    .ml(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(category)
            }))
            .child(div().flex_1())
            .child(div().w(px(36.)).flex().justify_center().child(super::icon(
                "select_chevron",
                20.,
                "--interactive-text-active",
            )))
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                if let Some(state) = this
                    .server_settings
                    .as_mut()
                    .map(|open| &mut open.engagement)
                {
                    state.open_select = if state.open_select == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                }
                cx.notify();
            }))
    }

    /// (id, name, category name) of the server's channels of a kind, in channel order.
    fn channels_of_kind(&self, guild: Id, voice: bool) -> Vec<(String, String, Option<String>)> {
        let Some(known) = self.stores.guilds.iter().find(|known| known.id == guild) else {
            return Vec::new();
        };
        known
            .channels
            .iter()
            .filter(|channel| {
                if voice {
                    matches!(channel.kind, ChannelKind::Voice)
                } else {
                    matches!(channel.kind, ChannelKind::Text)
                }
            })
            .map(|channel| {
                let category = channel
                    .parent_id
                    .and_then(|parent| known.channels.iter().find(|other| other.id == parent))
                    .map(|parent| parent.name.clone());
                (channel.id.to_string(), channel.name.clone(), category)
            })
            .collect()
    }

    pub(super) fn server_engagement_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.server_settings.as_ref().map(|open| &open.engagement) else {
            return div().into_any_element();
        };
        if state.saved.is_none() {
            return div().into_any_element();
        }
        let now = &state.now;
        let heading = |text: &'static str| {
            div()
                .mb(px(4.))
                .text_size(px(20.))
                .line_height(px(24.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let note = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let divider = || div().my(px(40.)).h(px(1.)).bg(gpui::rgba(0x97979f70));
        let flag = |bit: u64| now.flags & bit == 0;
        let system = now.system_channel.as_ref().and_then(|id| {
            self.channels_of_kind(guild, false)
                .into_iter()
                .find(|(known, _, _)| known == id)
        });
        let afk = now.afk_channel.as_ref().and_then(|id| {
            self.channels_of_kind(guild, true)
                .into_iter()
                .find(|(known, _, _)| known == id)
        });
        let widget_channel = now.widget_channel.as_ref().and_then(|id| {
            self.channels_of_kind(guild, false)
                .into_iter()
                .find(|(known, _, _)| known == id)
        });
        let timeout = TIMEOUTS
            .iter()
            .find(|(seconds, _)| *seconds == now.afk_timeout)
            .map_or("", |(_, label)| label)
            .to_owned();
        // `pk`: All Messages says why big servers stay quiet on phones (`L+P4t2`, 2500 members or more).
        let member_count = self
            .stores
            .member_counts
            .get_member_count(guild)
            .unwrap_or(0);
        let all_note = all_messages_note(member_count);
        let radio_row =
            |id: &'static str, label: &'static str, value: u64, cx: &mut Context<Self>| {
                let picked = now.default_notifications == value;
                let note = (value == 0).then_some(all_note).flatten();
                div()
                    .id(id)
                    .min_h(px(36.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(super::create_channel::radio(picked))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(label),
                            )
                            .children(note.map(|note| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(note)
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.engagement_edit(|values| values.default_notifications = value, cx)
                    }))
            };
        let toggle_flag = move |bit: u64| move |values: &mut Values| values.flags ^= bit;
        let widget_private = self
            .fetched
            .get(&format!("{}{guild}", super::server_settings::PROFILE_KEY))
            .is_none_or(|profile| !matches!(profile["visibility"].as_u64(), Some(1 | 3)));
        div()
            .flex()
            .flex_col()
            .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight(600.)).text_color(color("--text-strong")).child("Engagement"))
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child("Manage settings that help keep your server active."),
            )
            .child(div().mt(px(40.)).child(heading("System Messages")).child(note("Configure system event messages sent to your server.")))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(Self::engagement_switch("Send a random welcome message when someone joins this server.", flag(SUPPRESS_JOIN), "engagement-join", cx, toggle_flag(SUPPRESS_JOIN)))
                    .child(Self::engagement_switch("Prompt members to reply to welcome messages with a sticker.", flag(SUPPRESS_JOIN_REPLIES), "engagement-replies", cx, toggle_flag(SUPPRESS_JOIN_REPLIES)))
                    .child(Self::engagement_switch("Send a message when someone boosts this server.", flag(SUPPRESS_BOOSTS), "engagement-boosts", cx, toggle_flag(SUPPRESS_BOOSTS)))
                    .child(Self::engagement_switch("Send helpful tips for server setup.", flag(SUPPRESS_TIPS), "engagement-tips", cx, toggle_flag(SUPPRESS_TIPS)))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap(px(24.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(div().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("System Messages Channel"))
                                    .child(div().text_size(px(14.)).line_height(px(18.)).text_color(color("--text-subtle")).child("This is the channel we send system event messages to.")),
                            )
                            .child(
                                div().w(px(264.)).flex_none().child(self.engagement_select_box(
                                    "engagement-system",
                                    Select::System,
                                    (system.is_some().then_some("channel_type_text"),
                                    system.as_ref().map_or("No System Messages".to_owned(), |(_, name, _)| name.clone()),
                                    system.as_ref().and_then(|(_, _, category)| category.clone())),
                                    false,
                                    cx,
                                )),
                            ),
                    ),
            )
            .child(divider())
            .child(heading("Activity Feed Settings"))
            .child(note("Shows a feed of activity from games and connected apps in this server."))
            .child(div().mt(px(16.)).child(Self::engagement_switch("Display Activity Feed in this server", now.feed_on(), "engagement-feed", cx, |values| {
                let on = values.feed_on();
                values.features.retain(|feature| feature != FEED_ON && feature != FEED_OFF);
                values.features.push(if on { FEED_OFF } else { FEED_ON }.to_owned());
            })))
            .child(divider())
            .child(div().text_size(px(16.)).line_height(px(20.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("Default Notification Settings"))
            .child(
                div()
                    .mt(px(4.))
                    .mb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child("This will determine whether members who have not explicitly set their notification settings receive a notification for every message sent in this server or not."),
            )
            .child(radio_row("engagement-all", "All Messages", 0, cx))
            .child(radio_row("engagement-mentions", "Only @mentions", 1, cx))
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child("We highly recommend setting this to only @mentions for a Community Server."),
            )
            .child(divider())
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(
                        div().flex_1().child(Self::field_label("Inactive Channel")).child(self.engagement_select_box(
                            "engagement-afk",
                            Select::AfkChannel,
                            (afk.is_some().then_some("channel_type_voice"),
                            afk.as_ref().map_or("No Inactive Channel".to_owned(), |(_, name, _)| name.clone()),
                            afk.as_ref().and_then(|(_, _, category)| category.clone())),
                            false,
                            cx,
                        )),
                    )
                    .child(
                        div().flex_1().child(Self::field_label("Inactive Timeout")).child(self.engagement_select_box(
                            "engagement-timeout",
                            Select::AfkTimeout,
                            (None,
                            timeout,
                            None),
                            now.afk_channel.is_none(),
                            cx,
                        )),
                    ),
            )
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .child("Automatically move members to this channel and mute them when they have been idle for longer than the inactive timeout. This does not affect browsers."),
            )
            .child(divider())
            .child(heading("Server Widget"))
            .child(note("Embed an HTML widget on your website to display your online members, voice channels, and invite link."))
            .child(div().mt(px(16.)).child(Self::engagement_switch("Enable Server Widget", now.widget, "engagement-widget", cx, |values| values.widget = !values.widget)))
            .when(now.widget, |this| {
                this.child(
                    div()
                        .mt(px(24.))
                        .child(Self::field_label("Invite Channel"))
                        .child(self.engagement_select_box(
                            "engagement-widget-channel",
                            Select::WidgetChannel,
                            (
                                widget_channel.is_some().then_some("channel_type_text"),
                                widget_channel
                                    .as_ref()
                                    .map_or("No Invite".to_owned(), |(_, name, _)| name.clone()),
                                widget_channel.as_ref().and_then(|(_, _, category)| category.clone()),
                            ),
                            false,
                            cx,
                        ))
                        .child(
                            div()
                                .mt(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child("If a channel is selected then an invite link will be generated with the widget,
otherwise only online members and voice channels will be provided."),
                        ),
                )
            })
            .when(widget_private && !now.widget, |this| {
                this.child(
                    div()
                        .mt(px(32.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child("By enabling the widget, your server profile will be visible to others outside of your server members. You can control Profile privacy in Server Settings > Profile."),
                )
            })
            .when(
                now.features
                    .iter()
                    .any(|f| f == "GUILD_SPACE" || f == "GUILD_HOME_DEPRECATION_OVERRIDE"),
                |this| {
                    let published = now.features.iter().any(|f| f == "GUILD_SPACE_PUBLISHED");
                    this.child(divider())
                        .child(heading("Server Hub"))
                        .child(note(
                            "Introduce your server with customizable widgets and highlights.",
                        ))
                        .child(
                            div()
                                .id("engagement-open-server-hub-nux")
                                .mt(px(12.))
                                .h(px(36.))
                                .px(px(14.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .self_start()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child("Preview Server Hub")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_guild_space_nux(guild, published, cx);
                                })),
                        )
                },
            )
            .into_any_element()
    }

    /// The open select's list.
    pub(super) fn engagement_select_layer(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let state = &open.engagement;
        let which = state.open_select?;
        let guild = open.guild;
        let key = match which {
            Select::System => "engagement-system",
            Select::AfkChannel => "engagement-afk",
            Select::AfkTimeout => "engagement-timeout",
            Select::WidgetChannel => "engagement-widget-channel",
        };
        let field = self.tip_anchors.borrow().get(key).copied()?.dilate(px(1.));
        let row = |id: String,
                   label: String,
                   category: Option<String>,
                   selected: bool,
                   cx: &mut Context<Self>,
                   run: Box<dyn Fn(&mut Values)>| {
            div()
                .id(SharedString::from(format!("engagement-option-{id}")))
                .flex_none()
                .h(px(40.))
                .px(px(8.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-normal")))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .children(category.map(|category| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(category)
                }))
                .child(div().flex_1())
                .when(selected, |this| {
                    this.child(super::icon("select_check", 20., "--text-brand"))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(state) = this
                        .server_settings
                        .as_mut()
                        .map(|open| &mut open.engagement)
                    {
                        run(&mut state.now);
                        state.open_select = None;
                    }
                    cx.notify();
                }))
                .into_any_element()
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        match which {
            Select::System => {
                rows.push(row(
                    "none".to_owned(),
                    "No System Messages".to_owned(),
                    None,
                    state.now.system_channel.is_none(),
                    cx,
                    Box::new(|values| values.system_channel = None),
                ));
                for (id, name, category) in self.channels_of_kind(guild, false) {
                    let picked = state.now.system_channel.as_deref() == Some(id.as_str());
                    let chosen = id.clone();
                    rows.push(row(
                        id,
                        name,
                        category,
                        picked,
                        cx,
                        Box::new(move |values| values.system_channel = Some(chosen.clone())),
                    ));
                }
            }
            Select::AfkChannel => {
                rows.push(row(
                    "none".to_owned(),
                    "No Inactive Channel".to_owned(),
                    None,
                    state.now.afk_channel.is_none(),
                    cx,
                    Box::new(|values| values.afk_channel = None),
                ));
                for (id, name, category) in self.channels_of_kind(guild, true) {
                    let picked = state.now.afk_channel.as_deref() == Some(id.as_str());
                    let chosen = id.clone();
                    rows.push(row(
                        id,
                        name,
                        category,
                        picked,
                        cx,
                        Box::new(move |values| values.afk_channel = Some(chosen.clone())),
                    ));
                }
            }
            Select::WidgetChannel => {
                rows.push(row(
                    "none".to_owned(),
                    "No Invite".to_owned(),
                    None,
                    state.now.widget_channel.is_none(),
                    cx,
                    Box::new(|values| values.widget_channel = None),
                ));
                for (id, name, category) in self.channels_of_kind(guild, false) {
                    let picked = state.now.widget_channel.as_deref() == Some(id.as_str());
                    let chosen = id.clone();
                    rows.push(row(
                        id,
                        name,
                        category,
                        picked,
                        cx,
                        Box::new(move |values| values.widget_channel = Some(chosen.clone())),
                    ));
                }
            }
            Select::AfkTimeout => {
                for (seconds, label) in TIMEOUTS {
                    rows.push(row(
                        seconds.to_string(),
                        label.to_owned(),
                        None,
                        state.now.afk_timeout == seconds,
                        cx,
                        Box::new(move |values| values.afk_timeout = seconds),
                    ));
                }
            }
        }
        let menu_h = ((rows.len() as f32) * 40. + 10.).min(210.);
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let top = if space_below < menu_h && space_above > space_below {
            (field.top() - px(4.) - px(menu_h)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            deferred(
                div()
                    .id("engagement-select-list")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .max_h(px(210.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(state) = this
                                    .server_settings
                                    .as_mut()
                                    .map(|open| &mut open.engagement)
                            {
                                state.open_select = None;
                                cx.notify();
                            }
                        },
                    ))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engagement_reads_and_saves_like_aa() {
        let guild = serde_json::json!({
            "system_channel_flags": 8,
            "system_channel_id": "1",
            "afk_channel_id": null,
            "afk_timeout": 300,
            "default_message_notifications": 1,
            "features": ["A"],
        });
        let widget = serde_json::json!({"enabled": false, "channel_id": null});
        let saved = Values::read(&guild, Some(&widget));
        assert_eq!(saved.flags, SUPPRESS_JOIN_REPLIES);
        assert!(saved.feed_on());
        let mut now = saved.clone();
        now.flags ^= SUPPRESS_BOOSTS;
        assert!(now.guild_changed(&saved));
        assert!(!now.widget_changed(&saved));
        let body = now.guild_body(&saved);
        assert_eq!(body["system_channel_flags"], 10);
        assert!(body.get("features").is_none());
        now.features.push(FEED_OFF.to_owned());
        assert!(!now.feed_on());
        assert_eq!(
            now.guild_body(&saved)["features"],
            serde_json::json!(["A", FEED_OFF])
        );
    }
}

/// `pk` of module 989433: the All Messages option's description for a server of 2500 members or more.
fn all_messages_note(member_count: u64) -> Option<&'static str> {
    (member_count >= 2500).then(|| dc_intl::t("L+P4t2"))
}

#[cfg(test)]
mod note_tests {
    use super::*;

    #[test]
    fn big_servers_get_the_all_messages_note() {
        assert_eq!(all_messages_note(2499), None);
        assert!(all_messages_note(2500).is_some());
    }
}
