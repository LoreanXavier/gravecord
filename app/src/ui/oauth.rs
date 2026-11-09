//! The OAuth2 consent modal an Embedded App SDK `AUTHORIZE` opens (modules 610056, 716965, 596339 `DI`, 647053, 123677).
//!
//! The official order (`DI` in 596339): the request is parsed, the application's integration types decide GUILD or
//! USER install, then `GET /oauth2/authorize?...` and `GET /applications/{id}/disclosures` run together. When the
//! request is `prompt=none` and the GET says `authorized` with every disclosure acknowledged, the grant is renewed
//! without a modal (`POST /oauth2/authorize` with `authorize: true`). Otherwise the modal shows; Cancel posts
//! `authorize: false` and the SDK is told what the returned `location` says, the close button and Escape tell it
//! "User cancelled authorization", Authorize posts `authorize: true`, then the disclosures are acknowledged and the
//! `code` of the returned `location` goes back to the page.
//!
//! Granting is the owner's decision: a capture build (`--features capture`, the parity and live test harness) never
//! sends an `authorize: true` POST, and does not post the Cancel button's `authorize: false` either; it answers the
//! page as the close button does. `GRAVECORD_ALLOW_AUTHORIZE` lifts that for a deliberate run.

use super::oauth_spec::{self as spec, P};
use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_webhost::sdk;
use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement,
    ScrollHandle, SharedString, StyledText, Window, canvas, deferred, div, img, prelude::*, px,
    rgba,
};
use serde_json::{Value, json};
use std::cell::Cell;
use std::rc::Rc;

pub(super) const GUILD_INSTALL: u64 = 0;
pub(super) const USER_INSTALL: u64 = 1;
/// `APPLICATIONS_COMMANDS` and `BOT`, the scopes that need a server chosen in a guild install.
const COMMANDS: &str = "applications.commands";
const BOT: &str = "bot";
const WEBHOOK: &str = "webhook.incoming";
/// Application flag bit 33, `PARENT`: the application's terms apply to all its games (`G.t`).
const PARENT_FLAG: u64 = 1 << 33;
/// The sentinel at the end of the content counts as seen this close to the end of the scroll (measured: the sentinel
/// sits 701px into the content of the golden's 739px, whose viewport is 445px; 294 - 256 = 38, one pixel is the
/// sentinel's own height).
const END_SLACK: f32 = 37.;

/// What a request to authorize carries (`DI`'s destructured argument).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Args {
    pub client_id: String,
    pub response_type: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub scopes: Vec<String>,
    pub permissions: u64,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub prompt: Option<String>,
    pub integration_type: Option<u64>,
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// `DI`'s first checks, in its order; the `scopes` field wins over `scope` when it is not empty (`AUTHORIZE`).
pub(super) fn parse_args(args: &Value) -> Result<Args, (u32, String)> {
    let client_id = text_of(&args["client_id"])
        .ok_or((sdk::code::OAUTH2_ERROR, "No Client ID provided".to_owned()))?;
    if !args["redirect_uri"].is_null() {
        return Err((
            sdk::code::OAUTH2_ERROR,
            "Redirect URI cannot be used in the RPC OAuth2 Authorization flow".to_owned(),
        ));
    }
    let raw = match &args["scopes"] {
        Value::String(text) if !text.is_empty() => &args["scopes"],
        Value::Array(items) if !items.is_empty() => &args["scopes"],
        _ => &args["scope"],
    };
    let scopes = match raw {
        Value::String(text) => text
            .split(' ')
            .filter(|scope| !scope.is_empty())
            .map(str::to_owned)
            .collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    Ok(Args {
        client_id,
        response_type: text_of(&args["response_type"]).unwrap_or_else(|| "code".to_owned()),
        code_challenge: text_of(&args["code_challenge"]),
        code_challenge_method: text_of(&args["code_challenge_method"]),
        state: text_of(&args["state"]),
        nonce: text_of(&args["nonce"]),
        scopes,
        permissions: text_of(&args["permissions"])
            .and_then(|text| text.parse().ok())
            .unwrap_or(0),
        guild_id: text_of(&args["guild_id"]),
        channel_id: text_of(&args["channel_id"]),
        prompt: text_of(&args["prompt"]),
        integration_type: text_of(&args["integration_type"]).and_then(|text| text.parse().ok()),
    })
}

/// `J.e` (123677): `rpc.api` is dropped, and `bot` brings `applications.commands` along.
pub(super) fn requested_scopes(raw: &[String]) -> Vec<String> {
    let mut scopes: Vec<String> = raw
        .iter()
        .filter(|scope| !spec::HIDDEN.contains(&scope.as_str()))
        .cloned()
        .collect();
    if scopes.iter().any(|scope| scope == BOT) && !scopes.iter().any(|scope| scope == COMMANDS) {
        scopes.push(COMMANDS.to_owned());
    }
    scopes
}

/// `k$` filtered by the request: what the modal lists, in the modal's order.
pub(super) fn account_scopes(requested: &[String]) -> Vec<&'static str> {
    spec::ORDER
        .iter()
        .copied()
        .filter(|scope| requested.iter().any(|asked| asked == scope))
        .collect()
}

/// The error the modal shows for a request it cannot grant.
pub(super) fn scope_error(requested: &[String]) -> Option<String> {
    if requested.is_empty() {
        return Some("No scopes were provided.".to_owned());
    }
    requested
        .iter()
        .find(|scope| {
            !spec::ORDER.contains(&scope.as_str())
                && !spec::ACCEPTED_EXTRA.contains(&scope.as_str())
        })
        .map(|scope| format!("Invalid scope: {scope}"))
}

/// A query value, percent-encoded outside the unreserved set; a space is `%20`.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn query(pairs: &[(&str, Option<String>)]) -> String {
    pairs
        .iter()
        .filter_map(|(name, value)| Some(format!("{name}={}", encode(value.as_deref()?))))
        .collect::<Vec<_>>()
        .join("&")
}

/// `qY` (716965): the GET the modal's data comes from. `scopes` are the request's, unfiltered.
pub(super) fn authorize_get_route(args: &Args, integration: u64) -> String {
    format!(
        "/oauth2/authorize?{}",
        query(&[
            ("client_id", Some(args.client_id.clone())),
            ("response_type", Some(args.response_type.clone())),
            ("code_challenge", args.code_challenge.clone()),
            ("code_challenge_method", args.code_challenge_method.clone()),
            ("scope", Some(args.scopes.join(" "))),
            ("state", args.state.clone()),
            ("integration_type", Some(integration.to_string())),
            ("nonce", args.nonce.clone()),
        ])
    )
}

