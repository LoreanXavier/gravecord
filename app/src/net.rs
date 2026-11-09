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
    Dispatch {
        name: String,
        data: Value,
    },
    Messages {
        channel: u64,
        page: Value,
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
    /// An event of a Go Live stream's own voice connection.
    Stream {
        key: dc_voice::streams::StreamKey,
        event: dc_voice::StreamEvent,
    },
    /// The body of a `Request::Fetch`.
    Fetched {
        key: String,
        body: Value,
    },
}

pub enum Request {
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
    Send {
        channel: u64,
        content: String,
        /// Files to upload first, sent with the message.
        files: Vec<Upload>,
        /// Guild, if any, and the message being answered.
        /// The guild, the message, and whether the reply pings its author (the reply bar's @ switch).
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
    /// Forward: an empty message whose `message_reference` is the source with `type: 1`, then the note on its own.
    Forward {
        destination: u64,
        guild: Option<u64>,
        channel: u64,
        message: u64,
        note: Option<String>,
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
}

pub(crate) const MESSAGE_PAGE: u32 = 50;
/// Held while a settings-proto PATCH is in flight.
static SETTINGS_PROTO_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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

/// A file to upload with a message: where it is, the name it goes by (the composer's Modify and spoiler actions rename
/// it) and its alt text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Upload {
    pub path: std::path::PathBuf,
    /// The name the message shows; empty means the file's own.
    pub name: String,
    pub description: Option<String>,
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
fn attachment_entry(
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

fn send_body(channel: u64, content: &str, reply: Option<(Option<u64>, u64, bool)>) -> Value {
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

pub async fn run(
    token: Token,
    updates: mpsc::UnboundedSender<Update>,
    mut requests: mpsc::UnboundedReceiver<Request>,
) {
    let identity = Identity::new();
    let client = match Client::new(token.clone(), identity.clone()) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            warn!("http client failed: {error}");
            return;
        }
    };
    let (commands, mut events) = spawn(Config {
        token,
        identity,
        state: IdentifyState::default(),
    });

    let forward = updates.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                Event::Dispatch { name, data } => {
                    let _ = forward.send(Update::Dispatch { name, data });
                }
                Event::AuthenticationFailed => warn!("gateway rejected the token"),
                Event::Reconnecting { delay } => warn!("gateway reconnecting in {delay:?}"),
                Event::Connected | Event::Closed { .. } => {}
            }
        }
    });

    while let Some(request) = requests.recv().await {
        let client = client.clone();
        let updates = updates.clone();
        match request {
            Request::LoadMessages { channel } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{channel}/messages?limit={MESSAGE_PAGE}");
                    match client.get(&route).await {
                        Ok(page) => {
                            let _ = updates.send(Update::Messages { channel, page });
                        }
                        Err(error) => warn!("loading messages failed: {error}"),
                    }
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
                        match upload_all(&client, channel, &files).await {
                            Ok(attachments) => body["attachments"] = Value::Array(attachments),
                            Err(error) => {
                                warn!("upload failed: {error}");
                                return;
                            }
                        }
                    }
                    if let Err(error) = client
                        .post(&format!("/channels/{channel}/messages"), &body)
                        .await
                    {
                        warn!("sending failed: {error}");
                    }
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
            Request::Forward {
                destination,
                guild,
                channel,
                message,
                note,
            } => {
                tokio::spawn(async move {
                    let route = format!("/channels/{destination}/messages");
                    let mut body = send_body(destination, "", None);
                    body["message_reference"] = json!({
                        "guild_id": guild.map(|guild| guild.to_string()),
                        "channel_id": channel.to_string(),
                        "message_id": message.to_string(),
                        "type": 1,
                    });
                    body["flags"] = json!(0);
                    if let Err(error) = client
                        .request(reqwest::Method::POST, &route, Some(&body))
                        .await
                    {
                        warn!("forwarding failed: {error}");
                        return;
                    }
                    if let Some(note) = note {
                        let body = send_body(destination, &note, None);
                        if let Err(error) = client
                            .request(reqwest::Method::POST, &route, Some(&body))
                            .await
                        {
                            warn!("forward note failed: {error}");
                        }
                    }
                });
            }
            Request::GuildSettings { body } => {
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
            Request::Rest {
                key,
                method,
                route,
                body,
            } => {
                let updates = updates.clone();
                tokio::spawn(async move {
                    // The server merges a settings PATCH into what it read a moment before, so two sent at once lose
                    // one of them: they go one after the other.
                    let _turn = if route.starts_with("/users/@me/settings-proto/") {
                        Some(SETTINGS_PROTO_TURN.lock().await)
                    } else {
                        None
                    };
                    let outcome = client.request(method, &route, body.as_ref()).await;
                    let report = match outcome {
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
                    if let Some(key) = key {
                        let _ = updates.send(Update::Fetched { key, body: report });
                    }
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
                    if let Err(error) = client.request(reqwest::Method::POST, &route, None).await {
                        warn!("typing notice failed: {error}");
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
                            // The shelf shows "Failed to fetch activities" instead of waiting.
                            if key.starts_with("activity-shelf:") {
                                let _ = updates.send(Update::Fetched {
                                    key,
                                    body: json!({"error": true}),
                                });
                            }
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
                    "guild_id": guild.to_string(),
                    "channel_id": channel.map(|channel| channel.to_string()),
                    "self_mute": self_mute,
                    "self_deaf": self_deaf,
                    "self_video": self_video,
                    "flags": flags,
                });
                let _ = commands.send(Command::Send {
                    op: Op::VoiceStateUpdate,
                    data,
                });
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
                    match client.fetch_public(&url).await {
                        Ok(bytes) => {
                            let _ = updates.send(Update::Image { key, bytes });
                        }
                        Err(error) => warn!("image failed: {error}"),
                    }
                });
            }
        }
    }
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
    use super::{guild_member_requests, percent_encode, send_body};
    use serde_json::json;

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
