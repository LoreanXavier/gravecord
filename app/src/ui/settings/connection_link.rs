//! The two-way link of the Xbox, PlayStation Network and Crunchyroll dialogs (modules 873885, 910316, 670404).
//!
//! The official order:
//! 1. The pre-connect slide's button asks `GET /connections/{type}/authorize?two_way_link_type=desktop&two_way_link=true`
//!    (`authorize` of module 77468 with `twoWayLinkType: DESKTOP`, module 13202) and opens the answered address in the
//!    browser (738104). The address's `state` becomes `expectedCallbackState` and the waiting slide shows
//!    (`handleWaitingForConnection`). The waiting slide's Retry asks again but keeps the state it waits for (it gets no
//!    `onWaitingForConnection`).
//! 2. The provider's sign-in ends with the gateway's `USER_CONNECTIONS_LINK_CALLBACK` (`provider`, `callback_code`,
//!    `callback_state`, module 225123). A state other than the expected one is ignored (`received mismatching callback
//!    state!`); the expected one keeps the code and opens the Discord consent slide (`handleAuthToken`).
//! 3. The consent slide (910316) is the OAuth2 hook 610056 for the platform's application (`isTrustedName`,
//!    `isEmbeddedFlow`, `isTwoWayLinkDiscordConsent`): `GET /oauth2/authorize` (guild install, the platform's scopes,
//!    PlayStation's redirect) and `GET /applications/{id}/disclosures`. Authorize and Continue posts
//!    `authorize: true`, acknowledges the disclosures (`Yx`), then `completeTwoWayLink` posts
//!    `/connections/{type}/callback` with the callback's code and state and the `code` of the answered location as
//!    `two_way_link_code` (module 306677 adds `insecure` and `friend_sync`). Success shows the last slide; any failure
//!    the error slide, PlayStation's with the parent-account text for `UNDER_MINIMUM_AGE` (20024).
//!
//! Granting stays the user's: a capture build sends none of the requests after the browser hand-off (the consent
//! GETs, the authorize POST, the acknowledgement or the callback); local fixtures (`fetched`, `gw`) stand in for their
//! answers. `GRAVECORD_ALLOW_AUTHORIZE` lifts that for a deliberate run.

use super::Root;
use super::connection_dialogs::{Dlg, Slide};
use crate::net::Request;
use crate::ui::oauth;
use gpui::Context;
use serde_json::{Value, json};

/// The answer keys of the consent slide's requests: `<LINK_KEY><step>:<type>`.
pub(super) const LINK_KEY: &str = "settings-connection-link-";

/// `UNDER_MINIMUM_AGE` of module 652215 (`t02`).
pub(super) const UNDER_MINIMUM_AGE: i64 = 20024;

/// The platform's application (`XBOX_APPLICATION_ID`, `PLAYSTATION_APPLICATION_ID` of module 333748, `ex` of 539170).
pub(super) fn client_id(kind: &str) -> Option<&'static str> {
    match kind {
        "xbox" => Some("622174530214821906"),
        "playstation" => Some("1008890872156405890"),
        "crunchyroll" => Some("981509069309354054"),
        _ => None,
    }
}

/// The scopes the consent asks for: `sS` (Xbox) and `NL` (PlayStation) of module 544105, `_B` of 539170.
pub(super) fn scopes(kind: &str) -> &'static [&'static str] {
    match kind {
        "xbox" => &[
            "voice",
            "dm_channels.read",
            "guilds",
            "guilds.members.read",
            "identify",
            "connections",
            "activities.read",
        ],
        "playstation" => &[
            "voice",
            "dm_channels.read",
            "guilds",
            "guilds.members.read",
            "identify",
            "connections",
        ],
        "crunchyroll" => &["activities.write", "email", "identify"],
        _ => &[],
    }
}

/// PlayStation's consent passes `redirectUri` (the `PLAYSTATION` link page of module 158375).
pub(super) fn redirect_uri(kind: &str) -> Option<&'static str> {
    (kind == "playstation").then_some("https://discord.com/connections/playstation/link")
}

