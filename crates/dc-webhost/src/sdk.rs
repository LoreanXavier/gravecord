//! The host side of the Embedded App SDK's wire protocol (module 596339 `bG`/`bB`, 223769, 440454, 818348), without
//! a webview: messages come in as the relay page reports them (the sender's origin vouched for by the browser) and
//! actions come out for the caller to post, close, or answer later (a command that needs the network or the user).
//!
//! Every message is `[opcode, payload]` with HANDSHAKE 0, FRAME 1, CLOSE 2, HELLO 3. Replies go to the origin seen at
//! the handshake, never `"*"` (the caller passes [`Bridge::origin`] to the relay).

use serde_json::{Value, json};
use std::collections::BTreeSet;

const HANDSHAKE: u64 = 0;
const FRAME: u64 = 1;
const CLOSE: u64 = 2;
const HELLO: u64 = 3;

/// `Lw6`, the error codes of a reply (818348 `em`).
pub mod code {
    pub const INVALID_PAYLOAD: u32 = 4000;
    pub const INVALID_COMMAND: u32 = 4002;
    pub const INVALID_EVENT: u32 = 4004;
    pub const INVALID_PERMISSIONS: u32 = 4006;
    pub const INVALID_CLIENTID: u32 = 4007;
    pub const INVALID_TOKEN: u32 = 4009;
    pub const OAUTH2_ERROR: u32 = 5000;
    pub const UNKNOWN_ERROR: u32 = 1000;
    /// The close codes (`eT`).
    pub const CLOSE_NORMAL: u32 = 1000;
    pub const CLOSE_UNSUPPORTED: u32 = 1003;
    pub const INVALID_ORIGIN: u32 = 4001;
}

/// What the bridge needs to know about the activity and the account; the caller refreshes it as the world changes.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub application_id: String,
    /// `https://{app}.discordsays.com`, the only origin that may talk.
    pub origin: String,
    pub frame_id: String,
    /// The `user` of READY (639621): id, username, discriminator, global_name, avatar, flags, ...
    pub user: Value,
    pub channel_id: Option<String>,
    pub guild_id: Option<String>,
    /// The users in the instance, as objects.
    pub participants: Vec<Value>,
    pub locale: String,
    pub cdn_host: String,
    pub api_endpoint: String,
    pub custom_id: Option<String>,
    pub referrer_id: Option<String>,
}

/// A command that cannot be answered from local state.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// `AUTHORIZE`: the user decides (the consent UI), then the code comes back.
    Authorize { nonce: String, args: Value },
    /// `AUTHENTICATE`: the app's own access token, checked at `GET /oauth2/@me` with that token as Bearer.
    Authenticate { nonce: String, token: String },
    /// `OPEN_EXTERNAL_LINK`.
    OpenExternalLink { nonce: String, url: String },
    /// `REQUEST_PROXY_TICKET_REFRESH`.
    ProxyTicket { nonce: String },
    /// `ENCOURAGE_HW_ACCELERATION`: the host may ask the user to turn hardware acceleration on, then answers
    /// `{enabled}` with the setting as it is.
    EncourageHardwareAcceleration { nonce: String },
    /// `SHARE_LINK` (module 529999 / 342384): open the Activity share modal with a generated activity link.
    ShareLink {
        nonce: String,
        message: String,
        custom_id: Option<String>,
        link_id: Option<String>,
    },
    /// `SHARE_CONTENT` (module 529999): open the Activity share modal with custom title/subtitle/media.
    ShareContent { nonce: String, args: Value },
    /// `OPEN_SHARE_MOMENT_DIALOG` (module 190930): open the Activity Share Moment modal for a Discord CDN media URL.
    ShareMoment { nonce: String, media_url: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// A message for the page (to [`Bridge::origin`]).
    Post(Value),
    /// A CLOSE the page should get, after which the connection is over.
    Close { code: u32, message: String },
    /// Needs an answer later through [`Bridge::answer`].
    Ask(Ask),
    /// The page asked for something this host does not do; logged by the caller at debug.
    Unhandled(String),
    /// The page closed the connection (the activity is left unless the code is normal, then an error shows).
    Disconnected { code: u32 },
}

