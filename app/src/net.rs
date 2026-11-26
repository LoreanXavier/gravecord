//! The network side. Runs on tokio and talks to the UI thread over channels, so gpui never blocks on I/O.

use dc_gateway::{Command, Config, Event, spawn};
use dc_http::Client;
/// Whether an address is on one of Discord's hosts (the pinned client keeps its header set for those only), for code
/// that decides what may be fetched without sending anything itself.
pub(crate) use dc_http::plain::is_discord_url;
use dc_proto::identity::{IdentifyState, Identity};
use dc_proto::op::Op;
use dc_proto::pin;
use dc_proto::secrets::Token;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

pub enum Update {
    SpotifyReply {
        request: u64,
        reply: crate::spotify::Reply,
    },
    GatewayConnected(bool),
    /// The network came or went, or the machine woke (`system_events`).
    System(dc_gateway::SystemEvent),
    /// The gateway forgot its session (`CONNECTION_CLOSED` of the official client): the voice connection ends.
    GatewayReset,
    /// The local RPC server: a game connected, set or cleared its activity, or left (`dc-rpc`).
    Rpc(dc_rpc::Event),
    /// One pass of the game scan (`games`).
    Games(Box<crate::games::GameScan>),
    /// The foreground window changed (`dc_overlay::platform::watch`): the overlay follows the focused game.
    OverlayFocus(Option<dc_overlay::decision::Foreground>),
    NotificationClicked(u64),
    Tray(crate::tray::Action),
    /// The updater's state changed (`updater::state`): the title bar button follows it.
    Updater,
    SpotifySocket {
        account: String,
        connected: bool,
    },
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
    /// An event of the voice connection with this number (`Request::VoiceConnect`): a connection that was replaced
    /// still reports its end, and that must not reach the call that replaced it.
    Voice(u64, dc_voice::Event),
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
        generation: u64,
        key: dc_voice::streams::StreamKey,
        event: dc_voice::StreamEvent,
    },
    /// A `Request::Image` that could not be downloaded, and whether asking again may help.
    ImageFailed {
        key: String,
        failure: crate::ui::image_retry::Failure,
    },
    /// A settings write ended (`PATCH /users/@me/settings-proto/1`), with the whole settings message the server
    /// answered (base64) when it did.
    SettingsWritten(Option<String>),
    /// The body of a `Request::Fetch`. A failed fetch arrives here too, with the body `fetch_failure` builds.
    Fetched {
        key: String,
        body: Value,
    },
    /// The result of a conditional CDN detectable games/non-games fetch.
    DetectablesCdn {
        key: String,
        status: u16,
        etag: Option<String>,
        games: Option<std::sync::Arc<Vec<dc_detect::DetectableGame>>>,
    },
    /// A `Request::Save` (bookmark or reminder PUT/DELETE) returned an HTTP error: revert the optimistic state.
    SaveFailed {
        channel: dc_state::Id,
        message: dc_state::Id,
        save: bool,
    },
    /// A `Request::React` PUT/DELETE returned an HTTP error: revert the optimistic state.
    ReactFailed {
        channel: dc_state::Id,
        message: dc_state::Id,
        emoji: String,
        add: bool,
    },
    /// The gateway closed with 4004 or a request came back 401: the stored login is dead.
    AuthFailed,
    /// A request was refused because the account has limited access (module 218642).
    LimitedAccess,
    /// `PATCH /users/@me` answered with a new token (a password change): the shell keeps it where the login is
    /// kept (`UPDATE_TOKEN`, module 631670 `KD`).
    TokenRenewed(Token),
}

/// The key of a message edit's answer: `message-edit:{channel}:{message}`.
pub const EDIT_KEY: &str = "message-edit:";

pub enum Request {
    Spotify(crate::spotify::Command),
    /// A system event for the gateway socket (expedited heartbeat or backoff reset).
    System(dc_gateway::SystemEvent),
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
        generation: u64,
    },
    /// Opens the voice connection once the gateway named the server.
    VoiceConnect {
        join: dc_voice::Join,
        controls: dc_voice::Controls,
        generation: u64,
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
    /// Not in the pinned client: OpenInApp's short link resolved by HEAD requests (no authorization, five redirects at
    /// most), handed back under `key` as `Update::Fetched` with `{"url": <final address>}`.
    ResolveRedirect {
        key: String,
        url: String,
    },
    /// GET a public JSON file (no authorization) and hand it back under `key` as `Update::Fetched`; a failure hands back
    /// `null`.
    FetchPublicJson {
        key: String,
        url: String,
    },
    /// Not in the pinned client: MoreCommands' `/friendcloud` picture. Each avatar is fetched (no authorization) and
    /// drawn round at its place with a grey ring on a transparent `width` by `height` canvas, off the UI thread; the
    /// PNG lands in a temporary `affinities-cloud.png`, handed back under `key` as `{"path": …}` (`null` on failure).
    FriendCloud {
        key: String,
        width: u32,
        height: u32,
        avatars: Vec<CloudAvatar>,
    },
    /// Not in the pinned client: ExpressionManager's Clone Sticker (`doClone`): the sticker's picture from the media proxy
    /// at the largest size under 512 KiB (4096 halving to 16), posted to the server as a new sticker. The answer comes
    /// under `key` as `Update::Fetched` with `{ok, name, message}`.
    /// Not in the pinned client: ClipUpload's `sendClipUpload`: the stamped file at `path` reserved as a clip
    /// (`reserveClipUpload`), put, and sent as a message with the clip fields (and the reply's reference). The answer
    /// comes under `key` as `Update::Fetched` with `{ok, message, path}`.
    PluginClipUpload {
        key: String,
        channel: u64,
        path: std::path::PathBuf,
        options: crate::plugins::clip_upload::Options,
        reply: Option<(Option<u64>, u64)>,
    },
    PluginCloneSticker {
        key: String,
        guild: u64,
        sticker: u64,
        format: u64,
        name: String,
        tags: String,
        description: String,
    },
    /// Not in the pinned client: CustomRPC's `fetchAssetIds`: each picture's application asset id (a link becomes an
    /// external asset, `mp:` and its path; a key the application's asset of that name). The answer comes under `key`
    /// as `Update::Fetched` with an object of key to id (`null` for none).
    PluginRpcAssets {
        key: String,
        app_id: String,
        keys: Vec<String>,
    },
    /// Not in the pinned client: Translate+'s translation of a message's text (`plugins::translate_plus`): Google
    /// Translate, the Toki Pona translator and the two dictionaries, none with authorization. The answer comes under
    /// `key` as `Update::Fetched` (`{src, text}`, or `null` when the Toki Pona call failed, as the original then shows
    /// nothing).
    /// Not in the pinned client: a call to a plugin's own web service (Decor's decorations, Streaks' counts), never to a
    /// Discord host, with the service's own token as the bearer when it has one (never the account's). `exchange`
    /// marks the call that trades an OAuth2 code for that token. The answer comes under `key` as `Update::Fetched`
    /// with `{ok, status, body}`: `body` is the answer's JSON, else its text (`null` when there was none).
    PluginService {
        key: String,
        method: reqwest::Method,
        url: String,
        bearer: Option<String>,
        body: ServiceBody,
        exchange: bool,
    },
    PluginTranslate {
        key: String,
        text: String,
        target: String,
        toki: bool,
        sitelen: bool,
        shavian: bool,
    },
    /// Not in the pinned client: GifProviderSwitcher's GIF calls to Tenor or Giphy (no authorization), their answer
    /// in the shape of Discord's own (`plugins::gif_provider`) under `key` as `Update::Fetched` (`null` on failure).
    GifProvider {
        key: String,
        provider: crate::plugins::gif_provider::Provider,
        ask: crate::plugins::gif_provider::Ask,
        limit: u32,
    },
    /// Not in the pinned client: ZipPreview's archive. `native` is the Discord attachment path its native helper
    /// fetches from `cdn.discordapp.com` (after the user agreed); else `url` is fetched as it is. At most 50 MB is read
    /// and the archive is parsed off the UI thread into `plugins::zip_preview`, then `Update::Fetched` under `key`.
    FetchZip {
        key: String,
        url: String,
        native: Option<String>,
    },
    /// Conditionally GET a public CDN JSON list and parse it off the UI thread.
    FetchCdn {
        key: String,
        url: String,
        etag: Option<String>,
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
    /// The newest page of the channel the app is about to open, asked for the moment the login is known (the start
    /// cache or the saved place names it), before the window and the gateway. Its answer is an ordinary
    /// `Update::Messages`; the view's own `LoadMessages` of that channel before READY is answered by it instead of a
    /// second request (`StartPage`). After READY every load goes to the server again.
    PrefetchMessages {
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
        /// Flags added to the voice flag: SilentMessageToggle's SUPPRESS_NOTIFICATIONS (a plugin), else 0.
        extra_flags: u64,
    },
    /// `PATCH /channels/{channel}/messages/{message}`; the answer comes back as `Update::Fetched` under
    /// [`EDIT_KEY`]`{channel}:{message}`.
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
    /// Not in the pinned client: DownloadAllAttachments (`app/src/ui/plugin_hooks/download_all.rs`). Each file is
    /// fetched from its first source that answers (four at a time); one file is saved as it is, several as a stored
    /// ZIP named `archive`, into Downloads under a free name. The outcome comes back as
    /// `Update::Fetched { key, body: {"saved": n, "total": m} }`.
    DownloadAll {
        /// Each file's sources in order, and its (already unique) name.
        files: Vec<(Vec<String>, String)>,
        archive: String,
        key: String,
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
    /// Op 37 `GUILD_SUBSCRIPTIONS_BULK` (module 868 `updateGuildSubscriptions`): guild id to subscription, as
    /// `dc_state::guild_subscriptions` builds it; split into several sends past the size limit.
    Subscribe {
        subscriptions: serde_json::Map<String, Value>,
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
static IMAGE_FETCHES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(48);
/// The longest one picture download may take before it counts as failed.
/// How long the open channel's page may go unanswered before it is asked again on a new connection
/// (`Client::get_hedged`). A page answers in a few hundred ms; a connection routed to a dead address hung about 7 s.
const PAGE_HEDGE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
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
/// A body that failed the end-marker check (`image_retry::complete`) and came back the same length on the next try
/// is the whole file after all (a valid picture with an unusual trailer): it is accepted, so a check that is wrong
/// about one file can only cost one download, never a picture that is fetched for ever and never drawn.
fn same_length_again(url: &str, length: usize) -> bool {
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<String, usize>>> =
        std::sync::Mutex::new(None);
    let Ok(mut seen) = SEEN.lock() else {
        return true;
    };
    let seen = seen.get_or_insert_with(Default::default);
    if seen.get(url) == Some(&length) {
        seen.remove(url);
        return true;
    }
    seen.insert(url.to_owned(), length);
    false
}

/// The HTTP status of a failed download, 0 when there was no answer.
fn http_status(error: &dc_http::Error) -> u16 {
    match error {
        dc_http::Error::Transport(error) => error.status().map_or(0, |status| status.as_u16()),
        dc_http::Error::Status { status, .. } => status.as_u16(),
        dc_http::Error::RateLimited { .. } => 429,
        dc_http::Error::Header => 0,
    }
}

/// Capture runs only: `GRAVECORD_IMAGE_FAIL_STATUS=<status>` gives the injected failures that HTTP status (0, a
/// transport error, when unset).
#[cfg(feature = "capture")]
fn injected_image_status() -> u16 {
    std::env::var("GRAVECORD_IMAGE_FAIL_STATUS")
        .ok()
        .and_then(|status| status.parse().ok())
        .unwrap_or(0)
}

#[cfg(not(feature = "capture"))]
fn injected_image_status() -> u16 {
    0
}

/// Pictures drawn from vector files (Lottie, Rive) at once, each on a blocking thread. Without a limit a picker of
/// forty stickers starts forty threads that each render a frame.
static IMAGE_DRAWS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(6);
/// Lottie stills drawn at once. Parsing and preparing one file holds some 30 MB for a moment, and the segment heap kept
/// the pages that six at once had freed committed: a sticker picker opening took the process from 120 to 440 MB. Two at
/// a time draw a picker's forty stills in about 150 ms instead of 90, and the picker fills in as soon (one at a time
/// left some cells blank 100 ms longer).
static LOTTIE_STILLS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
/// Content-addressed CDN assets cached raw on disk under `img-cache` when the start cache is active.
pub(crate) fn disk_cacheable_raw_image(key: &str, url: &str) -> bool {
    crate::ui::start_cache::enabled()
        && !url.starts_with("data:")
        && (key.starts_with("avatar:")
            || key.starts_with("guild:")
            || key.starts_with("rail-image:")
            || key.starts_with("avatar-decoration:")
            || key.starts_with("nameplate:")
            || key.starts_with(crate::ui::profile_effects::ART_PREFIX)
            || key.starts_with("gif:")
            || key.starts_with("gif-category:")
            || key.starts_with("badge:")
            || key.starts_with("nitro")
            || key.starts_with("profile-game:")
            || url.contains("/collectibles-shop/")
            || url.contains("/clan-badges/")
            || url.contains("/app-icons/")
            || url.contains("/banners/")
            || url.contains("/stickers/")
            || url.contains("/role-icons/")
            || url.contains("/assets/"))
}

async fn fetch_cached_sheet(client: &Arc<Client>, url: &str) -> Result<Vec<u8>, dc_http::Error> {
    if crate::ui::start_cache::enabled() {
        let lookup = url.to_owned();
        if let Ok(Some(cached)) =
            tokio::task::spawn_blocking(move || crate::ui::shop::art_cache::read_raw(&lookup)).await
        {
            if cached.stale {
                let refresh_client = Arc::clone(client);
                let refresh_url = url.to_owned();
                tokio::spawn(async move {
                    if let Ok(fresh) = refresh_client.fetch_public(&refresh_url).await {
                        tokio::task::spawn_blocking(move || {
                            crate::ui::shop::art_cache::write_raw(&refresh_url, &fresh);
                        });
                    }
                });
            }
            return Ok(cached.bytes);
        }
    }
    let bytes = client.fetch_public(url).await?;
    if crate::ui::start_cache::enabled() {
        let save_url = url.to_owned();
        let save_bytes = bytes.clone();
        tokio::task::spawn_blocking(move || {
            crate::ui::shop::art_cache::write_raw(&save_url, &save_bytes);
        });
    }
    Ok(bytes)
}

/// The box a Shop picture is drawn in (logical width, height, and the window's scale), for its name on disk.
fn shop_art_box(key: &str) -> Option<(f32, f32, f32)> {
    let scale = SHOP_SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.;
    let boxes = SHOP_FIT.lock().ok()?;
    let (width, height) = *boxes.as_ref()?.get(key)?;
    Some((width, height, scale))
}

/// Shop pictures cut down at once: half the cores (2 to 6). Behind the 3 slots of `IMAGE_DRAWS` a Shop page's fifty
/// pictures queued for seconds.
static SHOP_DRAWS: std::sync::LazyLock<tokio::sync::Semaphore> = std::sync::LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
    tokio::sync::Semaphore::new((cores / 2).clamp(2, 6))
});
/// The longest side, in pixels, a Shop picture is kept at; the main view sets it from the window and its scale.
/// The logical boxes the Shop draws pictures in, by image key, and the window's scale factor times one hundred.
pub(crate) static SHOP_FIT: std::sync::Mutex<
    Option<std::collections::HashMap<String, (f32, f32)>>,
> = std::sync::Mutex::new(None);
pub(crate) static SHOP_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);
pub(crate) static SHOP_IMAGE_SIDE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(1920);

static SETTINGS_PROTO_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `PATCH /users/@me/settings-proto/1`: a write of the account's settings.
fn is_settings_write(request: &Request) -> bool {
    matches!(
        request,
        Request::Rest { method, route, .. }
            if *method == reqwest::Method::PATCH && route == "/users/@me/settings-proto/1"
    )
}

/// Whether the stored login was refused this run (a 401, or the gateway's 4004); the capture harness reports it
/// (`signed-in`), so a check stops instead of signing in again and again with a dead token.
static LOGIN_REFUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub(crate) fn login_refused() -> bool {
    LOGIN_REFUSED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The app's end of the request channel. It counts the settings writes on their way to the server: the server answers
/// every one of them with a `USER_SETTINGS_PROTO_UPDATE`, and writes go one after the other, so the answer to an older
/// write arrives while a newer choice is already on screen. Applied, it put the older choice back for a moment (a
/// theme picked three times in a row walked through all three again). While the count is above zero those answers are
/// left out, and the answer of the last write brings the server's settings in (`Update::SettingsWritten`).
/// The bytes of a base64 `data:` URL, or `None` for anything else.
fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let rest = url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()
}

/// One avatar of MoreCommands' `/friendcloud`: its picture and its square on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudAvatar {
    pub url: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
}

