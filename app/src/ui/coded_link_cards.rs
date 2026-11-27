//! The cards of a message's coded links (`renderCodedLinks` of module 679636 over `codedLinks`, module 486889 `ei`,
//! in `dc_state::coded_links`), drawn in the order of the links, before the gift cards.
//!
//! - `INVITE`: the invite card (`cr`, `invite_embed.rs`); a DM with someone outside a few official servers
//!   (`shouldRenderInvite`, `cT`) shows none for those.
//! - `TEMPLATE`: `lO`, the server template card over the invite button frame (module 529200).
//! - `EVENT`: module 484724 over module 427080, the event card.
//! - `CHANNEL_LINK`: `ak`/`aS`, a voice or Stage channel the account may see and join, with Join Voice.
//! - `QUESTS_EMBED`: the quest card (module 107543 / 562293, `quest_promoted.rs`).
//!
//! The other types (shops, games, apps, user profiles, Conjure) have no card here yet; see `other_card`.
//!
//! A click on an invite link (module 697794 `Y`) resolves the invite first, then goes straight to the server for a
//! member the invite gives no new roles, and opens the invite dialog otherwise (`InviteClick`).
use super::voice_invite::{ButtonLook, ds_button};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::coded_links::{self, CodedLink, Kind};
use dc_state::{ChannelKind, Id, Message, permissions};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, StyledImage, div, img,
    prelude::*, px,
};
use serde_json::Value;
use std::collections::HashSet;

/// The key of a template's resolve (`GET /guilds/templates/{code}`, GuildTemplateStore's `resolveGuildTemplate`).
pub(super) const TEMPLATE_KEY: &str = "coded-template:";
/// The key of an event's fetch (`GET /guilds/{guild}/scheduled-events/{event}`, module 496092 `fetchGuildEvent`).
pub(super) const EVENT_KEY: &str = "coded-event:";
/// The key of a `/gifts/<code>` place's resolve (`resolveGiftCode(code, true, true)` of the app router, module
/// 596339).
pub(super) const GIFT_ROUTE_KEY: &str = "gift-route-resolve:";
/// The key of an event cover picture.
const EVENT_COVER_KEY: &str = "coded-event-cover:";
/// `cT` of module 679636: official servers whose invites a DM shows no card for.
const DM_HIDDEN_INVITES: [&str; 4] = [
    "discord-developers",
    "discord-testers",
    "discord-townhall",
    "discordgameslab",
];
/// `--radius-sm`.
const RADIUS_SM: f32 = 8.;

/// What the coded link pipeline keeps between frames.
#[derive(Default)]
pub(super) struct State {
    /// An invite link that was clicked while its invite was not resolved yet (`resolveInvite` of 697794's `Y`).
    pub invite_click: Option<String>,
    /// A `/gifts/<code>` place that waits for its gift to resolve.
    pub gift_route: Option<String>,
    /// An event link that was clicked while its event was not fetched yet.
    pub event_click: Option<(Id, Id)>,
    /// What was asked for already.
    asked: HashSet<String>,
}

/// What a click on an invite link does (module 697794 `Y`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum InviteClick {
    /// Not resolved yet: resolve it, then decide.
    Resolve,
    /// `INVITE_MODAL_OPEN`: the invite dialog (also for an expired, banned or failed invite).
    Modal,
    /// `transitionToInvite`: the server, for a member the invite gives no new roles.
    Transition { guild: Id, channel: Option<Id> },
}

fn snowflake(value: &Value) -> Option<Id> {
    value.as_str()?.parse().ok()
}

