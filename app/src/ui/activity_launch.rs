//! Launching, joining and leaving an Embedded Activity, and what the gateway says about running ones
//! (`EMBEDDED_ACTIVITY_UPDATE_V2`). The official flow (modules 795816, 211449, 181855, 730602, 933958):
//!
//! 1. Confirmations: leave the activity we are in, the age gate, the "Heads Up!" notice of an app Discord has not
//!    released.
//! 2. Join when the channel already hosts an instance of the app (`POST /activities/{channel}/{app}`), else start:
//!    the app's `PRIMARY_ENTRY_POINT` command is run as an interaction (`POST /interactions`).
//! 3. The activity opens only when the gateway lists *this session* among the instance's participants. Then a proxy
//!    ticket is asked (`POST /applications/{app}/proxy-tickets`) and the page `https://{app}.discordsays.com/?...`
//!    is hosted.
//! 4. Leave: `POST /applications/{app}/activities/{location}/instances/{launch}/leave`.

use super::activity_shelf::{ShelfCard, parse_shelf, remember_activity, shelf_key};
use super::modal::{self, Tone};
use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};
use serde_json::{Value, json};

pub(super) const ACTIVITY_PREFIX: &str = "activity:";
const EVER_LAUNCHED: &str = "activities.ever_launched";
/// `ACTIVITY_APPLICATION_HOST` in the page's `GLOBAL_ENV`.
const ACTIVITY_HOST: &str = "discordsays.com";
/// Application flag bit 1, `EMBEDDED_RELEASED`.
const EMBEDDED_RELEASED: u64 = 1 << 1;
/// `PRIMARY_ENTRY_POINT`.
const PRIMARY_ENTRY_POINT: u64 = 4;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Participant {
    pub user_id: String,
    pub session_id: String,
}

/// An instance the gateway reports (`EMBEDDED_ACTIVITY_UPDATE_V2`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Instance {
    pub application_id: String,
    pub launch_id: String,
    pub composite_instance_id: String,
    pub location_id: String,
    pub channel_id: Option<String>,
    pub guild_id: Option<String>,
    pub participants: Vec<Participant>,
}

/// The activity this session has open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Running {
    pub application_id: String,
    pub name: String,
    pub launch_id: String,
    pub location_id: String,
    pub channel: Id,
    pub guild: Id,
    pub instance: Instance,
    /// `discord_proxy_ticket`, once answered.
    pub ticket: Option<String>,
    pub url: Option<String>,
    /// The `frame_id` the page is given and has to repeat in its handshake.
    pub frame_id: String,
    /// When this session joined: the panel's "elapsed" counts from here.
    pub started: std::time::Instant,
}

#[derive(Clone)]
enum Dialog {
    /// 181855 (a): "You sure?" about leaving the activity we are in.
    Leave { current: String, next: String },
    /// `ActivityAgeGateModal`.
    AgeGate { app: String, name: String },
    /// Module 130574, the first-time notice.
    HeadsUp { app: String, name: String },
    /// "Failed to Launch Activity" (module 85982) with the reason as its subtitle.
    Failed { text: String },
}

/// `f` of module 872142 for a failed request (its `errorType` 2): the message the "Failed to Launch Activity" dialog
/// (module 85982) shows for the API error `code`, "Unable to launch activity" for any other.
/// GAP: the Brazil notice (`GJ27pD`, error 20060 for an account located in BR) needs the location metadata.
pub(super) fn launch_error_message(code: Option<u64>) -> &'static str {
    dc_intl::t(match code {
        // `INVALID_ACTIVITY_LAUNCH_NO_ACCESS`
        Some(50106) => "GyzcrS",
        // `INVALID_ACTIVITY_LAUNCH_PREMIUM_TIER`
        Some(50107) => "zxv7EF",
        // `INVALID_PERMISSIONS`
        Some(50013) => "hHGrWz",
        // `INVALID_ACTIVITY_LAUNCH_AFK_CHANNEL`
        Some(50148) => "j29zCr",
        // `INVALID_ACTIVITY_LAUNCH_AGE_GATED`
        Some(50165) => "4WuFRE",
        // `INVALID_ACTIVITY_LAUNCH_DEV_PREVIEW_GUILD_SIZE`
        Some(50209) => "RvkXdb",
        // `ACTIVITY_CONFIGURATION_DOES_NOT_SUPPORT_PLATFORM`
        Some(50231) => "uGDCcw",
        _ => "IOy+I5",
    })
}