/// `/friendcloud`'s canvas: each picture clipped to its circle (`arc` + `clip` + `drawImage`), then a 3px `#808080`
/// ring 1px outside it; saved as a PNG in a temporary folder of its own.
fn friend_cloud_png(
    width: u32,
    height: u32,
    pictures: &[(CloudAvatar, Vec<u8>)],
) -> Option<std::path::PathBuf> {
    let mut canvas = tiny_skia::Pixmap::new(width.max(1), height.max(1))?;
    for (avatar, bytes) in pictures {
        let Ok(decoded) = image::load_from_memory(bytes) else {
            continue;
        };
        let side = avatar.size.round().max(1.) as u32;
        let resized = decoded
            .resize_exact(side, side, image::imageops::FilterType::Triangle)
            .to_rgba8();
        let mut data = resized.into_raw();
        // tiny-skia keeps premultiplied colour.
        for pixel in data.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        let Some(picture) =
            tiny_skia::Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(side, side)?)
        else {
            continue;
        };
        let (cx, cy, radius) = (
            (avatar.x + avatar.size / 2.) as f32,
            (avatar.y + avatar.size / 2.) as f32,
            (avatar.size / 2.) as f32,
        );
        let Some(circle) = tiny_skia::PathBuilder::from_circle(cx, cy, radius) else {
            continue;
        };
        let mut mask = tiny_skia::Mask::new(width.max(1), height.max(1))?;
        mask.fill_path(
            &circle,
            tiny_skia::FillRule::Winding,
            true,
            tiny_skia::Transform::identity(),
        );
        let scale = avatar.size as f32 / side as f32;
        canvas.draw_pixmap(
            0,
            0,
            picture.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bilinear,
                ..Default::default()
            },
            tiny_skia::Transform::from_row(scale, 0., 0., scale, avatar.x as f32, avatar.y as f32),
            Some(&mask),
        );
        if let Some(ring) = tiny_skia::PathBuilder::from_circle(cx, cy, radius + 1.) {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(0x80, 0x80, 0x80, 0xff);
            paint.anti_alias = true;
            let stroke = tiny_skia::Stroke {
                width: 3.,
                ..Default::default()
            };
            canvas.stroke_path(
                &ring,
                &paint,
                &stroke,
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }
    let png = canvas.encode_png().ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_millis());
    let dir = std::env::temp_dir().join(format!("gravecord-friendcloud-{stamp}"));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("affinities-cloud.png");
    std::fs::write(&path, png).ok()?;
    Some(path)
}

#[derive(Clone)]
pub struct Requests {
    sender: mpsc::UnboundedSender<Request>,
    settings_writes: Arc<std::sync::atomic::AtomicUsize>,
}

/// The network side is gone: the request was not sent.
#[derive(Debug)]
pub struct Closed;

/// The network's end of the request channel.
pub struct RequestQueue {
    receiver: mpsc::UnboundedReceiver<Request>,
    settings_writes: Arc<std::sync::atomic::AtomicUsize>,
}

pub fn request_channel() -> (Requests, RequestQueue) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let settings_writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        Requests {
            sender,
            settings_writes: settings_writes.clone(),
        },
        RequestQueue {
            receiver,
            settings_writes,
        },
    )
}

impl Requests {
    pub fn send(&self, request: Request) -> Result<(), Closed> {
        use std::sync::atomic::Ordering;
        let counted = is_settings_write(&request);
        if counted {
            self.settings_writes.fetch_add(1, Ordering::SeqCst);
        }
        let sent = self.sender.send(request).map_err(|_| Closed);
        if counted && sent.is_err() {
            self.settings_writes.fetch_sub(1, Ordering::SeqCst);
        }
        sent
    }

    /// Whether a settings write is still on its way.
    pub fn settings_writes_pending(&self) -> bool {
        self.settings_writes
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
    }
}

/// One counted settings write on the network side. However the request ends (answered, failed, refused), dropping
/// this takes it off the count and tells the app, with the settings the server answered if it did.
struct SettingsWrite {
    count: Arc<std::sync::atomic::AtomicUsize>,
    updates: mpsc::UnboundedSender<Update>,
    answer: Option<String>,
}

impl Drop for SettingsWrite {
    fn drop(&mut self) {
        self.count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self
            .updates
            .send(Update::SettingsWritten(self.answer.take()));
    }
}