/// The pre-connect slide's request (`authorize` of module 77468 with `twoWayLinkType: "desktop"`).
pub(super) fn authorize_route(kind: &str) -> String {
    format!("/connections/{kind}/authorize?two_way_link_type=desktop&two_way_link=true")
}

/// A query parameter of an address (`vA` of module 370480).
pub(super) fn query_param(url: &str, name: &str) -> Option<String> {
    oauth::location_params(url)
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// The consent's request as the OAuth2 hook gets it from module 910316 (`responseType: "code"`, no state or nonce).
pub(super) fn consent_args(kind: &str) -> Option<oauth::Args> {
    Some(oauth::Args {
        client_id: client_id(kind)?.to_owned(),
        response_type: "code".to_owned(),
        redirect_uri: redirect_uri(kind).map(str::to_owned),
        code_challenge: None,
        code_challenge_method: None,
        state: None,
        nonce: None,
        scopes: scopes(kind)
            .iter()
            .map(|scope| (*scope).to_owned())
            .collect(),
        permissions: 0,
        guild_id: None,
        channel_id: None,
        prompt: None,
        integration_type: None,
    })
}

/// `completeTwoWayLink` of module 77468 then `Q` of 306677: the callback's body, or `None` when the location carries
/// no code (an `error` in it, or none at all), which the dialog shows as its error slide.
pub(super) fn complete_body(
    kind: &str,
    location: Option<&str>,
    code: &str,
    state: &str,
) -> Option<Value> {
    let location = location?;
    if query_param(location, "error").is_some_and(|error| !error.is_empty()) {
        return None;
    }
    let link_code = query_param(location, "code")?;
    Some(json!({
        "code": code,
        "state": state,
        "two_way_link_code": link_code,
        "insecure": false,
        "friend_sync": super::connection_card::FRIEND_SYNC.contains(&kind),
    }))
}

/// The error slide's body (module 158375 `A` for PlayStation; the others have one text).
pub(super) fn error_body(kind: &str, code: Option<i64>) -> Option<&'static str> {
    (kind == "playstation").then(|| {
        if code == Some(UNDER_MINIMUM_AGE) {
            dc_intl::t("3dIn2A")
        } else {
            dc_intl::t("qE9nqE")
        }
    })
}

/// What the consent slide is waiting for or showing.
#[derive(Default)]
pub(in crate::ui) struct Consent {
    pub(super) get: Option<Value>,
    pub(super) disclosures: Option<Value>,
    /// A failed request's message (the hook's `Error` header over it).
    pub(super) failed: Option<String>,
    /// Authorize and Continue was pressed (`E` of 910316, never reset).
    pub(super) loading: bool,
    /// The granted POST's `location`, kept while the disclosures are acknowledged.
    pub(super) location: Option<String>,
    pub(super) fake: usize,
    /// The slide's content scrolls (`$m` of the legacy modal under the slide container's maximum height).
    pub(super) scroll: gpui::ScrollHandle,
}

impl Root {
    fn link_info_mut(&mut self) -> Option<&mut super::connection_dialogs::Info> {
        match self.settings_extra.conn.as_mut() {
            Some(Dlg::Info(info)) => Some(info),
            _ => None,
        }
    }

    /// The authorize request of a pre-connect or waiting slide answered (`D` of 873885): the address opens, and the
    /// pre-connect slide waits for its `state`.
    pub(super) fn connection_link_authorized(&mut self, url: Option<&str>, cx: &mut Context<Self>) {
        let Some(info) = self.link_info_mut() else {
            return;
        };
        let Some(url) = url else {
            // `missing authorizeURL`: `onError()`.
            info.go_error(None);
            cx.notify();
            return;
        };
        match (info.slide, query_param(url, "state")) {
            (Slide::Connect, Some(state)) => {
                info.expected = Some(state);
                info.slide = Slide::Waiting;
            }
            (Slide::Connect, None) => {
                tracing::warn!("connection link: the authorize address has no state");
                info.go_error(None);
            }
            // Retry on the waiting slide has no `onWaitingForConnection`: the expected state stays.
            _ => {}
        }
        cx.notify();
    }

