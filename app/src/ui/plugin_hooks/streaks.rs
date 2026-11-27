//! Streaks (Equicord `src/equicordplugins/streaks`, GPL-3.0-or-later): a flame and the day count beside a person you
//! exchange DMs with on consecutive days, from the Streaks service. The badge sits after the name of their messages
//! (in a DM, and elsewhere with `showOutsideDMs`; on your own DM messages with `showOnSelf`, as the other person's)
//! and in their row of the direct messages list, with "N day streak" as its tooltip; it takes its tier's colour (the
//! seven colour settings) while both of you wrote today, else grey. A DM message reports the day to the service
//! (`MESSAGE_CREATE`: yours or theirs, until both sides are in, and when this machine's time zone is not the one the
//! service has for you). Everything needs the service's sign-in (`service_auth.rs`): the card's "Log In to Streaks
//! API" opens the consent modal of its application; "Log Out of Streaks API" forgets the token; a token the service
//! refuses is forgotten with "Streaks authorization expired. Reauthorize in the Streaks plugin settings.". Not part of
//! the pinned client.
//!
//! GAP: `backfillRecentActivity` (at the start, each streak DM's last 100 messages are read and today's reported) and
//! `reauthorize` (a refused token is renewed by a silent grant), both left to the user's own sign-in; `migrate`
//! has nothing to move (no local streaks of the old version here). A day in another person's time zone is taken as
//! this machine's date or UTC's (the app has no time zone database).

use super::super::tooltip::{Body, Side, Tip};
use super::super::{Root, color};
use crate::net::{Request, ServiceBody};
use crate::plugins::streaks::{self as api, Streak, Today};
use dc_state::{Id, Message};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb, svg,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;

const ID: &str = "Streaks";
/// The sign-in's caller name (`service_auth.rs`) and the credential entry's prefix.
pub(in crate::ui) const SERVICE: &str = "streaks";
const KEY: &str = "plugin-streaks:";

#[derive(Default)]
struct State {
    started: Option<Id>,
    streaks: HashMap<Id, Streak>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
    static ZONE: String = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_owned());
}

fn with<R>(read: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| read(&mut state.borrow_mut()))
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("Streaks")
}

fn today() -> Today {
    ZONE.with(|zone| Today::now(zone))
}

impl Root {
    fn streaks_me(&self) -> Option<Id> {
        self.stores.current_user.as_ref().map(|me| me.id)
    }

    fn streaks_token(&self) -> Option<String> {
        super::service_auth::token(SERVICE, self.streaks_me()?)
    }

    fn streaks_send(&self, step: String, method: reqwest::Method, path: &str, body: ServiceBody) {
        let Some(bearer) = self.streaks_token() else {
            return;
        };
        let _ = self.requests.send(Request::PluginService {
            key: format!("{KEY}{step}"),
            method,
            url: format!("{}{path}", api::API_URL),
            bearer: Some(bearer),
            body,
            exchange: false,
        });
    }

    /// `CONNECTION_OPEN`: the sign-in read once per account, then the streaks.
    fn streaks_start(&mut self, cx: &mut Context<Self>) {
        let Some(me) = self.streaks_me() else {
            return;
        };
        if with(|state| state.started.replace(me) == Some(me)) {
            return;
        }
        with(|state| state.streaks.clear());
        self.service_token_load(SERVICE, me, |this, cx| this.streaks_fetch(cx), cx);
    }

    /// `fetch()`.
    fn streaks_fetch(&mut self, cx: &mut Context<Self>) {
        self.streaks_send(
            "all".to_owned(),
            reqwest::Method::GET,
            "/streaks",
            ServiceBody::None,
        );
        cx.notify();
    }