/// What a launch is waiting on.
struct Launching {
    app: String,
    channel: Id,
    guild: Id,
}

#[derive(Default)]
pub(crate) struct Activities {
    pub instances: Vec<Instance>,
    pub running: Option<Running>,
    launching: Option<Launching>,
    dialog: Option<Dialog>,
    pub(super) host: Option<super::activity_host::ActivityHost>,
    /// The consent flow an SDK `AUTHORIZE` started.
    pub(super) oauth: Option<super::oauth::Flow>,
    /// Application names the shelf has told, for rows of activities running in other channels.
    pub(super) names: std::collections::HashMap<String, String>,
    /// The call view painted the stage this frame.
    pub(super) stage_seen: bool,
    /// The whole seconds the panel last showed, so a tick repaints it.
    pub(super) shown_seconds: Option<u64>,
    /// The page is in the picture-in-picture window (the call view's stage is not on screen).
    pub(super) pip: bool,
}

impl Activities {
    pub(super) fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The page could not be created: say so.
    pub(super) fn host_failed(&mut self, why: &str) {
        self.dialog = Some(Dialog::Failed {
            text: format!("The activity could not be shown. {why}"),
        });
    }

    /// The instance of `app` in `channel`, if one runs there.
    pub fn instance_in(&self, channel: Id, app: &str) -> Option<&Instance> {
        let channel = channel.to_string();
        self.instances
            .iter()
            .find(|i| i.application_id == app && i.channel_id.as_deref() == Some(channel.as_str()))
    }

    /// Instances in a channel, for the sidebar row and the call view.
    pub fn in_channel(&self, channel: Id) -> Vec<&Instance> {
        let channel = channel.to_string();
        self.instances
            .iter()
            .filter(|i| i.channel_id.as_deref() == Some(channel.as_str()))
            .collect()
    }

    /// Whether `participant` is this account in this gateway session.
    fn is_me(participant: &Participant, me: Id, session: &str) -> bool {
        participant.user_id == me.to_string() && participant.session_id == session
    }
}

/// `EMBEDDED_ACTIVITY_UPDATE_V2`'s `instance`.
pub(crate) fn parse_instance(data: &Value) -> Option<Instance> {
    let instance = if data["instance"].is_object() {
        &data["instance"]
    } else {
        data
    };
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let location = &instance["location"];
    Some(Instance {
        application_id: text(&instance["application_id"])?,
        launch_id: text(&instance["launch_id"]).unwrap_or_default(),
        composite_instance_id: text(&instance["composite_instance_id"]).unwrap_or_default(),
        location_id: text(&location["id"]).unwrap_or_default(),
        channel_id: text(&location["channel_id"]),
        guild_id: text(&location["guild_id"]).or_else(|| text(&data["guild_id"])),
        participants: instance["participants"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(Participant {
                    user_id: text(&p["user_id"])?,
                    session_id: text(&p["session_id"]).unwrap_or_default(),
                })
            })
            .collect(),
    })
}