#[derive(Debug, Default)]
pub struct Bridge {
    origin: Option<String>,
    authenticated: bool,
    /// An AUTHORIZE waits for the user (`authorization.authing`).
    authing: bool,
    /// The page sits in the picture-in-picture window.
    pip: bool,
    scopes: BTreeSet<String>,
    subscriptions: BTreeSet<String>,
}

fn error(cmd: &str, nonce: &str, code: u32, message: &str) -> Value {
    json!([FRAME, {"cmd": cmd, "data": {"code": code, "message": message}, "evt": "ERROR", "nonce": nonce}])
}

fn reply(cmd: &str, nonce: &str, data: Value) -> Value {
    json!([FRAME, {"cmd": cmd, "data": data, "evt": null, "nonce": nonce}])
}

fn dispatch(evt: &str, data: Value) -> Value {
    json!([FRAME, {"cmd": "DISPATCH", "data": data, "evt": evt, "nonce": null}])
}

/// The events a page may subscribe to from here: (name, needs authentication).
const EVENTS: &[(&str, bool)] = &[
    ("ACTIVITY_LAYOUT_MODE_UPDATE", false),
    ("ACTIVITY_PIP_MODE_UPDATE", false),
    ("ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE", true),
    ("THERMAL_STATE_UPDATE", true),
];