    /// The gateway's `USER_CONNECTIONS_LINK_CALLBACK` (`R` of 873885, subscribed while a pre-connect slide shows).
    pub(in crate::ui) fn connection_link_callback(&mut self, data: &Value, cx: &mut Context<Self>) {
        let Some(info) = self.link_info_mut() else {
            return;
        };
        if !matches!(info.slide, Slide::Connect | Slide::Waiting) {
            return;
        }
        let state = data["callback_state"].as_str();
        if state.is_none() || state != info.expected.as_deref() {
            tracing::warn!("{} link: received mismatching callback state!", info.kind);
            return;
        }
        let code = data["callback_code"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        info.callback = Some((code, state.unwrap_or_default().to_owned()));
        info.expected = None;
        info.slide = Slide::Consent;
        *info.consent = Consent {
            fake: oauth::fake_index(),
            ..Consent::default()
        };
        let kind = info.kind;
        self.connection_consent_fetch(kind);
        cx.notify();
    }

    /// The hook's two GETs (`qY` of 716965 and `vG` of the disclosures), integration type GUILD_INSTALL (no install
    /// configuration is read when scopes are given).
    fn connection_consent_fetch(&mut self, kind: &'static str) {
        let Some(args) = consent_args(kind) else {
            return;
        };
        let get = oauth::authorize_get_route(&args, oauth::GUILD_INSTALL);
        let disclosures = format!("/applications/{}/disclosures", args.client_id);
        if !oauth::grants_allowed() {
            tracing::info!(
                "connection {kind}: consent GET {get} and {disclosures} not sent in a harness run"
            );
            self.settings_extra.last_url = Some(get);
            return;
        }
        for (step, route) in [("get", get), ("disc", disclosures)] {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{LINK_KEY}{step}:{kind}")),
                method: reqwest::Method::GET,
                route,
                body: None,
            });
        }
    }

    /// Authorize and Continue (`D` of 910316: loading, then `sendAuthorize(true)`).
    pub(super) fn connection_consent_authorize(&mut self, cx: &mut Context<Self>) {
        let location = self.oauth_location();
        let Some(info) = self.link_info_mut() else {
            return;
        };
        // Without the hook's data there is no `sendAuthorize` to call.
        if info.slide != Slide::Consent
            || info.consent.get.is_none()
            || info.consent.failed.is_some()
            || info.consent.loading
        {
            return;
        }
        info.consent.loading = true;
        let kind = info.kind;
        cx.notify();
        let Some(args) = consent_args(kind) else {
            return;
        };
        let requested = oauth::requested_scopes(&args.scopes);
        let route = oauth::authorize_post_route(&args, &requested);
        let body = oauth::post_body(true, oauth::GUILD_INSTALL, 0, None, &location);
        if !oauth::grants_allowed() {
            tracing::info!(
                "connection {kind}: authorize POST {route} not sent from a capture build"
            );
            self.settings_extra.last_url = Some(format!("POST {route}"));
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{LINK_KEY}post:{kind}")),
            method: reqwest::Method::POST,
            route,
            body: Some(body),
        });
    }

    /// An answer of the consent slide's requests; `rest` is `<step>:<type>`.
    pub(super) fn connection_link_answered(
        &mut self,
        rest: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some((step, kind)) = rest.split_once(':') else {
            return;
        };
        let Some(info) = self.link_info_mut() else {
            return;
        };
        if info.kind != kind || info.slide != Slide::Consent {
            return;
        }
        let kind = info.kind;
        let ok = body["ok"].as_bool() == Some(true);
        let failure = || oauth::failure_message(body).unwrap_or_else(|| "Unknown Error".to_owned());
        // The request to send next, if any: (step, method, route, body).
        let mut next: Option<(&str, reqwest::Method, String, Value)> = None;
        match step {
            "get" | "disc" if !ok => info.consent.failed = Some(failure()),
            "get" => info.consent.get = Some(body["body"].clone()),
            "disc" => info.consent.disclosures = Some(body["body"].clone()),
            "post" if info.consent.loading => {
                if !ok {
                    // `tr`'s catch: the message takes the slide's content; the button keeps loading.
                    info.consent.failed = Some(oauth::failure_message(body).unwrap_or_else(|| {
                        oauth::plain(crate::ui::oauth_spec::ERROR_GENERIC, &[])
                    }));
                } else {
                    info.consent.location = body["body"]["location"].as_str().map(str::to_owned);
                    // `Yx`: the disclosures are acknowledged after a grant, then the callback runs.
                    let ids = oauth::disclosure_ids(info.consent.disclosures.as_ref());
                    if let Some(client) = client_id(kind) {
                        next = Some((
                            "ack",
                            reqwest::Method::POST,
                            format!("/applications/{client}/disclosures"),
                            json!({ "disclosures": ids }),
                        ));
                    }
                }
            }
            "ack" if !ok => info.consent.failed = Some(failure()),
            "ack" => {
                let (code, state) = info.callback.clone().unwrap_or_default();
                match complete_body(kind, info.consent.location.as_deref(), &code, &state) {
                    Some(payload) => {
                        next = Some((
                            "done",
                            reqwest::Method::POST,
                            format!("/connections/{kind}/callback"),
                            payload,
                        ))
                    }
                    None => {
                        tracing::error!("Two-way link: missing authorize code");
                        info.go_error(None);
                    }
                }
            }
            // `onContinue`: DISCORD_CONSENT goes on to SUCCESS.
            "done" if ok => info.slide = Slide::Success,
            "done" => {
                tracing::error!("{kind} link error: status {}", body["status"]);
                info.go_error(body["body"]["code"].as_i64());
            }
            _ => {}
        }
        if step == "get" && ok {
            let application = &body["body"]["application"];
            if let (Some(id), Some(hash)) =
                (application["id"].as_str(), application["icon"].as_str())
            {
                self.request_image(
                    format!("shelf-icon:{id}"),
                    format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=128"),
                );
            }
        }
        if let Some((step, method, route, payload)) = next {
            if oauth::grants_allowed() {
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{LINK_KEY}{step}:{kind}")),
                    method,
                    route,
                    body: Some(payload),
                });
            } else {
                tracing::info!(
                    "connection {kind}: {step} POST {route} not sent from a capture build"
                );
                self.settings_extra.last_url = Some(format!("POST {route}"));
            }
        }
        cx.notify();
    }
    /// Module 272047 / 359800 (`in-game-auth-check-modal`): opens the Social SDK game-initiated account link modal.
    #[allow(dead_code)]
    pub(crate) fn open_in_game_auth_check_modal(&mut self, cx: &mut Context<Self>) {
        self.in_game_auth_check_modal = true;
        cx.notify();
    }

    /// Module 359800: auto-dismisses the in-game auth check modal when link completes & shows `uG6teD` success toast.
    #[allow(dead_code)]
    pub(crate) fn complete_in_game_auth_check(&mut self, cx: &mut Context<Self>) {
        if self.in_game_auth_check_modal {
            self.in_game_auth_check_modal = false;
            self.show_success_toast(dc_intl::t("uG6teD").to_owned(), cx);
            cx.notify();
        }
    }

    /// Module 272047: minimal `size="sm"` (400px) Social SDK account link waiting modal (`ULvRFd` / `HJJDr+`, no footer).
    pub(in crate::ui) fn in_game_auth_check_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use crate::ui::{color, icon};
        use gpui::{FontWeight, IntoElement, MouseButton, deferred, div, prelude::*, px, rgba};
        if !self.in_game_auth_check_modal {
            return None;
        }
        Some(
            deferred(
                div()
                    .id("in-game-auth-check-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.in_game_auth_check_modal = false;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("in-game-auth-check-modal")
                            .occlude()
                            .w(px(400.))
                            .p(px(24.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("ULvRFd")),
                                    )
                                    .child(
                                        div()
                                            .id("in-game-auth-check-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                16.,
                                                "--interactive-text-default",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.in_game_auth_check_modal = false;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-subtle"))
                                    .child(dc_intl::t("HJJDr+")),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_in_game_auth_check(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        match arg.trim() {
            "close" => {
                self.in_game_auth_check_modal = false;
                cx.notify();
                "in-game-auth-check: closed".to_owned()
            }
            "complete" => {
                self.complete_in_game_auth_check(cx);
                "in-game-auth-check: completed".to_owned()
            }
            _ => {
                self.open_in_game_auth_check_modal(cx);
                "in-game-auth-check: opened".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pre_connect_slide_asks_for_a_desktop_two_way_link() {
        assert_eq!(
            authorize_route("xbox"),
            "/connections/xbox/authorize?two_way_link_type=desktop&two_way_link=true"
        );
        assert_eq!(
            query_param(
                "https://login.live.com/oauth20_authorize.srf?client_id=1&state=ab%2Bc",
                "state"
            )
            .as_deref(),
            Some("ab+c")
        );
        assert_eq!(query_param("https://example.com/", "state"), None);
    }

    #[test]
    fn the_consent_requests_the_platform_application_and_scopes() {
        let xbox = consent_args("xbox").unwrap();
        assert_eq!(
            oauth::authorize_get_route(&xbox, oauth::GUILD_INSTALL),
            "/oauth2/authorize?client_id=622174530214821906&response_type=code&scope=voice%20dm_channels.read%20guilds%20guilds.members.read%20identify%20connections%20activities.read&integration_type=0"
        );
        let playstation = consent_args("playstation").unwrap();
        let requested = oauth::requested_scopes(&playstation.scopes);
        assert_eq!(
            oauth::authorize_post_route(&playstation, &requested),
            "/oauth2/authorize?client_id=1008890872156405890&response_type=code&redirect_uri=https%3A%2F%2Fdiscord.com%2Fconnections%2Fplaystation%2Flink&scope=voice%20dm_channels.read%20guilds%20guilds.members.read%20identify%20connections"
        );
        let crunchyroll = consent_args("crunchyroll").unwrap();
        assert_eq!(crunchyroll.client_id, "981509069309354054");
        assert_eq!(
            crunchyroll.scopes,
            ["activities.write", "email", "identify"]
        );
        assert!(consent_args("github").is_none());
        // Every scope is one the hook lists (no "Invalid scope").
        for kind in ["xbox", "playstation", "crunchyroll"] {
            let args = consent_args(kind).unwrap();
            assert_eq!(
                oauth::scope_error(&oauth::requested_scopes(&args.scopes)),
                None
            );
        }
    }

    #[test]
    fn the_callback_carries_both_codes() {
        let body = complete_body(
            "xbox",
            Some("https://discord.com/connections/xbox/link?code=tw1&state=x"),
            "cb",
            "st",
        )
        .unwrap();
        assert_eq!(
            body,
            json!({"code": "cb", "state": "st", "two_way_link_code": "tw1", "insecure": false, "friend_sync": false})
        );
        assert_eq!(complete_body("xbox", None, "cb", "st"), None);
        assert_eq!(
            complete_body(
                "xbox",
                Some("https://discord.com/oauth2/error?error=access_denied&code=1"),
                "cb",
                "st"
            ),
            None
        );
    }

    #[test]
    fn playstation_names_the_parent_account_error() {
        assert_eq!(
            error_body("playstation", Some(UNDER_MINIMUM_AGE)),
            Some(
                "This PlayStation Network account requires communication features to be enabled by a parent account."
            )
        );
        assert_eq!(
            error_body("playstation", None),
            Some(
                "We were unable to link your account for PlayStation Network. Please try again or contact support if the issue persists."
            )
        );
        assert_eq!(error_body("xbox", Some(UNDER_MINIMUM_AGE)), None);
    }

    #[test]
    fn in_game_auth_check_272047_intl() {
        for key in ["ULvRFd", "HJJDr+", "uG6teD"] {
            assert!(dc_intl::has_key(key), "missing 272047 key {key}");
        }
    }
}
