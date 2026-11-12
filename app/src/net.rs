//! The network side. Runs on tokio and talks to the UI thread over channels, so gpui never blocks on I/O.

use dc_gateway::{Command, Config, Event, spawn};
use dc_http::Client;
use dc_proto::identity::{IdentifyState, Identity};
use dc_proto::op::Op;
use dc_proto::pin;
use dc_proto::secrets::Token;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::warn;

pub enum Update {
    /// What the last run left (`ui/start_cache.rs`), applied before the gateway answers.
    Cached(Box<crate::ui::start_cache::Cached>),
    GatewayConnected(bool),
    /// The local RPC server: a game connected, set or cleared its activity, or left (`dc-rpc`).
    Rpc(dc_rpc::Event),
    /// One pass of the game scan (`games`).
    Games(Box<crate::games::GameScan>),
    NotificationClicked(u64),
    Tray(crate::tray::Action),
    /// The updater's state changed (`updater::state`): the title bar button follows it.
    Updater,
    /// The connecting screen's spinner, fetched and decoded.
    Spinner(Arc<crate::spinner::Clip>),
    /// `None` resets from the channel's interval; a value sets the server's remaining cooldown.
    Slowmode {
        channel: u64,
        kind: dc_state::slowmode::Kind,
        millis: Option<u64>,
    },
    SlowmodeRejected {
        channel: u64,
        draft: RejectedDraft,
        reason: Option<String>,
    },
    /// The end of a [`Request::SendLocal`], after the retries: the message the server made, or why it did not.
    SendResult {
        channel: u64,
        nonce: String,
        outcome: crate::send_policy::Outcome,
    },
    /// How much of file `index` of a [`Request::SendLocal`] has gone up (the placeholder row's progress bar).
    SendProgress {
        nonce: String,
        index: usize,
        sent: u64,
        total: u64,
    },
    Dispatch {
        name: String,
        data: Value,
    },
    Messages {
        channel: u64,
        page: Value,
        /// When the request started, in Unix milliseconds: messages that arrived since stay in the window.
        started_ms: u64,
    },
    /// A `before`, `after` or `around` page (`fetchMessages`), with what it was asked for.
    MessagesLoaded {
        spec: dc_state::message_window::FetchSpec,
        page: Value,
    },
    /// `LOAD_MESSAGES_FAILURE`.
    MessagesFailed {
        channel: u64,
    },
    Image {
        key: String,
        bytes: Vec<u8>,
    },
    /// PNG images in bulk, such as the cells of the emoji sprite sheet.
    Images(Vec<(String, Vec<u8>)>),
    /// An event of the voice connection.
    Voice(dc_voice::Event),
    /// The system-wide push to talk keybind went down or up.
    PushToTalk(bool),
    /// The system-wide `SAVE_CLIP` keybind went down.
    ClipSaveKey,
    /// A custom keybind (`KeybindsStore`) went down or up.
    Keybind {
        id: String,
        down: bool,
    },
    /// An event of a Go Live stream's own voice connection.
    Stream {
        key: dc_voice::streams::StreamKey,
        event: dc_voice::StreamEvent,
    },
    /// A `Request::Image` that could not be downloaded.
    ImageFailed {
        key: String,
    },
    /// The body of a `Request::Fetch`. A failed fetch arrives here too, with the body `fetch_failure` builds.
    Fetched {
        key: String,
        body: Value,
    },
    /// The gateway closed with 4004 or a request came back 401: the stored login is dead.
    AuthFailed,
    /// `PATCH /users/@me` answered with a new token (a password change): the shell keeps it where the login is
    /// kept (`UPDATE_TOKEN`, module 631670 `KD`).
    TokenRenewed(Token),
}

pub enum Request {
    /// Op 3 `PRESENCE_UPDATE` with the account's status and activities (`presenceUpdate` in module 868). Refused in
    /// a harness run (`games::presence_refused`).
    Presence {
        status: String,
        activities: Vec<Value>,
        /// When the account's idle began, zero while it is active (`getIdleSince`, module 885576).
        since: u64,
        afk: bool,
    },
    /// Op 4 `VOICE_STATE_UPDATE`, as the bundle's `voiceStateUpdate` writes it. No channel leaves voice.
    VoiceState {
        guild: u64,
        channel: Option<u64>,
        self_mute: bool,
        self_deaf: bool,
        self_video: bool,
        flags: u64,
    },
    /// A stream op on the main gateway: 18 create, 19 delete, 20 watch, 21 ping, 22 set paused (the payloads come
    /// from `dc_voice::streams`).
    StreamOp {
        op: Op,
        data: Value,
    },
    /// Opens the second voice connection of a stream. Its events come back as `Update::Stream`; a sender's preview
    /// thumbnails are uploaded here (`POST /streams/<key>/preview`).
    StreamConnect {
        join: dc_voice::streams::StreamJoin,
        source: Option<dc_voice::StreamSource>,
        hub: dc_voice::VideoHub,
        controls: dc_voice::StreamControls,
    },
    /// Opens the voice connection once the gateway named the server.
    VoiceConnect {
        join: dc_voice::Join,
        controls: dc_voice::Controls,
    },
    /// Capture only (`webhook demo`): makes a webhook in the fixture channel, posts the component messages of
    /// `webhook_demo` through it, deletes it, and answers under `webhook-demo` with the message ids. The address of
    /// the webhook never leaves this task.
    #[cfg(feature = "capture")]
    WebhookDemo {
        channel: u64,
    },
    /// POST a form with a file and report like `Rest`.
    RestForm {
        key: String,
        route: String,
        form: dc_http::Form,
    },
    /// Upload Logs (Voice & Video > Advanced): the log files and a `media_engine_state.json` as one multipart body to
    /// `/debug-logs/multi/1` (`uploadDebugFiles`, category RTC). The outcome comes back under `voice-logs-upload` as
    /// `{"ok", "status", "body", "count"}`, `count` being the parts sent.
    UploadLogs {
        files: Vec<std::path::PathBuf>,
        state: Value,
    },
    /// `POST /users/@me/scheduled-messages` (`createScheduledMessage` in module 151282): the message the composer
    /// holds, scheduled instead of sent. Files are uploaded first, as for a sent message. The outcome comes back under
    /// `scheduled-create` shaped like `Rest`'s.
    Schedule {
        channel: u64,
        content: String,
        files: Vec<Upload>,
        reply: Option<(Option<u64>, u64, bool)>,
        sticker_ids: Vec<u64>,
        /// ISO 8601 UTC with milliseconds, as `toISOString()` writes it.
        timestamp: String,
    },
    /// GET a route and hand its JSON back under `key`.
    Fetch {
        key: String,
        route: String,
    },
    Typing {
        channel: u64,
    },
    React {
        channel: u64,
        message: u64,
        emoji: String,
        add: bool,
        /// The `location` query: where the reaction was given (`qN` in the bundle), e.g. `Message`.
        location: &'static str,
        /// A Super Reaction: `type=1` instead of `0`.
        burst: bool,
    },
    LoadMessages {
        channel: u64,
    },
    /// `fetchMessages` for a page `before` or `after` the window, or `around` a message (`jumpToMessage`).
    FetchMessages {
        spec: dc_state::message_window::FetchSpec,
    },
    Send {
        channel: u64,
        content: String,
        /// Files to upload first, sent with the message.
        files: Vec<Upload>,
        /// Guild, if any, and the message being answered.
        /// The guild, the message, and whether the reply pings its author (the reply bar's @ switch).
        reply: Option<(Option<u64>, u64, bool)>,
    },
    /// A message the view already shows (`ui/send_state.rs`): it carries the nonce of that local row, is sent with the
    /// official retries (`send_policy.rs`) and ends as `Update::SendResult`.
    SendLocal {
        nonce: String,
        channel: u64,
        content: String,
        files: Vec<Upload>,
        reply: Option<(Option<u64>, u64, bool)>,
        sticker_ids: Vec<u64>,
        /// Guild, channel and id of the message a forward points at.
        forward: Option<ForwardRef>,
    },
    /// A voice message: the recording is uploaded, then a message with an empty content, the voice flag (8192) and the
    /// attachment's `duration_secs` and `waveform` is created (`em` in module 841817). The outcome is
    /// `Update::Fetched { key, body }` with `{"ok", "status", "body"}`.
    SendVoice {
        key: String,
        channel: u64,
        file: Vec<u8>,
        duration_secs: f64,
        waveform: String,
        reply: Option<(Option<u64>, u64, bool)>,
    },
    Edit {
        channel: u64,
        message: u64,
        content: String,
    },
    Delete {
        channel: u64,
        message: u64,
    },
    /// `POST /channels/{channel}/messages/{message}/ack`: `{manual, mention_count}` for Mark Unread, else
    /// `{token, last_viewed}` (days since 2015-01-01), as recorded in TRAFFIC/bookmark-unread and mark-read.
    Ack {
        channel: u64,
        message: u64,
        manual: bool,
        mention_count: u64,
        last_viewed: u64,
    },
    /// `PUT`/`DELETE /users/@me/saved-messages/{channel}/{message}`: a bookmark, or a reminder with `due_at`.
    Save {
        channel: u64,
        message: u64,
        due_at: Option<String>,
        save: bool,
    },
    /// `PATCH /users/@me/guilds/settings`: notification levels and mutes.
    GuildSettings {
        body: Value,
    },
    /// Any other REST call. With a `key`, the outcome comes back as `Update::Fetched { key, body }` where `body`
    /// is `{"ok": bool, "status": u16, "body": <response>}`.
    Rest {
        key: Option<String>,
        method: reqwest::Method,
        route: String,
        body: Option<Value>,
    },
    /// Op 31 `REQUEST_SOUNDBOARD_SOUNDS`: the soundboard sounds of these guilds, answered by `SOUNDBOARD_SOUNDS`
    /// dispatches.
    SoundboardSounds {
        guilds: Vec<u64>,
    },
    /// Op 8 `REQUEST_GUILD_MEMBERS`, answered by `GUILD_MEMBERS_CHUNK` dispatches: members whose name starts with
    /// `query` (at most `limit`), or the members with these ids (`GUILD_MEMBERS_REQUEST` in module 225123 sends ids
    /// in chunks of 100). Presences are asked for as the client does.
    GuildMembers {
        guilds: Vec<u64>,
        query: String,
        limit: u64,
        user_ids: Vec<u64>,
    },
    /// Saves a file (a CDN URL, no authorization) into the user's Downloads folder under a free name.
    Download {
        url: String,
        filename: String,
        /// When set, the outcome comes back as `Update::Fetched { key, body: {"ok": bool} }`.
        done: Option<String>,
    },
    /// `GET` with an application's own access token as the bearer (never the account token). The outcome comes
    /// back as `Update::Fetched { key, body }` shaped like `Request::Rest`'s.
    BearerGet {
        key: String,
        route: String,
        bearer: String,
    },
    /// `POST /interactions`: an application command run by hand.
    Interaction {
        body: Value,
    },
    /// `PIN`/unpin: `PUT` or `DELETE /channels/{channel}/messages/pins/{message}`.
    Pin {
        channel: u64,
        message: u64,
        pin: bool,
    },
    Subscribe {
        guild: u64,
        channel: u64,
    },
    /// Op 28 `REQUEST_FORUM_UNREADS` (module 868 `requestForumUnreads`): the unread counts of a forum's posts, as
    /// (thread, acked message) pairs. The answer is the `FORUM_UNREADS` dispatch.
    ForumUnreads {
        guild: u64,
        channel: u64,
        threads: Vec<(u64, u64)>,
    },
    Image {
        key: String,
        url: String,
    },
    Sprite {
        key: String,
        tint: u32,
    },
    /// The picker's emoji sheet, cut into one PNG per `spriteIndex` (keys `sprite:<index>`).
    EmojiSheet,
    /// The colored sprite the composer's emoji button shows while hovered, cut into 77 cells (keys
    /// `button-sprite:<index>`).
    EmojiButtonSprites,
}