/// The routes that change the account itself: its settings, its per-server notification settings, its relationships,
/// its profile, its server memberships, and anything that spends or grants.
/// One frame of the Gift Nitro box (module 446263, the Standard Box loop file) as PNG bytes, `side` pixels on the
/// longer side. The renderer has no merge paths (`mm`, the "Zipper" layer): they are left out, which keeps the
/// shapes' own fills and strokes.
pub(crate) fn gift_box_png(json: &[u8], side: u32, frame: f32) -> Option<Vec<u8>> {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(Value::Array(items)) = map.get_mut("it") {
                    items.retain(|item| item["ty"] != "mm");
                }
                map.values_mut().for_each(strip);
            }
            Value::Array(list) => list.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut document: Value = serde_json::from_slice(json).ok()?;
    strip(&mut document);
    crate::lottie::frame_png(&serde_json::to_vec(&document).ok()?, side, Some(frame))
}

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
            Request::Spotify(_) => "Spotify",
            Request::System(_) => "System",
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
            Request::FetchCdn { .. } => "FetchCdn",
            Request::FetchPublicJson { .. } => "FetchPublicJson",
            Request::FriendCloud { .. } => "FriendCloud",
            Request::FetchZip { .. } => "FetchZip",
            Request::GifProvider { .. } => "GifProvider",
            Request::PluginTranslate { .. } => "PluginTranslate",
            Request::PluginService { .. } => "PluginService",
            Request::PluginRpcAssets { .. } => "PluginRpcAssets",
            Request::PluginCloneSticker { .. } => "PluginCloneSticker",
            Request::PluginClipUpload { .. } => "PluginClipUpload",
            Request::ResolveRedirect { .. } => "ResolveRedirect",
            Request::Typing { .. } => "Typing",
            Request::React { .. } => "React",
            Request::LoadMessages { .. } => "LoadMessages",
            Request::PrefetchMessages { .. } => "PrefetchMessages",
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
            Request::DownloadAll { .. } => "DownloadAll",
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
            | Request::FetchCdn { .. }
            | Request::FetchPublicJson { .. }
            | Request::FriendCloud { .. }
            | Request::FetchZip { .. }
            | Request::GifProvider { .. }
            | Request::PluginTranslate { .. }
            | Request::ResolveRedirect { .. }
            | Request::LoadMessages { .. }
            | Request::PrefetchMessages { .. }
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
            Request::System(_) => Inert::Allow,
            // Not in the pinned client: a plugin service's reads pass; its writes and the token trade do not.
            Request::PluginService {
                method, exchange, ..
            } => {
                if *method == reqwest::Method::GET && !exchange {
                    Inert::Allow
                } else {
                    Inert::Deny
                }
            }
            Request::Spotify(_)
            | Request::Presence { .. }
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
            // Not in the pinned client: plugin writes (ExpressionManager's new sticker, CustomRPC's external assets).
            | Request::PluginCloneSticker { .. }
            | Request::PluginRpcAssets { .. }
            | Request::PluginClipUpload { .. }
            | Request::Edit { .. }
            | Request::Delete { .. }
            | Request::Ack { .. }
            | Request::Save { .. }
            | Request::GuildSettings { .. }
            | Request::Download { .. }
            | Request::DownloadAll { .. }
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
    // Not in the pinned client: a refused plugin write answers as its failure, so the plugin does not wait on it.
    match request {
        Request::PluginCloneSticker { key, name, .. } => {
            let body = json!({"ok": false, "name": name, "message": null, "refused": true});
            let _ = updates.send(Update::Fetched {
                key: key.clone(),
                body,
            });
        }
        Request::PluginClipUpload { key, path, .. } => {
            let body = json!({"ok": false, "message": null, "path": path, "refused": true});
            let _ = updates.send(Update::Fetched {
                key: key.clone(),
                body,
            });
        }
        Request::PluginService { key, .. } => {
            let _ = updates.send(Update::Fetched {
                key: key.clone(),
                body: json!({"ok": false, "status": 0, "body": null, "refused": true}),
            });
        }
        Request::PluginRpcAssets { key, keys, .. } => {
            let body = Value::Object(
                keys.iter()
                    .map(|asset| (asset.clone(), Value::Null))
                    .collect(),
            );
            let _ = updates.send(Update::Fetched {
                key: key.clone(),
                body,
            });
        }
        _ => {}
    }
    if let Request::Spotify(crate::spotify::Command::Perform { request, .. }) = request {
        let _ = updates.send(Update::SpotifyReply {
            request: *request,
            reply: crate::spotify::Reply::Refused,
        });
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
///
/// The emoji sheet is stored at 3360 by 3120: decoded whole it took 42 MB, twice over while it was converted. It is
/// read one row of cells at a time instead, and each cell's PNG is kept at its length (an encoder's buffer grows to
/// about twice what it holds, and 2,000 cells are kept for the rest of the run).
fn sheet_cells(
    sheet: &[u8],
    count: u32,
    columns: u32,
    prefix: &str,
) -> image::ImageResult<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::with_capacity(count as usize);
    let mut keep = |index: u32, mut cell: image::RgbaImage| -> image::ImageResult<()> {
        if cell.width() != SHEET_CELL {
            cell = image::imageops::resize(
                &cell,
                SHEET_CELL,
                SHEET_CELL,
                image::imageops::FilterType::Triangle,
            );
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        cell.write_to(&mut bytes, image::ImageFormat::Png)?;
        let mut bytes = bytes.into_inner();
        bytes.shrink_to_fit();
        out.push((format!("{prefix}:{index}"), bytes));
        Ok(())
    };
    let cell_size = |width: u32| {
        let scale = f64::from(width) / f64::from(columns * SHEET_CELL);
        (f64::from(SHEET_CELL) * scale).round() as u32
    };
    if let Some(mut rows) = SheetRows::new(sheet) {
        let (width, height) = (rows.width, rows.height);
        let size = cell_size(width);
        let mut band: Option<(u32, image::RgbaImage)> = None;
        for index in 0..count {
            let (column, row) = (index % columns, index / columns);
            let (x, y) = (column * size, row * size);
            if x + size > width || y + size > height {
                break;
            }
            if band.as_ref().is_none_or(|(at, _)| *at != row) {
                band = Some((row, rows.next_band(size)?));
            }
            let Some((_, pixels)) = &band else {
                break;
            };
            keep(
                index,
                image::imageops::crop_imm(pixels, x, 0, size, size).to_image(),
            )?;
        }
        return Ok(out);
    }
    let sheet = image::load_from_memory_with_format(sheet, image::ImageFormat::Png)?.to_rgba8();
    let size = cell_size(sheet.width());
    for index in 0..count {
        let (column, row) = (index % columns, index / columns);
        let (x, y) = (column * size, row * size);
        if x + size > sheet.width() || y + size > sheet.height() {
            break;
        }
        keep(
            index,
            image::imageops::crop_imm(&sheet, x, y, size, size).to_image(),
        )?;
    }
    Ok(out)
}

/// A PNG read from the top a band of rows at a time, as RGBA: the pixels `image` decodes it to (`png` with the
/// `EXPAND` transformation, then RGB given an opaque alpha). Only 8-bit RGB and RGBA output that is not interlaced is
/// read this way; anything else is decoded whole.
struct SheetRows<'a> {
    reader: png::Reader<std::io::Cursor<&'a [u8]>>,
    width: u32,
    height: u32,
    alpha: bool,
}

impl<'a> SheetRows<'a> {
    fn new(bytes: &'a [u8]) -> Option<Self> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let reader = decoder.read_info().ok()?;
        let (width, height) = (reader.info().width, reader.info().height);
        let alpha = match reader.output_color_type() {
            (png::ColorType::Rgba, png::BitDepth::Eight) => true,
            (png::ColorType::Rgb, png::BitDepth::Eight) => false,
            _ => return None,
        };
        if reader.info().interlaced {
            return None;
        }
        Some(Self {
            reader,
            width,
            height,
            alpha,
        })
    }