/// `Y`'s decision over the resolved invite (`body`, as the invite fetch keeps it), whether the account is in a
/// server, and the roles it has there.
pub(super) fn invite_click(
    body: Option<&Value>,
    member_of: &dyn Fn(Id) -> bool,
    my_roles: &dyn Fn(Id) -> Vec<Id>,
) -> InviteClick {
    let Some(body) = body else {
        return InviteClick::Resolve;
    };
    // Module 970163 turns every failure into an absent invite: EXPIRED, BANNED and ERROR all open the dialog, which
    // shows why.
    if body["invite_resolve_failed"] == true || body["fetch_failed"] == true {
        return InviteClick::Modal;
    }
    let Some(guild) = snowflake(&body["guild"]["id"]) else {
        return InviteClick::Modal;
    };
    if !member_of(guild) {
        return InviteClick::Modal;
    }
    let granted: Vec<Id> = body["roles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|role| snowflake(&role["id"]))
        .collect();
    let have = my_roles(guild);
    if granted.iter().any(|role| !have.contains(role)) {
        return InviteClick::Modal;
    }
    InviteClick::Transition {
        guild,
        channel: snowflake(&body["channel"]["id"]),
    }
}

/// `y$` then module 970163's query: `GET /invites/{code}` with the counts, the expiration, the event and the target
/// channel and message of the invite key, and the permissions.
pub(super) fn invite_resolve_route(key: &str) -> String {
    let parts = coded_links::split_invite_key(key);
    let mut route = format!(
        "/invites/{}?with_counts=true&with_expiration=true",
        parts.base_code
    );
    if let Some(event) = &parts.event {
        route.push_str(&format!("&guild_scheduled_event_id={event}"));
    }
    if let Some(channel) = &parts.channel {
        route.push_str(&format!("&target_channel_id={channel}"));
    }
    if let Some(message) = &parts.message {
        route.push_str(&format!("&target_message_id={message}"));
    }
    route.push_str("&with_permissions=true");
    route
}

/// `ly` of module 346542: the scheduled event an invite carries (its id), when it has not ended (`d6` of module
/// 698441: the status is not one of `lV`, completed and canceled).
pub(super) fn invite_event(body: &Value) -> Option<Id> {
    use dc_state::scheduled_events::{STATUS_CANCELED, STATUS_COMPLETED};
    let event = &body["guild_scheduled_event"];
    let status = event["status"].as_u64().unwrap_or(0);
    if status == STATUS_COMPLETED || status == STATUS_CANCELED {
        return None;
    }
    snowflake(&event["id"])
}

/// The parts of an `EVENT` code: `{guild}-{event}[-{recurrence}]`, all numbers (`j` of module 484724).
pub(super) fn event_code(code: &str) -> Option<(Id, Id, Option<String>)> {
    let parts: Vec<&str> = code.split('-').collect();
    let number = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    match parts.as_slice() {
        [guild, event] if number(guild) && number(event) => {
            Some((guild.parse().ok()?, event.parse().ok()?, None))
        }
        [guild, event, recurrence, ..] if number(guild) && number(event) && number(recurrence) => {
            Some((
                guild.parse().ok()?,
                event.parse().ok()?,
                Some((*recurrence).to_owned()),
            ))
        }
        _ => None,
    }
}

/// The template card's state (GuildTemplateStore: `RESOLVING`, `EXPIRED`, resolved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TemplateCard {
    Resolving,
    Expired,
    Resolved { name: String, usage_count: u64 },
}

pub(super) fn template_card(body: Option<&Value>) -> TemplateCard {
    match body {
        None => TemplateCard::Resolving,
        Some(body) if body["fetch_failed"] == true || body["code"].as_str().is_none() => {
            TemplateCard::Expired
        }
        Some(body) => TemplateCard::Resolved {
            name: body["name"].as_str().unwrap_or_default().to_owned(),
            usage_count: body["usage_count"].as_u64().unwrap_or(0),
        },
    }
}

/// The event card's button (`$` of module 427080).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventButton {
    /// `XpeFYr` "Join" (active): join the server (not a member), or the event's channel (an active event).
    Join,
    /// `GoCQxU` "View Details" (secondary, no action): an active event somewhere else.
    ViewDetails,
    /// `aW2YlJ` "Go To Channel" (active): the account is in the event's channel.
    GoToChannel,
    /// `Pj7Xrv` "Event is over" (secondary, disabled).
    Over,
    /// `DlcqlU` "Interested", active with the check when interested, secondary with the bell otherwise.
    Interested(bool),
}

pub(super) fn event_button(
    member: bool,
    active: bool,
    ended: bool,
    external: bool,
    in_channel: bool,
    interested: bool,
) -> EventButton {
    if !member {
        return EventButton::Join;
    }
    if active {
        return if external {
            EventButton::ViewDetails
        } else if in_channel {
            EventButton::GoToChannel
        } else {
            EventButton::Join
        };
    }
    if ended {
        EventButton::Over
    } else {
        EventButton::Interested(interested)
    }
}

/// A message with `$b` runs (module 792996's rich formats) as text with its bold parts.
fn bold_runs(key: &str, values: &dc_intl::Args<'_>) -> AnyElement {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (part, tag) in dc_intl::format_runs(key, values) {
        let start = text.len();
        text.push_str(&part);
        if tag.as_deref() == Some("$b") {
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
            ));
        }
    }
    gpui::StyledText::new(text)
        .with_highlights(highlights)
        .into_any_element()
}

/// `.wrapper_jsxF_b`: the invite button frame, 160 to 432 wide, 16px in, `--background-surface-high` behind a 1px
/// `--border-subtle` outline at `--radius-sm`.
fn frame() -> gpui::Div {
    div()
        .w_full()
        .max_w(px(432.))
        .min_w(px(160.))
        .p(px(16.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(color("--background-surface-high"))
        .flex()
        .flex_col()
}

/// `.header_jsxF_b` on `heading-sm/semibold`: 16/20 in the bold weight, `--interactive-text-default`, 12px above the
/// body, one line.
fn header(text: &str) -> gpui::Div {
    div()
        .mb(px(12.))
        .truncate()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(700.))
        .text_color(color("--interactive-text-default"))
        .child(text.to_owned())
}

/// `.content_jsxF_b`: a wrapping row, centred, 16px apart.
fn body() -> gpui::Div {
    div().flex().flex_wrap().items_center().gap(px(16.))
}

/// `.headerLine_aoI0WQ`: the icon and the text, taking the row before the button.
fn header_line() -> gpui::Div {
    div()
        .flex_grow(1000.)
        .flex_shrink_0()
        .max_w_full()
        .flex()
        .items_center()
        .gap(px(16.))
}

/// `InviteButton.Info`: the title (heading-sm/semibold, one line, 2px above the detail; red when expired, a link to
/// the destination when it can be clicked) over the detail (body-sm/normal, 14/16, `--interactive-text-default`).
fn info(title: AnyElement, detail: Option<AnyElement>) -> gpui::Div {
    div()
        .min_w(px(1.))
        .flex()
        .flex_col()
        .justify_center()
        .child(title)
        .children(detail.map(|detail| {
            div()
                .text_size(px(14.))
                .line_height(px(16.))
                .text_color(color("--interactive-text-default"))
                .child(detail)
        }))
}

fn title_text(text: String, expired: bool) -> gpui::Div {
    div()
        .mb(px(2.))
        .truncate()
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(600.))
        .text_color(color(if expired {
            "--text-feedback-critical"
        } else {
            "--text-strong"
        }))
        .child(text)
}