/// `Gq` (716965): the POST's route, with the filtered scopes.
pub(super) fn authorize_post_route(args: &Args, requested: &[String]) -> String {
    format!(
        "/oauth2/authorize?{}",
        query(&[
            ("client_id", Some(args.client_id.clone())),
            ("response_type", Some(args.response_type.clone())),
            ("code_challenge", args.code_challenge.clone()),
            ("code_challenge_method", args.code_challenge_method.clone()),
            ("scope", Some(requested.join(" "))),
            ("state", args.state.clone()),
            ("nonce", args.nonce.clone()),
        ])
    )
}

/// Where the user was when the grant was asked for (`location_context`): `"10000"` and type 10000 when unknown.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Location {
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub channel_type: Option<u64>,
}

/// `Gq`'s body. `guild` is the server picked for a guild install.
pub(super) fn post_body(
    authorize: bool,
    integration: u64,
    permissions: u64,
    guild: Option<&str>,
    location: &Location,
) -> Value {
    let guild = guild.filter(|_| integration == GUILD_INSTALL);
    let mut body = json!({
        "permissions": permissions.to_string(),
        "authorize": authorize,
        "integration_type": integration,
        "location_context": {
            "guild_id": location.guild_id.clone().unwrap_or_else(|| "10000".to_owned()),
            "channel_id": location.channel_id.clone().unwrap_or_else(|| "10000".to_owned()),
            "channel_type": location.channel_type.unwrap_or(10000),
        },
    });
    if let Some(guild) = guild {
        body["guild_id"] = json!(guild);
    }
    body
}

/// What the SDK is told once `location` is known (the `AUTHORIZE` handler's `.then`).
pub(super) fn reply_from_location(location: &str) -> Result<Value, (u32, String)> {
    let params = location_params(location);
    let get = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if let Some(error) = get("error").filter(|error| !error.is_empty()) {
        let description = get("error_description").unwrap_or("unknown error");
        return Err((
            sdk::code::OAUTH2_ERROR,
            format!("OAuth2 Error: {error}: {description}"),
        ));
    }
    match get("code") {
        Some(code) => Ok(json!({ "code": code })),
        None => Err((
            sdk::code::OAUTH2_ERROR,
            "OAuth2 Error: Unable to find auth code".to_owned(),
        )),
    }
}

/// What the close button, Escape and a dismissed error tell the SDK (`onCloseCallback` in `Dm`).
pub(super) fn cancel_reply() -> Result<Value, (u32, String)> {
    Err((
        sdk::code::OAUTH2_ERROR,
        "User cancelled authorization".to_owned(),
    ))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => out.push(b' '),
            b'%' if at + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        at += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            other => out.push(other),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The query (and fragment) parameters of a URL, decoded.
fn location_params(location: &str) -> Vec<(String, String)> {
    let rest = location.split_once('?').map_or("", |(_, rest)| rest);
    rest.split('#')
        .flat_map(|part| part.split('&'))
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (percent_decode(name), percent_decode(value)))
        .collect()
}

/// The sentinel at the end of the content is in view (the `intObserver`): the list does not scroll, or the scroll is
/// at its end.
pub(super) fn at_end(max_scroll: f32, scrolled: f32) -> bool {
    max_scroll <= 0. || max_scroll - scrolled <= END_SLACK
}

/// Whether the account's own `Authorize` and the Cancel button's deny may be posted from this build.
fn grants_allowed() -> bool {
    !cfg!(feature = "capture") || std::env::var_os("GRAVECORD_ALLOW_AUTHORIZE").is_some()
}

// --- the messages ------------------------------------------------------------------------------------------------

/// A message as the modal draws it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Seg {
    Text(String),
    Bold(String),
    Link(String, String),
}

fn flatten(parts: &[P], vars: &[(&str, String)]) -> String {
    parts
        .iter()
        .map(|part| match part {
            P::Text(text) => (*text).to_owned(),
            P::Var(name) | P::Number(name) | P::Date(name) => vars
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default(),
            P::Bold(inner) | P::Link(inner, _) => flatten(inner, vars),
        })
        .collect()
}

/// An ICU message with its variables, as runs of plain, bold and link text.
pub(super) fn segs(parts: &[P], vars: &[(&str, String)]) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut push = |seg: Seg| match (out.last_mut(), &seg) {
        (Some(Seg::Text(last)), Seg::Text(more)) => last.push_str(more),
        _ => out.push(seg),
    };
    for part in parts {
        match part {
            P::Bold(inner) => push(Seg::Bold(flatten(inner, vars))),
            P::Link(inner, url) => {
                let url = vars
                    .iter()
                    .find(|(key, _)| key == url)
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default();
                push(Seg::Link(flatten(inner, vars), url));
            }
            other => push(Seg::Text(flatten(std::slice::from_ref(other), vars))),
        }
    }
    out
}

pub(super) fn plain(parts: &[P], vars: &[(&str, String)]) -> String {
    flatten(parts, vars)
}

/// `rY` (647053): the lines one scope is listed as.
pub(super) fn scope_lines(scope: &str, scopes: &[&str]) -> Vec<String> {
    let lines =
        |parts: &[&[P]]| -> Vec<String> { parts.iter().map(|part| plain(part, &[])).collect() };
    match scope {
        "guilds.members.read" => {
            if scopes.contains(&"voice") {
                lines(&[spec::MEMBERS_VOICE])
            } else {
                lines(&[spec::MEMBERS])
            }
        }
        "sdk.social_layer_presence" | "sdk.social_layer" => {
            let mut out = if scope == "sdk.social_layer" {
                lines(&[
                    spec::SOCIAL_MANAGE_FRIENDS,
                    spec::SOCIAL_MEMBERSHIP,
                    spec::SOCIAL_FRIENDS_STATUS,
                    spec::SOCIAL_UPDATE_ACTIVITY,
                    spec::SOCIAL_MESSAGES,
                ])
            } else {
                lines(&[
                    spec::SOCIAL_MANAGE_FRIENDS,
                    spec::SOCIAL_FRIENDS_STATUS,
                    spec::SOCIAL_UPDATE_ACTIVITY,
                    spec::SOCIAL_GAME_INVITES,
                ])
            };
            if !scopes.contains(&"application_identities.write") {
                out.push(plain(spec::IDENTITIES_WRITE, &[]));
            }
            out
        }
        other => vec![
            spec::SIMPLE
                .iter()
                .find(|(name, _)| *name == other)
                .map_or_else(|| other.to_owned(), |(_, text)| (*text).to_owned()),
        ],
    }
}