pub(crate) const MESSAGE_PAGE: u32 = dc_state::message_window::INITIAL_PAGE;
/// Held while a settings-proto PATCH is in flight.
/// Pictures downloaded at once. Opening a picker or a shop asks for hundreds together; without a limit they all start
/// at once and the connections, the memory and the CPU are shared out among them.
static IMAGE_FETCHES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(24);
/// The longest one picture download may take before it counts as failed.
const IMAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Download slots not in use, for the harness's `badge state` line.
#[cfg(feature = "capture")]
pub(crate) fn image_permits_free() -> usize {
    IMAGE_FETCHES.available_permits()
}
/// The last transport error of each picture that failed, by image key, for the harness's `badge state` line.
#[cfg(feature = "capture")]
pub(crate) static IMAGE_ERRORS: std::sync::Mutex<std::collections::BTreeMap<String, String>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// Capture runs only: `GRAVECORD_IMAGE_FAIL_FIRST=<key prefix>=<count>` makes the first `count` downloads of each
/// matching picture fail, to reproduce a transport error that the retry has to survive.
#[cfg(feature = "capture")]
fn injected_image_failure(key: &str) -> bool {
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<String, u32>>> =
        std::sync::Mutex::new(None);
    let Ok(spec) = std::env::var("GRAVECORD_IMAGE_FAIL_FIRST") else {
        return false;
    };
    let Some((prefix, count)) = spec.rsplit_once('=') else {
        return false;
    };
    let Ok(count) = count.parse::<u32>() else {
        return false;
    };
    if !key.starts_with(prefix) {
        return false;
    }
    let Ok(mut seen) = SEEN.lock() else {
        return false;
    };
    let tries = seen
        .get_or_insert_with(Default::default)
        .entry(key.to_owned())
        .or_insert(0);
    *tries += 1;
    *tries <= count
}
/// Pictures drawn from vector files (Lottie, Rive) at once, each on a blocking thread. Without a limit a picker of
/// forty stickers starts forty threads that each render a frame.
static IMAGE_DRAWS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);
/// The longest side, in pixels, a Shop picture is kept at; the main view sets it from the window and its scale.
/// The logical boxes the Shop draws pictures in, by image key, and the window's scale factor times one hundred.
pub(crate) static SHOP_FIT: std::sync::Mutex<
    Option<std::collections::HashMap<String, (f32, f32)>>,
> = std::sync::Mutex::new(None);
pub(crate) static SHOP_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);
pub(crate) static SHOP_IMAGE_SIDE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1920);

static SETTINGS_PROTO_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The routes that change the account itself: its settings, its per-server notification settings, its relationships,
/// its profile, its server memberships, and anything that spends or grants.
fn account_route(route: &str) -> bool {
    let path = route.split('?').next().unwrap_or(route);
    // Opening a DM channel changes nothing about the account.
    if path == "/users/@me/channels" {
        return false;
    }
    // The username availability check (`attemptUsername`, module 493527) answers `{taken}` and stores nothing.
    if path == "/users/@me/pomelo-attempt" {
        return false;
    }
    // The backup codes key is mailed to the account's address.
    path == "/auth/verify/view-backup-codes-challenge"
        || path == "/users/@me"
        || path.starts_with("/users/@me/")
        || path.ends_with("/members/@me")
        || path.ends_with("/profile/@me")
        || path.starts_with("/oauth2/")
        || path.starts_with("/store/")
        || path.contains("/billing/")
        // Student Hub: the email lookup mails a code and the waitlist signup stores the address.
        || path == "/guilds/automations/email-domain-lookup"
        || path.starts_with("/guilds/automations/email-domain-lookup/")
        || path == "/hub-waitlist/signup"
}

/// What an inert run (`GRAVECORD_INERT=1`, capture feature) does with a request from the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Inert {
    Allow,
    Deny,
}

impl Request {
    /// The variant's name, for the inert log line.
    pub(crate) fn variant(&self) -> &'static str {
        match self {
            Request::Presence { .. } => "Presence",
            Request::VoiceState { .. } => "VoiceState",
            Request::StreamOp { .. } => "StreamOp",
            Request::StreamConnect { .. } => "StreamConnect",
            Request::VoiceConnect { .. } => "VoiceConnect",
            #[cfg(feature = "capture")]
            Request::WebhookDemo { .. } => "WebhookDemo",
            Request::RestForm { .. } => "RestForm",
            Request::UploadLogs { .. } => "UploadLogs",
            Request::Schedule { .. } => "Schedule",
            Request::Fetch { .. } => "Fetch",
            Request::Typing { .. } => "Typing",
            Request::React { .. } => "React",
            Request::LoadMessages { .. } => "LoadMessages",
            Request::FetchMessages { .. } => "FetchMessages",
            Request::Send { .. } => "Send",
            Request::SendLocal { .. } => "SendLocal",
            Request::SendVoice { .. } => "SendVoice",
            Request::Edit { .. } => "Edit",
            Request::Delete { .. } => "Delete",
            Request::Ack { .. } => "Ack",
            Request::Save { .. } => "Save",
            Request::GuildSettings { .. } => "GuildSettings",
            Request::Rest { .. } => "Rest",
            Request::SoundboardSounds { .. } => "SoundboardSounds",
            Request::GuildMembers { .. } => "GuildMembers",
            Request::Download { .. } => "Download",
            Request::BearerGet { .. } => "BearerGet",
            Request::Interaction { .. } => "Interaction",
            Request::Pin { .. } => "Pin",
            Request::Subscribe { .. } => "Subscribe",
            Request::ForumUnreads { .. } => "ForumUnreads",
            Request::Image { .. } => "Image",
            Request::Sprite { .. } => "Sprite",
            Request::EmojiSheet => "EmojiSheet",
            Request::EmojiButtonSprites => "EmojiButtonSprites",
        }
    }

    /// Inert mode is default-deny: only reads and subscriptions pass. No wildcard arm, so a new variant must be
    /// classified here before the crate compiles.
    pub(crate) fn inert(&self) -> Inert {
        match self {
            Request::Fetch { .. }
            | Request::LoadMessages { .. }
            | Request::FetchMessages { .. }
            | Request::SoundboardSounds { .. }
            | Request::GuildMembers { .. }
            | Request::BearerGet { .. }
            | Request::Subscribe { .. }
            | Request::ForumUnreads { .. }
            | Request::Image { .. }
            | Request::Sprite { .. }
            | Request::EmojiSheet
            | Request::EmojiButtonSprites => Inert::Allow,
            Request::Rest { method, route, .. } => {
                // `POST /channels/{id}/post-data` is how the official forum list reads post previews; it changes nothing.
                if method == reqwest::Method::GET
                    || (method == reqwest::Method::POST
                        && route.starts_with("/channels/")
                        && route.ends_with("/post-data"))
                {
                    Inert::Allow
                } else {
                    Inert::Deny
                }
            }
            Request::Presence { .. }
            | Request::VoiceState { .. }
            | Request::StreamOp { .. }
            | Request::StreamConnect { .. }
            | Request::VoiceConnect { .. }
            | Request::RestForm { .. }
            | Request::UploadLogs { .. }
            | Request::Schedule { .. }
            | Request::Typing { .. }
            | Request::React { .. }
            | Request::Send { .. }
            | Request::SendLocal { .. }
            | Request::SendVoice { .. }
            | Request::Edit { .. }
            | Request::Delete { .. }
            | Request::Ack { .. }
            | Request::Save { .. }
            | Request::GuildSettings { .. }
            | Request::Download { .. }
            | Request::Interaction { .. }
            | Request::Pin { .. } => Inert::Deny,
            #[cfg(feature = "capture")]
            Request::WebhookDemo { .. } => Inert::Deny,
        }
    }

    /// The method and route for the log line, with every number masked.
    fn inert_route(&self) -> String {
        let (method, route) = match self {
            Request::Rest { method, route, .. } => (method.as_str(), route.as_str()),
            Request::RestForm { route, .. } => ("POST", route.as_str()),
            _ => ("-", "-"),
        };
        let mut masked = String::new();
        let mut digits = false;
        for ch in route.split('?').next().unwrap_or("").chars() {
            if ch.is_ascii_digit() {
                if !digits {
                    masked.push_str(":id");
                }
                digits = true;
            } else {
                digits = false;
                masked.push(ch);
            }
        }
        format!("{method} {masked}")
    }
}