impl Bridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// The origin the page has to be addressed at, once it has shaken hands.
    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }

    pub fn authenticated(&self) -> bool {
        self.authenticated
    }

    /// What the host posts when the frame has loaded (358415 `em`): the query parameters and the frame id.
    pub fn hello(ctx: &Context, query: &Value) -> Value {
        let mut payload = query.clone();
        payload["frame_id"] = json!(ctx.frame_id);
        payload["platform"] = json!("desktop");
        payload["discord_shim"] = json!("1");
        json!([HELLO, payload])
    }

    /// One message of the page. `origin` is the sender's origin as the browser reports it.
    pub fn handle(&mut self, origin: &str, data: &Value, ctx: &Context) -> Vec<Action> {
        let Some(list) = data.as_array() else {
            return vec![Action::Close {
                code: code::CLOSE_UNSUPPORTED,
                message: "Payload not recognized encoding".to_owned(),
            }];
        };
        let (Some(op), Some(payload)) = (list.first().and_then(Value::as_u64), list.get(1)) else {
            return vec![Action::Close {
                code: code::CLOSE_UNSUPPORTED,
                message: "Payload not recognized encoding".to_owned(),
            }];
        };
        match op {
            HANDSHAKE => self.handshake(origin, payload, ctx),
            FRAME => self.frame(origin, payload, ctx),
            CLOSE => {
                self.origin = None;
                let code = payload["code"].as_u64().unwrap_or(1000) as u32;
                vec![
                    Action::Close {
                        code,
                        message: payload["message"].as_str().unwrap_or_default().to_owned(),
                    },
                    Action::Disconnected { code },
                ]
            }
            _ => vec![Action::Close {
                code: code::CLOSE_UNSUPPORTED,
                message: "Payload not recognized encoding".to_owned(),
            }],
        }
    }

    fn handshake(&mut self, origin: &str, payload: &Value, ctx: &Context) -> Vec<Action> {
        let close = |message: &str| {
            vec![Action::Close {
                code: code::CLOSE_UNSUPPORTED,
                message: message.to_owned(),
            }]
        };
        if self.origin.is_some() {
            return close("Already connected");
        }
        // `v` 1 only, `encoding` json only, `client_id` and `frame_id` strings.
        if payload["v"].as_u64() != Some(1) {
            return vec![Action::Close {
                code: 4004,
                message: "Invalid version".to_owned(),
            }];
        }
        if payload["encoding"].as_str().is_some_and(|e| e != "json") {
            return vec![Action::Close {
                code: 4005,
                message: "Erlpack cannot be used on this client".to_owned(),
            }];
        }
        let (Some(client_id), Some(frame_id)) =
            (payload["client_id"].as_str(), payload["frame_id"].as_str())
        else {
            return close("Invalid handshake");
        };
        if frame_id != ctx.frame_id {
            return close("Unrecognized iframe ID");
        }
        if client_id.is_empty() {
            return vec![Action::Close {
                code: 4000,
                message: "No Client ID Specified".to_owned(),
            }];
        }
        // The page must come from the activity's own origin and name its own application.
        if origin != ctx.origin || client_id != ctx.application_id {
            return vec![Action::Close {
                code: code::INVALID_ORIGIN,
                message: "Invalid Origin".to_owned(),
            }];
        }
        self.origin = Some(origin.to_owned());
        vec![Action::Post(json!([FRAME, {
            "cmd": "DISPATCH",
            "evt": "READY",
            "nonce": null,
            "data": {
                "v": 1,
                "config": {
                    "cdn_host": ctx.cdn_host,
                    "api_endpoint": ctx.api_endpoint,
                    "environment": "production",
                },
                "user": ctx.user,
            }
        }]))]
    }

    fn frame(&mut self, origin: &str, payload: &Value, ctx: &Context) -> Vec<Action> {
        let Some(known) = self.origin.clone() else {
            return vec![Action::Close {
                code: code::CLOSE_UNSUPPORTED,
                message: "Not connected".to_owned(),
            }];
        };
        if origin != known {
            self.origin = None;
            return vec![Action::Close {
                code: code::INVALID_ORIGIN,
                message: "Origin has changed".to_owned(),
            }];
        }
        // A frame may also arrive as a JSON string.
        let parsed;
        let payload = if let Some(text) = payload.as_str() {
            match serde_json::from_str::<Value>(text) {
                Ok(value) => {
                    parsed = value;
                    &parsed
                }
                Err(_) => {
                    return vec![Action::Close {
                        code: code::CLOSE_UNSUPPORTED,
                        message: "Payload not recognized encoding".to_owned(),
                    }];
                }
            }
        } else {
            payload
        };
        let cmd = payload["cmd"].as_str().unwrap_or_default();
        let nonce = payload["nonce"].as_str().unwrap_or_default();
        if nonce.is_empty() {
            return vec![Action::Post(error(
                cmd,
                nonce,
                code::INVALID_PAYLOAD,
                "Payload requires a nonce",
            ))];
        }
        let args = &payload["args"];
        let post = |value: Value| vec![Action::Post(value)];
        let need_auth = |this: &Self| -> Option<Vec<Action>> {
            (!this.authenticated).then(|| {
                vec![Action::Post(error(
                    cmd,
                    nonce,
                    code::INVALID_PERMISSIONS,
                    "Not authenticated or invalid scope",
                ))]
            })
        };
        match cmd {
            "AUTHORIZE" => {
                // Module 596339 `[AUTHORIZE]` handler, in its order.
                let client = args["client_id"].as_str().unwrap_or_default();
                if client.is_empty() {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_CLIENTID,
                        "No client id provided",
                    ));
                }
                if self.authenticated {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_COMMAND,
                        "Already authenticated",
                    ));
                }
                if self.authing {
                    return post(error(cmd, nonce, code::INVALID_COMMAND, "Already authing"));
                }
                if args["response_type"].as_str() == Some("token") {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_COMMAND,
                        "Authorization response_type \"token\" is not supported",
                    ));
                }
                if client != ctx.application_id {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_CLIENTID,
                        "Application does not match the connection's",
                    ));
                }
                self.authing = true;
                vec![Action::Ask(Ask::Authorize {
                    nonce: nonce.to_owned(),
                    args: args.clone(),
                })]
            }
            "AUTHENTICATE" => {
                let Some(token) = args["access_token"].as_str().filter(|t| !t.is_empty()) else {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_TOKEN,
                        "No access token provided",
                    ));
                };
                if self.authenticated {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_TOKEN,
                        "Already authenticated",
                    ));
                }
                vec![Action::Ask(Ask::Authenticate {
                    nonce: nonce.to_owned(),
                    token: token.to_owned(),
                })]
            }
            "GET_CONTEXT" => post(reply(
                cmd,
                nonce,
                json!({
                    "surface": {"type": 0, "channel_id": ctx.channel_id, "guild_id": ctx.guild_id},
                    "launch": {
                        "custom_id": ctx.custom_id,
                        "referrer_id": ctx.referrer_id,
                        "interaction_id": null,
                    },
                    "platform": "desktop",
                }),
            )),
            "GET_CURRENT_USER" => post(reply(cmd, nonce, ctx.user.clone())),
            "GET_USER" => {
                let id = args["id"].as_str().unwrap_or_default();
                let found = std::iter::once(&ctx.user)
                    .chain(ctx.participants.iter())
                    .find(|user| user["id"].as_str() == Some(id))
                    .cloned()
                    .unwrap_or(Value::Null);
                post(reply(cmd, nonce, found))
            }
            "GET_PLATFORM_BEHAVIORS" => {
                post(reply(cmd, nonce, json!({"iosKeyboardResizesView": true})))
            }
            "USER_SETTINGS_GET_LOCALE" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                post(reply(cmd, nonce, json!({"locale": ctx.locale})))
            }
            "ENCOURAGE_HW_ACCELERATION" => vec![Action::Ask(Ask::EncourageHardwareAcceleration {
                nonce: nonce.to_owned(),
            })],
            "CAPTURE_LOG" | "SEND_ANALYTICS_EVENT" => post(reply(cmd, nonce, Value::Null)),
            "SET_ORIENTATION_LOCK_STATE" => post(reply(cmd, nonce, Value::Null)),
            "SET_CONFIG" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                post(reply(
                    cmd,
                    nonce,
                    json!({"use_interactive_pip": args["use_interactive_pip"].as_bool().unwrap_or(false)}),
                ))
            }
            "GET_ACTIVITY_INSTANCE_CONNECTED_PARTICIPANTS" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                post(reply(cmd, nonce, json!({"participants": ctx.participants})))
            }
            // The activity's presence is accepted and answered as the official client answers; this client does not
            // publish it (BLOCKED.md).
            "SET_ACTIVITY" => post(reply(cmd, nonce, args["activity"].clone())),
            "REQUEST_PROXY_TICKET_REFRESH" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                vec![Action::Ask(Ask::ProxyTicket {
                    nonce: nonce.to_owned(),
                })]
            }
            "OPEN_EXTERNAL_LINK" => {
                let url = args["url"].as_str().unwrap_or_default();
                if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 8000
                {
                    return post(reply(cmd, nonce, json!({"opened": false})));
                }
                vec![Action::Ask(Ask::OpenExternalLink {
                    nonce: nonce.to_owned(),
                    url: url.to_owned(),
                })]
            }
            "SHARE_LINK" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                vec![Action::Ask(Ask::ShareLink {
                    nonce: nonce.to_owned(),
                    message: args["message"].as_str().unwrap_or_default().to_owned(),
                    custom_id: args["custom_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    link_id: args["link_id"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                })]
            }
            "SHARE_CONTENT" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                vec![Action::Ask(Ask::ShareContent {
                    nonce: nonce.to_owned(),
                    args: args.clone(),
                })]
            }
            "OPEN_SHARE_MOMENT_DIALOG" => {
                if let Some(denied) = need_auth(self) {
                    return denied;
                }
                let media_url = args["mediaUrl"]
                    .as_str()
                    .or_else(|| args["media_url"].as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                let is_cdn = media_url.starts_with("https://cdn.discordapp.com/")
                    || media_url.starts_with("https://media.discordapp.net/");
                if !is_cdn {
                    return vec![Action::Post(error(
                        cmd,
                        nonce,
                        code::INVALID_PAYLOAD,
                        "mediaUrl must be a Discord CDN URL",
                    ))];
                }
                vec![Action::Ask(Ask::ShareMoment {
                    nonce: nonce.to_owned(),
                    media_url,
                })]
            }
            "SUBSCRIBE" | "UNSUBSCRIBE" => {
                let evt = payload["evt"].as_str().unwrap_or_default();
                let Some(&(_, authed)) = EVENTS.iter().find(|(name, _)| *name == evt) else {
                    return vec![
                        Action::Post(error(
                            cmd,
                            nonce,
                            code::INVALID_EVENT,
                            &format!("Invalid event: {evt}"),
                        )),
                        Action::Unhandled(format!("{cmd} {evt}")),
                    ];
                };
                if authed && !self.authenticated {
                    return post(error(
                        cmd,
                        nonce,
                        code::INVALID_PERMISSIONS,
                        "Not authenticated or invalid scope",
                    ));
                }
                let mut out = vec![Action::Post(reply(cmd, nonce, json!({"evt": evt})))];
                if cmd == "SUBSCRIBE" {
                    self.subscriptions.insert(evt.to_owned());
                    // The state is pushed at once for these (596339 `yd.SUBSCRIBE`).
                    match evt {
                        "ACTIVITY_LAYOUT_MODE_UPDATE" => {
                            out.push(Action::Post(dispatch(evt, json!({"layout_mode": 0}))))
                        }
                        "ACTIVITY_PIP_MODE_UPDATE" => out.push(Action::Post(dispatch(
                            evt,
                            json!({"is_pip_mode": self.pip}),
                        ))),
                        "ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE" => out.push(Action::Post(
                            dispatch(evt, json!({"participants": ctx.participants})),
                        )),
                        _ => {}
                    }
                } else {
                    self.subscriptions.remove(evt);
                }
                out
            }
            other => vec![
                Action::Post(error(
                    other,
                    nonce,
                    code::INVALID_COMMAND,
                    &format!("Invalid command: {other}"),
                )),
                Action::Unhandled(other.to_owned()),
            ],
        }
    }

    /// The answer to an [`Ask`]. `Ok` carries the command's `data`; `Err` the error code and message.
    pub fn answer(
        &mut self,
        cmd: &str,
        nonce: &str,
        result: Result<Value, (u32, String)>,
    ) -> Action {
        if cmd == "AUTHORIZE" {
            self.authing = false;
        }
        match result {
            Ok(data) => {
                if cmd == "AUTHENTICATE" {
                    self.authenticated = true;
                    for scope in data["scopes"].as_array().into_iter().flatten() {
                        if let Some(scope) = scope.as_str() {
                            self.scopes.insert(scope.to_owned());
                        }
                    }
                }
                Action::Post(reply(cmd, nonce, data))
            }
            Err((code, message)) => Action::Post(error(cmd, nonce, code, &message)),
        }
    }

    /// The page moved into or out of the picture-in-picture window: subscribers are told.
    pub fn set_pip(&mut self, pip: bool) -> Vec<Action> {
        if self.pip == pip {
            return Vec::new();
        }
        self.pip = pip;
        if self.subscriptions.contains("ACTIVITY_PIP_MODE_UPDATE") {
            vec![Action::Post(dispatch(
                "ACTIVITY_PIP_MODE_UPDATE",
                json!({"is_pip_mode": pip}),
            ))]
        } else {
            Vec::new()
        }
    }

    /// Pushes the participant list to a page that subscribed to it.
    pub fn participants_changed(&self, ctx: &Context) -> Vec<Action> {
        if self
            .subscriptions
            .contains("ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE")
        {
            vec![Action::Post(dispatch(
                "ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE",
                json!({"participants": ctx.participants}),
            ))]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context {
            application_id: "880".into(),
            origin: "https://880.discordsays.com".into(),
            frame_id: "frame-1".into(),
            user: json!({"id": "9", "username": "me"}),
            channel_id: Some("55".into()),
            guild_id: Some("44".into()),
            participants: vec![json!({"id": "9", "username": "me"})],
            locale: "en-US".into(),
            cdn_host: "cdn.discordapp.com".into(),
            api_endpoint: "//discord.com/api".into(),
            ..Default::default()
        }
    }

    const ORIGIN: &str = "https://880.discordsays.com";

    fn shaken() -> (Bridge, Context) {
        let mut bridge = Bridge::new();
        let ctx = ctx();
        let out = bridge.handle(
            ORIGIN,
            &json!([0, {"v": 1, "encoding": "json", "client_id": "880", "frame_id": "frame-1"}]),
            &ctx,
        );
        assert_eq!(out.len(), 1);
        (bridge, ctx)
    }

    fn command(bridge: &mut Bridge, ctx: &Context, cmd: &str, args: Value) -> Vec<Action> {
        bridge.handle(
            ORIGIN,
            &json!([1, {"cmd": cmd, "args": args, "nonce": "n1"}]),
            ctx,
        )
    }

    #[test]
    fn the_handshake_answers_ready_with_the_user_and_the_config() {
        let mut bridge = Bridge::new();
        let out = bridge.handle(
            ORIGIN,
            &json!([0, {"v": 1, "encoding": "json", "client_id": "880", "frame_id": "frame-1"}]),
            &ctx(),
        );
        let Action::Post(message) = &out[0] else {
            panic!("expected READY");
        };
        assert_eq!(message[0], 1);
        assert_eq!(message[1]["evt"], "READY");
        assert_eq!(message[1]["data"]["v"], 1);
        assert_eq!(message[1]["data"]["user"]["id"], "9");
        assert_eq!(message[1]["data"]["config"]["environment"], "production");
        assert_eq!(bridge.origin(), Some(ORIGIN));
    }

    #[test]
    fn a_handshake_from_the_wrong_place_is_closed() {
        let good =
            json!([0, {"v": 1, "encoding": "json", "client_id": "880", "frame_id": "frame-1"}]);
        let closed = |out: Vec<Action>, expected: u32| {
            assert_eq!(out.len(), 1);
            let Action::Close { code, .. } = &out[0] else {
                panic!("expected a close, got {out:?}");
            };
            assert_eq!(*code, expected);
        };
        // The origin must be the activity's own.
        closed(
            Bridge::new().handle("https://evil.example", &good, &ctx()),
            4001,
        );
        // The frame id must be the one the host registered.
        let wrong_frame = json!([0, {"v": 1, "client_id": "880", "frame_id": "other"}]);
        closed(Bridge::new().handle(ORIGIN, &wrong_frame, &ctx()), 1003);
        // The page may not name another application.
        let wrong_app = json!([0, {"v": 1, "client_id": "1", "frame_id": "frame-1"}]);
        closed(Bridge::new().handle(ORIGIN, &wrong_app, &ctx()), 4001);
        // Erlpack is refused, other versions too.
        let etf =
            json!([0, {"v": 1, "encoding": "etf", "client_id": "880", "frame_id": "frame-1"}]);
        closed(Bridge::new().handle(ORIGIN, &etf, &ctx()), 4005);
        let v2 = json!([0, {"v": 2, "client_id": "880", "frame_id": "frame-1"}]);
        closed(Bridge::new().handle(ORIGIN, &v2, &ctx()), 4004);
        // A second handshake is refused.
        let (mut bridge, ctx) = shaken();
        closed(bridge.handle(ORIGIN, &good, &ctx), 1003);
        // Anything that is not an array is not a message.
        closed(Bridge::new().handle(ORIGIN, &json!({"a": 1}), &ctx), 1003);
    }

    #[test]
    fn frames_need_a_nonce_and_a_known_command() {
        let (mut bridge, ctx) = shaken();
        let out = bridge.handle(
            ORIGIN,
            &json!([1, {"cmd": "GET_CONTEXT", "args": {}}]),
            &ctx,
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4000);
        let out = command(&mut bridge, &ctx, "SELECT_VOICE_CHANNEL", json!({}));
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4002);
        assert_eq!(message[1]["evt"], "ERROR");
        assert_eq!(out[1], Action::Unhandled("SELECT_VOICE_CHANNEL".into()));
    }

    #[test]
    fn a_frame_from_another_origin_closes_the_connection() {
        let (mut bridge, ctx) = shaken();
        let out = bridge.handle(
            "https://other.example",
            &json!([1, {"cmd": "GET_CONTEXT", "nonce": "n"}]),
            &ctx,
        );
        assert!(matches!(out[0], Action::Close { code: 4001, .. }));
        assert_eq!(bridge.origin(), None);
    }

    #[test]
    fn local_state_commands_answer_from_the_context() {
        let (mut bridge, ctx) = shaken();
        let Action::Post(message) = &command(&mut bridge, &ctx, "GET_CONTEXT", json!({}))[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["surface"]["channel_id"], "55");
        assert_eq!(message[1]["data"]["surface"]["type"], 0);
        assert_eq!(message[1]["data"]["platform"], "desktop");
        assert_eq!(message[1]["nonce"], "n1");
        let Action::Post(message) = &command(&mut bridge, &ctx, "GET_USER", json!({"id": "9"}))[0]
        else {
            panic!()
        };
        assert_eq!(message[1]["data"]["username"], "me");
        let Action::Post(message) = &command(&mut bridge, &ctx, "GET_USER", json!({"id": "77"}))[0]
        else {
            panic!()
        };
        assert!(message[1]["data"].is_null());
        let Action::Post(message) =
            &command(&mut bridge, &ctx, "GET_PLATFORM_BEHAVIORS", json!({}))[0]
        else {
            panic!()
        };
        assert_eq!(message[1]["data"]["iosKeyboardResizesView"], true);
    }

    #[test]
    fn authenticated_commands_refuse_until_authenticate_succeeds() {
        let (mut bridge, ctx) = shaken();
        let out = command(
            &mut bridge,
            &ctx,
            "GET_ACTIVITY_INSTANCE_CONNECTED_PARTICIPANTS",
            json!({}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4006);
        // AUTHENTICATE asks the host, which checks the token; only a success unlocks the rest.
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHENTICATE",
            json!({"access_token": "tok"}),
        );
        assert_eq!(
            out[0],
            Action::Ask(Ask::Authenticate {
                nonce: "n1".into(),
                token: "tok".into()
            })
        );
        let done = bridge.answer(
            "AUTHENTICATE",
            "n1",
            Ok(json!({"scopes": ["identify"], "user": {"id": "9"}})),
        );
        assert!(matches!(done, Action::Post(_)));
        assert!(bridge.authenticated());
        let out = command(
            &mut bridge,
            &ctx,
            "GET_ACTIVITY_INSTANCE_CONNECTED_PARTICIPANTS",
            json!({}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["participants"][0]["id"], "9");
    }

    #[test]
    fn authenticate_without_a_token_is_invalid_token() {
        let (mut bridge, ctx) = shaken();
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHENTICATE",
            json!({"access_token": null}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4009);
    }

    #[test]
    fn authorize_must_name_this_application_and_goes_to_the_user() {
        let (mut bridge, ctx) = shaken();
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHORIZE",
            json!({"client_id": "1", "scope": ["identify"]}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4007);
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHORIZE",
            json!({"client_id": "880", "scope": ["identify"], "response_type": "token"}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4002);
        assert_eq!(
            message[1]["data"]["message"],
            "Authorization response_type \"token\" is not supported"
        );
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHORIZE",
            json!({"client_id": "880", "scope": ["identify"], "response_type": "code"}),
        );
        assert!(matches!(&out[0], Action::Ask(Ask::Authorize { nonce, .. }) if nonce == "n1"));
        // Until the user has answered, a second AUTHORIZE is refused; the answer frees it.
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHORIZE",
            json!({"client_id": "880", "scope": ["identify"], "response_type": "code"}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["message"], "Already authing");
        bridge.answer(
            "AUTHORIZE",
            "n1",
            Err((
                code::OAUTH2_ERROR,
                "User cancelled authorization".to_owned(),
            )),
        );
        let out = command(
            &mut bridge,
            &ctx,
            "AUTHORIZE",
            json!({"client_id": "880", "scope": ["identify"], "response_type": "code"}),
        );
        assert!(matches!(&out[0], Action::Ask(Ask::Authorize { .. })));
    }

    #[test]
    fn the_pip_state_is_pushed_to_subscribers_when_it_changes() {
        let (mut bridge, ctx) = shaken();
        assert!(bridge.set_pip(true).is_empty());
        let out = command_evt(&mut bridge, &ctx, "SUBSCRIBE", "ACTIVITY_PIP_MODE_UPDATE");
        let Action::Post(pushed) = &out[1] else {
            panic!()
        };
        assert_eq!(pushed[1]["data"]["is_pip_mode"], true);
        assert!(bridge.set_pip(true).is_empty());
        let out = bridge.set_pip(false);
        let Action::Post(pushed) = &out[0] else {
            panic!()
        };
        assert_eq!(pushed[1]["data"]["is_pip_mode"], false);
    }

    #[test]
    fn subscribe_pushes_the_state_and_unknown_events_are_invalid() {
        let (mut bridge, ctx) = shaken();
        let out = command_evt(
            &mut bridge,
            &ctx,
            "SUBSCRIBE",
            "ACTIVITY_LAYOUT_MODE_UPDATE",
        );
        assert_eq!(out.len(), 2);
        let Action::Post(pushed) = &out[1] else {
            panic!()
        };
        assert_eq!(pushed[1]["evt"], "ACTIVITY_LAYOUT_MODE_UPDATE");
        assert_eq!(pushed[1]["data"]["layout_mode"], 0);
        let out = command_evt(&mut bridge, &ctx, "SUBSCRIBE", "MESSAGE_CREATE");
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4004);
        // The participants event needs authentication.
        let out = command_evt(
            &mut bridge,
            &ctx,
            "SUBSCRIBE",
            "ACTIVITY_INSTANCE_PARTICIPANTS_UPDATE",
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["code"], 4006);
        // Only a subscribed page hears about participants.
        assert!(bridge.participants_changed(&ctx).is_empty());
    }

    fn command_evt(bridge: &mut Bridge, ctx: &Context, cmd: &str, evt: &str) -> Vec<Action> {
        bridge.handle(
            ORIGIN,
            &json!([1, {"cmd": cmd, "evt": evt, "args": {}, "nonce": "n1"}]),
            ctx,
        )
    }

    #[test]
    fn a_page_close_is_echoed_and_reported() {
        let (mut bridge, ctx) = shaken();
        let out = bridge.handle(ORIGIN, &json!([2, {"code": 1000}]), &ctx);
        assert!(matches!(out[0], Action::Close { code: 1000, .. }));
        assert_eq!(out[1], Action::Disconnected { code: 1000 });
        assert_eq!(bridge.origin(), None);
    }

    #[test]
    fn hello_carries_the_frame_id_and_the_platform() {
        let hello = Bridge::hello(&ctx(), &json!({"instance_id": "C1"}));
        assert_eq!(hello[0], 3);
        assert_eq!(hello[1]["frame_id"], "frame-1");
        assert_eq!(hello[1]["platform"], "desktop");
        assert_eq!(hello[1]["instance_id"], "C1");
    }

    #[test]
    fn external_links_must_be_http() {
        let (mut bridge, ctx) = shaken();
        let out = command(
            &mut bridge,
            &ctx,
            "OPEN_EXTERNAL_LINK",
            json!({"url": "file:///c:/x"}),
        );
        let Action::Post(message) = &out[0] else {
            panic!()
        };
        assert_eq!(message[1]["data"]["opened"], false);
        let out = command(
            &mut bridge,
            &ctx,
            "OPEN_EXTERNAL_LINK",
            json!({"url": "https://example.com"}),
        );
        assert!(matches!(&out[0], Action::Ask(Ask::OpenExternalLink { .. })));
    }
}