/// One row of the list: the words, and whether it is the made-up "Not allowed" line.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ScopeRow {
    pub text: String,
    pub fake: bool,
}

/// `eU`: every account scope's lines, the direct-message line of a user install that creates commands, then the
/// made-up one. Empty when nothing is requested that the modal lists.
pub(super) fn scope_rows(
    accounts: &[&str],
    requested: &[String],
    integration: u64,
    fake: usize,
) -> Vec<ScopeRow> {
    if accounts.is_empty() {
        return Vec::new();
    }
    let mut rows: Vec<ScopeRow> = accounts
        .iter()
        .flat_map(|scope| scope_lines(scope, accounts))
        .map(|text| ScopeRow { text, fake: false })
        .collect();
    if integration == USER_INSTALL && requested.iter().any(|scope| scope == COMMANDS) {
        rows.push(ScopeRow {
            text: plain(spec::SEND_DMS, &[]),
            fake: false,
        });
    }
    rows.push(ScopeRow {
        text: spec::FAKE[fake % spec::FAKE.len()].to_owned(),
        fake: true,
    });
    rows
}

/// `eU`'s heading, `1Hz+Sl` (the developer of ...) or `PZpY9c` for an application Discord's own name is trusted for.
pub(super) fn scopes_heading(application: &str, trusted: bool) -> String {
    let message = if trusted {
        spec::ALLOW_TRUSTED
    } else {
        spec::ALLOW_DEVELOPER
    };
    plain(message, &[("application", application.to_owned())])
}

/// `i4`: what the application can do with messages.
pub(super) fn messages_line(requested: &[String]) -> &'static [P] {
    if requested
        .iter()
        .any(|scope| scope == "dm_channels.messages.write" || scope == "sdk.social_layer")
    {
        spec::CAN_READ_SEND
    } else if requested
        .iter()
        .any(|scope| spec::CAN_READ_MESSAGES.contains(&scope.as_str()))
    {
        spec::CAN_READ
    } else {
        spec::CANNOT_READ
    }
}

/// One line of the details under the scopes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub icon: &'static str,
    pub segs: Vec<Seg>,
}

fn flags_of(application: &Value) -> u64 {
    application["flags_new"]
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| application["flags_new"].as_u64())
        .or_else(|| application["flags"].as_u64())
        .unwrap_or(0)
}

/// A snowflake's creation time as `Feb 27, 2024` (`Intl` `date` default, en-US) in the time zone `zone`.
fn snowflake_date_in<Z: chrono::TimeZone>(id: &str, zone: &Z) -> String
where
    Z::Offset: std::fmt::Display,
{
    let millis = (id.parse::<u64>().unwrap_or(0) >> 22) as i64 + 1_420_070_400_000;
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|time| time.with_timezone(zone).format("%b %-d, %Y").to_string())
        .unwrap_or_default()
}

/// `em`: the details under the scopes. `disclosures` are the ids the application declared (1 IP, 2 advertisements).
pub(super) fn details(
    application: &Value,
    requested: &[String],
    disclosures: &[u64],
    guild_count: Option<u64>,
) -> Vec<Entry> {
    let name = application["name"].as_str().unwrap_or_default().to_owned();
    let url = |key: &str| application[key].as_str().map(str::to_owned);
    let (policy, terms) = (url("privacy_policy_url"), url("terms_of_service_url"));
    let parent = flags_of(application) & PARENT_FLAG != 0;
    let mut vars: Vec<(&str, String)> = vec![
        ("application", name.clone()),
        (
            "discordPrivacyPolicyURL",
            "https://discord.com/privacy".to_owned(),
        ),
    ];
    if let Some(policy) = &policy {
        vars.push(("privacyPolicyURL", policy.clone()));
    }
    if let Some(terms) = &terms {
        vars.push(("termsOfServiceURL", terms.clone()));
    }
    let privacy = match (&policy, &terms, parent) {
        (Some(_), Some(_), true) => spec::PRIVACY_BOTH_PARENT,
        (Some(_), Some(_), false) => spec::PRIVACY_BOTH,
        (Some(_), None, true) => spec::PRIVACY_POLICY_PARENT,
        (Some(_), None, false) => spec::PRIVACY_POLICY,
        (None, Some(_), true) => spec::PRIVACY_TERMS_PARENT,
        (None, Some(_), false) => spec::PRIVACY_TERMS,
        (None, None, true) => spec::PRIVACY_NONE_PARENT,
        (None, None, false) => spec::PRIVACY_NONE,
    };
    let created = snowflake_date_in(
        application["id"].as_str().unwrap_or_default(),
        &chrono::Local,
    );
    let mut out = vec![
        Entry {
            icon: "oauth_entry_lock",
            segs: segs(privacy, &vars),
        },
        Entry {
            icon: "oauth_entry_clock",
            segs: segs(spec::ACTIVE_SINCE, &[("date", created)]),
        },
    ];
    if let Some(count) = guild_count.filter(|_| requested.iter().any(|scope| scope == BOT)) {
        out.push(Entry {
            icon: "oauth_entry_robot",
            segs: segs(spec::USED_IN, &[("guildCount", group_digits(count))]),
        });
    }
    out.push(Entry {
        icon: "oauth_entry_shield",
        segs: segs(messages_line(requested), &[]),
    });
    for disclosure in disclosures {
        match disclosure {
            1 => out.push(Entry {
                icon: "oauth_entry_globe",
                segs: segs(spec::IP_LOCATION, &[]),
            }),
            2 => out.push(Entry {
                icon: "oauth_entry_embed",
                segs: segs(spec::ADVERTISEMENTS, &[]),
            }),
            _ => {}
        }
    }
    out
}

fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The message of a failed request (`t.message`, else `key: value` of the first field).
pub(super) fn failure_message(body: &Value) -> Option<String> {
    let body = &body["body"];
    if let Some(message) = body["message"].as_str().filter(|text| !text.is_empty()) {
        return Some(message.to_owned());
    }
    let (key, value) = body.as_object()?.iter().next()?;
    Some(format!(
        "{key}: {}",
        value
            .as_str()
            .map_or_else(|| value.to_string(), str::to_owned)
    ))
}