/// The account update (`PATCH /users/@me`, module 631670 `KD`) answers a password change with the account's new
/// token: the official client takes it out of the body before the user record is stored (`delete i.token`) and hands
/// it to the token store. The old token is dead from then on. `None` for every other answer.
pub(crate) fn take_renewed_token(method: &str, route: &str, answer: &mut Value) -> Option<Token> {
    // Module 631670 `KD` for the account edit; module 98207 for the authenticator app (`MFA_ENABLE_SUCCESS`,
    // `MFA_DISABLE_SUCCESS`), whose answers renew the token too.
    let renews = matches!(
        (method, route),
        ("PATCH", "/users/@me")
            | ("POST", "/users/@me/mfa/totp/enable")
            | ("POST", "/users/@me/mfa/totp/disable")
    );
    if !renews {
        return None;
    }
    let token = answer.as_object_mut()?.remove("token")?;
    token
        .as_str()
        .filter(|token| !token.is_empty())
        .map(Token::new)
}

/// Applies inert mode to a request taken from the UI: true when it is refused (logged and counted).
pub(crate) fn inert_refuses(request: &Request, updates: &mpsc::UnboundedSender<Update>) -> bool {
    if !crate::inert::on() {
        return false;
    }
    if request.inert() == Inert::Allow {
        crate::inert::request_allowed();
        return false;
    }
    crate::inert::note(&format!(
        "refused {} {}",
        request.variant(),
        request.inert_route()
    ));
    let key = match request {
        Request::Rest { key, .. } => key.clone(),
        Request::RestForm { key, .. } => Some(key.clone()),
        _ => None,
    };
    if let Some(key) = key {
        let body = json!({"ok": false, "status": 0, "body": null, "refused": true});
        let _ = updates.send(Update::Fetched { key, body });
    }
    // A send the view shows as a row ends as a failed message, as one whose request never got an answer does.
    if let Request::SendLocal { nonce, channel, .. } = request {
        let _ = updates.send(Update::SendResult {
            channel: *channel,
            nonce: nonce.clone(),
            outcome: crate::send_policy::Outcome::Failed(crate::send_policy::Failure::refused()),
        });
    }
    true
}

/// A harness run (the `capture` feature, or `--capture`) never writes to the account by accident: such a request is
/// refused here, whatever the caller checked, unless the run says `GRAVECORD_ALLOW_ACCOUNT_WRITE=1`. A settings write
/// with no settings in it is refused in every build: the server would replace the whole message with nothing.
fn account_write_refused(
    capture: bool,
    allowed: bool,
    method: &reqwest::Method,
    route: &str,
    body: Option<&Value>,
) -> bool {
    if method == reqwest::Method::GET {
        return false;
    }
    let empty_settings = route.starts_with("/users/@me/settings-proto/")
        && body.is_some_and(|body| body["settings"].as_str().is_some_and(str::is_empty));
    empty_settings || (capture && !allowed && account_route(route))
}

/// A harness run never rings anyone's device: `POST /channels/<id>/call/ring` and `.../call/stop-ringing` are
/// refused here, whatever the caller checked, unless the environment says `GRAVECORD_ALLOW_RING=1`.
fn ring_refused(capture: bool, allowed: bool, method: &reqwest::Method, route: &str) -> bool {
    if !capture || allowed || method == reqwest::Method::GET {
        return false;
    }
    let path = route.split('?').next().unwrap_or(route);
    path.ends_with("/call/ring") || path.ends_with("/call/stop-ringing")
}

fn refuses_ring(method: &reqwest::Method, route: &str) -> bool {
    let allowed = std::env::var("GRAVECORD_ALLOW_RING").as_deref() == Ok("1");
    ring_refused(crate::storage::is_capture_run(), allowed, method, route)
}

/// Whether a harness run refuses this request: an account write, with no override.
#[cfg(test)]
pub(crate) fn refuses_in_capture(method: &reqwest::Method, route: &str) -> bool {
    account_write_refused(true, false, method, route, None)
}

fn refuses_account_write(method: &reqwest::Method, route: &str, body: Option<&Value>) -> bool {
    let allowed = std::env::var("GRAVECORD_ALLOW_ACCOUNT_WRITE").as_deref() == Ok("1");
    account_write_refused(
        crate::storage::is_capture_run(),
        allowed,
        method,
        route,
        body,
    )
}
/// The picker's unicode emoji sheet (module 617057): 40px cells, 42 to a row, drawn at 1680 by 1560.
const EMOJI_SHEET: &str = "https://cdn.discordapp.com/assets/content/c357d57faac49918709981e6fb30e526fa62b6d440f99fd8bd2b88f0542ad6c5.png";
/// Emoji with skin tones come from their own sheet (`spritesheet--40.png`, module 125371), 10 to a row
/// (`DiversityPerRow`, module 307731).
const DIVERSITY_SHEET: &str = "https://cdn.discordapp.com/assets/content/2f1adbbeee210ebdfaf92e6cdb29be32af98df2f576a85c6f18515eecb49a628.png";
const SHEET_COLUMNS: u32 = 42;
const DIVERSITY_COLUMNS: u32 = 10;
const SHEET_CELL: u32 = 40;

/// Every cell of a sheet as a PNG under `<prefix>:<index>`, scaled to 40px when the sheet is stored larger.
fn sheet_cells(
    sheet: &[u8],
    count: u32,
    columns: u32,
    prefix: &str,
) -> image::ImageResult<Vec<(String, Vec<u8>)>> {
    let sheet = image::load_from_memory_with_format(sheet, image::ImageFormat::Png)?.to_rgba8();
    let scale = f64::from(sheet.width()) / f64::from(columns * SHEET_CELL);
    let size = (f64::from(SHEET_CELL) * scale).round() as u32;
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count {
        let (column, row) = (index % columns, index / columns);
        let (x, y) = (column * size, row * size);
        if x + size > sheet.width() || y + size > sheet.height() {
            break;
        }
        let mut cell = image::imageops::crop_imm(&sheet, x, y, size, size).to_image();
        if size != SHEET_CELL {
            cell = image::imageops::resize(
                &cell,
                SHEET_CELL,
                SHEET_CELL,
                image::imageops::FilterType::Triangle,
            );
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        cell.write_to(&mut bytes, image::ImageFormat::Png)?;
        out.push((format!("{prefix}:{index}"), bytes.into_inner()));
    }
    Ok(out)
}

/// The composer's emoji button is one cell of this mask sprite, at -180px -36px in a 360x72 layout, 18px square.
const EMOJI_SPRITE: &str = "https://discordapp.com/assets/decf85f79e02d276.png";
/// `spriteColored_SjG8Ug` (module 346089): the same 20 by 4 layout in color.
const EMOJI_SPRITE_COLORED: &str = "https://discordapp.com/assets/1b05b930a77fbff9.png";

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The body the official client posts, field for field (`TRAFFIC`).
/// The cloud upload of module 48325: for each file, `POST /channels/{id}/attachments` with
/// `{files: [{filename, file_size, id, is_clip}]}` for a signed URL, then a PUT of the bytes there. The message then
/// names each upload (`OW` in module 292348): `{id, filename, uploaded_filename}`, ids counting from 0.
async fn upload_all(
    client: &dc_http::Client,
    channel: u64,
    files: &[Upload],
) -> Result<Vec<Value>, String> {
    let mut attachments = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let bytes = std::fs::read(&file.path).map_err(|error| error.to_string())?;
        let filename = file.filename();
        let request = json!({"files": [{
            "filename": filename,
            "file_size": bytes.len(),
            "id": (index + 1).to_string(),
            "is_clip": false,
        }]});
        let granted = client
            .post(&format!("/channels/{channel}/attachments"), &request)
            .await
            .map_err(|error| error.to_string())?;
        let slot = &granted["attachments"][0];
        let (Some(url), Some(uploaded)) = (
            slot["upload_url"].as_str(),
            slot["upload_filename"].as_str(),
        ) else {
            return Err("no upload url".to_owned());
        };
        client
            .put_upload(url, bytes)
            .await
            .map_err(|error| error.to_string())?;
        attachments.push(attachment_entry(
            index,
            &filename,
            uploaded,
            file.description.as_deref(),
        ));
    }
    Ok(attachments)
}

/// The request of `Request::Schedule`: the uploads, then the create (`pr`/`_sendMessage` of modules 151282 and 138399).
/// The outcome is `Request::Rest`'s report shape; a harness run refuses it unless the run allows account writes.
async fn schedule_message(
    client: &dc_http::Client,
    channel: u64,
    content: &str,
    files: &[Upload],
    reply: Option<(Option<u64>, u64, bool)>,
    sticker_ids: &[u64],
    timestamp: &str,
) -> Value {
    const ROUTE: &str = "/users/@me/scheduled-messages";
    let (message_reference, allowed_mentions) = match reply {
        Some((guild, message, ping)) => {
            let mut reference =
                json!({"channel_id": channel.to_string(), "message_id": message.to_string()});
            if let Some(guild) = guild {
                reference["guild_id"] = json!(guild.to_string());
            }
            (
                Some(reference),
                Some(json!({"parse": ["users", "roles", "everyone"], "replied_user": ping})),
            )
        }
        None => (None, None),
    };
    let build = |attachments: Vec<Value>| {
        dc_state::scheduled_message::create_body(dc_state::scheduled_message::CreateRequest {
            channel,
            content,
            scheduled_timestamp: timestamp,
            flags: None,
            message_reference: message_reference.clone(),
            allowed_mentions: allowed_mentions.clone(),
            sticker_ids,
            attachments,
        })
    };
    if refuses_account_write(&reqwest::Method::POST, ROUTE, Some(&build(Vec::new()))) {
        warn!("account write refused in this run");
        return json!({"ok": false, "status": 0, "body": null, "refused": true});
    }
    let attachments = if files.is_empty() {
        Vec::new()
    } else {
        match upload_all(client, channel, files).await {
            Ok(attachments) => attachments,
            Err(error) => {
                warn!("upload failed: {error}");
                return json!({"ok": false, "status": 0, "body": {"message": error}});
            }
        }
    };
    match client.post(ROUTE, &build(attachments)).await {
        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
        Err(dc_http::Error::Status { status, body, .. }) => {
            warn!("{ROUTE} failed: {status}");
            json!({"ok": false, "status": status.as_u16(), "body": body})
        }
        Err(error) => {
            warn!("{ROUTE} failed: {error}");
            json!({"ok": false, "status": 0, "body": null})
        }
    }
}

/// The name a voice message's file goes by. The desktop client's native recorder names its file; the name is not in
/// the snapshot, this is the name the message list shows for the file.
pub(crate) const VOICE_FILENAME: &str = "voice-message.ogg";