/// `.guildIconExpired_jsxF_b`: 48px, 12px corners, the envelope (26px) in `--icon-feedback-critical` on
/// `--opacity-red-12`.
fn expired_icon() -> AnyElement {
    div()
        .size(px(48.))
        .flex_none()
        .rounded(px(12.))
        .border_1()
        .border_color(color("--opacity-red-12"))
        .bg(color("--opacity-red-12"))
        .flex()
        .items_center()
        .justify_center()
        .child(icon("access_envelope", 26., "--icon-feedback-critical"))
        .into_any_element()
}

/// `InviteButton.Body` while resolving: the masked placeholder (a 50px tile and two bars, 259 by 50) and the fake
/// button (at least 66 by 38), in the dark images' `--primary-600`.
fn resolving() -> AnyElement {
    let bar = |left: f32, top: f32, width: f32, height: f32| {
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .rounded(px(3.))
            .bg(color("--primary-600"))
    };
    div()
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(16.))
        .child(
            div()
                .relative()
                .flex_grow(1000.)
                .flex_shrink_0()
                .w(px(259.))
                .h(px(50.))
                .child(bar(0., 0., 50., 50.).rounded(px(16.)))
                .child(bar(59., 6., 130., 20.))
                .child(bar(59., 30., 200., 16.)),
        )
        .child(
            div()
                .flex_grow(1.)
                .flex_shrink_0()
                .min_w(px(66.))
                .h(px(38.))
                .rounded(px(3.))
                .bg(color("--primary-600")),
        )
        .into_any_element()
}