/// A guild install of `bot` or `applications.commands` needs a server chosen (`tg`), a webhook one more channel.
pub(super) fn needs_guild(requested: &[String], integration: u64) -> bool {
    integration == GUILD_INSTALL
        && requested
            .iter()
            .any(|scope| scope == BOT || scope == COMMANDS || scope == WEBHOOK)
}

// --- the flow -----------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(super) enum View {
    /// Requests are out; the modal is not shown yet.
    Loading,
    Consent,
    /// An error the modal shows (`title: Error`, the message as the subtitle, a Close button).
    Failed(String),
}

/// What a POST of `/oauth2/authorize` is for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Posting {
    Authorize,
    Deny,
    /// Renewal of a grant without a modal.
    Silent,
}

pub(crate) struct Flow {
    nonce: String,
    args: Args,
    requested: Vec<String>,
    integration: Option<u64>,
    get: Option<Value>,
    disclosures: Option<Value>,
    pub(super) view: View,
    posting: Option<Posting>,
    /// The `location` of a granted POST, kept while the disclosures are acknowledged.
    location: Option<String>,
    pub(super) scroll: ScrollHandle,
    reached: Rc<Cell<bool>>,
    fake: usize,
}

impl Flow {
    fn new(nonce: String, args: Args) -> Self {
        let requested = requested_scopes(&args.scopes);
        // The made-up line is random in the official client; the capture harness shows the golden's.
        let fake = if cfg!(feature = "capture") {
            6
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.subsec_nanos() as usize)
        };
        Self {
            nonce,
            args,
            requested,
            integration: None,
            get: None,
            disclosures: None,
            view: View::Loading,
            posting: None,
            location: None,
            scroll: ScrollHandle::new(),
            reached: Rc::new(Cell::new(false)),
            fake,
        }
    }

    pub(super) fn consent_open(&self) -> bool {
        matches!(self.view, View::Consent | View::Failed(_))
    }

    #[cfg(feature = "capture")]
    pub(super) fn describe(&self) -> String {
        format!(
            "view={:?} reached={} posting={:?}",
            self.view,
            self.reached.get(),
            self.posting
        )
    }
}

impl Root {
    fn oauth_location(&self) -> Location {
        let dm = self
            .stores
            .private_channels
            .iter()
            .find(|known| Some(known.id) == self.channel);
        let guild_channel = self.current_channel_for_oauth();
        Location {
            guild_id: self.guild.map(|guild| guild.to_string()),
            channel_id: self.channel.map(|channel| channel.to_string()),
            channel_type: dm.map(|dm| dm.kind).or(guild_channel),
        }
    }

    fn current_channel_for_oauth(&self) -> Option<u64> {
        let guild = self.stores.guild(self.guild?)?;
        let channel = guild
            .channels
            .iter()
            .find(|channel| Some(channel.id) == self.channel)?;
        Some(match channel.kind {
            dc_state::ChannelKind::Text => 0,
            dc_state::ChannelKind::Voice => 2,
            dc_state::ChannelKind::Category => 4,
            dc_state::ChannelKind::Other(kind) => kind,
        })
    }