/// The upload and the message of `Request::SendVoice`: the cloud upload of module 48325 for one file, then the message
/// the way `em` in module 841817 sends it (`content: ""`, `flags: IS_VOICE_MESSAGE`), its attachment entry from `OW`
/// in module 292348 with `duration_secs` and `waveform`.
async fn send_voice(
    client: &dc_http::Client,
    channel: u64,
    file: Vec<u8>,
    duration_secs: f64,
    waveform: &str,
    reply: Option<(Option<u64>, u64, bool)>,
) -> Value {
    let request = json!({"files": [{
        "filename": VOICE_FILENAME,
        "file_size": file.len(),
        "id": "1",
        "is_clip": false,
    }]});
    let granted = match client
        .post(&format!("/channels/{channel}/attachments"), &request)
        .await
    {
        Ok(granted) => granted,
        Err(error) => {
            warn!("voice upload request failed: {error}");
            return json!({"ok": false, "status": 0, "body": {"message": error.to_string()}});
        }
    };
    let slot = &granted["attachments"][0];
    let (Some(url), Some(uploaded)) = (
        slot["upload_url"].as_str(),
        slot["upload_filename"].as_str(),
    ) else {
        return json!({"ok": false, "status": 0, "body": {"message": "no upload url"}});
    };
    if let Err(error) = client.put_upload(url, file).await {
        warn!("voice upload failed: {error}");
        return json!({"ok": false, "status": 0, "body": {"message": error.to_string()}});
    }
    let mut entry = attachment_entry(0, VOICE_FILENAME, uploaded, None);
    entry["duration_secs"] = json!(duration_secs);
    entry["waveform"] = json!(waveform);
    let mut body = send_body(channel, "", reply);
    body["flags"] = json!(dc_state::VOICE_MESSAGE_FLAG);
    body["attachments"] = json!([entry]);
    match client
        .post(&format!("/channels/{channel}/messages"), &body)
        .await
    {
        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
        Err(dc_http::Error::Status { status, body, .. }) => {
            warn!("sending a voice message failed: {status}");
            json!({"ok": false, "status": status.as_u16(), "body": body})
        }
        Err(error) => {
            warn!("sending a voice message failed: {error}");
            json!({"ok": false, "status": 0, "body": null})
        }
    }
}

/// A file to upload with a message: where it is, the name it goes by (the composer's Modify and spoiler actions rename
/// it) and its alt text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Upload {
    pub path: std::path::PathBuf,
    /// The name the message shows; empty means the file's own.
    pub name: String,
    pub description: Option<String>,
}

pub(crate) struct RejectedDraft {
    pub content: String,
    pub files: Vec<Upload>,
    pub reply: Option<(Option<u64>, u64, bool)>,
}

impl Upload {
    pub fn filename(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned())
    }
}

/// One entry of the message's `attachments` (`OW` in module 292348); the description is the image's alt text.
pub(crate) fn attachment_entry(
    index: usize,
    filename: &str,
    uploaded: &str,
    description: Option<&str>,
) -> Value {
    let mut entry = json!({
        "id": index.to_string(),
        "filename": filename,
        "uploaded_filename": uploaded,
    });
    if let Some(description) = description.filter(|text| !text.trim().is_empty()) {
        entry["description"] = json!(description);
    }
    entry
}

/// A fresh nonce: the snowflake of now, as the client stamps outgoing messages and interactions.
pub(crate) fn nonce() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    pin::nonce_at(now)
}

pub(crate) fn send_body(
    channel: u64,
    content: &str,
    reply: Option<(Option<u64>, u64, bool)>,
) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let mut body = json!({
        "mobile_network_type": "unknown",
        "content": content,
        "nonce": pin::nonce_at(now),
        "tts": false,
    });
    if let Some((guild, message, ping)) = reply {
        let mut reference =
            json!({"channel_id": channel.to_string(), "message_id": message.to_string()});
        if let Some(guild) = guild {
            reference["guild_id"] = json!(guild.to_string());
        }
        body["message_reference"] = reference;
        body["allowed_mentions"] =
            json!({"parse": ["users", "roles", "everyone"], "replied_user": ping});
    }
    body["flags"] = json!(0);
    body
}

/// The guild (if any), channel and id of the message a forward points at.
pub(crate) type ForwardRef = (Option<u64>, u64, u64);

/// The request of [`Request::SendLocal`]: the uploads, then the create, which is sent again after a gateway error or
/// a transport failure (`send_policy`: three more tries, a jittered wait before each, 60 seconds for an attempt).
async fn send_local(
    client: &Client,
    channel: u64,
    (nonce, updates): (&str, &mpsc::UnboundedSender<Update>),
    content: &str,
    files: &[Upload],
    reply: Option<(Option<u64>, u64, bool)>,
    (sticker_ids, forward): (&[u64], Option<ForwardRef>),
) -> crate::send_policy::Outcome {
    use crate::send_policy::{Backoff, Failure, Outcome, RETRIES, TIMEOUT, status_is_retried};
    let mut body = send_body(channel, content, reply);
    body["nonce"] = json!(nonce);
    if !sticker_ids.is_empty() {
        // `sticker_ids` beside the content, as module 989161 builds the request.
        body["sticker_ids"] = json!(sticker_ids.iter().map(u64::to_string).collect::<Vec<_>>());
    }
    if let Some((guild, source, message)) = forward {
        // `message_reference` of type 1 and no flags: the forward of module 138399.
        body["message_reference"] = json!({
            "guild_id": guild.map(|guild| guild.to_string()),
            "channel_id": source.to_string(),
            "message_id": message.to_string(),
            "type": 1,
        });
        body["flags"] = json!(0);
    }
    if !files.is_empty() {
        let cancel = crate::upload_flow::register(nonce);
        let progress = {
            let (updates, nonce) = (updates.clone(), nonce.to_owned());
            Arc::new(move |index, sent, total| {
                let _ = updates.send(Update::SendProgress {
                    nonce: nonce.clone(),
                    index,
                    sent,
                    total,
                });
            })
        };
        let uploaded =
            crate::upload_flow::upload_files(client, channel, files, progress, cancel).await;
        crate::upload_flow::forget(nonce);
        match uploaded {
            Ok(attachments) => body["attachments"] = Value::Array(attachments),
            Err(ended) => {
                warn!("upload ended: {ended:?}");
                return match ended {
                    crate::upload_flow::End::Failed(failure) if failure.code.is_some() => {
                        Outcome::Failed(failure)
                    }
                    _ => Outcome::Dropped,
                };
            }
        }
    }
    let route = format!("/channels/{channel}/messages");
    let mut backoff = Backoff::default();
    let mut retries = RETRIES;
    loop {
        let failure = match tokio::time::timeout(TIMEOUT, client.post(&route, &body)).await {
            Ok(Ok(sent)) => return Outcome::Sent(sent),
            Ok(Err(dc_http::Error::Status {
                status,
                body: answer,
                ..
            })) => {
                if retries > 0 && status_is_retried(status.as_u16()) {
                    Failure::from_response(status.as_u16(), &answer)
                } else {
                    warn!("sending failed: {status}");
                    return Outcome::Failed(Failure::from_response(status.as_u16(), &answer));
                }
            }
            Ok(Err(dc_http::Error::RateLimited { .. })) => {
                return Outcome::Failed(Failure {
                    status: Some(429),
                    ..Failure::default()
                });
            }
            // A transport error or a timeout: sent again while the retries last.
            Ok(Err(error)) => {
                warn!("sending failed: {error}");
                Failure::default()
            }
            Err(_) => {
                warn!("sending timed out");
                Failure::default()
            }
        };
        if retries == 0 {
            return Outcome::Failed(failure);
        }
        retries -= 1;
        let jitter = f64::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.subsec_nanos())
                % 1000,
        ) / 1000.;
        let wait = backoff.fail(jitter, None);
        warn!("sending again in {wait:?} (failure {})", backoff.fails());
        tokio::time::sleep(wait).await;
    }
}