    /// The gateway's events: the start, and a DM message reports the day (`MESSAGE_CREATE`).
    pub(in crate::ui) fn streaks_dispatch(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) {
        if !on() {
            return;
        }
        self.streaks_start(cx);
        if name != "MESSAGE_CREATE" || self.streaks_token().is_none() {
            return;
        }
        if data["author"]["bot"].as_bool() == Some(true) {
            return;
        }
        let Some(channel) = data["channel_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        let Some(recipient) = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == channel && dm.kind == 1)
            .and_then(|dm| dm.recipients.first().copied())
        else {
            return;
        };
        let (Some(me), Some(author)) = (
            self.streaks_me(),
            data["author"]["id"]
                .as_str()
                .and_then(|id| id.parse::<Id>().ok()),
        ) else {
            return;
        };
        let today = today();
        let cached = with(|state| state.streaks.get(&recipient).cloned());
        if api::reports(cached.as_ref(), me, author, recipient, &today) {
            self.streaks_send(
                format!("update:{recipient}"),
                reqwest::Method::POST,
                &format!("/streaks/{recipient}"),
                ServiceBody::Json(json!({"sender_id": author.to_string(), "timezone": today.zone})),
            );
        }
    }

    /// The grant's code, traded for the service's token (`authorize`'s callback: `AUTHORIZE_URL?code=…`, answered
    /// with `{access_token}`).
    pub(in crate::ui) fn streaks_code(&mut self, code: String, _cx: &mut Context<Self>) {
        let mut url = url::Url::parse(api::AUTHORIZE_URL).expect("static address");
        url.query_pairs_mut().append_pair("code", &code);
        let _ = self.requests.send(Request::PluginService {
            key: format!("{KEY}token"),
            method: reqwest::Method::GET,
            url: url.to_string(),
            bearer: None,
            body: ServiceBody::None,
            exchange: true,
        });
    }

    /// The service's answers. True when the key was Streaks'.
    pub(in crate::ui) fn streaks_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(step) = key.strip_prefix(KEY) else {
            return false;
        };
        let ok = body["ok"].as_bool() == Some(true);
        if step == "token" {
            let token = body["body"]["access_token"]
                .as_str()
                .filter(|token| !token.is_empty());
            match (ok, token, self.streaks_me()) {
                (true, Some(token), Some(me)) => {
                    self.service_token_set(SERVICE, me, token.to_owned());
                    self.streaks_fetch(cx);
                }
                _ => {
                    let status = body["status"].as_u64().unwrap_or(0);
                    self.put_app_toast(
                        format!("Failed to authorize: Request not OK: {status}"),
                        false,
                        cx,
                    );
                }
            }
            cx.notify();
            return true;
        }
        if body["status"].as_u64() == Some(401) {
            // `expireToken`.
            if let Some(me) = self.streaks_me() {
                self.service_token_forget(SERVICE, me);
            }
            with(|state| state.streaks.clear());
            self.put_app_toast(
                "Streaks authorization expired. Reauthorize in the Streaks plugin settings."
                    .to_owned(),
                false,
                cx,
            );
            cx.notify();
            return true;
        }
        if !ok {
            return true;
        }
        let Some(me) = self.streaks_me() else {
            return true;
        };
        if step == "all" {
            let streaks = api::by_other(&body["body"], me);
            with(|state| state.streaks = streaks);
        } else if let Some(recipient) = step
            .strip_prefix("update:")
            .and_then(|id| id.parse::<Id>().ok())
            && let Some(streak) = Streak::from_value(&body["body"])
        {
            with(|state| state.streaks.insert(recipient, streak));
        }
        cx.notify();
        true
    }