    /// The SDK's `AUTHORIZE`: start the flow, or answer it at once when the request is unusable.
    pub(super) fn oauth_begin(&mut self, nonce: String, args: &Value, cx: &mut Context<Self>) {
        let args = match parse_args(args) {
            Ok(args) => args,
            Err(error) => {
                self.activity_reply("AUTHORIZE", &nonce, Err(error), cx);
                return;
            }
        };
        tracing::info!(
            scopes = args.scopes.len(),
            prompt = args.prompt.as_deref().unwrap_or("-"),
            "activity asked to authorize"
        );
        let client = args.client_id.clone();
        let known = args.integration_type;
        let mut flow = Flow::new(nonce.clone(), args);
        flow.integration = known;
        self.activities.oauth = Some(flow);
        match known {
            Some(integration) => self.oauth_send_gets(integration),
            None => {
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("activity:oauth-public:{nonce}")),
                    method: reqwest::Method::GET,
                    route: format!("/applications/{client}/public"),
                    body: None,
                });
            }
        }
        cx.notify();
    }

    fn oauth_send_gets(&mut self, integration: u64) {
        let Some(flow) = self.activities.oauth.as_mut() else {
            return;
        };
        flow.integration = Some(integration);
        let nonce = flow.nonce.clone();
        let route = authorize_get_route(&flow.args, integration);
        let client = flow.args.client_id.clone();
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("activity:oauth-get:{nonce}")),
            method: reqwest::Method::GET,
            route,
            body: None,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("activity:oauth-disc:{nonce}")),
            method: reqwest::Method::GET,
            route: format!("/applications/{client}/disclosures"),
            body: None,
        });
    }

    /// The network answers of the flow. `nonce` is the SDK command's.
    pub(super) fn oauth_fetched(
        &mut self,
        step: &str,
        nonce: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if self
            .activities
            .oauth
            .as_ref()
            .is_none_or(|flow| flow.nonce != nonce)
        {
            return;
        }
        let ok = body["ok"].as_bool() == Some(true);
        match step {
            "oauth-public" => {
                if !ok {
                    self.oauth_finish(
                        Err((
                            sdk::code::OAUTH2_ERROR,
                            "OAuth2 Authorization Error: Unknown Error".to_owned(),
                        )),
                        cx,
                    );
                    return;
                }
                let config = &body["body"]["integration_types_config"];
                let user = !config[USER_INSTALL.to_string()].is_null();
                self.oauth_send_gets(if user { USER_INSTALL } else { GUILD_INSTALL });
            }
            "oauth-get" | "oauth-disc" => {
                if !ok {
                    let why = failure_message(body).unwrap_or_else(|| "Unknown Error".to_owned());
                    self.oauth_finish(
                        Err((
                            sdk::code::OAUTH2_ERROR,
                            format!("OAuth2 Authorization Error: {why}"),
                        )),
                        cx,
                    );
                    return;
                }
                if let Some(flow) = self.activities.oauth.as_mut() {
                    if step == "oauth-get" {
                        flow.get = Some(body["body"].clone());
                    } else {
                        flow.disclosures = Some(body["body"].clone());
                    }
                }
                self.oauth_ready(cx);
            }
            "oauth-post" => self.oauth_posted(body, cx),
            "oauth-ack" => {
                // The acknowledgement's answer does not matter to the page.
                let location = self
                    .activities
                    .oauth
                    .as_mut()
                    .and_then(|flow| flow.location.take());
                if let Some(location) = location {
                    self.oauth_finish(reply_from_location(&location), cx);
                }
            }
            _ => {}
        }
    }

    /// Both GETs are in: the silent renewal, or the modal.
    fn oauth_ready(&mut self, cx: &mut Context<Self>) {
        let Some(flow) = self.activities.oauth.as_mut() else {
            return;
        };
        let (Some(get), Some(disclosures)) = (flow.get.clone(), flow.disclosures.clone()) else {
            return;
        };
        if flow.view != View::Loading {
            return;
        }
        let application = &get["application"];
        if let (Some(id), Some(hash)) = (application["id"].as_str(), application["icon"].as_str()) {
            let (id, hash) = (id.to_owned(), hash.to_owned());
            self.request_image(
                format!("shelf-icon:{id}"),
                format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=128"),
            );
        }
        let Some(flow) = self.activities.oauth.as_mut() else {
            return;
        };
        let renew = flow.args.prompt.as_deref() == Some("none")
            && get["authorized"].as_bool() == Some(true)
            && disclosures["all_acked"].as_bool() == Some(true);
        if renew && grants_allowed() {
            flow.posting = Some(Posting::Silent);
            self.oauth_post(true, cx);
            return;
        }
        if renew {
            tracing::debug!("silent renewal not sent from a capture build; the modal is shown");
        }
        flow.view = match scope_error(&flow.requested) {
            Some(message) => View::Failed(message),
            None => View::Consent,
        };
        cx.notify();
    }

    fn oauth_post(&mut self, authorize: bool, cx: &mut Context<Self>) {
        let location = self.oauth_location();
        let Some(flow) = self.activities.oauth.as_mut() else {
            return;
        };
        let integration = flow.integration.unwrap_or(GUILD_INSTALL);
        let guild = flow.args.guild_id.clone();
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("activity:oauth-post:{}", flow.nonce)),
            method: reqwest::Method::POST,
            route: authorize_post_route(&flow.args, &flow.requested),
            body: Some(post_body(
                authorize,
                integration,
                flow.args.permissions,
                guild.as_deref(),
                &location,
            )),
        });
        cx.notify();
    }

    fn oauth_posted(&mut self, body: &Value, cx: &mut Context<Self>) {
        let ok = body["ok"].as_bool() == Some(true);
        let Some(flow) = self.activities.oauth.as_mut() else {
            return;
        };
        let posting = flow.posting.take();
        let location = body["body"]["location"].as_str().map(str::to_owned);
        match (ok, location, posting) {
            (true, Some(location), Some(Posting::Authorize | Posting::Silent)) => {
                // The disclosures are acknowledged after a grant (`Yx`), then the page gets its code.
                let ids: Vec<Value> = flow
                    .disclosures
                    .as_ref()
                    .and_then(|body| body["disclosures"].as_array().cloned())
                    .unwrap_or_default();
                flow.location = Some(location);
                let (nonce, client) = (flow.nonce.clone(), flow.args.client_id.clone());
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("activity:oauth-ack:{nonce}")),
                    method: reqwest::Method::POST,
                    route: format!("/applications/{client}/disclosures"),
                    body: Some(json!({ "disclosures": ids })),
                });
            }
            (true, Some(location), _) => self.oauth_finish(reply_from_location(&location), cx),
            (true, None, _) => self.oauth_finish(
                Err((
                    sdk::code::OAUTH2_ERROR,
                    "OAuth2 Error: No location provided".to_owned(),
                )),
                cx,
            ),
            (false, _, Some(Posting::Silent)) => {
                let why = failure_message(body).unwrap_or_else(|| "Unknown Error".to_owned());
                self.oauth_finish(
                    Err((
                        sdk::code::OAUTH2_ERROR,
                        format!("OAuth2 Authorize Error: {why}"),
                    )),
                    cx,
                );
            }
            (false, _, _) => {
                // The modal stays, with the error in place of its body.
                flow.view = View::Failed(
                    failure_message(body).unwrap_or_else(|| plain(spec::ERROR_GENERIC, &[])),
                );
                cx.notify();
            }
        }
    }

    /// Ends the flow with the SDK's answer.
    fn oauth_finish(&mut self, result: Result<Value, (u32, String)>, cx: &mut Context<Self>) {
        let Some(flow) = self.activities.oauth.take() else {
            return;
        };
        match &result {
            Ok(_) => tracing::info!("authorization answered: code handed to the activity"),
            Err((code, message)) => {
                tracing::info!(code, message = %message, "authorization answered: error")
            }
        }
        self.activity_reply("AUTHORIZE", &flow.nonce, result, cx);
        cx.notify();
    }

    /// The close button, the scrim and Escape.
    pub(super) fn oauth_close(&mut self, cx: &mut Context<Self>) {
        if self.activities.oauth.is_some() {
            self.oauth_finish(cancel_reply(), cx);
        }
    }

    /// Escape: true when the flow was open.
    pub(super) fn oauth_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self
            .activities
            .oauth
            .as_ref()
            .is_some_and(Flow::consent_open)
        {
            self.oauth_close(cx);
            return true;
        }
        false
    }

    /// The Cancel button: the deny is posted and the page told what comes back; a capture build answers as the close
    /// button does.
    pub(super) fn oauth_cancel(&mut self, cx: &mut Context<Self>) {
        if !grants_allowed() {
            tracing::info!("cancel: the deny is not posted from a capture build");
            self.oauth_finish(cancel_reply(), cx);
            return;
        }
        if let Some(flow) = self.activities.oauth.as_mut() {
            flow.posting = Some(Posting::Deny);
        }
        self.oauth_post(false, cx);
    }

    /// The Authorize button, only ever by the owner's own click.
    pub(super) fn oauth_authorize(&mut self, cx: &mut Context<Self>) {
        if !grants_allowed() {
            tracing::info!("authorize: nothing is posted from a capture build");
            return;
        }
        if let Some(flow) = self.activities.oauth.as_mut() {
            if flow.posting.is_some() {
                return;
            }
            flow.posting = Some(Posting::Authorize);
        }
        self.oauth_post(true, cx);
    }

    /// Whether the consent modal (or its error) is on screen.
    pub(super) fn oauth_open(&self) -> bool {
        self.activities
            .oauth
            .as_ref()
            .is_some_and(Flow::consent_open)
    }
}