/// Cuts the cell and paints it in `tint` (0xRRGGBBAA), which is what the CSS mask over a colored box does.
fn emoji_cell(sprite: &[u8], tint: u32) -> image::ImageResult<Vec<u8>> {
    let sheet = image::load_from_memory_with_format(sprite, image::ImageFormat::Png)?.to_rgba8();
    // The sheet is drawn at 360px wide but stored larger, by a factor that need not be whole.
    let scale = f64::from(sheet.width()) / 360.;
    let [red, green, blue, _] = tint.to_be_bytes();
    let scaled = |value: f64| (value * scale).round() as u32;
    let cell =
        image::imageops::crop_imm(&sheet, scaled(180.), scaled(36.), scaled(18.), scaled(18.))
            .to_image();
    let painted = image::RgbaImage::from_fn(cell.width(), cell.height(), |x, y| {
        image::Rgba([red, green, blue, cell.get_pixel(x, y)[3]])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    painted.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

/// The gateway's command channel and whether a voice state put this account in a channel or call, for the exit: the
/// gateway leaves the call itself before it closes (`dc_gateway::voice_leave`), but only when it is told to shut
/// down while the runtime still runs.
static EXIT_GATEWAY: std::sync::Mutex<Option<mpsc::UnboundedSender<Command>>> =
    std::sync::Mutex::new(None);
static VOICE_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called once when the window has closed, before the runtime is dropped: when a call is open, asks the gateway to
/// leave it and close, and gives it a moment to send the frame.
pub fn close_gateway_at_exit() {
    if !VOICE_OPEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let sender = EXIT_GATEWAY.lock().ok().and_then(|mut slot| slot.take());
    if let Some(sender) = sender {
        let _ = sender.send(Command::Shutdown);
        std::thread::sleep(std::time::Duration::from_millis(600));
    }
}

struct Shutdown(mpsc::UnboundedSender<Command>);

impl Drop for Shutdown {
    fn drop(&mut self) {
        let _ = self.0.send(Command::Shutdown);
    }
}

pub async fn run(
    token: Token,
    updates: mpsc::UnboundedSender<Update>,
    mut requests: mpsc::UnboundedReceiver<Request>,
) {
    let identity = Identity::new();
    let dead = updates.clone();
    let client = match Client::new(token.clone(), identity.clone()).map(|client| {
        client.on_unauthorized(move || {
            let _ = dead.send(Update::AuthFailed);
        })
    }) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            warn!("http client failed: {error}");
            return;
        }
    };
    // The RPC pipe lives as long as this task, that is as long as the login.
    let _rpc = crate::rpc::start(client.clone(), updates.clone());
    let (commands, mut events) = spawn(Config {
        token,
        identity,
        state: IdentifyState::default(),
    });

    if let Ok(mut slot) = EXIT_GATEWAY.lock() {
        *slot = Some(commands.clone());
    }
    // Dropping this future (logging out) closes the gateway.
    let _gateway = Shutdown(commands.clone());
    let forward = updates.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                Event::Dispatch { name, data } => {
                    let connected = matches!(name.as_str(), "READY" | "RESUMED");
                    let _ = forward.send(Update::Dispatch { name, data });
                    if connected {
                        let _ = forward.send(Update::GatewayConnected(true));
                    }
                }
                Event::AuthenticationFailed => {
                    warn!("gateway rejected the token");
                    let _ = forward.send(Update::AuthFailed);
                }
                Event::Reconnecting { delay } => {
                    warn!("gateway reconnecting in {delay:?}");
                    let _ = forward.send(Update::GatewayConnected(false));
                }
                Event::Connected => {}
                Event::Closed { .. } => {
                    let _ = forward.send(Update::GatewayConnected(false));
                }
            }
        }
    });

    while let Some(request) = requests.recv().await {
        if inert_refuses(&request, &updates) {
            continue;
        }
        let client = client.clone();
        let updates = updates.clone();
        match request {
            Request::LoadMessages { channel } => {
                tokio::spawn(async move {
                    let started_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_millis() as u64)
                        .unwrap_or_default();
                    let route = format!("/channels/{channel}/messages?limit={MESSAGE_PAGE}");
                    match client.get(&route).await {
                        Ok(page) => {
                            let _ = updates.send(Update::Messages {
                                channel,
                                page,
                                started_ms,
                            });
                        }
                        Err(error) => warn!("loading messages failed: {error}"),
                    }
                });
            }
            Request::FetchMessages { spec } => {
                tokio::spawn(async move {
                    match client.get(&spec.route()).await {
                        Ok(page) => {
                            let _ = updates.send(Update::MessagesLoaded { spec, page });
                        }
                        Err(error) => {
                            warn!("loading messages failed: {error}");
                            let _ = updates.send(Update::MessagesFailed {
                                channel: spec.channel,
                            });
                        }
                    }
                });
            }
            Request::SendLocal {
                nonce,
                channel,
                content,
                files,
                reply,
                sticker_ids,
                forward,
            } => {
                tokio::spawn(async move {
                    let outcome = send_local(
                        &client,
                        channel,
                        (&nonce, &updates),
                        &content,
                        &files,
                        reply,
                        (&sticker_ids, forward),
                    )
                    .await;
                    let _ = updates.send(Update::SendResult {
                        channel,
                        nonce,
                        outcome,
                    });
                });
            }
            Request::Send {
                channel,
                content,
                files,
                reply,
            } => {
                tokio::spawn(async move {
                    let mut body = send_body(channel, &content, reply);
                    if !files.is_empty() {
                        let _ = updates.send(Update::Slowmode {
                            channel,
                            kind: dc_state::slowmode::Kind::SendMessage,
                            millis: None,
                        });
                        match upload_all(&client, channel, &files).await {
                            Ok(attachments) => body["attachments"] = Value::Array(attachments),
                            Err(error) => {
                                let _ = updates.send(Update::Slowmode {
                                    channel,
                                    kind: dc_state::slowmode::Kind::SendMessage,
                                    millis: Some(0),
                                });
                                warn!("upload failed: {error}");
                                return;
                            }
                        }
                    }
                    match client
                        .post(&format!("/channels/{channel}/messages"), &body)
                        .await
                    {
                        Ok(_) => {
                            let _ = updates.send(Update::Slowmode {
                                channel,
                                kind: dc_state::slowmode::Kind::SendMessage,
                                millis: None,
                            });
                        }
                        Err(error) => {
                            if let dc_http::Error::Status { body, .. } = &error
                                && body["code"].as_u64() == Some(20016)
                                && let Some(seconds) = body["retry_after"]
                                    .as_f64()
                                    .filter(|value| value.is_finite() && *value > 0.)
                            {
                                let _ = updates.send(Update::Slowmode {
                                    channel,
                                    kind: dc_state::slowmode::Kind::SendMessage,
                                    millis: Some((seconds * 1000.) as u64),
                                });
                                let _ = updates.send(Update::SlowmodeRejected {
                                    channel,
                                    draft: RejectedDraft {
                                        content,
                                        files,
                                        reply,
                                    },
                                    reason: body["message"].as_str().map(str::to_owned),
                                });
                            }
                            warn!("sending failed: {error}");
                        }
                    }
                });
            }
            Request::SendVoice {
                key,
                channel,
                file,
                duration_secs,
                waveform,
                reply,
            } => {
                tokio::spawn(async move {
                    let body =
                        send_voice(&client, channel, file, duration_secs, &waveform, reply).await;
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::Edit {
                channel,
                message,
                content,
            } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages/{message}");
                    let body = json!({"content": content});
                    if let Err(error) = client
                        .request(reqwest::Method::PATCH, &route, Some(&body))
                        .await
                    {
                        warn!("editing failed: {error}");
                    }
                });
            }
            Request::Delete { channel, message } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages/{message}");
                    if let Err(error) = client.request(reqwest::Method::DELETE, &route, None).await
                    {
                        warn!("deleting failed: {error}");
                    }
                });
            }
            Request::Ack {
                channel,
                message,
                manual,
                mention_count,
                last_viewed,
            } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages/{message}/ack");
                    let body = if manual {
                        json!({"manual": true, "mention_count": mention_count})
                    } else {
                        json!({"token": null, "last_viewed": last_viewed})
                    };
                    if let Err(error) = client
                        .request(reqwest::Method::POST, &route, Some(&body))
                        .await
                    {
                        warn!("ack failed: {error}");
                    }
                });
            }
            Request::Save {
                channel,
                message,
                due_at,
                save,
            } => {
                tokio::spawn(async move {
                    let route = format!("/users/@me/saved-messages/{channel}/{message}");
                    let result = if save {
                        let mut body = json!({"source": "context_menu"});
                        if let Some(due_at) = due_at {
                            body["due_at"] = json!(due_at);
                        }
                        client
                            .request(reqwest::Method::PUT, &route, Some(&body))
                            .await
                    } else {
                        client.request(reqwest::Method::DELETE, &route, None).await
                    };
                    if let Err(error) = result {
                        warn!("saving failed: {error}");
                    }
                });
            }
            Request::GuildSettings { body } => {
                if refuses_account_write(
                    &reqwest::Method::PATCH,
                    "/users/@me/guilds/settings",
                    Some(&body),
                ) {
                    warn!("guild settings write refused in this run");
                    continue;
                }
                tokio::spawn(async move {
                    if let Err(error) = client
                        .request(
                            reqwest::Method::PATCH,
                            "/users/@me/guilds/settings",
                            Some(&body),
                        )
                        .await
                    {
                        warn!("guild settings failed: {error}");
                    }
                });
            }
            Request::Schedule {
                channel,
                content,
                files,
                reply,
                sticker_ids,
                timestamp,
            } => {
                tokio::spawn(async move {
                    let report = schedule_message(
                        &client,
                        channel,
                        &content,
                        &files,
                        reply,
                        &sticker_ids,
                        &timestamp,
                    )
                    .await;
                    let _ = updates.send(Update::Fetched {
                        key: "scheduled-create".to_owned(),
                        body: report,
                    });
                });
            }
            Request::Rest {
                key,
                method,
                route,
                body,
            } => {
                let updates = updates.clone();
                if refuses_ring(&method, &route) {
                    warn!("call ring refused in this run");
                    if let Some(key) = key {
                        let body = json!({"ok": false, "status": 0, "body": null, "refused": true});
                        let _ = updates.send(Update::Fetched { key, body });
                    }
                    continue;
                }
                if refuses_account_write(&method, &route, body.as_ref()) {
                    warn!("account write refused in this run");
                    if let Some(key) = key {
                        let body = json!({"ok": false, "status": 0, "body": null, "refused": true});
                        let _ = updates.send(Update::Fetched { key, body });
                    }
                    continue;
                }
                let gateway = commands.clone();
                tokio::spawn(async move {
                    // The server merges a settings PATCH into what it read a moment before, so two sent at once lose
                    // one of them: they go one after the other.
                    let _turn = if route.starts_with("/users/@me/settings-proto/") {
                        Some(SETTINGS_PROTO_TURN.lock().await)
                    } else {
                        None
                    };
                    let method_name = method.to_string();
                    let outcome = client.request(method, &route, body.as_ref()).await;
                    let report = match outcome {
                        Ok(mut value) => {
                            if let Some(token) =
                                take_renewed_token(&method_name, &route, &mut value)
                            {
                                client.set_token(token.clone());
                                let _ = gateway.send(Command::SetToken(token.clone()));
                                let _ = updates.send(Update::TokenRenewed(token));
                            }
                            json!({"ok": true, "status": 200, "body": value})
                        }
                        Err(dc_http::Error::Status { status, body, .. }) => {
                            warn!("{route} failed: {status}");
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{route} failed: {error}");
                            json!({"ok": false, "status": 0, "body": null})
                        }
                    };
                    if let Some(key) = key {
                        let _ = updates.send(Update::Fetched { key, body: report });
                    }
                });
            }
            #[cfg(feature = "capture")]
            Request::WebhookDemo { channel } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let report = webhook_demo(&client, channel).await;
                    let _ = updates.send(Update::Fetched {
                        key: "webhook-demo".to_owned(),
                        body: report,
                    });
                });
            }
            Request::UploadLogs { files, state } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
                    for path in &files {
                        let Some(name) = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                        else {
                            continue;
                        };
                        if let Ok(bytes) = std::fs::read(path) {
                            parts.push((name, bytes));
                        }
                    }
                    let count = parts.len() + 1;
                    parts.push((
                        "media_engine_state.json".to_owned(),
                        serde_json::to_vec_pretty(&state).unwrap_or_default(),
                    ));
                    let report = if parts.len() == 1 {
                        // `UploadErrorCodes.NO_FILE`: nothing to send.
                        json!({"ok": false, "status": 0, "body": null, "count": 0})
                    } else {
                        match client.post_files("/debug-logs/multi/1", &parts).await {
                            Ok(value) => {
                                json!({"ok": true, "status": 200, "body": value, "count": count})
                            }
                            Err(dc_http::Error::Status { status, body, .. }) => {
                                warn!("debug log upload failed: {status}");
                                json!({"ok": false, "status": status.as_u16(), "body": body, "count": count})
                            }
                            Err(error) => {
                                warn!("debug log upload failed: {error}");
                                json!({"ok": false, "status": 0, "body": null, "count": count})
                            }
                        }
                    };
                    let _ = updates.send(Update::Fetched {
                        key: "voice-logs-upload".to_owned(),
                        body: report,
                    });
                });
            }
            Request::RestForm { key, route, form } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let report = match client.post_form(&route, &form).await {
                        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
                        Err(dc_http::Error::Status { status, body, .. }) => {
                            warn!("{route} failed: {status}");
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{route} failed: {error}");
                            json!({"ok": false, "status": 0, "body": null})
                        }
                    };
                    let _ = updates.send(Update::Fetched { key, body: report });
                });
            }
            Request::Download {
                url,
                filename,
                done,
            } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let saved = save_download(&url, &filename).await;
                    if let Some(key) = done {
                        let _ = updates.send(Update::Fetched {
                            key,
                            body: json!({"ok": saved}),
                        });
                    }
                });
            }
            Request::BearerGet { key, route, bearer } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let report = match client.get_as_bearer(&route, &bearer).await {
                        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
                        Err(dc_http::Error::Status { status, body, .. }) => {
                            warn!("{route} failed: {status}");
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{route} failed: {error}");
                            json!({"ok": false, "status": 0, "body": null})
                        }
                    };
                    let _ = updates.send(Update::Fetched { key, body: report });
                });
            }
            Request::Interaction { body } => {
                tokio::spawn(async move {
                    if let Err(error) = client
                        .request(reqwest::Method::POST, "/interactions", Some(&body))
                        .await
                    {
                        warn!("interaction failed: {error}");
                    }
                });
            }
            Request::Pin {
                channel,
                message,
                pin,
            } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages/pins/{message}");
                    let method = if pin {
                        reqwest::Method::PUT
                    } else {
                        reqwest::Method::DELETE
                    };
                    if let Err(error) = client.request(method, &route, None).await {
                        warn!("pinning failed: {error}");
                    }
                });
            }
            Request::React {
                channel,
                message,
                emoji,
                add,
                location,
                burst,
            } => {
                tokio::spawn(async move {
                    let route = format!(
                        "/channels/{channel}/messages/{message}/reactions/{}/@me?location={}&type={}",
                        percent_encode(&emoji),
                        percent_encode(location),
                        u8::from(burst),
                    );
                    let method = if add {
                        reqwest::Method::PUT
                    } else {
                        reqwest::Method::DELETE
                    };
                    if let Err(error) = client.request(method, &route, None).await {
                        warn!("reaction failed: {error}");
                    }
                });
            }
            Request::Typing { channel } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/typing");
                    match client.request(reqwest::Method::POST, &route, None).await {
                        Ok(body) => {
                            for (field, kind) in [
                                (
                                    "message_send_cooldown_ms",
                                    dc_state::slowmode::Kind::SendMessage,
                                ),
                                (
                                    "thread_create_cooldown_ms",
                                    dc_state::slowmode::Kind::CreateThread,
                                ),
                            ] {
                                if let Some(millis) =
                                    body[field].as_u64().filter(|millis| *millis > 0)
                                {
                                    let _ = updates.send(Update::Slowmode {
                                        channel,
                                        kind,
                                        millis: Some(millis),
                                    });
                                }
                            }
                        }
                        Err(error) => warn!("typing notice failed: {error}"),
                    }
                });
            }
            Request::Fetch { key, route } => {
                tokio::spawn(async move {
                    match client.get(&route).await {
                        Ok(body) => {
                            let _ = updates.send(Update::Fetched { key, body });
                        }
                        Err(error) => {
                            warn!("fetch failed: {error}");
                            let body = fetch_failure(&key, &error);
                            let _ = updates.send(Update::Fetched { key, body });
                        }
                    }
                });
            }
            Request::VoiceState {
                guild,
                channel,
                self_mute,
                self_deaf,
                self_video,
                flags,
            } => {
                let data = json!({
                    // A DM or group DM call has no guild: `guildId` is `null` in `voiceStateUpdate` (module 868).
                    "guild_id": (guild != 0).then(|| guild.to_string()),
                    "channel_id": channel.map(|channel| channel.to_string()),
                    "self_mute": self_mute,
                    "self_deaf": self_deaf,
                    "self_video": self_video,
                    "flags": flags,
                });
                VOICE_OPEN.store(channel.is_some(), std::sync::atomic::Ordering::Relaxed);
                let _ = commands.send(Command::Send {
                    op: Op::VoiceStateUpdate,
                    data,
                });
            }
            Request::Presence {
                status,
                activities,
                since,
                afk,
            } => {
                if !crate::games::presence_refused(crate::storage::is_capture_run()) {
                    let data = crate::games::presence_body_idle(&status, &activities, since, afk);
                    let _ = commands.send(Command::Send {
                        op: Op::PresenceUpdate,
                        data,
                    });
                }
            }
            Request::StreamOp { op, data } => {
                let _ = commands.send(Command::Send { op, data });
            }
            Request::StreamConnect {
                join,
                source,
                hub,
                controls,
            } => {
                let key = join.key;
                let (task, mut events) = dc_voice::connect_stream(join, source, &hub, &controls);
                let updates = updates.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.recv().await {
                        if let dc_voice::StreamEvent::Preview { jpeg } = &event {
                            let client = client.clone();
                            let body = preview_body(jpeg);
                            tokio::spawn(async move {
                                let route = format!("/streams/{key}/preview");
                                if let Err(error) = client.post(&route, &body).await {
                                    warn!("stream preview failed: {error}");
                                }
                            });
                            continue;
                        }
                        if updates.send(Update::Stream { key, event }).is_err() {
                            break;
                        }
                    }
                    let _ = task.await;
                });
            }
            Request::VoiceConnect { join, controls } => {
                let (task, mut events) = dc_voice::connect(join, &controls);
                let updates = updates.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.recv().await {
                        if updates.send(Update::Voice(event)).is_err() {
                            break;
                        }
                    }
                    let _ = task.await;
                });
            }
            Request::SoundboardSounds { guilds } => {
                let ids: Vec<String> = guilds.iter().map(u64::to_string).collect();
                let _ = commands.send(Command::Send {
                    op: Op::RequestSoundboardSounds,
                    data: json!({"guild_ids": ids}),
                });
            }
            Request::GuildMembers {
                guilds,
                query,
                limit,
                user_ids,
            } => {
                for data in guild_member_requests(&guilds, &query, limit, &user_ids) {
                    let _ = commands.send(Command::Send {
                        op: Op::RequestGuildMembers,
                        data,
                    });
                }
            }
            Request::Subscribe { guild, channel } => {
                let subscription = json!({"typing": true, "activities": true, "threads": true, "channels": {channel.to_string(): [[0, 99]]}});
                let data = json!({"subscriptions": {guild.to_string(): subscription}});
                let _ = commands.send(Command::Send {
                    op: Op::GuildSubscriptionsBulk,
                    data,
                });
            }
            Request::ForumUnreads {
                guild,
                channel,
                threads,
            } => {
                let threads: Vec<Value> = threads
                    .iter()
                    .map(|(thread, ack)| {
                        json!({"thread_id": thread.to_string(), "ack_message_id": ack.to_string()})
                    })
                    .collect();
                let _ = commands.send(Command::Send {
                    op: Op::RequestForumUnreads,
                    data: json!({"guild_id": guild.to_string(), "channel_id": channel.to_string(), "threads": threads}),
                });
            }
            Request::Sprite { key, tint } => {
                tokio::spawn(async move {
                    match client
                        .fetch_public(EMOJI_SPRITE)
                        .await
                        .map(|bytes| emoji_cell(&bytes, tint))
                    {
                        Ok(Ok(bytes)) => {
                            let _ = updates.send(Update::Image { key, bytes });
                        }
                        Ok(Err(error)) => warn!("sprite failed: {error}"),
                        Err(error) => warn!("sprite failed: {error}"),
                    }
                });
            }
            Request::EmojiButtonSprites => {
                tokio::spawn(async move {
                    match client.fetch_public(EMOJI_SPRITE_COLORED).await {
                        Ok(bytes) => {
                            let cut = tokio::task::spawn_blocking(move || {
                                sheet_cells(&bytes, 77, 20, "button-sprite")
                            })
                            .await;
                            match cut {
                                Ok(Ok(cells)) => {
                                    let _ = updates.send(Update::Images(cells));
                                }
                                Ok(Err(error)) => warn!("emoji button sprite failed: {error}"),
                                Err(error) => warn!("emoji button sprite failed: {error}"),
                            }
                        }
                        Err(error) => warn!("emoji button sprite failed: {error}"),
                    }
                });
            }
            Request::EmojiSheet => {
                let count = |diversity: bool| {
                    dc_tokens::emoji_data::EMOJIS
                        .iter()
                        .filter(|emoji| emoji.has_diversity == diversity)
                        .filter_map(|emoji| emoji.sprite)
                        .max()
                        .map_or(0, |last| u32::from(last) + 1)
                };
                let sheets = [
                    (EMOJI_SHEET, count(false), SHEET_COLUMNS, "sprite"),
                    (DIVERSITY_SHEET, count(true), DIVERSITY_COLUMNS, "dsprite"),
                ];
                for (url, count, columns, prefix) in sheets {
                    let client = client.clone();
                    let updates = updates.clone();
                    tokio::spawn(async move {
                        match client.fetch_public(url).await {
                            Ok(bytes) => {
                                let cut = tokio::task::spawn_blocking(move || {
                                    sheet_cells(&bytes, count, columns, prefix)
                                })
                                .await;
                                match cut {
                                    Ok(Ok(cells)) => {
                                        let _ = updates.send(Update::Images(cells));
                                    }
                                    Ok(Err(error)) => warn!("emoji sheet failed: {error}"),
                                    Err(error) => warn!("emoji sheet failed: {error}"),
                                }
                            }
                            Err(error) => warn!("emoji sheet failed: {error}"),
                        }
                    });
                }
            }
            Request::Image { key, url } => {
                tokio::spawn(async move {
                    let fetched = {
                        let _slot = IMAGE_FETCHES.acquire().await;
                        // The client has no timeout of its own: a request that stalls would hold its slot for ever
                        // and its picture would never arrive. It fails after `IMAGE_TIMEOUT` instead, and the UI
                        // asks again.
                        #[cfg(feature = "capture")]
                        let injected = injected_image_failure(&key);
                        #[cfg(not(feature = "capture"))]
                        let injected = false;
                        if injected {
                            Err("injected failure".to_owned())
                        } else {
                            match tokio::time::timeout(IMAGE_TIMEOUT, client.fetch_public(&url))
                                .await
                            {
                                Ok(result) => result.map_err(|error| error.to_string()),
                                Err(_) => {
                                    Err(format!("no answer in {} s", IMAGE_TIMEOUT.as_secs()))
                                }
                            }
                        }
                    };
                    match fetched {
                        Ok(bytes)
                            if url
                                .split(['?', '#'])
                                .next()
                                .is_some_and(|path| path.ends_with(".riv")) =>
                        {
                            // Rive art (the Shop's Nitro credit coins): the first frame of the artboard named after
                            // the `#`, its spaces written as underscores, drawn off the UI thread.
                            let artboard = url
                                .split_once('#')
                                .map_or(String::new(), |(_, name)| name.replace('_', " "));
                            let _slot = IMAGE_DRAWS.acquire().await;
                            match tokio::task::spawn_blocking(move || {
                                crate::rive::first_frame_png(&bytes, &artboard, 512)
                            })
                            .await
                            {
                                Ok(Some(bytes)) => {
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                _ => {
                                    warn!("rive failed");
                                    let _ = updates.send(Update::ImageFailed { key });
                                }
                            }
                        }
                        Ok(bytes)
                            if url
                                .split('?')
                                .next()
                                .is_some_and(|path| path.ends_with(".json"))
                                // The sticker player renders the frames itself from the file as it is.
                                && !key.starts_with("stickeranim:") =>
                        {
                            // Lottie sticker art: one frame at twice the picker's cell, drawn off the UI thread.
                            let _slot = IMAGE_DRAWS.acquire().await;
                            match tokio::task::spawn_blocking(move || {
                                crate::lottie::first_frame_png(&bytes, 192)
                            })
                            .await
                            {
                                Ok(Some(bytes)) => {
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                _ => {
                                    warn!("lottie failed");
                                    let _ = updates.send(Update::ImageFailed { key });
                                }
                            }
                        }
                        Ok(bytes) if key.starts_with("shop:img:") => {
                            // The Shop's pictures come at the size of the artwork (a banner can be 3000 pixels
                            // wide); decoded for the screen they would hold hundreds of megabytes. They are cut down
                            // here, off the UI thread, to what the window can show.
                            let side = SHOP_IMAGE_SIDE.load(std::sync::atomic::Ordering::Relaxed);
                            let scale =
                                SHOP_SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.;
                            let drawn_in = SHOP_FIT
                                .lock()
                                .ok()
                                .and_then(|mut boxes| boxes.as_mut()?.remove(&key));
                            let _slot = IMAGE_DRAWS.acquire().await;
                            let fitted = tokio::task::spawn_blocking(move || {
                                match drawn_in {
                                    Some((width, height)) => crate::ui::image_fit::fit_box(
                                        &bytes,
                                        width * scale,
                                        height * scale,
                                        side,
                                    ),
                                    None => crate::ui::image_fit::fit(&bytes, side),
                                }
                                .unwrap_or(bytes)
                            })
                            .await;
                            match fitted {
                                Ok(bytes) => {
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                Err(_) => {
                                    let _ = updates.send(Update::ImageFailed { key });
                                }
                            }
                        }
                        Ok(bytes) => {
                            let _ = updates.send(Update::Image { key, bytes });
                        }
                        Err(error) => {
                            warn!("image failed: {error}");
                            #[cfg(feature = "capture")]
                            if let Ok(mut errors) = IMAGE_ERRORS.lock() {
                                errors.insert(key.clone(), error.to_string());
                            }
                            let _ = updates.send(Update::ImageFailed { key });
                        }
                    }
                });
            }
        }
    }
}