    /// The next `rows` rows of the picture.
    fn next_band(&mut self, rows: u32) -> image::ImageResult<image::RgbaImage> {
        fn decoding(
            error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
        ) -> image::ImageError {
            image::ImageError::Decoding(image::error::DecodingError::new(
                image::error::ImageFormatHint::Exact(image::ImageFormat::Png),
                error,
            ))
        }
        let mut band = Vec::with_capacity(self.width as usize * rows as usize * 4);
        for _ in 0..rows {
            let row = self
                .reader
                .next_row()
                .map_err(decoding)?
                .ok_or_else(|| decoding("the picture ended before its rows"))?;
            if self.alpha {
                band.extend_from_slice(row.data());
            } else {
                for pixel in row.data().as_chunks::<3>().0 {
                    band.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
            }
        }
        image::RgbaImage::from_raw(self.width, rows, band)
            .ok_or_else(|| decoding("a row is shorter than the picture"))
    }
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
    // `_sendMessage` takes `@silent` off before it schedules too (module 677413).
    let (silent, content) = dc_state::split_silent_prefix(content);
    let build = |attachments: Vec<Value>| {
        dc_state::scheduled_message::create_body(dc_state::scheduled_message::CreateRequest {
            channel,
            content,
            scheduled_timestamp: timestamp,
            flags: silent.then_some(dc_state::SUPPRESS_NOTIFICATIONS_FLAG),
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
    extra_flags: u64,
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
    body["flags"] = json!(dc_state::VOICE_MESSAGE_FLAG | extra_flags);
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
    // A leading `@silent` is sent as the SUPPRESS_NOTIFICATIONS flag (module 339200's silentPrefix, 677413).
    let (silent, content) = dc_state::split_silent_prefix(content);
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
    body["flags"] = json!(if silent {
        dc_state::SUPPRESS_NOTIFICATIONS_FLAG
    } else {
        0
    });
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
        // `message_reference` of type 1: the forward of module 138399. Its flags are SUPPRESS_NOTIFICATIONS when the
        // note beside it starts with `@silent` (module 92961), which the forward's own content carries.
        body["message_reference"] = json!({
            "guild_id": guild.map(|guild| guild.to_string()),
            "channel_id": source.to_string(),
            "message_id": message.to_string(),
            "type": 1,
        });
        // Not in the pinned client: BetterForwards' choice of attachments and embeds (`forward_only`, which module
        // 138399 sends for `forwardOptions`; `plugins/better_forwards.rs`).
        if let Some((attachment_ids, embed_indices)) = crate::plugins::better_forwards::take(nonce)
        {
            body["message_reference"]["forward_only"] = json!({
                "attachment_ids": attachment_ids,
                "embed_indices": embed_indices,
            });
        }
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
        let mut retry_after_wait = None;
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
            Ok(Err(dc_http::Error::RateLimited { retry_after, .. })) => {
                let failure = Failure {
                    status: Some(429),
                    retry_after,
                    ..Failure::default()
                };
                if retries > 0 {
                    retry_after_wait = retry_after
                        .filter(|seconds| seconds.is_finite() && *seconds > 0.)
                        .map(std::time::Duration::from_secs_f64);
                    failure
                } else {
                    warn!("sending stayed rate limited");
                    return Outcome::Failed(failure);
                }
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
        let wait = backoff.fail(jitter, retry_after_wait);
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

/// The page asked for at the start (`Request::PrefetchMessages`), from the request to its one use.
#[derive(Debug, Default, PartialEq)]
enum StartPage {
    /// Nothing was asked for yet.
    #[default]
    None,
    /// On its way; nobody has asked for it yet.
    InFlight(u64),
    /// On its way, and the view's load of that channel waits for it.
    Claimed(u64),
    /// Arrived (and was delivered) before the view asked: the page, kept for that ask.
    Done {
        channel: u64,
        page: Value,
        started_ms: u64,
    },
    /// Used, failed or superseded: every load goes to the server.
    Spent,
}

/// What a `LoadMessages` of `channel` does with the start page.
#[derive(Debug, PartialEq)]
enum StartPageUse {
    /// The page is on its way and will be delivered: no request.
    Wait,
    /// The page arrived before: deliver it again (the view now has the channel open), no request.
    Deliver { page: Value, started_ms: u64 },
    /// Ask the server.
    Fetch,
    /// After READY, the page that came before it: drawn now, and the server asked as for `Fetch`, its answer
    /// replacing the page (as a live page replaces the start cache's).
    DeliverThenFetch { page: Value, started_ms: u64 },
}

impl StartPage {
    /// A `LoadMessages` of `channel` arrived. Only a load before READY uses the page alone: what was posted between
    /// the page and READY never reaches the gateway's events, so a load after READY always asks the server. A page
    /// that is in by then (a first start opens its channel only at READY) is drawn while that answer comes.
    fn claim(&mut self, channel: u64, ready_seen: bool) -> StartPageUse {
        if ready_seen {
            return match std::mem::take(self) {
                StartPage::None => StartPageUse::Fetch,
                StartPage::Done {
                    channel: asked,
                    page,
                    started_ms,
                } if asked == channel => {
                    *self = StartPage::Spent;
                    StartPageUse::DeliverThenFetch { page, started_ms }
                }
                _ => {
                    *self = StartPage::Spent;
                    StartPageUse::Fetch
                }
            };
        }
        match std::mem::take(self) {
            StartPage::InFlight(asked) if asked == channel => {
                *self = StartPage::Claimed(asked);
                StartPageUse::Wait
            }
            StartPage::Done {
                channel: asked,
                page,
                started_ms,
            } if asked == channel => {
                *self = StartPage::Spent;
                StartPageUse::Deliver { page, started_ms }
            }
            other => {
                *self = other;
                StartPageUse::Fetch
            }
        }
    }

    /// The page came in: whether it is delivered. A page nobody asked for yet is kept for the ask; one a load waits
    /// for goes out; one superseded by a load after READY is dropped, so it can never land after the newer page.
    fn arrived(&mut self, page: &Value, started_ms: u64) -> bool {
        let (next, deliver) = match std::mem::take(self) {
            StartPage::InFlight(channel) => (
                StartPage::Done {
                    channel,
                    page: page.clone(),
                    started_ms,
                },
                true,
            ),
            StartPage::Claimed(_) => (StartPage::Spent, true),
            _ => (StartPage::Spent, false),
        };
        *self = next;
        deliver
    }

    /// The page failed. A load that waited for it must now ask the server: the channel is returned for that.
    fn failed(&mut self) -> Option<u64> {
        match std::mem::replace(self, StartPage::Spent) {
            StartPage::Claimed(channel) => Some(channel),
            _ => None,
        }
    }
}

/// `GET /channels/{id}/messages`, the newest page, delivered as `Update::Messages` (or `MessagesFailed`).
async fn load_messages(
    client: &Client,
    channel: u64,
    updates: &mpsc::UnboundedSender<Update>,
) -> bool {
    crate::start_trace::mark("msgs_req");
    let started_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default();
    let route = format!("/channels/{channel}/messages?limit={MESSAGE_PAGE}");
    match client.get_hedged(&route, PAGE_HEDGE_AFTER).await {
        Ok(page) => {
            crate::start_trace::mark("msgs_recv");
            let _ = updates.send(Update::Messages {
                channel,
                page,
                started_ms,
            });
            true
        }
        Err(error) => {
            warn!("loading messages failed: {error}");
            // The channel the app reopened may be one this account lost access to: the loading
            // screen must not wait for a page that will never come.
            let _ = updates.send(Update::MessagesFailed { channel });
            false
        }
    }
}

/// The network session of one login, started as soon as the login is known: before the window exists at a start
/// with a stored login, so the gateway and the first requests do not wait for the window and the fonts.
pub struct Session {
    pub updates: mpsc::UnboundedSender<Update>,
    pub update_rx: mpsc::UnboundedReceiver<Update>,
    pub requests: Requests,
    pub task: tokio::task::JoinHandle<()>,
}

impl Session {
    pub fn start(handle: &tokio::runtime::Handle, token: Token) -> Self {
        let (updates, update_rx) = mpsc::unbounded_channel();
        let (requests, queue) = request_channel();
        let task = handle.spawn(run(token, updates.clone(), queue));
        Self {
            updates,
            update_rx,
            requests,
            task,
        }
    }
}

struct Shutdown(mpsc::UnboundedSender<Command>);

impl Drop for Shutdown {
    fn drop(&mut self) {
        let _ = self.0.send(Command::Shutdown);
    }
}

pub async fn run(token: Token, updates: mpsc::UnboundedSender<Update>, requests: RequestQueue) {
    crate::start_trace::mark("net_run");
    let RequestQueue {
        receiver: mut requests,
        settings_writes,
    } = requests;
    let identity = Identity::new();
    let dead = updates.clone();
    let limited = updates.clone();
    let client = match Client::new(token.clone(), identity.clone()).map(|client| {
        client
            .on_unauthorized(move || {
                LOGIN_REFUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = dead.send(Update::AuthFailed);
            })
            .on_limited_access(move || {
                let _ = limited.send(Update::LimitedAccess);
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
    let spotify = crate::spotify::Worker::start(client.clone(), updates.clone());
    let spotify_observer = spotify.as_ref().map(crate::spotify::Worker::observer);
    let (commands, mut events) = spawn(Config {
        token,
        identity,
        state: IdentifyState::default(),
    });
    // Network and power events, for as long as this login (`system_events`).
    let (system_send, mut system_events) = mpsc::unbounded_channel();
    let _system_watch = crate::system_events::start(system_send);
    let system_updates = updates.clone();
    tokio::spawn(async move {
        while let Some(event) = system_events.recv().await {
            if system_updates.send(Update::System(event)).is_err() {
                break;
            }
        }
    });

    if let Ok(mut slot) = EXIT_GATEWAY.lock() {
        *slot = Some(commands.clone());
    }
    // Dropping this future (logging out) closes the gateway.
    let _gateway = Shutdown(commands.clone());
    let forward = updates.clone();
    let start_page = Arc::new(std::sync::Mutex::new(StartPage::None));
    let ready_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_ready = ready_seen.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                Event::Dispatch { name, data } => {
                    if let Some(observer) = &spotify_observer {
                        observer.event(&name, &data);
                    }
                    match name.as_str() {
                        "READY" => crate::start_trace::mark("gw_ready_recv"),
                        "READY_SUPPLEMENTAL" => crate::start_trace::mark("gw_supp_recv"),
                        _ => {}
                    }
                    let connected = matches!(name.as_str(), "READY" | "RESUMED");
                    if connected {
                        saw_ready.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    let _ = forward.send(Update::Dispatch { name, data });
                    if connected {
                        let _ = forward.send(Update::GatewayConnected(true));
                    }
                }
                Event::AuthenticationFailed => {
                    warn!("gateway rejected the token");
                    LOGIN_REFUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = forward.send(Update::AuthFailed);
                }
                Event::Reconnecting { delay } => {
                    warn!("gateway reconnecting in {delay:?}");
                    let _ = forward.send(Update::GatewayConnected(false));
                }
                Event::Connected => crate::start_trace::mark("gw_hello"),
                Event::Reset => {
                    let _ = forward.send(Update::GatewayReset);
                }
                Event::Closed { .. } => {
                    let _ = forward.send(Update::GatewayConnected(false));
                }
            }
        }
    });

    while let Some(request) = requests.recv().await {
        let mut settings_write = is_settings_write(&request).then(|| SettingsWrite {
            count: settings_writes.clone(),
            updates: updates.clone(),
            answer: None,
        });
        // A simulated call answers its own voice requests (`ui/callsim.rs`); nothing of them goes further.
        #[cfg(feature = "capture")]
        let Some(request) = crate::ui::callsim::intercept(request, &updates) else {
            continue;
        };
        if inert_refuses(&request, &updates) {
            continue;
        }
        let client = client.clone();
        let updates = updates.clone();
        match request {
            Request::Spotify(command) => {
                if let Some(worker) = &spotify {
                    worker.request(command);
                } else if let crate::spotify::Command::Perform { request, .. } = command {
                    let _ = updates.send(Update::SpotifyReply {
                        request,
                        reply: crate::spotify::Reply::Refused,
                    });
                }
            }
            Request::LoadMessages { channel } => {
                let ready = ready_seen.load(std::sync::atomic::Ordering::SeqCst);
                let used = start_page
                    .lock()
                    .map(|mut page| page.claim(channel, ready))
                    .unwrap_or(StartPageUse::Fetch);
                match used {
                    StartPageUse::Wait => {}
                    StartPageUse::Deliver { page, started_ms } => {
                        let _ = updates.send(Update::Messages {
                            channel,
                            page,
                            started_ms,
                        });
                    }
                    StartPageUse::Fetch => {
                        tokio::spawn(async move {
                            load_messages(&client, channel, &updates).await;
                        });
                    }
                    StartPageUse::DeliverThenFetch { page, started_ms } => {
                        crate::start_trace::mark("start_page_after_ready");
                        let _ = updates.send(Update::Messages {
                            channel,
                            page,
                            started_ms,
                        });
                        tokio::spawn(async move {
                            load_messages(&client, channel, &updates).await;
                        });
                    }
                }
            }
            Request::PrefetchMessages { channel } => {
                let first = start_page.lock().is_ok_and(|mut page| {
                    let first = *page == StartPage::None;
                    if first {
                        *page = StartPage::InFlight(channel);
                    }
                    first
                });
                if first {
                    crate::start_trace::mark("msgs_prefetch");
                    let start_page = start_page.clone();
                    tokio::spawn(async move {
                        let started_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|elapsed| elapsed.as_millis() as u64)
                            .unwrap_or_default();
                        let route = format!("/channels/{channel}/messages?limit={MESSAGE_PAGE}");
                        match client.get_hedged(&route, PAGE_HEDGE_AFTER).await {
                            Ok(page) => {
                                crate::start_trace::mark("msgs_recv");
                                let deliver = start_page
                                    .lock()
                                    .is_ok_and(|mut slot| slot.arrived(&page, started_ms));
                                if deliver {
                                    let _ = updates.send(Update::Messages {
                                        channel,
                                        page,
                                        started_ms,
                                    });
                                }
                            }
                            Err(error) => {
                                debug!("the start page did not load: {error}");
                                let waiting =
                                    start_page.lock().ok().and_then(|mut slot| slot.failed());
                                if let Some(channel) = waiting {
                                    load_messages(&client, channel, &updates).await;
                                }
                            }
                        }
                    });
                }
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
                extra_flags,
            } => {
                tokio::spawn(async move {
                    let body = send_voice(
                        &client,
                        channel,
                        file,
                        duration_secs,
                        &waveform,
                        reply,
                        extra_flags,
                    )
                    .await;
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::Edit {
                channel,
                message,
                content,
            } => {
                // The answer comes back (`MESSAGE_END_EDIT {response}` and `MESSAGE_EDIT_FAILED_AUTOMOD` of
                // `editMessage`, module 138399) under `message-edit:{channel}:{message}`, shaped like `Request::Rest`'s.
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages/{message}");
                    let body = json!({"content": content});
                    let report = match client
                        .request(reqwest::Method::PATCH, &route, Some(&body))
                        .await
                    {
                        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
                        Err(dc_http::Error::Status { status, body, .. }) => {
                            warn!("editing failed: {status}");
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("editing failed: {error}");
                            json!({"ok": false, "status": 0, "body": null})
                        }
                    };
                    let _ = updates.send(Update::Fetched {
                        key: format!("{EDIT_KEY}{channel}:{message}"),
                        body: report,
                    });
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
                        let _ = updates.send(Update::SaveFailed {
                            channel,
                            message,
                            save,
                        });
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
                    if let (Some(write), Ok(value)) = (settings_write.as_mut(), outcome.as_ref()) {
                        write.answer = value["settings"].as_str().map(str::to_owned);
                    }
                    // The count goes down before the answer reaches the app, so the app sees "none pending" with it.
                    drop(settings_write);
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
                            warn!("{} failed: {status}", dc_http::redact::url(&route));
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{} failed: {error}", dc_http::redact::url(&route));
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
                            warn!("{} failed: {status}", dc_http::redact::url(&route));
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{} failed: {error}", dc_http::redact::url(&route));
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
                            body: json!({
                                "ok": saved.is_some(),
                                "filename": saved,
                            }),
                        });
                    }
                });
            }
            Request::DownloadAll {
                files,
                archive,
                key,
            } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let total = files.len();
                    let saved = save_all(files, &archive).await;
                    let _ = updates.send(Update::Fetched {
                        key,
                        body: json!({"saved": saved, "total": total}),
                    });
                });
            }
            Request::BearerGet { key, route, bearer } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    let report = match client.get_as_bearer(&route, &bearer).await {
                        Ok(value) => json!({"ok": true, "status": 200, "body": value}),
                        Err(dc_http::Error::Status { status, body, .. }) => {
                            warn!("{} failed: {status}", dc_http::redact::url(&route));
                            json!({"ok": false, "status": status.as_u16(), "body": body})
                        }
                        Err(error) => {
                            warn!("{} failed: {error}", dc_http::redact::url(&route));
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
                        let _ = updates.send(Update::ReactFailed {
                            channel,
                            message,
                            emoji,
                            add,
                        });
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
                let flight = InFlight::start();
                tokio::spawn(async move {
                    let _flight = flight;
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
            Request::ResolveRedirect { key, url } => {
                tokio::spawn(async move {
                    let resolved = resolve_redirect(&url).await;
                    let _ = updates.send(Update::Fetched {
                        key,
                        body: json!({"url": resolved}),
                    });
                });
            }
            Request::FetchPublicJson { key, url } => {
                tokio::spawn(async move {
                    let body = match client.fetch_public(&url).await {
                        Ok(bytes) => {
                            serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|error| {
                                warn!("public json not parsed: {error}");
                                Value::Null
                            })
                        }
                        Err(error) => {
                            warn!("public json fetch failed: {error}");
                            Value::Null
                        }
                    };
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::FriendCloud {
                key,
                width,
                height,
                avatars,
            } => {
                tokio::spawn(async move {
                    let mut pictures = Vec::new();
                    for avatar in avatars {
                        // `loadFriendImage`: a picture that fails is left out.
                        if let Ok(bytes) = client.fetch_public(&avatar.url).await {
                            pictures.push((avatar, bytes));
                        }
                    }
                    let path = tokio::task::spawn_blocking(move || {
                        friend_cloud_png(width, height, &pictures)
                    })
                    .await
                    .ok()
                    .flatten();
                    let _ = updates.send(Update::Fetched {
                        key,
                        body: json!({"path": path.map(|path| path.display().to_string())}),
                    });
                });
            }
            Request::PluginClipUpload {
                key,
                channel,
                path,
                options,
                reply,
            } => {
                tokio::spawn(async move {
                    let mut body = clip_upload(&client, channel, &path, &options, reply).await;
                    body["path"] = json!(path);
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::PluginCloneSticker {
                key,
                guild,
                sticker,
                format,
                name,
                tags,
                description,
            } => {
                tokio::spawn(async move {
                    let body =
                        clone_sticker(&client, guild, sticker, format, &name, &tags, &description)
                            .await;
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::PluginRpcAssets { key, app_id, keys } => {
                tokio::spawn(async move {
                    let mut out = serde_json::Map::new();
                    let (links, names): (Vec<String>, Vec<String>) = keys
                        .into_iter()
                        .partition(|key| key.starts_with("http://") || key.starts_with("https://"));
                    if !names.is_empty() {
                        let listed = client
                            .get(&format!("/oauth2/applications/{app_id}/assets"))
                            .await
                            .unwrap_or(Value::Null);
                        for name in names {
                            let id = listed
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|asset| asset["name"].as_str() == Some(name.as_str()))
                                .and_then(|asset| asset["id"].as_str().map(str::to_owned));
                            out.insert(name, id.map_or(Value::Null, Value::from));
                        }
                    }
                    if !links.is_empty() {
                        let answer = client
                            .post(
                                &format!("/applications/{app_id}/external-assets"),
                                &json!({"urls": links}),
                            )
                            .await
                            .unwrap_or(Value::Null);
                        for (index, link) in links.into_iter().enumerate() {
                            let path = answer[index]["external_asset_path"].as_str();
                            out.insert(
                                link,
                                path.map_or(Value::Null, |path| Value::from(format!("mp:{path}"))),
                            );
                        }
                    }
                    let _ = updates.send(Update::Fetched {
                        key,
                        body: Value::Object(out),
                    });
                });
            }
            Request::PluginService {
                key,
                method,
                url,
                bearer,
                body,
                ..
            } => {
                tokio::spawn(async move {
                    let body = plugin_service(method, &url, bearer, body).await;
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::PluginTranslate {
                key,
                text,
                target,
                toki,
                sitelen,
                shavian,
            } => {
                tokio::spawn(async move {
                    let body = translate_plus(&client, text, &target, toki, sitelen, shavian)
                        .await
                        .unwrap_or(Value::Null);
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::GifProvider {
                key,
                provider,
                ask,
                limit,
            } => {
                tokio::spawn(async move {
                    use crate::plugins::gif_provider::{self as gif, Ask, Provider};
                    let json = |url: String| {
                        let client = client.clone();
                        async move {
                            let bytes = client.fetch_public(&url).await.ok()?;
                            serde_json::from_slice::<Value>(&bytes).ok()
                        }
                    };
                    let body = match &ask {
                        Ask::Share { .. } => {
                            if let Some(url) = gif::address(provider, &ask, 0, "", 0) {
                                let _ = json(url).await;
                            }
                            return;
                        }
                        Ask::Categories => match gif::address(provider, &ask, 0, "", 0) {
                            Some(url) => json(url)
                                .await
                                .map_or(Value::Null, |body| gif::categories(provider, &body)),
                            None => Value::Null,
                        },
                        Ask::Trending | Ask::Search(_) => {
                            // `fetchTenorResults` follows `next` until the limit; Giphy pages by offset.
                            let mut items: Vec<Value> = Vec::new();
                            let mut pos = String::new();
                            let mut offset = 0;
                            let mut failed = false;
                            while (items.len() as u32) < limit {
                                let size = (limit - items.len() as u32).min(gif::PAGE);
                                let Some(url) = gif::address(provider, &ask, size, &pos, offset)
                                else {
                                    break;
                                };
                                let Some(body) = json(url).await else {
                                    failed = items.is_empty();
                                    break;
                                };
                                let (page, next) = gif::page(provider, &body);
                                if page.is_empty() {
                                    break;
                                }
                                let before = items.len();
                                items.extend(page);
                                offset += size;
                                match (provider, next) {
                                    (Provider::Tenor, Some(next))
                                        if next != pos && items.len() > before =>
                                    {
                                        pos = next
                                    }
                                    (Provider::Giphy, _) if items.len() > before => {}
                                    _ => break,
                                }
                            }
                            if failed {
                                Value::Null
                            } else {
                                gif::results(provider, &items, limit as usize)
                            }
                        }
                    };
                    let _ = updates.send(Update::Fetched { key, body });
                });
            }
            Request::FetchZip { key, url, native } => {
                tokio::spawn(async move {
                    use crate::plugins::zip_preview as zip;
                    let fetch_url = native.as_ref().map_or_else(
                        || url.clone(),
                        |path| format!("https://cdn.discordapp.com/attachments/{path}"),
                    );
                    // Only Discord's hosts are fetched for a preview, whoever asked (`zip::load`).
                    if native.is_none() && zip::load(&url) != zip::Load::Discord {
                        zip::reject(&url, zip::NOT_FETCHED.to_owned());
                        let _ = updates.send(Update::Fetched {
                            key,
                            body: Value::Null,
                        });
                        return;
                    }
                    let fetched = client
                        .fetch_public_limited(&fetch_url, zip::MAX_ZIP_BYTES)
                        .await;
                    let bytes = match (fetched, native.is_some()) {
                        (Ok(dc_http::LimitedGet::Body(bytes)), _) => Ok(bytes),
                        (Ok(dc_http::LimitedGet::TooLarge), _) => Err(zip::TOO_LARGE.to_owned()),
                        (Ok(dc_http::LimitedGet::Status(status, reason)), true) => {
                            Err(format!("Fetch failed: {status} {reason}"))
                        }
                        (Ok(dc_http::LimitedGet::Status(..)), false) => {
                            Err("Could not fetch ZIP.".to_owned())
                        }
                        (Err(error), _) => Err(error.to_string()),
                    };
                    let parsed = match bytes {
                        Ok(bytes) => tokio::task::spawn_blocking(move || zip::parse(&bytes))
                            .await
                            .unwrap_or_else(|_| Err("Failed to preview ZIP.".to_owned())),
                        Err(message) => Err(message),
                    };
                    match parsed {
                        Ok(archive) => {
                            zip::set_state(&url, zip::State::Resolved(std::sync::Arc::new(archive)))
                        }
                        Err(message) => zip::reject(&url, message),
                    }
                    let _ = updates.send(Update::Fetched {
                        key,
                        body: Value::Null,
                    });
                });
            }
            Request::FetchCdn { key, url, etag } => {
                tokio::spawn(async move {
                    let update = match client.fetch_public_conditional(&url, etag.as_deref()).await
                    {
                        Ok(dc_http::ConditionalGet::NotModified { etag }) => {
                            Update::DetectablesCdn {
                                key,
                                status: 304,
                                etag,
                                games: None,
                            }
                        }
                        Ok(dc_http::ConditionalGet::Ok { etag, bytes }) => {
                            let parse_key = key.clone();
                            let parse_etag = etag.clone();
                            let games = tokio::task::spawn_blocking(move || {
                                crate::games::parse_and_cache_cdn(
                                    &parse_key,
                                    parse_etag.as_deref(),
                                    &bytes,
                                )
                            })
                            .await
                            .ok()
                            .flatten();
                            Update::DetectablesCdn {
                                key,
                                status: 200,
                                etag,
                                games,
                            }
                        }
                        Err(error) => {
                            warn!("cdn fetch failed for {key}: {error}");
                            Update::DetectablesCdn {
                                key,
                                status: 0,
                                etag: None,
                                games: None,
                            }
                        }
                    };
                    let _ = updates.send(update);
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
            Request::System(event) => {
                let _ = commands.send(Command::System(event));
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
                generation,
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
                        if updates
                            .send(Update::Stream {
                                generation,
                                key,
                                event,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    let _ = task.await;
                });
            }
            Request::VoiceConnect {
                join,
                controls,
                generation,
            } => {
                let (task, mut events) = dc_voice::connect(join, &controls);
                let updates = updates.clone();
                tokio::spawn(async move {
                    while let Some(event) = events.recv().await {
                        if updates.send(Update::Voice(generation, event)).is_err() {
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
            Request::Subscribe { subscriptions } => {
                for data in dc_state::guild_subscriptions::bulk_payloads(&subscriptions) {
                    debug!("guild subscriptions sent: {data}");
                    let _ = commands.send(Command::Send {
                        op: Op::GuildSubscriptionsBulk,
                        data,
                    });
                }
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
                    match fetch_cached_sheet(&client, EMOJI_SPRITE)
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
                    match fetch_cached_sheet(&client, EMOJI_SPRITE_COLORED).await {
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
                        match fetch_cached_sheet(&client, url).await {
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
                let flight = InFlight::start();
                crate::start_trace::mark("img_first_req");
                tokio::spawn(async move {
                    let _flight = flight;
                    // A `data:` picture (UserPFP's uploaded avatars) is decoded here, never fetched.
                    if let Some(bytes) = data_url_bytes(&url) {
                        let _ = updates.send(Update::Image { key, bytes });
                        return;
                    }
                    // A Shop or CDN picture kept on disk is drawn at once; a stale one is fetched again behind it.
                    let shop_box = shop_art_box(&key);
                    let cache_raw =
                        !key.starts_with("shop:img:") && disk_cacheable_raw_image(&key, &url);
                    if key.starts_with("shop:img:") {
                        let (at, drawn_in) = (url.clone(), shop_box);
                        let cached = tokio::task::spawn_blocking(move || {
                            crate::ui::shop::art_cache::read(&at, drawn_in)
                        })
                        .await
                        .ok()
                        .flatten();
                        if let Some(cached) = cached {
                            let _ = updates.send(Update::Image {
                                key: key.clone(),
                                bytes: cached.bytes,
                            });
                            if !cached.stale {
                                return;
                            }
                        }
                    } else if cache_raw {
                        let at = url.clone();
                        let cached = tokio::task::spawn_blocking(move || {
                            crate::ui::shop::art_cache::read_raw(&at)
                        })
                        .await
                        .ok()
                        .flatten();
                        if let Some(cached) = cached {
                            let image_key = key.clone();
                            let _slot = IMAGE_DRAWS.acquire().await;
                            let decoded = tokio::task::spawn_blocking(move || {
                                cached_image_bytes(&image_key, cached.bytes)
                            })
                            .await
                            .ok()
                            .flatten();
                            if let Some(bytes) = decoded {
                                let _ = updates.send(Update::Image {
                                    key: key.clone(),
                                    bytes,
                                });
                                if !cached.stale {
                                    return;
                                }
                            }
                        }
                    }
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
                            Err(("injected failure".to_owned(), injected_image_status()))
                        } else {
                            match tokio::time::timeout(IMAGE_TIMEOUT, client.fetch_public(&url))
                                .await
                            {
                                Ok(result) => {
                                    result.map_err(|error| (error.to_string(), http_status(&error)))
                                }
                                Err(_) => {
                                    Err((format!("no answer in {} s", IMAGE_TIMEOUT.as_secs()), 0))
                                }
                            }
                        }
                    };
                    // A body cut short (a connection dropped mid-transfer) would be stored and fail to decode for
                    // good, on disk too: it is a failure to retry instead.
                    let fetched = fetched.and_then(|bytes| {
                        if crate::ui::image_retry::complete(&bytes)
                            || same_length_again(&url, bytes.len())
                        {
                            Ok(bytes)
                        } else {
                            Err((format!("body cut short at {} bytes", bytes.len()), 0))
                        }
                    });
                    match fetched {
                        // (`nitro-art:gift-box` is the Gift Inventory's PNG, a different picture.)
                        Ok(bytes) if key == "nitro-art:gift-box-lottie" => {
                            // The Gift Nitro box (module 446263, the Standard Box loop file): one frame at twice its
                            // 435px width, drawn off the UI thread.
                            let _slot = IMAGE_DRAWS.acquire().await;
                            // Frames 0 to 8 and 60 to 71 are the resting pose the capture shows.
                            match tokio::task::spawn_blocking(move || gift_box_png(&bytes, 870, 0.))
                                .await
                            {
                                Ok(Some(bytes)) => {
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                _ => {
                                    warn!("lottie failed");
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
                                }
                            }
                        }
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
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
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
                            let _slot = LOTTIE_STILLS.acquire().await;
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
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
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
                            let _slot = SHOP_DRAWS.acquire().await;
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
                                    let (at, written) = (url.clone(), bytes.clone());
                                    tokio::task::spawn_blocking(move || {
                                        crate::ui::shop::art_cache::write(&at, shop_box, &written)
                                    });
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                Err(_) => {
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
                                }
                            }
                        }
                        Ok(bytes)
                            if (key.starts_with("gif:") || key.starts_with("gif-category:"))
                                && bytes.len() >= 12
                                && &bytes[4..8] == b"ftyp" =>
                        {
                            if cache_raw {
                                let (at, written) = (url.clone(), bytes.clone());
                                tokio::task::spawn_blocking(move || {
                                    crate::ui::shop::art_cache::write_raw(&at, &written);
                                });
                            }
                            let _decode = IMAGE_DRAWS.acquire().await;
                            let png = tokio::task::spawn_blocking(move || {
                                crate::video_clip::mp4_first_frame_png(&bytes)
                            })
                            .await
                            .ok()
                            .flatten();
                            match png {
                                Some(png) => {
                                    let _ = updates.send(Update::Image { key, bytes: png });
                                }
                                None => {
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
                                }
                            }
                        }
                        Ok(bytes) if url.starts_with(crate::ui::cdn::SPOTIFY_IMAGES) => {
                            // Spotify's album art is 640px; a card draws it at 100px at most. Drawn from the whole
                            // file the GPU would sample it down five times and more, which aliases (the official
                            // client's Chromium filters it), and the decoded copy would hold 1.6 MB. It is cut to the
                            // card's size times the window scale here, off the UI thread, with a proper filter.
                            let scale =
                                SHOP_SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.;
                            let side =
                                crate::ui::cdn::side_for(crate::ui::cdn::ACTIVITY_ART_DRAWN, scale);
                            let _slot = IMAGE_DRAWS.acquire().await;
                            let fitted = tokio::task::spawn_blocking(move || {
                                crate::ui::image_fit::fit(&bytes, side).unwrap_or(bytes)
                            })
                            .await;
                            match fitted {
                                Ok(bytes) => {
                                    let _ = updates.send(Update::Image { key, bytes });
                                }
                                Err(_) => {
                                    let _ = updates.send(Update::ImageFailed {
                                        key,
                                        failure: crate::ui::image_retry::Failure::Transient,
                                    });
                                }
                            }
                        }
                        Ok(bytes) => {
                            if cache_raw {
                                let (at, written) = (url.clone(), bytes.clone());
                                tokio::task::spawn_blocking(move || {
                                    crate::ui::shop::art_cache::write_raw(&at, &written);
                                });
                            }
                            let _ = updates.send(Update::Image { key, bytes });
                        }
                        Err((error, status)) => {
                            warn!("image failed: {error}");
                            #[cfg(feature = "capture")]
                            if let Ok(mut errors) = IMAGE_ERRORS.lock() {
                                errors.insert(key.clone(), error.clone());
                            }
                            let _ = updates.send(Update::ImageFailed {
                                key,
                                failure: crate::ui::image_retry::Failure::of_status(status),
                            });
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
pub(crate) fn fetch_failure(key: &str, error: &dc_http::Error) -> Value {
    let (status, body) = match error {
        dc_http::Error::Status { status, body, .. } => (status.as_u16(), body.clone()),
        // The wait the last 429 asked for stays where readers of a 429 body look (`body.retry_after`, seconds).
        dc_http::Error::RateLimited { retry_after, .. } => (
            429,
            retry_after.map_or(Value::Null, |seconds| json!({"retry_after": seconds})),
        ),
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

/// What a plugin service call sends (`Request::PluginService`).
pub enum ServiceBody {
    None,
    /// `multipart/form-data`: text fields, and files read from disk at send time.
    Form(Vec<(String, ServicePart)>),
}

pub enum ServicePart {
    Text(String),
    File {
        path: std::path::PathBuf,
        name: String,
        mime: String,
    },
}

/// Not in the pinned client: a plugin service call (`Request::PluginService`). A Discord address is refused, so a
/// service token never reaches Discord and the account's never leaves for the service.
async fn plugin_service(
    method: reqwest::Method,
    url: &str,
    bearer: Option<String>,
    body: ServiceBody,
) -> Value {
    if is_discord_url(url) || !url.starts_with("https://") {
        warn!("plugin service call refused: not a service address");
        return json!({"ok": false, "status": 0, "body": null});
    }
    let mut request = dc_http::plain::client()
        .request(method, dc_proto::replay::rewrite(url).as_ref())
        .header(reqwest::header::ACCEPT, "application/json, text/plain");
    if let Some(bearer) = bearer {
        request = request.bearer_auth(bearer);
    }
    request = match body {
        ServiceBody::None => request,
        ServiceBody::Form(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for (name, part) in parts {
                form = match part {
                    ServicePart::Text(text) => form.text(name, text),
                    ServicePart::File { path, name: file, mime } => {
                        let Ok(Ok(bytes)) = tokio::task::spawn_blocking(move || std::fs::read(path)).await else {
                            warn!("plugin service file not read");
                            return json!({"ok": false, "status": 0, "body": null});
                        };
                        let Ok(part) =
                            reqwest::multipart::Part::bytes(bytes).file_name(file).mime_str(&mime)
                        else {
                            return json!({"ok": false, "status": 0, "body": null});
                        };
                        form.part(name, part)
                    }
                };
            }
            request.multipart(form)
        }
    };
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let bytes = response.bytes().await.map(|bytes| bytes.to_vec()).unwrap_or_default();
            let body = serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|_| {
                if bytes.is_empty() {
                    Value::Null
                } else {
                    Value::from(String::from_utf8_lossy(&bytes).into_owned())
                }
            });
            if !status.is_success() {
                warn!("plugin service call failed: {}", status.as_u16());
            }
            json!({"ok": status.is_success(), "status": status.as_u16(), "body": body})
        }
        Err(error) => {
            warn!("plugin service call failed: {}", dc_http::redact::url(&error.to_string()));
            json!({"ok": false, "status": 0, "body": null})
        }
    }
}

/// Whether a fetched body is the failure `fetch_failure` builds.
pub fn is_fetch_failure(body: &Value) -> bool {
    body["fetch_failed"] == true
}

/// Fetches `url` and writes it into the Downloads folder under a free name; returns the saved file name on success.
async fn save_download(url: &str, filename: &str) -> Option<String> {
    let folder = std::env::var_os("USERPROFILE")
        .map(|home| std::path::PathBuf::from(home).join("Downloads"))?;
    let _ = std::fs::create_dir_all(&folder);
    let response = match dc_http::plain::client_for(url).get(url).send().await {
        Ok(response) => response,
        Err(error) => {
            warn!("download failed: {error}");
            return None;
        }
    };
    if !response.status().is_success() {
        warn!("download failed: HTTP {}", response.status());
        return None;
    }
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!("download failed: {error}");
            return None;
        }
    };
    let target = free_name(&folder, filename, |path| path.exists());
    match std::fs::write(&target, &bytes) {
        Ok(()) => {
            let name = target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| filename.to_owned());
            Some(name)
        }
        Err(error) => {
            warn!("download not saved: {error}");
            None
        }
    }
}

/// The first source of a file that answers with success.
async fn fetch_first(sources: &[String]) -> Option<Vec<u8>> {
    for source in sources {
        match dc_http::plain::client_for(source).get(source).send().await {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => return Some(bytes.to_vec()),
                Err(error) => warn!("attachment download failed: {error}"),
            },
            Ok(response) => warn!("attachment download failed: {}", response.status()),
            Err(error) => warn!("attachment download failed: {error}"),
        }
    }
    None
}

/// `downloadAll` of DownloadAllAttachments after its size check: the files four at a time (`pLimit(4)`), then one
/// file or the stored ZIP into Downloads. The number saved; 0 when nothing could be fetched or written.
async fn save_all(files: Vec<(Vec<String>, String)>, archive: &str) -> usize {
    let Some(folder) = std::env::var_os("USERPROFILE")
        .map(|home| std::path::PathBuf::from(home).join("Downloads"))
    else {
        return 0;
    };
    save_all_into(&folder, files, archive).await
}

/// [`save_all`] into `folder`.
async fn save_all_into(
    folder: &std::path::Path,
    files: Vec<(Vec<String>, String)>,
    archive: &str,
) -> usize {
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let mut set = tokio::task::JoinSet::new();
    for (index, (sources, name)) in files.into_iter().enumerate() {
        let limit = limit.clone();
        set.spawn(async move {
            let _permit = limit.acquire_owned().await.ok()?;
            fetch_first(&sources)
                .await
                .map(|bytes| (index, name, bytes))
        });
    }
    let mut fetched = Vec::new();
    while let Some(done) = set.join_next().await {
        if let Ok(Some(file)) = done {
            fetched.push(file);
        }
    }
    fetched.sort_by_key(|(index, ..)| *index);
    let (name, bytes) = match fetched.as_slice() {
        [] => return 0,
        [(_, name, bytes)] => (name.clone(), bytes.clone()),
        _ => {
            let files: Vec<(String, Vec<u8>)> = fetched
                .iter()
                .map(|(_, name, bytes)| (name.clone(), bytes.clone()))
                .collect();
            (archive.to_owned(), crate::plugins::zip::stored(&files))
        }
    };
    let target = free_name(folder, &name, |path| path.exists());
    match std::fs::write(&target, &bytes) {
        Ok(()) => fetched.len(),
        Err(error) => {
            warn!("attachments not saved: {error}");
            0
        }
    }
}

/// Image and fetch requests whose task has not ended yet (`settle` in the capture harness waits for none).
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One request counted in [`IN_FLIGHT`] from its start until its task ends (the guard is dropped with it).
struct InFlight;

impl InFlight {
    fn start() -> Self {
        IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// How many image and fetch requests are still on their way.
#[cfg(feature = "capture")]
pub(crate) fn in_flight() -> usize {
    IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst)
}

fn cached_image_bytes(key: &str, bytes: Vec<u8>) -> Option<Vec<u8>> {
    // A file cut short on disk (written by a build before the completeness check) is fetched again.
    if !crate::ui::image_retry::complete(&bytes) {
        return None;
    }
    if (key.starts_with("gif:") || key.starts_with("gif-category:"))
        && bytes.get(4..8) == Some(b"ftyp")
    {
        // The raw disk cache keeps the source MP4, just as it arrived over HTTP. A cache hit must use the
        // same poster decoder as a download; feeding MP4 bytes to an image marks an undrawable tile as loaded.
        crate::video_clip::mp4_first_frame_png(&bytes)
    } else {
        Some(bytes)
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

/// Not in the pinned client: ExpressionManager's `doClone` (`plugin_hooks/expression_manager.rs`).
/// Not in the pinned client: ClipUpload's `sendClipUpload` (see `Request::PluginClipUpload`). `{ok, message}`; the
/// message is the one the original's toast shows (`getErrorMessage`), or none for its generic text.
async fn clip_upload(
    client: &Client,
    channel: u64,
    path: &std::path::Path,
    options: &crate::plugins::clip_upload::Options,
    reply: Option<(Option<u64>, u64)>,
) -> Value {
    let failed = |message: Option<&str>| json!({"ok": false, "message": message});
    let reading = path.to_owned();
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(reading)).await {
        Ok(Ok(bytes)) => bytes,
        _ => return failed(Some("Couldn't read the selected file.")),
    };
    let size = bytes.len() as u64;
    let granted = match client
        .post(
            &format!("/channels/{channel}/attachments"),
            &options.reserve_body(size),
        )
        .await
    {
        Ok(granted) => granted,
        Err(error) => {
            warn!("clip upload slot failed: {error}");
            return failed(None);
        }
    };
    let slot = &granted["attachments"][0];
    let (Some(url), Some(uploaded)) = (
        slot["upload_url"].as_str(),
        slot["upload_filename"].as_str(),
    ) else {
        return failed(Some("Discord did not return an upload slot."));
    };
    if let Err(error) = client.put_upload(url, bytes).await {
        warn!("clip upload failed: {error}");
        return failed(Some("Upload failed."));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let reference = reply.map(|(guild, message)| {
        let mut reference =
            json!({"message_id": message.to_string(), "channel_id": channel.to_string()});
        if let Some(guild) = guild {
            reference["guild_id"] = json!(guild.to_string());
        }
        reference
    });
    let body = options.message_body(channel, size, uploaded, &pin::nonce_at(now), reference);
    match client
        .post(&format!("/channels/{channel}/messages"), &body)
        .await
    {
        Ok(_) => json!({"ok": true, "message": null}),
        Err(dc_http::Error::Status { status, body, .. }) => {
            warn!("sending a clip failed: {status}");
            // GAP: the original converts the file with FFmpeg and tries again on "not a valid clip"; there is no
            // FFmpeg here, so that answer ends with the original's own word for a file it cannot use.
            if crate::plugins::clip_upload::not_a_valid_clip(&body) {
                failed(Some(
                    "This file is not compatible. Use an MP4 clip with H.264 video and AAC audio.",
                ))
            } else {
                failed(None)
            }
        }
        Err(error) => {
            warn!("sending a clip failed: {error}");
            failed(None)
        }
    }
}

async fn clone_sticker(
    client: &Client,
    guild: u64,
    sticker: u64,
    format: u64,
    name: &str,
    tags: &str,
    description: &str,
) -> Value {
    const MAX_BYTES: usize = 512 * 1024;
    let failed = |message: String| json!({"ok": false, "name": name, "message": message});
    // `fetchSticker`: what the store did not have.
    let (mut tags, mut description) = (tags.to_owned(), description.to_owned());
    if tags.is_empty()
        && let Ok(known) = client.get(&format!("/stickers/{sticker}")).await
    {
        tags = known["tags"].as_str().unwrap_or_default().to_owned();
        if description.is_empty() {
            description = known["description"].as_str().unwrap_or_default().to_owned();
        }
    }
    // `fetchBlob`: the largest size within the limit.
    let mut file = None;
    let mut size = 4096;
    while size >= 16 {
        let url = crate::ui::expression_sticker_url(sticker, format, size);
        match client.fetch_public_limited(&url, MAX_BYTES).await {
            Ok(dc_http::LimitedGet::Body(bytes)) => {
                file = Some(bytes);
                break;
            }
            Ok(dc_http::LimitedGet::TooLarge) => size /= 2,
            Ok(dc_http::LimitedGet::Status(status, _)) => {
                return failed(format!("Failed to fetch {url}: {status}"));
            }
            Err(error) => return failed(error.to_string()),
        }
    }
    let Some(file) = file else {
        return failed(format!(
            "Failed to fetch sticker within size limit of {}KiB",
            MAX_BYTES / 1024
        ));
    };
    let file_type = match format {
        4 => "image/gif",
        3 => "application/json",
        _ => "image/png",
    };
    let form = dc_http::Form {
        fields: vec![
            ("name".to_owned(), name.to_owned()),
            (
                "tags".to_owned(),
                if tags.is_empty() {
                    "\u{1f47b}".to_owned()
                } else {
                    tags
                },
            ),
            ("description".to_owned(), description),
        ],
        file_field: "file".to_owned(),
        file_name: "blob".to_owned(),
        file_type: file_type.to_owned(),
        file,
        headers: Vec::new(),
    };
    match client
        .post_form(&format!("/guilds/{guild}/stickers"), &form)
        .await
    {
        Ok(_) => json!({"ok": true, "name": name}),
        Err(dc_http::Error::Status { body, .. }) => failed(
            body["message"]
                .as_str()
                .unwrap_or("Something went wrong (check console!)")
                .to_owned(),
        ),
        Err(error) => failed(error.to_string()),
    }
}

/// Not in the pinned client: Translate+'s `translate` (`plugins::translate_plus`). The dictionaries are kept once
/// fetched (the original fetches them again each time).
async fn translate_plus(
    client: &Client,
    text: String,
    target: &str,
    toki: bool,
    sitelen: bool,
    shavian: bool,
) -> Option<Value> {
    use crate::plugins::translate_plus::{self as tp, Route};
    static DICTIONARIES: std::sync::Mutex<Vec<(&'static str, Value)>> =
        std::sync::Mutex::new(Vec::new());
    let dictionary = |url: &'static str| async move {
        let kept = DICTIONARIES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(at, _)| *at == url)
            .map(|(_, value)| value.clone());
        if let Some(kept) = kept {
            return Some(kept);
        }
        let bytes = client.fetch_public(url).await.ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        DICTIONARIES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((url, value.clone()));
        Some(value)
    };
    // `google`: an empty text stays empty, a failure is the original's notice.
    let google = |text: String| async move {
        if text.is_empty() {
            return (String::new(), String::new());
        }
        match client.fetch_public(&tp::google_url(target, &text)).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(body) => tp::google_answer(&body),
                Err(_) => ("en".to_owned(), tp::FAILED.to_owned()),
            },
            Err(_) => ("en".to_owned(), tp::FAILED.to_owned()),
        }
    };
    let (src, out) = match tp::route(&text, toki, sitelen, shavian) {
        Route::Toki { sitelen } => {
            let text = if sitelen {
                tp::translate_sitelen(&text, &dictionary(tp::SITELEN_URL).await?)
            } else {
                text
            };
            let bytes = client
                .post_public_json(
                    tp::TOKI_URL,
                    &json!({"text": text, "src": "tl", "target": "en"}),
                )
                .await
                .ok()?;
            let answer: Value = serde_json::from_slice(&bytes).ok()?;
            let english = answer["translation"][0].as_str()?.to_owned();
            let out = if target == "en" {
                english
            } else {
                google(english).await.1
            };
            ("tp".to_owned(), out)
        }
        Route::Shavian => {
            let english = tp::translate_shavian(&text, &dictionary(tp::SHAVIAN_URL).await?);
            let out = if target == "en" {
                english
            } else {
                google(english).await.1
            };
            ("sh".to_owned(), out)
        }
        Route::Google => google(text).await,
    };
    Some(json!({"src": src, "text": out}))
}

#[cfg(test)]
mod tests {
    /// DownloadAllAttachments' path: first source that answers, the stored ZIP of what came, one file alone.
    #[test]
    fn download_all_saves_what_answered_as_a_zip_or_one_file() {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut request = [0u8; 1024];
                let read = stream.read(&mut request).unwrap_or(0);
                let line = String::from_utf8_lossy(&request[..read]).to_string();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
                let (status, body) = match path.as_str() {
                    "/a" => ("200 OK", "AAA"),
                    "/b" => ("200 OK", "BB"),
                    _ => ("404 Not Found", ""),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}
Content-Length: {}
Connection: close

{body}",
                    body.len()
                );
            }
        });
        let folder = std::env::temp_dir().join(format!("gc-download-all-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let url = |path: &str| format!("{base}{path}");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let saved = runtime.block_on(super::save_all_into(
            &folder,
            vec![
                (vec![url("/missing"), url("/a")], "a.txt".to_owned()),
                (vec![url("/b")], "b.txt".to_owned()),
                (vec![url("/missing")], "c.txt".to_owned()),
            ],
            "attachments-1.zip",
        ));
        assert_eq!(saved, 2);
        let zip = std::fs::read(folder.join("attachments-1.zip")).unwrap();
        assert_eq!(
            zip,
            crate::plugins::zip::stored(&[
                ("a.txt".to_owned(), b"AAA".to_vec()),
                ("b.txt".to_owned(), b"BB".to_vec()),
            ])
        );
        let one = runtime.block_on(super::save_all_into(
            &folder,
            vec![(vec![url("/b")], "b.txt".to_owned())],
            "attachments-2.zip",
        ));
        assert_eq!(one, 1);
        assert_eq!(std::fs::read(folder.join("b.txt")).unwrap(), b"BB");
        assert!(!folder.join("attachments-2.zip").exists());
        let none = runtime.block_on(super::save_all_into(
            &folder,
            vec![(vec![url("/missing")], "c.txt".to_owned())],
            "attachments-3.zip",
        ));
        assert_eq!(none, 0);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_body_failing_the_end_check_twice_at_the_same_length_is_accepted() {
        let url = "https://cdn.example/odd-trailer.jpg";
        assert!(!super::same_length_again(url, 100));
        // Cut short at another length: still a failure.
        assert!(!super::same_length_again(url, 90));
        assert!(super::same_length_again(url, 90));
        // Accepted once; a later cut body is checked afresh.
        assert!(!super::same_length_again(url, 90));
    }

    #[test]
    fn cached_gif_video_is_decoded_before_it_becomes_an_image() {
        let video = crate::video_vod::synthetic_mp4(32, 32, 30, &[[220, 40, 20]], 1).unwrap();
        let poster = super::cached_image_bytes("gif:favorite", video).unwrap();
        assert!(poster.starts_with(b"\x89PNG"));
        let image = image::load_from_memory(&poster).unwrap();
        assert_eq!((image.width(), image.height()), (32, 32));
        assert!(super::cached_image_bytes("gif:broken", b"0000ftypbroken".to_vec()).is_none());
        assert_eq!(
            super::cached_image_bytes("gif:still", b"GIF89a;".to_vec()).unwrap(),
            b"GIF89a;"
        );
        // A picture cut short on disk is fetched again instead of drawn.
        assert!(super::cached_image_bytes("gif:cut", b"GIF89a".to_vec()).is_none());
    }
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
            &dc_http::Error::RateLimited {
                route: "/x".into(),
                retry_after: Some(2.5),
            },
        );
        assert_eq!(limited["status"], 429);
        assert_eq!(limited["body"]["retry_after"], 2.5);
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

    /// Module 339200's silentPrefix: `@silent` goes out as flag 4096, without the prefix.
    #[test]
    fn a_silent_prefix_becomes_the_suppress_notifications_flag() {
        let body = send_body(5, "@silent  hush", None);
        assert_eq!(body["content"], "hush");
        assert_eq!(body["flags"], 4096);
        let plain = send_body(5, "@silently", None);
        assert_eq!(plain["content"], "@silently");
        assert_eq!(plain["flags"], 0);
    }
}

/// The preview upload's body: `{thumbnail: "data:image/jpeg;base64,..."}`.
fn preview_body(jpeg: &[u8]) -> Value {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(jpeg);
    json!({"thumbnail": format!("data:image/jpeg;base64,{encoded}")})
}

#[cfg(test)]
mod start_page_tests {
    use super::{StartPage, StartPageUse};
    use serde_json::json;

    #[test]
    fn a_load_before_ready_waits_for_the_page_on_its_way() {
        let mut page = StartPage::InFlight(7);
        assert_eq!(page.claim(7, false), StartPageUse::Wait);
        // The page arrives: it goes out once, to the load that waited, and is not kept.
        assert!(page.arrived(&json!([{"id": "1"}]), 5));
        assert_eq!(page, StartPage::Spent);
        assert_eq!(page.claim(7, false), StartPageUse::Fetch);
    }

    #[test]
    fn a_page_that_came_first_is_handed_to_the_load_once() {
        let mut page = StartPage::InFlight(7);
        assert!(page.arrived(&json!([{"id": "1"}]), 5));
        assert_eq!(
            page.claim(7, false),
            StartPageUse::Deliver {
                page: json!([{"id": "1"}]),
                started_ms: 5
            }
        );
        assert_eq!(page.claim(7, false), StartPageUse::Fetch);
    }

    #[test]
    fn another_channel_and_any_load_after_ready_ask_the_server() {
        let mut page = StartPage::InFlight(7);
        assert_eq!(page.claim(8, false), StartPageUse::Fetch);
        assert_eq!(page, StartPage::InFlight(7), "another channel leaves it");
        // After READY the page could miss what was posted before the gateway's events began.
        assert_eq!(page.claim(7, true), StartPageUse::Fetch);
        assert_eq!(page, StartPage::Spent);
        // A page superseded that way is dropped when it lands, never shown over the newer one.
        assert!(!page.arrived(&json!([]), 5));
        // A page in before READY is drawn after it too, while the server is asked again; only once.
        let mut kept = StartPage::InFlight(7);
        assert!(kept.arrived(&json!([]), 5));
        assert_eq!(
            kept.claim(8, true),
            StartPageUse::Fetch,
            "not for another channel"
        );
        assert_eq!(kept, StartPage::Spent);
        let mut kept = StartPage::InFlight(7);
        assert!(kept.arrived(&json!([]), 5));
        assert_eq!(
            kept.claim(7, true),
            StartPageUse::DeliverThenFetch {
                page: json!([]),
                started_ms: 5
            }
        );
        assert_eq!(kept, StartPage::Spent);
        assert_eq!(kept.claim(7, true), StartPageUse::Fetch);
    }

    #[test]
    fn a_failed_page_sends_the_waiting_load_to_the_server() {
        let mut waited = StartPage::InFlight(7);
        assert_eq!(waited.claim(7, false), StartPageUse::Wait);
        assert_eq!(waited.failed(), Some(7));
        let mut unasked = StartPage::InFlight(7);
        assert_eq!(unasked.failed(), None);
        assert_eq!(unasked.claim(7, false), StartPageUse::Fetch);
        // Nothing asked for at all: loads go to the server.
        assert_eq!(StartPage::None.claim(7, false), StartPageUse::Fetch);
    }
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
                    route: "/invites/fixture".into(),
                    retry_after: None,
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

/// OpenInApp's `getRedirect`: HEAD requests that follow `Location` five times at most; the last address reached, or
/// the one given when a request fails.
async fn resolve_redirect(url: &str) -> String {
    let Ok(client) = dc_http::plain::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return url.to_owned();
    };
    let mut current = url.to_owned();
    for _ in 0..5 {
        let Ok(response) = client.head(&current).send().await else {
            break;
        };
        let Some(next) = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
        else {
            break;
        };
        current = match reqwest::Url::parse(&current).and_then(|base| base.join(next)) {
            Ok(joined) => joined.to_string(),
            Err(_) => break,
        };
    }
    current
}

#[cfg(test)]
mod sheet_tests {
    use super::{SHEET_CELL, sheet_cells};

    /// The cells as they were cut before the sheets were read in bands: the whole sheet decoded, then cropped.
    fn cut_whole(sheet: &[u8], count: u32, columns: u32) -> Vec<image::RgbaImage> {
        let sheet = image::load_from_memory_with_format(sheet, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        let scale = f64::from(sheet.width()) / f64::from(columns * SHEET_CELL);
        let size = (f64::from(SHEET_CELL) * scale).round() as u32;
        let mut out = Vec::new();
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
            out.push(cell);
        }
        out
    }

    fn decoded(cells: &[(String, Vec<u8>)]) -> Vec<image::RgbaImage> {
        cells
            .iter()
            .map(|(_, png)| {
                image::load_from_memory_with_format(png, image::ImageFormat::Png)
                    .unwrap()
                    .to_rgba8()
            })
            .collect()
    }

    /// A sheet of `columns` by `rows` cells of `cell` pixels in the given PNG color type, with varied pixels.
    fn sheet(columns: u32, rows: u32, cell: u32, color: png::ColorType, trns: bool) -> Vec<u8> {
        let (width, height) = (columns * cell, rows * cell);
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let channels = match color {
                png::ColorType::Indexed => {
                    let palette: Vec<u8> = (0..=255u8)
                        .flat_map(|i| [i, i.wrapping_mul(7), 255 - i])
                        .collect();
                    encoder.set_palette(palette);
                    if trns {
                        let alpha: Vec<u8> = (0..=255u8).map(|i| i.wrapping_mul(13)).collect();
                        encoder.set_trns(alpha);
                    }
                    1
                }
                png::ColorType::Rgb => 3,
                _ => 4,
            };
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = (0..width * height * channels)
                .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        bytes
    }

    #[test]
    fn cells_read_in_bands_are_the_cells_of_the_whole_sheet() {
        for (color, trns) in [
            (png::ColorType::Indexed, true),
            (png::ColorType::Indexed, false),
            (png::ColorType::Rgb, false),
            (png::ColorType::Rgba, false),
        ] {
            // Stored at twice the drawn size, as the emoji sheet is, and at the drawn size; more cells asked for than
            // the sheet holds, so the cut stops where it ends.
            for cell in [80, 40] {
                let bytes = sheet(3, 2, cell, color, trns);
                let cells = sheet_cells(&bytes, 8, 3, "sprite").unwrap();
                assert_eq!(cells.len(), 6, "{color:?} {cell}");
                assert_eq!(cells[5].0, "sprite:5");
                assert_eq!(decoded(&cells), cut_whole(&bytes, 8, 3), "{color:?} {cell}");
                assert!(cells.iter().all(|(_, png)| png.capacity() == png.len()));
            }
        }
    }

    /// The real sheets, when `GRAVECORD_TEST_SHEETS` names their files (`;` between them, each `path,count,columns`).
    #[test]
    fn the_emoji_sheets_cut_as_before() {
        let Ok(list) = std::env::var("GRAVECORD_TEST_SHEETS") else {
            return;
        };
        for item in list.split(';').filter(|item| !item.is_empty()) {
            let mut parts = item.rsplitn(3, ',');
            let columns: u32 = parts.next().unwrap().parse().unwrap();
            let count: u32 = parts.next().unwrap().parse().unwrap();
            let bytes = std::fs::read(parts.next().unwrap()).unwrap();
            let cells = sheet_cells(&bytes, count, columns, "sprite").unwrap();
            assert_eq!(decoded(&cells), cut_whole(&bytes, count, columns), "{item}");
        }
    }
}