// --- the modal -----------------------------------------------------------------------------------------------------

fn rich(
    id: &'static str,
    segs: &[Seg],
    link_color: gpui::Hsla,
    cx: &mut Context<Root>,
) -> AnyElement {
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut links: Vec<(std::ops::Range<usize>, String, String)> = Vec::new();
    for seg in segs {
        let start = text.len();
        match seg {
            Seg::Text(more) => text.push_str(more),
            Seg::Bold(more) => {
                text.push_str(more);
                highlights.push((
                    start..text.len(),
                    HighlightStyle {
                        font_weight: Some(FontWeight(600.)),
                        ..Default::default()
                    },
                ));
            }
            Seg::Link(more, url) => {
                text.push_str(more);
                highlights.push((
                    start..text.len(),
                    HighlightStyle {
                        color: Some(link_color),
                        ..Default::default()
                    },
                ));
                links.push((start..text.len(), url.clone(), more.clone()));
            }
        }
    }
    let ranges: Vec<_> = links.iter().map(|(range, _, _)| range.clone()).collect();
    let root = cx.entity().downgrade();
    InteractiveText::new(id, StyledText::new(text).with_highlights(highlights))
        .on_click(ranges, move |at, _, cx| {
            if let Some((_, url, label)) = links.get(at) {
                let (url, label) = (url.clone(), label.clone());
                let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
            }
        })
        .into_any_element()
}

fn footer_button(
    id: &'static str,
    label: SharedString,
    primary: bool,
    disabled: bool,
    lead: Option<&'static str>,
    cx: &mut Context<Root>,
    run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
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
        .when(disabled, |this| this.opacity(0.5))
        .flex_1()
        .h(px(40.))
        .rounded(px(8.))
        .border_1()
        .border_color(border)
        .bg(background)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .when(!disabled, |this| this.cursor_pointer())
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(text)
        .children(lead.map(|name| icon(name, 20., "--white")))
        .child(label)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !disabled {
                run(this, cx);
            }
            cx.notify();
        }))
        .into_any_element()
}

impl Root {
    /// The consent modal, or its error.
    pub(super) fn oauth_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let flow = self.activities.oauth.as_ref()?;
        if let View::Failed(message) = &flow.view {
            let message = message.clone();
            return Some(super::modal::render(
                super::modal::Spec {
                    id: "oauth-error",
                    title: plain(spec::ERROR, &[]).into(),
                    subtitle: Some(div().child(message).into_any_element()),
                    body: None,
                    buttons: vec![super::modal::button(
                        "oauth-error-close",
                        plain(spec::CLOSE, &[]),
                        super::modal::Tone::Secondary,
                        |this, _, cx| this.oauth_close(cx),
                    )],
                    close: Box::new(|this, _, cx| this.oauth_close(cx)),
                },
                cx,
            ));
        }
        if flow.view != View::Consent {
            return None;
        }
        let get = flow.get.clone()?;
        let application = get["application"].clone();
        let name = application["name"].as_str().unwrap_or_default().to_owned();
        let integration = flow.integration.unwrap_or(GUILD_INSTALL);
        let requested = flow.requested.clone();
        let accounts = account_scopes(&requested);
        let rows = scope_rows(&accounts, &requested, integration, flow.fake);
        let trusted = flags_of(&application) & PARENT_FLAG != 0;
        let disclosures: Vec<u64> = flow
            .disclosures
            .as_ref()
            .and_then(|body| body["disclosures"].as_array())
            .map(|items| items.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        let guild_count = application["approximate_guild_count"]
            .as_u64()
            .or_else(|| get["bot"]["approximate_guild_count"].as_u64());
        let entries = details(&application, &requested, &disclosures, guild_count);
        let needs_guild = needs_guild(&requested, integration) && {
            // The server the request names, when the account may manage it.
            let named = flow.args.guild_id.as_deref();
            !named.is_some_and(|id| {
                get["guilds"].as_array().is_some_and(|guilds| {
                    guilds.iter().any(|guild| guild["id"].as_str() == Some(id))
                })
            })
        };
        let busy = flow.posting.is_some();
        let (scroll, reached) = (flow.scroll.clone(), flow.reached.clone());
        let has_scopes = !rows.is_empty();

        let app_id = application["id"].as_str().unwrap_or_default().to_owned();
        let app_icon = self.images.get(&format!("shelf-icon:{app_id}")).cloned();
        let me_avatar = self
            .stores
            .current_user
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let avatar = |picture: Option<std::sync::Arc<gpui::Image>>| {
            div()
                .size(px(82.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(80.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(color("--background-mod-subtle"))
                        .children(picture.map(|picture| img(picture).size(px(80.)).rounded_full())),
                )
        };
        let dots = div()
            .w(px(24.))
            .h(px(4.))
            .mx(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .children((0..3).map(|_| {
                div()
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--channels-default"))
            }));
        let head_inner = div()
            .parity("oauth-header")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .mb(px(16.))
                    .flex()
                    .items_center()
                    .child(avatar(app_icon))
                    .child(dots)
                    .child(avatar(me_avatar)),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(name.clone()),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(if has_scopes {
                        plain(spec::WANTS_ACCESS, &[])
                    } else {
                        plain(spec::CONNECT_TO_DISCORD, &[])
                    }),
            );
        let head = div().w_full().flex().justify_center().child(head_inner);
        let scope_list = div()
            .parity("oauth-scopes")
            .w_full()
            .flex()
            .flex_col()
            .pb(px(25.))
            .children(has_scopes.then(|| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(scopes_heading(&name, trusted))
            }))
            .children(rows.into_iter().map(|row| {
                div()
                    .mt(px(16.))
                    .flex()
                    .gap(px(12.))
                    .child(icon(
                        if row.fake {
                            "oauth_fake_x"
                        } else {
                            "circle_check"
                        },
                        20.,
                        "--text-muted",
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color(if row.fake {
                                "--text-muted"
                            } else {
                                "--text-default"
                            }))
                            .child(row.text),
                    )
            }));
        let link_color: gpui::Hsla = color("--text-link").into();
        let detail_list = div()
            .parity("oauth-details")
            .w_full()
            .pt(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .children(entries.into_iter().enumerate().map(|(at, entry)| {
                const IDS: [&str; 8] = [
                    "oauth-entry-0",
                    "oauth-entry-1",
                    "oauth-entry-2",
                    "oauth-entry-3",
                    "oauth-entry-4",
                    "oauth-entry-5",
                    "oauth-entry-6",
                    "oauth-entry-7",
                ];
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(entry.icon, 16., "--text-muted"))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(rich(IDS[at.min(7)], &entry.segs, link_color, cx)),
                    )
            }));
        let content = div()
            .w_full()
            .p(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-mod-subtle"))
            .flex()
            .flex_col()
            .child(scope_list)
            .child(detail_list);
        let authorize = div()
            .w(px(430.))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(head)
            .child(content);

        let scrolled = scroll.clone();
        let seen = reached.clone();
        let sensor = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let laid_out = scrolled.bounds().size.height > px(0.);
                let end = at_end(
                    f32::from(scrolled.max_offset().y),
                    f32::from(-scrolled.offset().y),
                );
                if laid_out && end && !seen.get() {
                    seen.set(true);
                    window.refresh();
                }
            },
        )
        .absolute()
        .size_0();
        let body = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .child(
                div()
                    .id("oauth-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .w(px(462.))
                            .pl(px(24.))
                            .pt(px(12.))
                            .pb(px(12.))
                            .child(authorize),
                    ),
            )
            .child(super::settings::auto_scrollbar(scroll.clone()))
            .child(sensor);