/// The body of an `Update::Fetched` for a failed `Request::Fetch`, one shape for every key: `fetch_failed` marks
/// it, `status` is the HTTP status (0 for a transport error), `body` the response. The invite lookup and the two
/// feeds that read their own flags keep them beside it.
fn fetch_failure(key: &str, error: &dc_http::Error) -> Value {
    let (status, body) = match error {
        dc_http::Error::Status { status, body, .. } => (status.as_u16(), body.clone()),
        dc_http::Error::RateLimited { .. } => (429, Value::Null),
        _ => (0, Value::Null),
    };
    let mut out = json!({"ok": false, "fetch_failed": true, "status": status, "body": body});
    if key.starts_with("invite:") {
        // 970163 resolves failures to an absent invite, with a separate USER_BANNED marker; the error body is not
        // kept.
        out["invite_resolve_failed"] = json!(true);
        out["banned"] = json!(out["body"]["code"] == 40007);
        out["body"] = Value::Null;
    } else if key.starts_with("activity-shelf:") || key == "content-inventory:feed" {
        out["error"] = json!(true);
    }
    out
}

/// Whether a fetched body is the failure `fetch_failure` builds.
pub fn is_fetch_failure(body: &Value) -> bool {
    body["fetch_failed"] == true
}

/// Fetches `url` and writes it into the Downloads folder under a free name; whether it was saved.
async fn save_download(url: &str, filename: &str) -> bool {
    let Some(folder) = std::env::var_os("USERPROFILE")
        .map(|home| std::path::PathBuf::from(home).join("Downloads"))
    else {
        return false;
    };
    let bytes = match reqwest::get(url).await {
        Ok(response) => match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!("download failed: {error}");
                return false;
            }
        },
        Err(error) => {
            warn!("download failed: {error}");
            return false;
        }
    };
    let target = free_name(&folder, filename, |path| path.exists());
    match std::fs::write(&target, &bytes) {
        Ok(()) => true,
        Err(error) => {
            warn!("download not saved: {error}");
            false
        }
    }
}