/// `https://{app}.discordsays.com` (956518 `Ay`).
pub(crate) fn activity_origin(app: &str) -> String {
    format!("https://{app}.{ACTIVITY_HOST}")
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The iframe's `src` (51250, 873727, 358415): the origin and the query the host gives every activity. Neither the
/// account token nor the session id is in it; the proxy ticket is the only credential.
pub(crate) fn activity_url(instance: &Instance, ticket: Option<&str>, frame_id: &str) -> String {
    let mut query: Vec<(&str, String)> = vec![
        (
            "instance_id",
            if instance.composite_instance_id.is_empty() {
                instance.launch_id.clone()
            } else {
                instance.composite_instance_id.clone()
            },
        ),
        ("location_id", instance.location_id.clone()),
        ("launch_id", instance.launch_id.clone()),
        ("theme", "darker".to_owned()),
        ("ui_density", "cozy".to_owned()),
        ("message_display_compact", "false".to_owned()),
        ("font_scale", "100".to_owned()),
        ("reduced_motion", "false".to_owned()),
        ("high_contrast", "false".to_owned()),
        ("forced_colors", "false".to_owned()),
        ("underline_links", "false".to_owned()),
    ];
    if let Some(ticket) = ticket {
        query.push(("discord_proxy_ticket", ticket.to_owned()));
    }
    if let Some(guild) = instance.guild_id.as_ref().filter(|g| !g.is_empty()) {
        query.push(("guild_id", guild.clone()));
    }
    if let Some(channel) = instance.channel_id.as_ref().filter(|c| !c.is_empty()) {
        query.push(("channel_id", channel.clone()));
    }
    query.push(("frame_id", frame_id.to_owned()));
    query.push(("platform", "desktop".to_owned()));
    let query: Vec<String> = query
        .into_iter()
        .map(|(name, value)| format!("{name}={}", percent_encode(&value)))
        .collect();
    format!(
        "{}/?{}",
        activity_origin(&instance.application_id),
        query.join("&")
    )
}

/// The body of `POST /interactions` that runs a `PRIMARY_ENTRY_POINT` command (545152 / 870801).
pub(crate) fn launch_interaction(
    command: &Value,
    guild: Option<Id>,
    channel: Id,
    session: &str,
    nonce: &str,
) -> Value {
    json!({
        "type": 2,
        "application_id": command["application_id"],
        "guild_id": guild.map(|guild| guild.to_string()),
        "channel_id": channel.to_string(),
        "session_id": session,
        "data": {
            "version": command["version"],
            "id": command["id"],
            "guild_id": command["guild_id"],
            "name": command["name"],
            "type": PRIMARY_ENTRY_POINT,
            "options": [],
            "application_command": command,
        },
        "nonce": nonce,
    })
}

/// The body of `POST /activities/{channel}/{app}` (join).
pub(crate) fn join_body(session: &str, guild: Option<Id>) -> Value {
    match guild {
        Some(guild) => json!({"session_id": session, "guild_id": guild.to_string()}),
        None => json!({"session_id": session}),
    }
}

/// The body of `POST /applications/{app}/proxy-tickets`.
pub(crate) fn ticket_body(channel: Id) -> Value {
    json!({"use_stateless_ticket": true, "channel_id": channel.to_string()})
}

/// The route and body of the leave call.
pub(crate) fn leave_request(running: &Running, session: &str) -> (String, Value) {
    (
        format!(
            "/applications/{}/activities/{}/instances/{}/leave",
            running.application_id, running.location_id, running.launch_id
        ),
        json!({"session_id": session}),
    )
}

/// A `PRIMARY_ENTRY_POINT` command of `app` in a command index response.
pub(crate) fn primary_command(index: &Value, app: &str) -> Option<Value> {
    index["application_commands"]
        .as_array()?
        .iter()
        .find(|command| {
            command["type"].as_u64() == Some(PRIMARY_ENTRY_POINT)
                && command["application_id"].as_str() == Some(app)
        })
        .cloned()
}

fn ever_launched(app: &str) -> bool {
    crate::storage::get(EVER_LAUNCHED)
        .and_then(|value| value.as_array().cloned())
        .is_some_and(|list| list.iter().any(|id| id.as_str() == Some(app)))
}

fn mark_launched(app: &str) {
    let mut list: Vec<Value> = crate::storage::get(EVER_LAUNCHED)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    if !list.iter().any(|id| id.as_str() == Some(app)) {
        list.push(json!(app));
        crate::storage::set(EVER_LAUNCHED, json!(list));
    }
}

impl Root {
    fn shelf_application(&self, app: &str) -> Option<(ShelfCard, u64, bool)> {
        let guild = self
            .call
            .as_ref()
            .map(|call| call.guild)
            .filter(|g| *g != 0);
        let body = self.fetched.get(&shelf_key(guild))?;
        let card = parse_shelf(body, chrono::Utc::now())
            .into_iter()
            .find(|card| card.app_id == app)?;
        let application = body["applications"]
            .as_array()?
            .iter()
            .find(|a| a["id"].as_str() == Some(app))?;
        let flags = match &application["flags"] {
            Value::Number(n) => n.as_u64().unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        Some((
            card,
            flags,
            application["is_verified"].as_bool().unwrap_or(false),
        ))
    }

    /// A card was chosen: the checks, then the join or the start.
    pub(super) fn launch_activity(&mut self, app: &str, cx: &mut Context<Self>) {
        tracing::info!(
            in_call = self.call.is_some(),
            launching = self.activities.launching.is_some(),
            "activity card chosen"
        );
        if self.call.is_none() {
            // The shelf opens from the call; without one there is no voice channel to start it in.
            self.activities.dialog = Some(Dialog::Failed {
                text: "Join a voice channel to start an Activity.".to_owned(),
            });
            cx.notify();
            return;
        }
        if self.activities.launching.is_some() {
            return;
        }
        if let Some(running) = &self.activities.running {
            if running.application_id == app {
                return;
            }
            self.activities.dialog = Some(Dialog::Leave {
                current: running.name.clone(),
                next: app.to_owned(),
            });
            cx.notify();
            return;
        }
        self.launch_after_leave(app, cx);
    }

    /// The age gate, then the first-time notice, then the request.
    fn launch_after_leave(&mut self, app: &str, cx: &mut Context<Self>) {
        let Some((card, flags, verified)) = self.shelf_application(app) else {
            tracing::info!("activity not in the shelf");
            return;
        };
        // `nsfwAllowed == null` is what this client knows of every account.
        if card.requires_age_gate {
            self.activities.dialog = Some(Dialog::AgeGate {
                app: app.to_owned(),
                name: card.name,
            });
            cx.notify();
            return;
        }
        self.launch_after_age_gate(app, cx, flags, verified, &card.name);
    }

    fn launch_after_age_gate(
        &mut self,
        app: &str,
        cx: &mut Context<Self>,
        flags: u64,
        verified: bool,
        name: &str,
    ) {
        if flags & EMBEDDED_RELEASED == 0 && !verified && !ever_launched(app) {
            self.activities.dialog = Some(Dialog::HeadsUp {
                app: app.to_owned(),
                name: name.to_owned(),
            });
            cx.notify();
            return;
        }
        self.launch_go(app, cx);
    }

    /// Join the running instance, or start one.
    fn launch_go(&mut self, app: &str, cx: &mut Context<Self>) {
        let Some(call) = &self.call else {
            return;
        };
        let (guild, channel) = (call.guild, call.channel);
        let session = self.stores.session_id.clone().unwrap_or_default();
        self.activities.launching = Some(Launching {
            app: app.to_owned(),
            channel,
            guild,
        });
        let guild_id = (guild != 0).then_some(guild);
        if self.activities.instance_in(channel, app).is_some() {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{ACTIVITY_PREFIX}join:{app}")),
                method: reqwest::Method::POST,
                route: format!("/activities/{channel}/{app}"),
                body: Some(join_body(&session, guild_id)),
            });
        } else {
            let _ = self.requests.send(Request::Fetch {
                key: format!("{ACTIVITY_PREFIX}index:{app}"),
                route: format!("/applications/{app}/application-command-index"),
            });
        }
        cx.notify();
    }

    /// The answers of the launch's requests. True when `key` was one of them.
    pub(super) fn activity_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(ACTIVITY_PREFIX) else {
            return false;
        };
        let (step, app) = rest.split_once(':').unwrap_or((rest, ""));
        tracing::info!(step, ok = ?body["ok"].as_bool(), status = ?body["status"].as_u64(), "activity request answered");
        let app = app.to_owned();
        // `launchFrame`'s catch (module 456006, `showErrorModal`): the message of module 872142 for the API error.
        let code = body["body"]["code"].as_u64();
        let failed = move |this: &mut Self, why: &str, cx: &mut Context<Self>| {
            tracing::warn!("activity launch failed: {why}");
            this.activities.launching = None;
            this.activities.dialog = Some(Dialog::Failed {
                text: launch_error_message(code).to_owned(),
            });
            cx.notify();
        };
        match step {
            "index" => {
                let Some(launching) = self.activities.launching.as_ref().filter(|l| l.app == app)
                else {
                    return true;
                };
                let (guild, channel) = (launching.guild, launching.channel);
                let Some(command) = primary_command(body, &app) else {
                    failed(self, "no primary app command for application", cx);
                    return true;
                };
                let session = self.stores.session_id.clone().unwrap_or_default();
                let guild_id = (guild != 0).then_some(guild);
                let nonce = crate::net::nonce();
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{ACTIVITY_PREFIX}interaction:{app}")),
                    method: reqwest::Method::POST,
                    route: "/interactions".to_owned(),
                    body: Some(launch_interaction(
                        &command, guild_id, channel, &session, &nonce,
                    )),
                });
            }
            "interaction" | "join" => {
                if body["ok"].as_bool() == Some(true) {
                    // `EMBEDDED_ACTIVITY_LAUNCH_SUCCESS`: the shelf closes and the gateway opens the activity.
                    self.activities.launching = None;
                    mark_launched(&app);
                    remember_activity(&app);
                    self.close_activity_shelf(cx);
                } else {
                    failed(self, &format!("{step}: status {}", body["status"]), cx);
                }
            }
            "ticket" => {
                let ticket = body["body"]["ticket"].as_str().map(str::to_owned);
                if body["ok"].as_bool() != Some(true) || ticket.is_none() {
                    failed(self, "proxy ticket", cx);
                    return true;
                }
                if let Some(running) = self
                    .activities
                    .running
                    .as_mut()
                    .filter(|running| running.application_id == app)
                {
                    running.url = Some(activity_url(
                        &running.instance,
                        ticket.as_deref(),
                        &running.frame_id,
                    ));
                    running.ticket = ticket;
                }
                cx.notify();
            }
            "ask" => self.activity_ask_answered(&app, body, cx),
            "oauth-public" | "oauth-get" | "oauth-disc" | "oauth-post" | "oauth-ack" => {
                self.oauth_fetched(step, &app, body, cx)
            }
            "leave" => {}
            _ => {}
        }
        cx.notify();
        true
    }

    /// `EMBEDDED_ACTIVITY_UPDATE_V2` and the instances `READY_SUPPLEMENTAL` lists.
    pub(super) fn activity_instance_update(&mut self, data: &Value, cx: &mut Context<Self>) {
        let Some(instance) = parse_instance(data) else {
            return;
        };
        tracing::info!(
            participants = instance.participants.len(),
            "activity instance update"
        );
        if !self.activities.names.contains_key(&instance.application_id) {
            // The row needs the app's name: the shelf has it.
            let guild = instance.guild_id.as_deref().and_then(|id| id.parse().ok());
            let key = shelf_key(guild);
            if self.requested.insert(key.clone()) {
                let route = match guild {
                    Some(guild) => format!("/activities/shelf?guild_id={guild}"),
                    None => "/activities/shelf".to_owned(),
                };
                let _ = self.requests.send(Request::Fetch { key, route });
            }
        }
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let session = self.stores.session_id.clone().unwrap_or_default();
        self.activities.instances.retain(|i| {
            i.composite_instance_id != instance.composite_instance_id
                || i.launch_id != instance.launch_id
        });
        let mine = me.is_some_and(|me| {
            instance
                .participants
                .iter()
                .any(|p| Activities::is_me(p, me, &session))
        });
        if !instance.participants.is_empty() {
            self.activities.instances.push(instance.clone());
        }
        tracing::info!(
            mine,
            running = self.activities.running.is_some(),
            "activity instance is ours"
        );
        match (&self.activities.running, mine) {
            (None, true) => {
                let channel: Id = instance
                    .channel_id
                    .as_deref()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_default();
                let guild: Id = instance
                    .guild_id
                    .as_deref()
                    .and_then(|id| id.parse().ok())
                    .unwrap_or_default();
                let name = self
                    .shelf_application(&instance.application_id)
                    .map(|(card, _, _)| card.name)
                    .unwrap_or_default();
                let app = instance.application_id.clone();
                self.activities.running = Some(Running {
                    application_id: app.clone(),
                    name,
                    launch_id: instance.launch_id.clone(),
                    location_id: instance.location_id.clone(),
                    channel,
                    guild,
                    instance,
                    ticket: None,
                    url: None,
                    frame_id: frame_id(),
                    started: std::time::Instant::now(),
                });
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{ACTIVITY_PREFIX}ticket:{app}")),
                    method: reqwest::Method::POST,
                    route: format!("/applications/{app}/proxy-tickets"),
                    body: Some(ticket_body(channel)),
                });
            }
            (Some(running), false)
                if running.launch_id == instance.launch_id
                    && running.application_id == instance.application_id =>
            {
                // We are no longer listed: the activity closes.
                self.activities.running = None;
                cx.notify();
            }
            (Some(running), true) if running.launch_id == instance.launch_id => {
                if let Some(running) = self.activities.running.as_mut() {
                    running.instance = instance;
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Leave the running activity: the request, then the page goes.
    pub(super) fn leave_activity(&mut self, cx: &mut Context<Self>) {
        let Some(running) = self.activities.running.take() else {
            return;
        };
        self.activities.oauth = None;
        let session = self.stores.session_id.clone().unwrap_or_default();
        let (route, body) = leave_request(&running, &session);
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{ACTIVITY_PREFIX}leave:{}", running.application_id)),
            method: reqwest::Method::POST,
            route,
            body: Some(body),
        });
        cx.notify();
    }

    /// A Leave button: the activity is left with `showFeedback` (`EMBEDDED_ACTIVITY_CLOSE`), so the Activity survey
    /// may follow (module 220162 `handleActivityClose`), with "Activity included ads" when the application's activity
    /// config (the shelf's entry of it) says it `displays_advertisements`.
    pub(super) fn leave_activity_by_hand(&mut self, cx: &mut Context<Self>) {
        let left = self.activities.running.as_ref().map(|running| {
            let guild = Some(running.guild).filter(|guild| *guild != 0);
            let ads = self.fetched.get(&shelf_key(guild)).is_some_and(|body| {
                body["activities"].as_array().is_some_and(|configs| {
                    configs.iter().any(|config| {
                        config["application_id"].as_str() == Some(running.application_id.as_str())
                            && config["displays_advertisements"].as_bool() == Some(true)
                    })
                })
            });
            (running.name.clone(), ads)
        });
        self.leave_activity(cx);
        if let Some((name, ads)) = left {
            self.activity_left(name, ads, cx);
        }
    }

    /// The call ended: an open activity is left (`handleRTCConnectionState`).
    pub(super) fn leave_activity_on_call_end(&mut self, cx: &mut Context<Self>) {
        self.leave_activity(cx);
    }

    /// The question or notice that is up, if any.
    pub(super) fn activity_dialog_layer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.activities.dialog.clone()?;
        let close = || -> modal::Handler { Box::new(|this, _, _| this.activities.dialog = None) };
        let cancel = || {
            modal::button(
                "activity-cancel",
                "Cancel",
                Tone::Secondary,
                |this, _, _| {
                    this.activities.dialog = None;
                },
            )
        };
        let text =
            |text: String| -> Option<AnyElement> { Some(div().child(text).into_any_element()) };
        Some(match dialog {
            Dialog::Leave { current, next } => modal::render(
                modal::Spec {
                    id: "activity-leave",
                    title: "You sure?".into(),
                    subtitle: text(format!(
                        "Are you sure you want to disconnect from {current} in its current channel?"
                    )),
                    body: None,
                    buttons: vec![
                        cancel(),
                        modal::button("activity-leave-confirm", "Confirm", Tone::Primary, move |this, _, cx| {
                            this.activities.dialog = None;
                            this.leave_activity(cx);
                            this.launch_activity(&next, cx);
                        }),
                    ],
                    close: close(),
                },
                cx,
            ),
            Dialog::AgeGate { app, name } => modal::render(
                modal::Spec {
                    id: "activity-age-gate",
                    title: "You\u{2019}re about to use an age-restricted activity".into(),
                    subtitle: text(format!(
                        "Heads up, {name} is only for adult players. Please play responsibly."
                    )),
                    body: None,
                    buttons: vec![
                        modal::button("activity-age-nope", "Nope", Tone::Secondary, |this, _, _| {
                            this.activities.dialog = None;
                        }),
                        modal::button("activity-age-continue", "Continue", Tone::Critical, move |this, _, cx| {
                            this.activities.dialog = None;
                            if let Some((card, flags, verified)) = this.shelf_application(&app) {
                                this.launch_after_age_gate(&app, cx, flags, verified, &card.name);
                            }
                        }),
                    ],
                    close: close(),
                },
                cx,
            ),
            Dialog::HeadsUp { app, name } => modal::render(
                modal::Spec {
                    id: "activity-heads-up",
                    title: "Heads Up!".into(),
                    subtitle: text(format!(
                        "You\u{2019}re about to test {name} for the first time. This app is not made by Discord. Only test apps from developers you trust."
                    )),
                    body: Some(
                        div()
                            .id("activity-private-learn-more")
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-link"))
                            .on_click(|_, _, cx| {
                                cx.open_url("https://support-dev.discord.com/hc/en-us/articles/21692628851351-Private-Activity-Safety")
                            })
                            .child("Learn more about private apps.")
                            .into_any_element(),
                    ),
                    buttons: vec![
                        cancel(),
                        modal::button("activity-heads-continue", "Continue", Tone::Primary, move |this, _, cx| {
                            this.activities.dialog = None;
                            this.launch_go(&app, cx);
                        }),
                    ],
                    close: close(),
                },
                cx,
            ),
            // Module 85982 on the dialog of module 189213: no children, so `sm`.
            Dialog::Failed { text: message } => modal::render_small(
                modal::Spec {
                    id: "activity-failed",
                    title: dc_intl::t("PtobXW").into(),
                    subtitle: text(message),
                    body: None,
                    buttons: vec![modal::button(
                        "activity-failed-okay",
                        dc_intl::t("BddRzS"),
                        Tone::Primary,
                        |this, _, _| this.activities.dialog = None,
                    )],
                    close: close(),
                },
                cx,
            ),
        })
    }

    /// Capture only: `activity failed <code>`, what a launch whose request failed with that API error shows.
    #[cfg(feature = "capture")]
    pub(crate) fn script_activity_failed(&mut self, code: Option<u64>) -> String {
        self.activities.launching = None;
        let text = launch_error_message(code).to_owned();
        self.activities.dialog = Some(Dialog::Failed { text: text.clone() });
        format!("failed: {text}")
    }

    /// A one-line state for the capture harness: what the shelf, the launch and the gateway say.
    #[cfg(feature = "capture")]
    pub(crate) fn activity_state_line(&self) -> String {
        let a = &self.activities;
        format!(
            "shelf={} launching={} instances={} running={} ticket={} url={} host=[{}] dialog={} oauth=[{}]",
            self.activity_shelf.is_some(),
            a.launching.is_some(),
            a.instances.len(),
            a.running.as_ref().map(|r| r.name.as_str()).unwrap_or("-"),
            a.running.as_ref().is_some_and(|r| r.ticket.is_some()),
            a.running.as_ref().is_some_and(|r| r.url.is_some()),
            a.host.as_ref().map(|h| h.state()).unwrap_or_default(),
            match &a.dialog {
                None => "-",
                Some(Dialog::Leave { .. }) => "leave",
                Some(Dialog::AgeGate { .. }) => "age-gate",
                Some(Dialog::HeadsUp { .. }) => "heads-up",
                Some(Dialog::Failed { .. }) => "failed",
            },
            a.oauth
                .as_ref()
                .map(|flow| flow.describe())
                .unwrap_or_default()
        )
    }

    /// The capture harness: launches the shelf activity called `name`. The answer names what happened.
    #[cfg(feature = "capture")]
    pub(crate) fn activity_launch_named(&mut self, name: &str, cx: &mut Context<Self>) -> String {
        let guild = self
            .call
            .as_ref()
            .map(|call| call.guild)
            .filter(|g| *g != 0);
        let Some(body) = self.fetched.get(&shelf_key(guild)) else {
            return "the shelf is not loaded".to_owned();
        };
        let Some(card) = parse_shelf(body, chrono::Utc::now())
            .into_iter()
            .find(|card| card.name == name)
        else {
            return "not in the shelf".to_owned();
        };
        self.launch_activity(&card.app_id, cx);
        "launching".to_owned()
    }

    /// Escape closes the notice. True when there was one.
    pub(super) fn activity_dialog_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.activities.dialog.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }
}