    /// `StreakBadge`: the flame and the count, `None` without a streak of at least a day.
    fn streaks_badge(&self, user: Id, place: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let streak = with(|state| state.streaks.get(&user).cloned())?;
        if streak.count < 1 {
            return None;
        }
        let tint = if api::active(&streak, &today()) {
            let key = api::color_key(streak.count);
            crate::plugins::with(|state| {
                let value = match key {
                    "eliteColor" => state.value(ID, "eliteColor"),
                    "diamondColor" => state.value(ID, "diamondColor"),
                    "platinumColor" => state.value(ID, "platinumColor"),
                    "goldColor" => state.value(ID, "goldColor"),
                    "silverColor" => state.value(ID, "silverColor"),
                    "bronzeColor" => state.value(ID, "bronzeColor"),
                    _ => state.value(ID, "defaultColor"),
                };
                value.as_str().and_then(api::parse_color)
            })
            .unwrap_or(api::IDLE_COLOR)
        } else {
            api::IDLE_COLOR
        };
        let badge = div()
            .ml(px(4.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                svg()
                    .path("icons/discover_fire.svg")
                    .size(px(16.))
                    .text_color(rgb(tint)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(12.))
                    .font_weight(FontWeight(700.))
                    .text_color(rgb(tint))
                    .child(streak.count.to_string()),
            );
        Some(
            self.with_tip(
                format!("streaks-{place}-{user}"),
                Tip::body(
                    Body::Text(format!("{} day streak", streak.count).into()),
                    Side::Top,
                ),
                badge,
                cx,
            )
            .into_any_element(),
        )
    }

    /// `renderMessageDecoration`: after the name of a message's author.
    pub(in crate::ui) fn streaks_message_badge(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        let dm = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.id == message.channel_id && dm.kind == 1);
        let author = message.author.id;
        if Some(author) == self.streaks_me() {
            let show =
                crate::plugins::with(|state| state.value(ID, "showOnSelf")) == Value::Bool(true);
            let recipient = dm.filter(|_| show)?.recipients.first().copied()?;
            return self.streaks_badge(recipient, &format!("message-{}", message.id), cx);
        }
        if dm.is_none()
            && crate::plugins::with(|state| state.value(ID, "showOutsideDMs")) == Value::Bool(false)
        {
            return None;
        }
        self.streaks_badge(author, &format!("message-{}", message.id), cx)
    }

    /// `renderMemberListDecorator` of type `dm`: in a person's row of the direct messages list.
    pub(in crate::ui) fn streaks_dm_badge(
        &self,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() || Some(user) == self.streaks_me() {
            return None;
        }
        self.streaks_badge(user, "dm", cx)
    }

    /// The card's `account` component: Log In to Streaks API, or Log Out of Streaks API.
    pub(in crate::ui) fn streaks_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let signed_in = self.streaks_token().is_some();
        div()
            .flex()
            .child(
                div()
                    .id("streaks-account")
                    .h(px(38.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .when(signed_in, |this| {
                        this.bg(color("--control-critical-primary-background-default"))
                            .hover(|style| {
                                style.bg(color("--control-critical-primary-background-hover"))
                            })
                            .child("Log Out of Streaks API")
                    })
                    .when(!signed_in, |this| {
                        this.bg(color("--control-primary-background-default"))
                            .hover(|style| style.bg(color("--control-primary-background-hover")))
                            .child("Log In to Streaks API")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let Some(me) = this.streaks_me() else {
                            return;
                        };
                        if signed_in {
                            this.service_token_forget(SERVICE, me);
                            with(|state| state.streaks.clear());
                        } else {
                            this.service_sign_in(
                                SERVICE,
                                api::CLIENT_ID.to_owned(),
                                api::AUTHORIZE_URL.to_owned(),
                                cx,
                            );
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Capture only: the stand-in sign-in changed (`plugin token Streaks …`).
    #[cfg(feature = "capture")]
    pub(in crate::ui) fn streaks_token_ready(&mut self, cx: &mut Context<Self>) {
        self.streaks_start(cx);
        self.streaks_fetch(cx);
    }

    /// Capture only: `streaks state`.
    #[cfg(feature = "capture")]
    pub(crate) fn streaks_state_line(&self) -> String {
        with(|state| {
            let mut counts: Vec<String> = state
                .streaks
                .iter()
                .map(|(user, streak)| format!("{user}:{}", streak.count))
                .collect();
            counts.sort();
            format!(
                "started={} signed_in={} streaks=[{}]",
                state.started.is_some(),
                self.streaks_token().is_some(),
                counts.join(",")
            )
        })
    }
}