/// `name`, or `name (1)`, `name (2)` ... until `taken` says the path is free.
fn free_name(
    folder: &std::path::Path,
    filename: &str,
    taken: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let (stem, extension) = match filename.rsplit_once('.') {
        Some((stem, extension)) => (stem.to_owned(), format!(".{extension}")),
        None => (filename.to_owned(), String::new()),
    };
    let mut target = folder.join(filename);
    let mut copy = 1;
    while taken(&target) {
        target = folder.join(format!("{stem} ({copy}){extension}"));
        copy += 1;
    }
    target
}

/// The payloads of op 8 for a query, or for a list of ids (100 per request): `{guild_id, query, limit, user_ids,
/// presences}` as `requestGuildMembers` in module 868 writes it.
fn guild_member_requests(guilds: &[u64], query: &str, limit: u64, user_ids: &[u64]) -> Vec<Value> {
    let ids: Vec<String> = guilds.iter().map(u64::to_string).collect();
    if user_ids.is_empty() {
        return vec![json!({"guild_id": ids, "query": query, "limit": limit, "presences": true})];
    }
    user_ids
        .chunks(100)
        .map(|chunk| {
            let users: Vec<String> = chunk.iter().map(u64::to_string).collect();
            json!({"guild_id": ids, "user_ids": users, "presences": true})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_password_change_answer_gives_up_its_token() {
        use super::take_renewed_token;
        use serde_json::json;
        let mut answer = json!({"id": "1", "username": "x", "token": "fixture-token-value"});
        let token = take_renewed_token("PATCH", "/users/@me", &mut answer).expect("token");
        assert_eq!(token.expose(), "fixture-token-value");
        // Like the official `delete i.token`: the user record never keeps it.
        assert!(answer.get("token").is_none());
        assert_eq!(answer["username"], "x");
        // Other answers, routes and methods carry none; an empty one is no token.
        let mut plain = json!({"id": "1"});
        assert!(take_renewed_token("PATCH", "/users/@me", &mut plain).is_none());
        let mut other = json!({"token": "fixture-token-value"});
        assert!(take_renewed_token("POST", "/users/@me", &mut other).is_none());
        assert!(take_renewed_token("PATCH", "/users/@me/profile", &mut other).is_none());
        let mut empty = json!({"token": ""});
        assert!(take_renewed_token("PATCH", "/users/@me", &mut empty).is_none());
        // Module 98207: turning the authenticator app on or off renews the token too.
        for route in ["/users/@me/mfa/totp/enable", "/users/@me/mfa/totp/disable"] {
            let mut answer = json!({"token": "fixture-token-value", "backup_codes": [{"code": "abcd1234", "consumed": false}]});
            let token = take_renewed_token("POST", route, &mut answer).expect("token");
            assert_eq!(token.expose(), "fixture-token-value");
            assert!(answer.get("token").is_none());
            assert!(answer["backup_codes"].is_array());
        }
        let mut sms = json!({"token": "fixture-token-value"});
        assert!(take_renewed_token("POST", "/users/@me/mfa/sms/enable", &mut sms).is_none());
    }

    use super::{
        account_write_refused, fetch_failure, guild_member_requests, is_fetch_failure,
        percent_encode, send_body,
    };
    use reqwest::Method;
    use serde_json::json;

    #[test]
    fn every_failed_fetch_has_one_shape_and_keeps_the_flags_old_readers_use() {
        let status = |code: u16, body| dc_http::Error::Status {
            route: "/x".into(),
            status: reqwest::StatusCode::from_u16(code).unwrap(),
            body,
        };
        let plain = fetch_failure("guild-nav:events:1", &status(403, json!({"message":"no"})));
        assert!(is_fetch_failure(&plain));
        assert_eq!(plain["ok"], false);
        assert_eq!(plain["status"], 403);
        assert_eq!(plain["body"]["message"], "no");
        let limited = fetch_failure(
            "presence-app:1",
            &dc_http::Error::RateLimited { route: "/x".into() },
        );
        assert_eq!(limited["status"], 429);
        assert_eq!(
            fetch_failure("content-inventory:feed", &status(500, json!(null)))["error"],
            true
        );
        assert_eq!(
            fetch_failure("activity-shelf:1", &status(500, json!(null)))["error"],
            true
        );
        assert!(!is_fetch_failure(&json!([])));
        assert!(!is_fetch_failure(&json!({"ok": false})));
    }

    #[test]
    fn a_harness_run_cannot_write_to_the_account_unless_it_says_so() {
        let refused = |capture, allowed, method: Method, route: &str| {
            account_write_refused(capture, allowed, &method, route, None)
        };
        for route in [
            "/users/@me/settings-proto/1",
            "/users/@me/settings-proto/2",
            "/users/@me/guilds/settings",
            "/users/@me/relationships/7",
            "/users/@me/guilds/9",
            "/users/@me",
            "/guilds/9/members/@me",
            "/oauth2/authorize?client_id=1",
            "/users/@me/billing/subscriptions",
        ] {
            assert!(refused(true, false, Method::PATCH, route), "{route}");
            assert!(refused(true, false, Method::DELETE, route), "{route}");
            // The run that says so, and every normal build, may.
            assert!(!refused(true, true, Method::PATCH, route), "{route}");
            assert!(!refused(false, false, Method::PATCH, route), "{route}");
            // Reading is never refused.
            assert!(!refused(true, false, Method::GET, route), "{route}");
        }
        // Fixture work stays possible: messages, channels, opening a DM.
        for route in [
            "/channels/5/messages",
            "/guilds/9/channels",
            "/users/@me/channels",
            "/channels/5/messages/6/ack",
        ] {
            assert!(!refused(true, false, Method::POST, route), "{route}");
        }
    }

    #[test]
    fn a_settings_write_with_nothing_in_it_is_refused_everywhere() {
        let empty = json!({"settings": ""});
        let filled = json!({"settings": "CgIIAQ=="});
        let route = "/users/@me/settings-proto/1";
        assert!(account_write_refused(
            false,
            false,
            &Method::PATCH,
            route,
            Some(&empty)
        ));
        assert!(account_write_refused(
            true,
            true,
            &Method::PATCH,
            route,
            Some(&empty)
        ));
        assert!(!account_write_refused(
            false,
            false,
            &Method::PATCH,
            route,
            Some(&filled)
        ));
    }

    #[test]
    fn member_requests_carry_a_query_or_ids_in_chunks_of_a_hundred() {
        let query = guild_member_requests(&[9], "lor", 10, &[]);
        assert_eq!(query.len(), 1);
        assert_eq!(query[0]["guild_id"], json!(["9"]));
        assert_eq!(query[0]["query"], "lor");
        assert_eq!(query[0]["limit"], 10);
        assert_eq!(query[0]["presences"], true);
        let ids: Vec<u64> = (1..=250).collect();
        let requests = guild_member_requests(&[9], "", 0, &ids);
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2]["user_ids"].as_array().unwrap().len(), 50);
        assert!(requests[0].get("query").is_none());
    }

    #[test]
    fn emoji_encode_as_in_the_recorded_route() {
        assert_eq!(percent_encode("\u{1f44d}"), "%F0%9F%91%8D");
        assert_eq!(percent_encode("name:123"), "name:123");
    }

    #[test]
    fn plain_send_matches_the_recorded_fields() {
        let body = send_body(5, "hi", None);
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            ["mobile_network_type", "content", "nonce", "tts", "flags"]
        );
    }

    #[test]
    fn a_reply_adds_the_reference_and_pings_by_the_switch() {
        let body = send_body(5, "hi", Some((Some(9), 7, false)));
        assert_eq!(body["message_reference"]["guild_id"], "9");
        assert_eq!(body["message_reference"]["message_id"], "7");
        assert_eq!(body["allowed_mentions"]["replied_user"], false);
        let pinging = send_body(5, "hi", Some((Some(9), 7, true)));
        assert_eq!(pinging["allowed_mentions"]["replied_user"], true);
    }

    #[test]
    fn a_dm_reply_has_no_guild() {
        let body = send_body(5, "hi", Some((None, 7, true)));
        assert!(body["message_reference"].get("guild_id").is_none());
    }
}