fn frame_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_launch_names_the_api_error_like_module_872142() {
        assert_eq!(
            launch_error_message(Some(50107)),
            "This server doesn't have a high enough boosting level for the requested activity"
        );
        assert_eq!(
            launch_error_message(Some(50148)),
            "You cannot launch Activities in this channel."
        );
        assert_eq!(
            launch_error_message(Some(50165)),
            "This Activity is age-restricted"
        );
        assert_eq!(launch_error_message(None), "Unable to launch activity");
        assert_eq!(
            launch_error_message(Some(10003)),
            "Unable to launch activity"
        );
        assert_eq!(dc_intl::t("PtobXW"), "Failed to Launch Activity");
    }

    fn instance() -> Instance {
        Instance {
            application_id: "880".into(),
            launch_id: "L1".into(),
            composite_instance_id: "C1".into(),
            location_id: "LOC".into(),
            channel_id: Some("55".into()),
            guild_id: Some("44".into()),
            participants: vec![Participant {
                user_id: "9".into(),
                session_id: "sess".into(),
            }],
        }
    }

    #[test]
    fn the_gateway_instance_is_read_with_its_participants() {
        let data = json!({
            "guild_id": "44",
            "instance": {
                "application_id": "880", "launch_id": "L1", "composite_instance_id": "C1",
                "location": {"id": "LOC", "kind": "gc", "channel_id": "55", "guild_id": "44"},
                "participants": [{"user_id": "9", "session_id": "sess", "nonce": "n"}],
                "content_classification": 0
            }
        });
        assert_eq!(parse_instance(&data), Some(instance()));
        assert!(parse_instance(&json!({"instance": {}})).is_none());
    }

    #[test]
    fn only_our_own_session_opens_the_activity() {
        let me = Participant {
            user_id: "9".into(),
            session_id: "sess".into(),
        };
        assert!(Activities::is_me(&me, 9, "sess"));
        assert!(!Activities::is_me(&me, 9, "other"));
        assert!(!Activities::is_me(&me, 8, "sess"));
    }

    #[test]
    fn the_page_url_carries_the_ticket_and_no_account_credential() {
        let url = activity_url(&instance(), Some("tk en"), "frame");
        assert!(url.starts_with(
            "https://880.discordsays.com/?instance_id=C1&location_id=LOC&launch_id=L1&"
        ));
        assert!(url.contains("&discord_proxy_ticket=tk%20en&guild_id=44&channel_id=55&frame_id=frame&platform=desktop"));
        assert!(!url.contains("session_id") && !url.contains("token="));
        let bare = activity_url(&instance(), None, "f");
        assert!(!bare.contains("discord_proxy_ticket"));
    }

    #[test]
    fn the_request_bodies_match_the_bundle() {
        let command = json!({"id": "c", "application_id": "880", "version": "v", "name": "launch", "type": 4, "guild_id": null});
        let body = launch_interaction(&command, Some(44), 55, "sess", "n1");
        assert_eq!(body["type"], 2);
        assert_eq!(body["application_id"], "880");
        assert_eq!(body["guild_id"], "44");
        assert_eq!(body["channel_id"], "55");
        assert_eq!(body["session_id"], "sess");
        assert_eq!(body["data"]["type"], 4);
        assert_eq!(body["data"]["options"], json!([]));
        assert_eq!(body["data"]["application_command"]["id"], "c");
        assert_eq!(body["nonce"], "n1");
        assert_eq!(
            join_body("s", Some(44)),
            json!({"session_id": "s", "guild_id": "44"})
        );
        assert_eq!(join_body("s", None), json!({"session_id": "s"}));
        assert_eq!(
            ticket_body(55),
            json!({"use_stateless_ticket": true, "channel_id": "55"})
        );
        let running = Running {
            application_id: "880".into(),
            name: "x".into(),
            launch_id: "L1".into(),
            location_id: "LOC".into(),
            channel: 55,
            guild: 44,
            instance: instance(),
            ticket: None,
            url: None,
            frame_id: "f".into(),
            started: std::time::Instant::now(),
        };
        assert_eq!(
            leave_request(&running, "sess"),
            (
                "/applications/880/activities/LOC/instances/L1/leave".to_owned(),
                json!({"session_id": "sess"})
            )
        );
    }

    #[test]
    fn the_primary_entry_point_is_picked_out_of_the_index() {
        let index = json!({"application_commands": [
            {"id": "1", "type": 1, "application_id": "880"},
            {"id": "2", "type": 4, "application_id": "999"},
            {"id": "3", "type": 4, "application_id": "880"},
        ]});
        assert_eq!(primary_command(&index, "880").unwrap()["id"], "3");
        assert!(primary_command(&index, "1").is_none());
    }

    #[test]
    fn instances_are_found_by_channel() {
        let mut activities = Activities::default();
        activities.instances.push(instance());
        assert!(activities.instance_in(55, "880").is_some());
        assert!(activities.instance_in(56, "880").is_none());
        assert_eq!(activities.in_channel(55).len(), 1);
    }
}