        let at_bottom = reached.get();
        let primary = if !at_bottom {
            footer_button(
                "oauth-keep-scrolling",
                plain(spec::KEEP_SCROLLING, &[]).into(),
                true,
                true,
                Some("oauth_arrow_down"),
                cx,
                |_, _| {},
            )
        } else if needs_guild {
            footer_button(
                "oauth-authorize",
                plain(spec::MISSED_FIELDS, &[]).into(),
                true,
                true,
                None,
                cx,
                |_, _| {},
            )
        } else {
            footer_button(
                "oauth-authorize",
                plain(spec::AUTHORIZE, &[]).into(),
                true,
                busy,
                None,
                cx,
                |this, cx| this.oauth_authorize(cx),
            )
        };
        let cancel = footer_button(
            "oauth-cancel",
            plain(spec::CANCEL, &[]).into(),
            false,
            busy,
            None,
            cx,
            |this, cx| this.oauth_cancel(cx),
        );
        let height = (f32::from(window.viewport_size().height) - 112.).max(300.);
        let modal = div()
            .parity_with_border("oauth-modal", 1.)
            .id("oauth-modal")
            .occlude()
            .w(px(480.))
            .h(px(height))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div().relative().h(px(48.)).flex_none().child(
                    div()
                        .id("oauth-close")
                        .absolute()
                        .top_0()
                        .right(px(8.))
                        .size(px(40.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.oauth_close(cx);
                            cx.notify();
                        })),
                ),
            )
            .child(div().h(px(16.)).flex_none())
            .child(body)
            .child(div().h(px(9.)).flex_none())
            .child(
                div()
                    .h(px(64.))
                    .flex_none()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .gap(px(8.))
                    .child(cancel)
                    .child(primary),
            );
        Some(
            deferred(
                div()
                    .id("oauth-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.oauth_close(cx);
                        cx.notify();
                    }))
                    .child(modal),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn wordle() -> Args {
        parse_args(&json!({
            "client_id": "880",
            "response_type": "code",
            "state": "",
            "prompt": "none",
            "scope": ["identify", "guilds", "guilds.members.read", "rpc.activities.write", "applications.commands"],
        }))
        .unwrap()
    }

    #[test]
    fn the_scope_list_follows_the_official_order_and_wording() {
        let requested = requested_scopes(&wordle().scopes);
        let accounts = account_scopes(&requested);
        assert_eq!(
            accounts,
            [
                "identify",
                "guilds",
                "guilds.members.read",
                "rpc.activities.write",
                "applications.commands"
            ]
        );
        // The golden: Wordle, a user install, the made-up line last.
        let rows = scope_rows(&accounts, &requested, USER_INSTALL, 6);
        let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Access your username, avatar, and banner",
                "Know what servers you're in",
                "Read your member info (nickname, avatar, roles, etc...) for servers you belong to",
                "Set your current activity",
                "Create commands",
                "Send you direct messages",
                "Paint a happy little tree",
            ]
        );
        assert!(rows.last().unwrap().fake && !rows[0].fake);
        // A guild install has no direct-message line.
        assert_eq!(scope_rows(&accounts, &requested, GUILD_INSTALL, 6).len(), 6);
        assert_eq!(
            scopes_heading("Wordle", false),
            "This will allow the developer of Wordle to:"
        );
    }

    #[test]
    fn members_line_changes_with_the_voice_scope() {
        let lines = scope_lines("guilds.members.read", &["guilds.members.read", "voice"]);
        assert!(lines[0].starts_with("Read member info"));
    }

    #[test]
    fn scopes_are_filtered_and_checked_like_the_modal() {
        assert_eq!(
            requested_scopes(&strings(&["rpc.api", "bot"])),
            ["bot", "applications.commands"]
        );
        assert_eq!(
            scope_error(&[]),
            Some("No scopes were provided.".to_owned())
        );
        assert_eq!(
            scope_error(&strings(&["identify", "nonsense"])),
            Some("Invalid scope: nonsense".to_owned())
        );
        assert_eq!(
            scope_error(&strings(&["identify", "webhook.incoming"])),
            None
        );
        // Nothing the modal lists: no scopes card at all.
        assert!(scope_rows(&account_scopes(&strings(&["webhook.incoming"])), &[], 0, 0).is_empty());
    }

    #[test]
    fn keep_scrolling_turns_into_authorize_only_at_the_end() {
        // The golden: 294px to scroll.
        assert!(!at_end(294., 0.));
        assert!(!at_end(294., 200.));
        assert!(!at_end(294., 256.));
        assert!(at_end(294., 257.));
        assert!(at_end(294., 294.));
        // A list that does not scroll shows the sentinel at once.
        assert!(at_end(0., 0.));
    }

    #[test]
    fn requests_carry_the_official_query() {
        let args = parse_args(&json!({
            "client_id": "880",
            "response_type": "code",
            "state": "a b",
            "code_challenge": "abc",
            "code_challenge_method": "S256",
            "scope": "identify guilds",
            "prompt": "none",
        }))
        .unwrap();
        assert_eq!(
            authorize_get_route(&args, USER_INSTALL),
            "/oauth2/authorize?client_id=880&response_type=code&code_challenge=abc&code_challenge_method=S256&scope=identify%20guilds&state=a%20b&integration_type=1"
        );
        assert_eq!(
            authorize_post_route(&args, &strings(&["identify"])),
            "/oauth2/authorize?client_id=880&response_type=code&code_challenge=abc&code_challenge_method=S256&scope=identify&state=a%20b"
        );
    }

    #[test]
    fn request_arguments_are_parsed_like_di() {
        assert_eq!(
            parse_args(&json!({"scope": ["identify"]})).unwrap_err().1,
            "No Client ID provided"
        );
        assert_eq!(
            parse_args(&json!({"client_id": "1", "redirect_uri": "https://x"}))
                .unwrap_err()
                .1,
            "Redirect URI cannot be used in the RPC OAuth2 Authorization flow"
        );
        // `scopes` wins over `scope` when it is not empty.
        let args = parse_args(&json!({"client_id": "1", "scopes": "a b", "scope": ["c"]})).unwrap();
        assert_eq!(args.scopes, ["a", "b"]);
        assert_eq!(args.response_type, "code");
        let args = parse_args(
            &json!({"client_id": "1", "scopes": "", "scope": ["c"], "integration_type": 1}),
        )
        .unwrap();
        assert_eq!(
            (args.scopes, args.integration_type),
            (strings(&["c"]), Some(1))
        );
    }

    #[test]
    fn the_post_body_is_the_official_one() {
        let somewhere = Location {
            guild_id: Some("10".to_owned()),
            channel_id: Some("20".to_owned()),
            channel_type: Some(2),
        };
        assert_eq!(
            post_body(true, USER_INSTALL, 0, Some("99"), &somewhere),
            json!({
                "permissions": "0",
                "authorize": true,
                "integration_type": 1,
                "location_context": {"guild_id": "10", "channel_id": "20", "channel_type": 2},
            })
        );
        // A guild install names its server; with no place the context is 10000.
        let nowhere = Location {
            guild_id: None,
            channel_id: None,
            channel_type: None,
        };
        assert_eq!(
            post_body(false, GUILD_INSTALL, 8, Some("99"), &nowhere),
            json!({
                "permissions": "8",
                "authorize": false,
                "integration_type": 0,
                "guild_id": "99",
                "location_context": {"guild_id": "10000", "channel_id": "10000", "channel_type": 10000},
            })
        );
    }

    #[test]
    fn the_location_decides_the_sdk_reply() {
        assert_eq!(
            reply_from_location("https://discord.com/oauth2/authorized?code=abc%20d&state=1")
                .unwrap(),
            json!({"code": "abc d"})
        );
        assert_eq!(
            reply_from_location(
                "https://x/?error=access_denied&error_description=The+resource+owner+denied"
            )
            .unwrap_err()
            .1,
            "OAuth2 Error: access_denied: The resource owner denied"
        );
        assert_eq!(
            reply_from_location("https://x/?state=1").unwrap_err().1,
            "OAuth2 Error: Unable to find auth code"
        );
    }

    #[test]
    fn cancel_answers_with_the_official_error() {
        assert_eq!(
            cancel_reply(),
            Err((5000, "User cancelled authorization".to_owned()))
        );
    }

    #[test]
    fn an_authorized_prompt_none_request_skips_the_modal() {
        // `DI`: prompt none, `authorized` and every disclosure acknowledged.
        let args = wordle();
        assert_eq!(args.prompt.as_deref(), Some("none"));
        let get = json!({"authorized": true});
        let disclosures = json!({"all_acked": true});
        let renew = args.prompt.as_deref() == Some("none")
            && get["authorized"].as_bool() == Some(true)
            && disclosures["all_acked"].as_bool() == Some(true);
        assert!(renew);
        // A capture build never sends it.
        assert_eq!(grants_allowed(), !cfg!(feature = "capture"));
    }

    #[test]
    fn the_details_match_the_golden() {
        let application = json!({
            "id": "1211781489931452447",
            "name": "Wordle",
            "privacy_policy_url": "https://example.com/p",
            "terms_of_service_url": "https://example.com/t",
            "flags": 0,
        });
        let entries = details(&application, &strings(&["identify"]), &[], None);
        let plain_text = |entry: &Entry| {
            entry
                .segs
                .iter()
                .map(|seg| match seg {
                    Seg::Text(text) | Seg::Bold(text) | Seg::Link(text, _) => text.as_str(),
                })
                .collect::<String>()
        };
        assert_eq!(
            plain_text(&entries[0]),
            "The developer of Wordle's privacy policy and terms of service apply to this application. Discord's Privacy Policy applies to data it receives."
        );
        assert!(
            matches!(&entries[0].segs[1], Seg::Link(text, url) if text == "privacy policy" && url == "https://example.com/p")
        );
        // The golden was made in UTC+3: the id is Feb 26, 21:00 UTC.
        let zone = chrono::FixedOffset::east_opt(3 * 3600).unwrap();
        assert_eq!(
            snowflake_date_in("1211781489931452447", &zone),
            "Feb 27, 2024"
        );
        assert!(plain_text(&entries[1]).starts_with("Active since Feb 2"));
        assert_eq!(
            plain_text(&entries[2]),
            "This application cannot read your messages or send messages as you."
        );
        assert_eq!(entries[2].segs[1], Seg::Bold("cannot".to_owned()));
        assert_eq!(entries.len(), 3);
        // A bot's server count and the disclosures follow.
        let more = details(&application, &strings(&["bot"]), &[1, 2], Some(12345));
        assert_eq!(plain_text(&more[2]), "Used in 12,345 servers");
        assert_eq!(more.len(), 6);
    }

    #[test]
    fn a_guild_install_of_commands_needs_a_server() {
        assert!(needs_guild(
            &strings(&["applications.commands"]),
            GUILD_INSTALL
        ));
        assert!(!needs_guild(
            &strings(&["applications.commands"]),
            USER_INSTALL
        ));
        assert!(!needs_guild(&strings(&["identify"]), GUILD_INSTALL));
    }

    #[test]
    fn failures_read_like_the_official_error_text() {
        assert_eq!(
            failure_message(&json!({"body": {"message": "Unknown Application", "code": 10002}})),
            Some("Unknown Application".to_owned())
        );
        assert_eq!(
            failure_message(&json!({"body": {"scope": "bad"}})),
            Some("scope: bad".to_owned())
        );
    }
}