/// The preview upload's body: `{thumbnail: "data:image/jpeg;base64,..."}`.
fn preview_body(jpeg: &[u8]) -> Value {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(jpeg);
    json!({"thumbnail": format!("data:image/jpeg;base64,{encoded}")})
}

#[cfg(test)]
mod preview_tests {
    use super::{Upload, attachment_entry};
    #[test]
    fn the_preview_body_is_a_jpeg_data_uri() {
        let body = super::preview_body(&[0xff, 0xd8, 0xff]);
        assert_eq!(body["thumbnail"], "data:image/jpeg;base64,/9j/");
    }

    #[test]
    fn a_renamed_upload_goes_by_its_new_name_with_its_alt_text() {
        let upload = Upload {
            path: "cat.png".into(),
            name: "SPOILER_cat.png".to_owned(),
            description: Some("a cat".to_owned()),
        };
        assert_eq!(upload.filename(), "SPOILER_cat.png");
        let plain = Upload {
            path: "cat.png".into(),
            name: String::new(),
            description: None,
        };
        assert_eq!(plain.filename(), "cat.png");
        let entry = attachment_entry(0, &upload.filename(), "up/1", upload.description.as_deref());
        assert_eq!(entry["description"], "a cat");
        assert_eq!(entry["filename"], "SPOILER_cat.png");
        assert!(
            attachment_entry(1, "a", "b", Some("  "))
                .get("description")
                .is_none()
        );
    }
}

#[cfg(test)]
mod fetch_failure_tests {
    use super::fetch_failure;
    use dc_http::Error;
    use serde_json::json;

    #[test]
    fn invite_failure_keeps_ban_reason_without_leaking_the_error_body() {
        let error = |code| Error::Status {
            route: "/invites/fixture".into(),
            status: reqwest::StatusCode::FORBIDDEN,
            body: json!({"code": code, "message": "not rendered"}),
        };
        let kept = |key, error| {
            let out = fetch_failure(key, &error);
            (
                out["invite_resolve_failed"].clone(),
                out["banned"].clone(),
                out["body"].clone(),
            )
        };
        assert_eq!(
            kept("invite:fixture", error(40007)),
            (json!(true), json!(true), json!(null))
        );
        assert_eq!(
            kept("invite:fixture", error(10006)),
            (json!(true), json!(false), json!(null))
        );
        assert_eq!(
            kept(
                "invite:fixture",
                Error::RateLimited {
                    route: "/invites/fixture".into()
                }
            ),
            (json!(true), json!(false), json!(null))
        );
        // Other keys have no invite flags; the shelf and the feed keep `error`.
        assert!(fetch_failure("profile:fixture", &error(40007))["invite_resolve_failed"].is_null());
        assert_eq!(
            fetch_failure("activity-shelf:fixture", &error(40007))["error"],
            true
        );
        assert_eq!(
            fetch_failure("profile:fixture", &error(40007))["status"],
            403
        );
    }
}

#[cfg(test)]
mod download_tests {
    use super::free_name;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_taken_name_gets_a_counter_before_the_extension() {
        let taken: Vec<PathBuf> = ["a/cat.png", "a/cat (1).png"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let free = |path: &Path| taken.iter().any(|known| known == path);
        assert_eq!(
            free_name(Path::new("a"), "dog.png", free),
            PathBuf::from("a/dog.png")
        );
        assert_eq!(
            free_name(Path::new("a"), "cat.png", free),
            PathBuf::from("a/cat (2).png")
        );
        assert_eq!(
            free_name(Path::new("a"), "cat", |_| false),
            PathBuf::from("a/cat")
        );
    }
}

#[cfg(test)]
mod ring_tests {
    use super::ring_refused;
    use reqwest::Method;

    #[test]
    fn a_harness_run_never_rings_unless_the_environment_allows_it() {
        let ring = "/channels/1568693333864480939/call/ring";
        let stop = "/channels/1568693333864480939/call/stop-ringing";
        assert!(ring_refused(true, false, &Method::POST, ring));
        assert!(ring_refused(true, false, &Method::POST, stop));
        assert!(ring_refused(
            true,
            false,
            &Method::POST,
            &format!("{ring}?x=1")
        ));
        assert!(!ring_refused(true, true, &Method::POST, ring));
        assert!(!ring_refused(false, false, &Method::POST, ring));
        // Reading the call is not a ring (`GET /channels/<id>/call`).
        assert!(!ring_refused(true, false, &Method::GET, "/channels/1/call"));
        assert!(!ring_refused(
            true,
            false,
            &Method::POST,
            "/channels/1/messages"
        ));
    }
}

#[cfg(feature = "capture")]
/// The body of `Request::WebhookDemo`: only the fixture channel is written to, and only ids are reported.
async fn webhook_demo(client: &dc_http::Client, channel: u64) -> Value {
    if channel != crate::webhook_demo::FIXTURE_CHANNEL {
        return json!({"ok": false, "error": "not the fixture channel"});
    }
    let made = client
        .request(
            reqwest::Method::POST,
            &format!("/channels/{channel}/webhooks"),
            Some(&json!({"name": "Components fixture"})),
        )
        .await;
    let Ok(made) = made else {
        return json!({"ok": false, "error": "webhook not created"});
    };
    let (Some(id), Some(secret)) = (made["id"].as_str(), made["token"].as_str()) else {
        return json!({"ok": false, "error": "webhook answer had no address"});
    };
    let mut ids = Vec::new();
    for (index, payload) in crate::webhook_demo::payloads().iter().enumerate() {
        let file = (index > 0).then(|| {
            (
                crate::webhook_demo::FILE_NAME,
                b"Components fixture report\nline two\n".to_vec(),
            )
        });
        match client.post_webhook(id, secret, payload, file).await {
            Ok(message) => ids.push(message["id"].clone()),
            Err(dc_http::Error::Status { status, body, .. }) => {
                warn!("webhook demo message {index} failed: {status}");
                ids.push(json!({"failed": status.as_u16(), "body": body}));
            }
            Err(_) => ids.push(json!({"failed": 0})),
        }
    }
    let _ = client
        .request(reqwest::Method::DELETE, &format!("/webhooks/{id}"), None)
        .await;
    json!({"ok": true, "ids": ids})
}
