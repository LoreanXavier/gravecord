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
        flags: u64,
    },
    /// Opens the voice connection once the gateway named the server.
    VoiceConnect {
        join: dc_voice::Join,
        controls: dc_voice::Controls,
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
        files: Vec<std::path::PathBuf>,
        /// Guild, if any, and the message being answered.
        reply: Option<(Option<u64>, u64)>,
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
    files: &[std::path::PathBuf],
) -> Result<Vec<Value>, String> {
    let mut attachments = Vec::new();
    for (index, path) in files.iter().enumerate() {
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let filename = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned());
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
        attachments.push(json!({
            "id": index.to_string(),
            "filename": filename,
            "uploaded_filename": uploaded,
        }));
    }
    Ok(attachments)
}

fn send_body(channel: u64, content: &str, reply: Option<(Option<u64>, u64)>) -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let mut body = json!({
        "mobile_network_type": "unknown",
        "content": content,
        "nonce": pin::nonce_at(now),
        "tts": false,
    });
    if let Some((guild, message)) = reply {
        let mut reference =
            json!({"channel_id": channel.to_string(), "message_id": message.to_string()});
        if let Some(guild) = guild {
            reference["guild_id"] = json!(guild.to_string());
        }
        body["message_reference"] = reference;
        body["allowed_mentions"] =
            json!({"parse": ["users", "roles", "everyone"], "replied_user": false});
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
                        Err(error) => warn!("fetch failed: {error}"),
                    }
                });
            }
            Request::VoiceState {
                guild,
                channel,
                self_mute,
                self_deaf,
                flags,
            } => {
                let data = json!({
                    "guild_id": guild.to_string(),
                    "channel_id": channel.map(|channel| channel.to_string()),
                    "self_mute": self_mute,
                    "self_deaf": self_deaf,
                    "self_video": false,
                    "flags": flags,
                });
                let _ = commands.send(Command::Send {
                    op: Op::VoiceStateUpdate,
                    data,
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

#[cfg(test)]
mod tests {
    use super::{percent_encode, send_body};

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
    fn a_reply_adds_the_reference_without_pinging() {
        let body = send_body(5, "hi", Some((Some(9), 7)));
        assert_eq!(body["message_reference"]["guild_id"], "9");
        assert_eq!(body["message_reference"]["message_id"], "7");
        assert_eq!(body["allowed_mentions"]["replied_user"], false);
    }

    #[test]
    fn a_dm_reply_has_no_guild() {
        let body = send_body(5, "hi", Some((None, 7)));
        assert!(body["message_reference"].get("guild_id").is_none());
    }
}