impl Root {
    /// `renderCodedLinks`: one card per coded link, in the links' order.
    pub(super) fn coded_link_cards(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let links = coded_links::message_coded_links(message.kind, &message.content);
        if links.is_empty() {
            return None;
        }
        let cards: Vec<AnyElement> = links
            .iter()
            .filter_map(|link| self.coded_link_card(link, message, cx))
            .collect();
        (!cards.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(cards)
                .into_any_element()
        })
    }

    fn coded_link_card(
        &self,
        link: &CodedLink,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match link.kind {
            Kind::Invite => {
                // `shouldRenderInvite`: a one-to-one DM hides the cards of a few official servers.
                let base = coded_links::split_invite_key(&link.code).base_code;
                if self.current_dm().is_some_and(|dm| dm.kind == 1)
                    && DM_HIDDEN_INVITES.contains(&base.as_str())
                {
                    return None;
                }
                self.invite_card(&link.code, message, cx)
            }
            Kind::Template => Some(self.template_link_card(&link.code, cx)),
            Kind::Event => self.event_link_card(&link.code, cx),
            Kind::ChannelLink => self.channel_link_card(&link.code, cx),
            Kind::QuestsEmbed => Some(self.quest_link_card(&link.code, message, cx)),
            _ => self.other_card(link, message, cx),
        }
    }

    /// The quest card of a `/quests/{id}` link (module 107543 + 562293).
    fn quest_link_card(
        &self,
        quest: &str,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let prefix = format!("msg-quest-{}", message.id);
        if let Some(found) = self
            .quests
            .store
            .quests
            .iter()
            .find(|known| known.id == quest)
        {
            return self.quest_promoted_card(found, &prefix, cx);
        }
        let reason = if self
            .quests
            .store
            .excluded_quests
            .iter()
            .any(|excluded| excluded == quest)
        {
            super::quest_promoted::QuestFallbackReason::Excluded
        } else {
            super::quest_promoted::QuestFallbackReason::NotFound
        };
        self.quest_embed_fallback_card(quest, reason, &prefix, cx)
    }

    /// The coded links of the open channel's messages that need something from the server: each invite
    /// (`resolveInvite`), template (`resolveGuildTemplate`) and event (`fetchGuildEvent`) once.
    pub(super) fn request_coded_links(&mut self) {
        let links: Vec<CodedLink> = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .into_iter()
            .flatten()
            .flat_map(|message| coded_links::message_coded_links(message.kind, &message.content))
            .collect();
        for link in links {
            match link.kind {
                Kind::Template => {
                    let key = format!("{TEMPLATE_KEY}{}", link.code);
                    if self.coded_links.asked.insert(key.clone()) {
                        let _ = self.requests.send(Request::Fetch {
                            route: format!("/guilds/templates/{}", link.code),
                            key,
                        });
                    }
                }
                Kind::Event => {
                    let Some((guild, event, _)) = event_code(&link.code) else {
                        continue;
                    };
                    // `getGuildEventUserCountsForEmbed` (module 484724, through 826383): the interested count.
                    if self.coded_links.asked.insert(format!("coded-event-counts:{event}")) {
                        self.event_fetch_counts(guild, event);
                    }
                    if self
                        .stores
                        .scheduled_events
                        .get_guild_scheduled_event(event)
                        .is_some()
                    {
                        self.request_event_cover(event);
                        continue;
                    }
                    let key = format!("{EVENT_KEY}{guild}:{event}");
                    if self.coded_links.asked.insert(key.clone()) {
                        let _ = self.requests.send(Request::Fetch {
                            key,
                            route: format!("/guilds/{guild}/scheduled-events/{event}"),
                        });
                    }
                }
                // The event card of an invite reads its count the same way (826383).
                Kind::Invite => {
                    let Some(body) = self.fetched.get(&format!("invite:{}", link.code)) else {
                        continue;
                    };
                    let (Some(event), Some(guild)) =
                        (invite_event(body), snowflake(&body["guild"]["id"]))
                    else {
                        continue;
                    };
                    if self.coded_links.asked.insert(format!("coded-event-counts:{event}")) {
                        self.event_fetch_counts(guild, event);
                    }
                }
                _ => {}
            }
        }
    }

    /// The app router's `/app/gifts/:giftCode` (module 596339): the gift is resolved, then its accept dialog opens
    /// (`h({processedCode})`), with no channel.
    pub(super) fn open_gift_link(&mut self, code: &str, cx: &mut Context<Self>) {
        if self.stores.gift_code(code).is_some() {
            self.open_gift_accept(code, None, cx);
            return;
        }
        self.coded_links.gift_route = Some(code.to_owned());
        self.stores
            .dispatch("GIFT_CODE_RESOLVE", &serde_json::json!({"code": code}));
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{GIFT_ROUTE_KEY}{code}")),
            method: reqwest::Method::GET,
            route: format!(
                "/entitlements/gift-codes/{code}?with_application=true&with_subscription_plan=true"
            ),
            body: None,
        });
        cx.notify();
    }

    /// True the first time `key` is asked for (a request a card sends once).
    pub(super) fn coded_link_ask(&mut self, key: String) -> bool {
        self.coded_links.asked.insert(key)
    }

    /// A fetched event or template; true when the key was one.
    pub(super) fn coded_link_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        // A template answer, found or not (a failed fetch never reaches the shared answers): GuildTemplateStore's
        // `GUILD_TEMPLATE_RESOLVE_SUCCESS` / `_FAILURE` (an unknown template is `EXPIRED`).
        if key.starts_with(TEMPLATE_KEY) {
            self.fetched.insert(key.to_owned(), body.clone());
            cx.notify();
            return true;
        }
        if let Some(code) = key.strip_prefix(GIFT_ROUTE_KEY) {
            let code = code.to_owned();
            if body["ok"].as_bool() == Some(true) {
                self.stores.dispatch(
                    "GIFT_CODE_RESOLVE_SUCCESS",
                    &serde_json::json!({"giftCode": body["body"].clone()}),
                );
            } else {
                self.stores.dispatch(
                    "GIFT_CODE_RESOLVE_FAILURE",
                    &serde_json::json!({"code": code, "error": body["body"].clone()}),
                );
            }
            if self.coded_links.gift_route.as_deref() == Some(code.as_str()) {
                self.coded_links.gift_route = None;
                if self.stores.gift_code(&code).is_some() {
                    self.open_gift_accept(&code, None, cx);
                } else {
                    // `HY`: the router's failure handler logs it; nothing opens.
                    tracing::info!("gift link: the code did not resolve");
                }
            }
            cx.notify();
            return true;
        }
        if let Some(rest) = key.strip_prefix(EVENT_KEY) {
            if body["fetch_failed"] != true {
                self.stores.dispatch(
                    "FETCH_GUILD_EVENT",
                    &serde_json::json!({"guildScheduledEvent": body}),
                );
                if let Some(event) = snowflake(&body["id"]) {
                    self.request_event_cover(event);
                }
            }
            let parts: Vec<Id> = rest
                .split(':')
                .filter_map(|part| part.parse().ok())
                .collect();
            if let [guild, event] = parts.as_slice()
                && self.coded_links.event_click == Some((*guild, *event))
            {
                self.coded_links.event_click = None;
                if body["fetch_failed"] == true {
                    self.show_inaccessible_link(false);
                } else {
                    self.open_event_link(*guild, *event, cx);
                }
            }
            cx.notify();
            return true;
        }
        false
    }

    fn request_event_cover(&mut self, event: Id) {
        let Some(found) = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event)
        else {
            return;
        };
        let Some(image) = found.image.clone() else {
            return;
        };
        let key = format!("{EVENT_COVER_KEY}{event}");
        if self.coded_links.asked.insert(key.clone()) {
            // Module 691012: `https://{CDN_HOST}/guild-events/{id}/{image}?size=1024`.
            self.request_image(
                key,
                format!("https://cdn.discordapp.com/guild-events/{event}/{image}?size=1024"),
            );
        }
    }

    /// Module 697794 `Y` for an invite link: resolve, then the server or the invite dialog.
    pub(super) fn open_invite_link(&mut self, key: &str, cx: &mut Context<Self>) {
        let body = self.fetched.get(&format!("invite:{key}")).cloned();
        let decision = {
            let member_of = |guild: Id| self.stores.guilds.iter().any(|known| known.id == guild);
            let my_roles = |guild: Id| {
                self.stores
                    .guild(guild)
                    .map(|known| known.my_roles.clone())
                    .unwrap_or_default()
            };
            invite_click(body.as_ref(), &member_of, &my_roles)
        };
        match decision {
            InviteClick::Resolve => {
                self.coded_links.invite_click = Some(key.to_owned());
                let _ = self.requests.send(Request::Fetch {
                    key: format!("invite:{key}"),
                    route: invite_resolve_route(key),
                });
            }
            InviteClick::Modal => self.open_accept_invite_modal(key, cx),
            InviteClick::Transition { guild, channel } => {
                let channel = channel.filter(|channel| {
                    self.stores.guild(guild).is_some_and(|known| {
                        known
                            .channels
                            .iter()
                            .any(|known| known.id == *channel && known.kind != ChannelKind::Voice)
                    })
                });
                self.select(guild, channel);
            }
        }
        cx.notify();
    }

    /// An invite resolve that a click waits on.
    pub(super) fn invite_click_resolved(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(code) = key.strip_prefix("invite:") else {
            return;
        };
        if self.coded_links.invite_click.as_deref() == Some(code) {
            self.coded_links.invite_click = None;
            let code = code.to_owned();
            self.open_invite_link(&code, cx);
        }
    }

    /// Module 697794 for `/events/<guild>/<event>`: the event is fetched if needed, the account lurks into a server
    /// it is not in (here: the inaccessible link dialog, as this client does not lurk), the server opens, then the
    /// event's details (`uR`).
    pub(super) fn open_event_link(&mut self, guild: Id, event: Id, cx: &mut Context<Self>) {
        if self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event)
            .is_none()
        {
            self.coded_links.event_click = Some((guild, event));
            let _ = self.requests.send(Request::Fetch {
                key: format!("{EVENT_KEY}{guild}:{event}"),
                route: format!("/guilds/{guild}/scheduled-events/{event}"),
            });
            return;
        }
        if !self.stores.guilds.iter().any(|known| known.id == guild) {
            self.show_inaccessible_link(false);
            cx.notify();
            return;
        }
        if self.guild != Some(guild) {
            let channel = super::last_channel::channel_of(guild);
            self.select(guild, channel);
        }
        self.open_guild_events(cx);
        self.open_event_detail(event, cx);
    }

    /// `lO` of module 679636.
    fn template_link_card(&self, code: &str, cx: &mut Context<Self>) -> AnyElement {
        let state = template_card(self.fetched.get(&format!("{TEMPLATE_KEY}{code}")));
        let frame = frame().parity_with_border("coded-template", 1.);
        match state {
            TemplateCard::Resolving => frame
                .child(header(dc_intl::t("Xj87Yf")))
                .child(body().child(resolving()))
                .into_any_element(),
            TemplateCard::Expired => frame
                .child(header(dc_intl::t("C7ZRNw")))
                .child(body().child(expired_icon()).child(info(
                    title_text(dc_intl::t("A6MwXE").to_owned(), true).into_any_element(),
                    None,
                )))
                .into_any_element(),
            TemplateCard::Resolved { name, usage_count } => {
                let usage = bold_runs(
                    "cGXXHL",
                    &[("usageCount", dc_intl::Arg::Num(usage_count as f64))],
                );
                let code = code.to_owned();
                let button = ds_button(
                    format!("coded-template-{code}"),
                    dc_intl::t("a3Gl+e"),
                    ButtonLook::Active,
                    false,
                    None,
                )
                .w_auto()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_template_accept_modal(&code, window, cx);
                    cx.notify();
                }));
                frame
                    .child(header(dc_intl::t("kAvFkO")))
                    .child(
                        body()
                            .child(
                                header_line()
                                    .child(
                                        gpui::svg()
                                            .path("icons/template_embed.svg")
                                            .size(px(40.))
                                            .flex_none()
                                            .text_color(color("--interactive-text-default")),
                                    )
                                    .child(info(
                                        title_text(name, false).into_any_element(),
                                        Some(usage),
                                    )),
                            )
                            .child(button),
                    )
                    .into_any_element()
            }
        }
    }

    /// `InviteButton.Icon` for a server: the 50px icon (`Sizes.LARGE`, 15px corners), the acronym at 16px on
    /// `--background-mod-subtle` when there is no picture.
    fn invite_guild_icon(&self, guild: Id, name: &str) -> gpui::Div {
        let picture = self.images.get(&format!("guild:{guild}")).cloned();
        let acronym = dc_state::Guild::acronym_of(name);
        // `R[LARGE]` of module 682163: the acronym's size by its length.
        let sizes = [18., 18., 16., 16., 14., 12., 10.];
        let size = sizes[acronym.chars().count().min(sizes.len() - 1)];
        let tile = div()
            .size(px(50.))
            .flex_none()
            .rounded(px(15.))
            .overflow_hidden()
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size))
            .line_height(px(50.))
            .text_color(color("--text-default"));
        match picture {
            Some(picture) => tile.child(
                img(picture)
                    .size(px(50.))
                    .rounded(px(15.))
                    .object_fit(ObjectFit::Cover),
            ),
            None => tile.child(acronym),
        }
    }

    /// `ak`/`aS` of module 679636: a voice or Stage channel the account may see and connect to.
    fn channel_link_card(&self, code: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (guild_part, channel_part) = code.split_once('/')?;
        let guild_id: Id = guild_part.parse().ok()?;
        let channel_id: Id = channel_part.parse().ok()?;
        let guild = self.stores.guild(guild_id)?;
        let channel = guild.channels.iter().find(|known| known.id == channel_id)?;
        let stage = channel.kind == ChannelKind::Other(13);
        if !(channel.kind == ChannelKind::Voice || stage) {
            return None;
        }
        if !self.my_permission(guild_id, Some(channel), permissions::VIEW_CHANNEL)
            || !self.my_permission(guild_id, Some(channel), permissions::CONNECT)
        {
            return None;
        }
        let name = channel.name.clone();
        let guild_name = guild.name.clone();
        let join = move |this: &mut Root, cx: &mut Context<Root>| {
            this.select(guild_id, None);
            this.request_join_voice(guild_id, channel_id, cx);
            cx.notify();
        };
        let channel_title = div()
            .id(SharedString::from(format!(
                "coded-channel-title-{channel_id}"
            )))
            .mb(px(2.))
            .flex()
            .items_center()
            .min_w(px(0.))
            .cursor_pointer()
            .hover(|style| style.underline())
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(icon(
                if stage {
                    "stage_channel"
                } else {
                    "channel_type_voice"
                },
                20.,
                "--text-muted",
            ))
            .child(div().ml(px(4.)).truncate().child(name))
            .on_click(cx.listener(move |this, _, _, cx| join(this, cx)));
        let detail = div()
            .flex()
            .items_center()
            .overflow_hidden()
            .child(dc_intl::format(
                "2wimj5",
                &[("guildName", dc_intl::Arg::from(guild_name.as_str()))],
            ))
            .into_any_element();
        let button = ds_button(
            format!("coded-channel-join-{channel_id}"),
            dc_intl::t(if stage { "7vb2cc" } else { "96ANUN" }),
            ButtonLook::Active,
            false,
            None,
        )
        .on_click(cx.listener(move |this, _, _, cx| join(this, cx)));
        Some(
            frame()
                .parity_with_border("coded-channel-link", 1.)
                .child(
                    body()
                        .child(
                            header_line()
                                .child(self.invite_guild_icon(guild_id, &guild.name))
                                .child(info(channel_title.into_any_element(), Some(detail))),
                        )
                        // `.buttonContainer_aoI0WQ`: the full-width button takes the rest of the row.
                        .child(div().flex_1().child(button)),
                )
                .into_any_element(),
        )
    }

    /// Module 484724 over module 427080: the event card. Nothing until the event is known (a fetch that fails draws
    /// nothing either).
    fn event_link_card(&self, code: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_guild, event_id, _recurrence) = event_code(code)?;
        let event = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event_id)?
            .clone();
        Some(self.scheduled_event_card(&event, None, cx))
    }

    /// Module 427080 for an event: from an event link (`invite` none, module 484724) or from an invite that carries
    /// one (`cr`'s `ly` branch, whose Join accepts the invite).
    pub(super) fn scheduled_event_card(
        &self,
        event: &dc_state::scheduled_events::ScheduledEvent,
        invite: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let event = event.clone();
        let guild_id = event.guild_id;
        let event_id = event.id;
        use dc_state::scheduled_events::{
            ENTITY_EXTERNAL, STATUS_ACTIVE, STATUS_CANCELED, STATUS_COMPLETED,
        };
        let guild = self.stores.guild(guild_id);
        let member = guild.is_some();
        let now = chrono::Local::now().naive_local();
        let status = super::guild_events::list::event_status(&event, now, false);
        let creator = event
            .creator_id
            .and_then(|creator| self.event_user(creator));
        let count = self.stores.scheduled_events.get_user_count(event_id, None);
        let active = event.status == STATUS_ACTIVE;
        let ended = matches!(event.status, STATUS_COMPLETED | STATUS_CANCELED);
        let external = event.entity_type == ENTITY_EXTERNAL;
        let in_channel = event.channel_id.is_some()
            && self
                .call
                .as_ref()
                .is_some_and(|call| Some(call.channel) == event.channel_id);
        let interested = self.event_interested(event_id);
        let decision = event_button(member, active, ended, external, in_channel, interested);
        let (label, look, leading, disabled) = match decision {
            EventButton::Join => ("XpeFYr", ButtonLook::Active, None, false),
            EventButton::ViewDetails => ("GoCQxU", ButtonLook::Secondary, None, true),
            EventButton::GoToChannel => ("aW2YlJ", ButtonLook::Active, None, false),
            EventButton::Over => ("Pj7Xrv", ButtonLook::Secondary, None, true),
            EventButton::Interested(true) => {
                ("DlcqlU", ButtonLook::Active, Some("checkmark_large"), false)
            }
            EventButton::Interested(false) => {
                ("DlcqlU", ButtonLook::Secondary, Some("bell"), false)
            }
        };
        let channel = event.channel_id;
        let button = ds_button(
            format!("coded-event-button-{event_id}"),
            dc_intl::t(label),
            look,
            false,
            leading,
        )
        .w_auto()
        .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            match decision {
                EventButton::Join if !member => match &invite {
                    // `onAcceptInstantInvite` of `cr`: the invite is accepted.
                    Some(key) => this.accept_invite_code(key, guild_id),
                    // `joinGuild` as a lurker: this client does not lurk, so the link is out of reach.
                    None => this.show_inaccessible_link(false),
                },
                EventButton::Join | EventButton::GoToChannel => {
                    if let Some(channel) = channel {
                        this.request_join_voice(guild_id, channel, cx);
                    }
                }
                EventButton::Interested(_) => this.toggle_event_interest(event_id, cx),
                EventButton::ViewDetails | EventButton::Over => {}
            }
            cx.notify();
        }));
        let cover = self
            .images
            .get(&format!("{EVENT_COVER_KEY}{event_id}"))
            .cloned()
            .map(|picture| {
                // `.banner_tDgX5x` over `.container_S6aeAs`: 164px, cover, out to the frame's edges.
                div()
                    .mt(px(-16.))
                    .mx(px(-16.))
                    .mb(px(16.))
                    .h(px(164.))
                    .rounded_t(px(4.))
                    .overflow_hidden()
                    .child(
                        img(picture)
                            .w_full()
                            .h(px(164.))
                            .object_fit(ObjectFit::Cover),
                    )
            });
        // `L` of module 557582: the status, then (pushed right) the creator's 20px avatar and the interested count
        // (`rsvpCount_dUw4ZS`: 20px, 8px in, 12px corners on `--background-base-lowest`).
        let status_line = div()
            .mb(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .items_center()
            .children(status.as_ref().map(super::guild_events::list::status_row))
            .child(div().flex_1())
            .children(creator.as_ref().map(|creator| {
                div()
                    .mr(px(8.))
                    .child(self.event_avatar(Some(creator), 20.))
            }))
            .child(
                div()
                    .h(px(20.))
                    .px(px(8.))
                    .rounded(px(12.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .mr(px(4.))
                            .child(icon("event_rsvp", 12., "--text-default")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(count.to_string()),
                    ),
            );
        // `D`: the name in heading-lg/medium (20/24), one line, under 534px; the description 8px below in 14/18.
        let name = div()
            .max_w(px(534.))
            .truncate()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(event.name.clone());
        let description = event
            .description
            .clone()
            .filter(|text| !text.is_empty())
            .map(|text| {
                div()
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(text)
            });
        // `U`: the server's icon, its name (body-sm/medium) and where the event is (body-xs/normal, 4px below).
        let (place_icon, place) = self.event_place(&event);
        let footer_guild = guild.map(|guild| {
            div()
                .flex_grow(1000.)
                .flex_shrink_0()
                .max_w_full()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(px(16.))
                .child(self.invite_guild_icon(guild.id, &guild.name))
                .child(
                    div()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(guild.name.clone()),
                        )
                        .child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .text_color(color("--text-default"))
                                .child(icon(place_icon, 12., "--text-default"))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .child(place),
                                ),
                        ),
                )
        });
        let footer = div()
            .mt(px(12.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(16.))
            .overflow_hidden()
            .children(footer_guild)
            .child(button);
        let open = member;
        frame()
            .id(SharedString::from(format!("coded-event-{event_id}")))
            .parity_with_border("coded-event", 1.)
            .when(open, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_event_link(guild_id, event_id, cx);
                    }))
            })
            .children(cover)
            .child(status_line)
            .child(name)
            .children(description)
            .child(footer)
            .into_any_element()
    }

    /// Capture only: `coded-links follow <url>` follows a link as a click on it in a message does;
    /// `coded-links state` says what the last message's coded links are and what is open;
    /// `coded-links template <code>` presses a template card's View Template.
    #[cfg(feature = "capture")]
    pub(crate) fn script_coded_links(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        match args {
            ["follow", url] => {
                self.follow_link(url, url, cx);
                format!("coded-links: followed {url}; {}", self.coded_links_open())
            }
            ["static", id] => {
                self.open_static_route(id, cx);
                format!("coded-links: static {id}; {}", self.coded_links_open())
            }
            ["template", code] => {
                let code = (*code).to_owned();
                if let Some(handle) = cx.windows().into_iter().next() {
                    let root = cx.entity();
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |_, window, cx| {
                            root.update(cx, |root, cx| {
                                root.open_template_accept_modal(&code, window, cx)
                            });
                        });
                    });
                }
                "coded-links: template card pressed".to_owned()
            }
            _ => {
                let last = self
                    .channel
                    .and_then(|channel| self.stores.messages.get(&channel))
                    .and_then(|messages| messages.last());
                let links: Vec<String> = last
                    .map(|message| coded_links::message_coded_links(message.kind, &message.content))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|link| format!("{:?}:{}", link.kind, link.code))
                    .collect();
                format!("coded-links: last={links:?}; {}", self.coded_links_open())
            }
        }
    }

    #[cfg(feature = "capture")]
    fn coded_links_open(&self) -> String {
        format!(
            "guild={:?} channel={:?} guide={} browser={} invite_modal={:?} template_modal={} gift_modal={} events={:?} boost_page={} pending_invite={:?}",
            self.guild,
            self.channel,
            self.guild_guide_open,
            self.channel_browser_open,
            self.accept_invite_modal
                .as_ref()
                .map(|modal| modal.code.clone()),
            self.template_accept.is_some(),
            self.settings_extra.gift.modal.is_some(),
            self.events_open(),
            self.boost_page.is_some(),
            self.coded_links.invite_click,
        )
    }

    /// The types without a card of their own here. The official cards need data this client does not load yet
    /// (application directory listings, SKUs and store listings, game profiles, guild products, collectibles); the
    /// link itself stays in the text and opens where `follow_link` takes it.
    fn other_card(
        &self,
        _link: &CodedLink,
        _message: &Message,
        _cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_invite_click_waits_for_its_resolve() {
        let none = |_: Id| false;
        let no_roles = |_: Id| Vec::new();
        assert_eq!(invite_click(None, &none, &no_roles), InviteClick::Resolve);
        let failed = json!({"invite_resolve_failed": true, "banned": false});
        assert_eq!(
            invite_click(Some(&failed), &none, &no_roles),
            InviteClick::Modal
        );
    }

    #[test]
    fn a_member_goes_to_the_server_unless_the_invite_gives_new_roles() {
        let body = json!({"guild": {"id": "10"}, "channel": {"id": "20"}, "roles": [{"id": "5"}]});
        let member = |guild: Id| guild == 10;
        let stranger = |_: Id| false;
        assert_eq!(
            invite_click(Some(&body), &member, &|_| vec![5]),
            InviteClick::Transition {
                guild: 10,
                channel: Some(20)
            }
        );
        assert_eq!(
            invite_click(Some(&body), &member, &|_| vec![]),
            InviteClick::Modal
        );
        assert_eq!(
            invite_click(Some(&body), &stranger, &|_| vec![5]),
            InviteClick::Modal
        );
        let plain = json!({"guild": {"id": "10"}, "channel": {"id": "20"}});
        assert!(matches!(
            invite_click(Some(&plain), &member, &|_| vec![]),
            InviteClick::Transition { .. }
        ));
    }

    #[test]
    fn invite_routes_carry_the_key_query() {
        assert_eq!(
            invite_resolve_route("abc"),
            "/invites/abc?with_counts=true&with_expiration=true&with_permissions=true"
        );
        assert_eq!(
            invite_resolve_route(
                "abc?event=99&channel=12345678901234567&message=12345678901234568"
            ),
            "/invites/abc?with_counts=true&with_expiration=true&guild_scheduled_event_id=99\
             &target_channel_id=12345678901234567&target_message_id=12345678901234568&with_permissions=true"
        );
    }

    #[test]
    fn an_invite_shows_its_event_until_the_event_ends() {
        let invite = |status: u64| json!({"guild_scheduled_event": {"id": "77", "guild_id": "1", "status": status}});
        assert_eq!(invite_event(&invite(1)), Some(77));
        assert_eq!(invite_event(&invite(2)), Some(77));
        assert_eq!(invite_event(&invite(3)), None);
        assert_eq!(invite_event(&invite(4)), None);
        assert_eq!(invite_event(&json!({"guild": {"id": "1"}})), None);
    }

    #[test]
    fn event_codes_need_numbers() {
        assert_eq!(event_code("1-2"), Some((1, 2, None)));
        assert_eq!(event_code("1-2-3"), Some((1, 2, Some("3".to_owned()))));
        assert_eq!(event_code("1-x"), None);
        assert_eq!(event_code("1"), None);
    }

    #[test]
    fn template_cards_follow_the_store_states() {
        assert_eq!(template_card(None), TemplateCard::Resolving);
        assert_eq!(
            template_card(Some(&json!({"fetch_failed": true, "status": 404}))),
            TemplateCard::Expired
        );
        assert_eq!(
            template_card(Some(
                &json!({"code": "abc", "name": "Gaming", "usage_count": 3})
            )),
            TemplateCard::Resolved {
                name: "Gaming".to_owned(),
                usage_count: 3
            }
        );
    }

    #[test]
    fn event_buttons_follow_module_427080() {
        assert_eq!(
            event_button(false, true, false, false, false, false),
            EventButton::Join
        );
        assert_eq!(
            event_button(true, true, false, true, false, false),
            EventButton::ViewDetails
        );
        assert_eq!(
            event_button(true, true, false, false, true, false),
            EventButton::GoToChannel
        );
        assert_eq!(
            event_button(true, true, false, false, false, false),
            EventButton::Join
        );
        assert_eq!(
            event_button(true, false, true, false, false, false),
            EventButton::Over
        );
        assert_eq!(
            event_button(true, false, false, false, false, true),
            EventButton::Interested(true)
        );
        assert_eq!(
            event_button(true, false, false, false, false, false),
            EventButton::Interested(false)
        );
    }
}
