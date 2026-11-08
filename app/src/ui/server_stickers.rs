//! Server Settings > Stickers (module 989433 `Nj`, `Nc`, `Nh`, `Nx`, tile `fC`, timeline `fF` and `fO`; requests module
//! 631576; dialog module 445002; slot tables module 682901 and 473145).
//!
//! `GET /guilds/{id}/stickers` lists them. The page is a timeline of the four levels with their slots; Upload Sticker
//! opens "Upload a file", which posts a form with the file (`POST /guilds/{id}/stickers`). A tile's pencil is the same
//! dialog without a file (`PATCH`), its X `DELETE`s. Lottie stickers (partners only) are uploaded but not previewed. The
//! buttons that ask for Boosts open Boost Perks' dialog; buying Boosts itself is not drawn.
//! Measured in `design/dom/settings/guild-stickers-darker-cozy.json` and `modals/sticker-upload-darker-cozy.json`.

use super::tooltip::{Body, Side, Tip};
use super::{Root, avatar_source, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User, permissions};
use gpui::{
    AnyElement, Context, Entity, FontWeight, Image, ImageFormat, IntoElement, PathPromptOptions,
    SharedString, Subscription, div, img, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::Value;
use std::sync::Arc;

pub(super) const STICKERS_PREFIX: &str = "guild-stickers:";
const CHANGED_PREFIX: &str = "guild-stickers:changed:";
const REMOVED_PREFIX: &str = "guild-stickers:removed:";
/// `upsellContainer_x-SGOT`'s picture, 660 by 218.
const UPSELL_ART: &str = "https://discord.com/assets/e392b6b2f35db628.svg";
/// Module 415358: the wumpus sticker in empty slots and locked levels, 69 by 54.
const SLOT_ART: &str = "https://discord.com/assets/6e989612eab7e1c3.svg";
/// Module 652215's `STICKERS_UPLOAD` article.
const UPLOAD_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/4403089981975";
/// The dialog's limit for everyone but staff, and how it is written.
const MAX_FILE_SIZE: usize = 524_288;
const MAX_FILE_SIZE_TEXT: &str = "512KB";
/// `.grid_7tmNTL`.
const COLUMNS: usize = 5;
const TILE_HEIGHT: f32 = 140.;
const TILE_GAP: f32 = 10.;
/// The timeline's marker (`fG`) and the bar's width (`fR`).
const MARKER: f32 = 16.;
const BAR_WIDTH: f32 = 24.;
/// `CREATE_GUILD_EXPRESSIONS` and `MANAGE_GUILD_EXPRESSIONS` in the permission table.
const CREATE_EXPRESSIONS: u64 = 1 << 43;
const MANAGE_EXPRESSIONS: u64 = 1 << 30;

/// `A.nk`: slots a level adds. `A.d8`: slots up to that level. `ej.M2T`: Boosts it takes.
const ADDITIONAL: [usize; 4] = [5, 10, 15, 30];
const TOTAL: [usize; 4] = [5, 15, 30, 60];
const REQUIRED: [u64; 4] = [0, 2, 7, 14];
const TITLES: [&str; 4] = ["Free Slots", "Level 1", "Level 2", "Level 3"];

/// `fh.K.MAX_STICKER_SLOTS`: level 3's end with the `MORE_STICKERS` feature.
const MAX_STICKER_SLOTS: usize = 120;

/// `ot.aG`: slots up to a level.
fn total_slots(tier: usize, more: bool) -> usize {
    if more && tier == 3 {
        MAX_STICKER_SLOTS
    } else {
        TOTAL[tier]
    }
}

/// `ot.yA`: free slots of one level.
fn slots_free(stickers: usize, tier: usize) -> usize {
    let before = if tier == 0 { 0 } else { TOTAL[tier - 1] };
    let in_tier = stickers.min(TOTAL[tier]).saturating_sub(before);
    ADDITIONAL[tier].saturating_sub(in_tier)
}

/// `v`: the level the next sticker goes into, which is the first with room.
fn open_tier(stickers: usize, more: bool) -> usize {
    (0..4)
        .find(|&tier| stickers < total_slots(tier, more))
        .unwrap_or(0)
}

/// What a level's card shows, from `renderTier`.
#[derive(Clone, Debug, PartialEq)]
struct TierCard {
    tier: usize,
    locked: bool,
    upload: bool,
    unlock_button: bool,
    /// `H`: the lock status or "Unlocked" at the header's end.
    status: bool,
    tiles: usize,
    placeholders: usize,
    /// Upload more stickers to make use of this level (an unlocked level left empty).
    unused: bool,
}

fn tier_card(
    tier: usize,
    stickers: usize,
    current: usize,
    can_create: bool,
    more: bool,
) -> TierCard {
    let before = if tier == 0 { 0 } else { TOTAL[tier - 1] };
    let total = total_slots(tier, more);
    let tiles = stickers.min(total).saturating_sub(before);
    let locked = current < tier;
    let open = open_tier(stickers, more);
    let upload = can_create && !locked && open == tier && tiles != total;
    let unlock_button = open == tier && open > current;
    let placeholders = if upload {
        (COLUMNS - tiles % COLUMNS).min(ADDITIONAL[tier].saturating_sub(tiles))
    } else {
        0
    };
    TierCard {
        tier,
        locked,
        upload,
        unlock_button,
        status: upload || unlock_button || tier > current,
        tiles,
        placeholders,
        unused: !locked && !upload && tiles == 0,
    }
}

impl TierCard {
    fn has_grid(&self) -> bool {
        self.upload || self.tiles > 0
    }

    /// The header's own height, with its line: the title with its slots needs 36, the rest 32.
    fn header_height(&self) -> f32 {
        if self.locked { 65. } else { 69. }
    }

    /// The whole card's height, borders included.
    fn height(&self) -> f32 {
        let body = if self.has_grid() {
            let rows = (self.tiles + self.placeholders).div_ceil(COLUMNS).max(1) as f32;
            16. + rows * TILE_HEIGHT + (rows - 1.) * TILE_GAP + 16.
        } else if self.unused {
            // `tierBody` with `unusedTierWrapper`: 16 + 4, the line, 4 + 16.
            60.
        } else {
            216.
        };
        2. + self.header_height() + body
    }
}

/// `measure`: each marker sits 32 below its header's top, measured from the first block's top. Above the cards are the
/// 60 tall intro and its 16 margin; the cards are 24 apart.
fn marker_offsets(cards: &[TierCard]) -> Vec<f32> {
    let mut top = 60. + 16.;
    cards
        .iter()
        .map(|card| {
            let y = top + 1. + 32.;
            top += card.height() + 24.;
            y
        })
        .collect()
}

/// `getProgressHeight`: down to the current level's marker, then the Boosts' share of the way to the next.
fn progress_height(offsets: &[f32], current: usize, boosts: u64) -> f32 {
    for tier in (1..4).rev() {
        if boosts == REQUIRED[tier] {
            return offsets[tier];
        }
        if boosts > REQUIRED[tier] {
            if tier == 3 {
                return offsets[current];
            }
            let share =
                (boosts - REQUIRED[tier]) as f32 / (REQUIRED[tier + 1] - REQUIRED[tier]) as f32;
            return offsets[tier] + share * (offsets[tier + 1] - offsets[tier]);
        }
    }
    if boosts == 0 {
        return offsets[0];
    }
    let share = boosts as f32 / REQUIRED[1] as f32;
    offsets[0] + share * (offsets[1] - offsets[0])
}

/// A file for the form, after `onFileSelect`.
pub(crate) struct StickerFile {
    name: String,
    file_type: String,
    bytes: Vec<u8>,
    /// MD5 of the file as chosen (`X-Discord-Original-MD5`).
    original_md5: String,
    preview: Option<Arc<Image>>,
}

/// Why a file was turned down (`ea`): `blocking` keeps Upload disabled until another is picked.
#[derive(Debug, PartialEq)]
struct FileError {
    message: String,
    blocking: bool,
}

/// `G`: fits a picture into 320 by 320 and writes it as a PNG.
fn fit_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(bytes).ok()?;
    let fitted = picture.resize(320, 320, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    fitted.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Module 515718 `LZ`: an animated PNG has `acTL` before its first `IDAT`.
fn is_animated_png(bytes: &[u8]) -> bool {
    let find = |needle: &[u8]| bytes.windows(4).position(|window| window == needle);
    match (find(b"acTL"), find(b"IDAT")) {
        (Some(animation), Some(data)) => animation < data,
        _ => false,
    }
}

/// The dialog's `onFileSelect`: the type by its extension, JPEGs and oversized still PNGs redrawn as PNGs, then the size
/// limit. `had_file` is whether a file is already chosen, which stays usable after a file that is too big.
fn prepare_file(
    name: &str,
    original: Vec<u8>,
    partnered: bool,
    had_file: bool,
) -> Result<StickerFile, FileError> {
    use md5::{Digest, Md5};
    let unsupported = || FileError {
        message: "Unsupported file format".to_owned(),
        blocking: true,
    };
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .ok_or_else(unsupported)?;
    let file_type = match extension.as_str() {
        "json" => "application/json",
        "png" => "image/png",
        "apng" => "image/apng",
        "gif" => "image/gif",
        "jpeg" | "jpg" => "image/jpeg",
        _ => return Err(unsupported()),
    };
    let original_md5: String = Md5::digest(&original)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let invalid = || FileError {
        message: "Invalid sticker file".to_owned(),
        blocking: true,
    };
    let (bytes, file_type) = match file_type {
        "image/jpeg" => (fit_png(&original).ok_or_else(invalid)?, "image/png"),
        "image/png" if original.len() > MAX_FILE_SIZE && !is_animated_png(&original) => {
            (fit_png(&original).ok_or_else(invalid)?, "image/png")
        }
        other => (original, other),
    };
    if bytes.len() > MAX_FILE_SIZE {
        return Err(FileError {
            message: format!(
                "That sticker was too big! Stickers must be under {MAX_FILE_SIZE_TEXT}."
            ),
            blocking: !had_file,
        });
    }
    let preview = match file_type {
        "application/json" => {
            if !partnered {
                return Err(unsupported());
            }
            None
        }
        "image/gif" => Some(Image::from_bytes(ImageFormat::Gif, bytes.clone())),
        _ => Some(Image::from_bytes(ImageFormat::Png, bytes.clone())),
    };
    Ok(StickerFile {
        name: name.to_owned(),
        file_type: file_type.to_owned(),
        bytes,
        original_md5,
        preview: preview.map(Arc::new),
    })
}

/// `g`: a sticker's file extension by its `format_type`.
fn extension(format: u64) -> &'static str {
    match format {
        2 => "apng",
        3 => "json",
        4 => "gif",
        _ => "png",
    }
}

/// `zg`: the picture of a sticker, by `format_type`, or `None` for Lottie. A still one comes as lossless WebP, and the
/// tile's 72 asks for the next size the proxy serves (`kr`), which is 80.
fn sticker_url(sticker: &Value) -> Option<String> {
    let id = sticker["id"].as_str()?;
    match sticker["format_type"].as_u64()? {
        3 => None,
        1 => Some(format!(
            "https://media.discordapp.net/stickers/{id}.webp?size=80&quality=lossless"
        )),
        format => Some(format!(
            "https://media.discordapp.net/stickers/{id}.{}?size=80",
            extension(format)
        )),
    }
}

/// `eo`: empty, or 2 to 100 characters.
fn description_ok(description: &str) -> bool {
    let length = description.chars().count();
    length == 0 || (2..=100).contains(&length)
}

/// "Upload a file" while it is open.
pub(crate) struct StickerDialog {
    pub(super) name: Entity<Editor>,
    pub(super) description: Entity<Editor>,
    file: Option<StickerFile>,
    /// The file field's text when editing (`sL`).
    file_name: String,
    emoji_id: Option<String>,
    emoji_name: Option<String>,
    error: Option<FileError>,
    busy: bool,
    /// The sticker being edited, with its picture's key.
    editing: Option<(String, Option<String>)>,
    _events: [Subscription; 2],
}

impl Root {
    pub(super) fn request_stickers(&mut self, guild: Id) {
        self.request_image(UPSELL_ART.to_owned(), UPSELL_ART.to_owned());
        self.request_image(SLOT_ART.to_owned(), SLOT_ART.to_owned());
        let _ = self.requests.send(Request::Fetch {
            key: format!("{STICKERS_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/stickers"),
        });
    }

    pub(super) fn stickers_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(CHANGED_PREFIX) {
            let report = self.fetched.get(key).cloned().unwrap_or_default();
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_stickers(guild);
            }
            if let Some(open) = self.server_settings.as_mut() {
                if report["ok"] == true {
                    open.sticker_dialog = None;
                } else if let Some(dialog) = open.sticker_dialog.as_mut() {
                    dialog.busy = false;
                    dialog.error = Some(FileError {
                        message: report["body"]["message"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                        blocking: false,
                    });
                }
            }
        } else if let Some(rest) = key.strip_prefix(REMOVED_PREFIX) {
            let report = self.fetched.get(key).cloned().unwrap_or_default();
            let (guild, sticker) = rest.split_once(':').unwrap_or((rest, ""));
            if report["ok"] == true {
                if let Ok(guild) = guild.parse::<Id>() {
                    self.request_stickers(guild);
                }
            } else if let Some(open) = self.server_settings.as_mut() {
                open.sticker_removing = None;
                open.sticker_error = Some((
                    sticker.to_owned(),
                    report["body"]["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                ));
            }
        } else if let Some(guild) = key
            .strip_prefix(STICKERS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            if let Some(open) = self.server_settings.as_mut() {
                open.sticker_removing = None;
            }
            for sticker in self.stickers_of(guild) {
                if let Some(user) = sticker["user"]
                    .as_object()
                    .map(|_| &sticker["user"])
                    .and_then(User::from_value)
                {
                    let (key, url) = avatar_source(&user);
                    self.request_image(key, url);
                }
                if let (Some(id), Some(url)) = (sticker["id"].as_str(), sticker_url(&sticker)) {
                    self.request_image(format!("sticker:{id}"), url);
                }
            }
        }
        cx.notify();
    }

    fn stickers_of(&self, guild: Id) -> Vec<Value> {
        self.fetched
            .get(&format!("{STICKERS_PREFIX}{guild}"))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default()
    }

    fn open_sticker_dialog(&mut self, cx: &mut Context<Self>) {
        let make = |placeholder: &str, limit: usize, cx: &mut Context<Self>| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_placeholder(placeholder.to_owned());
                editor
            });
            let events = cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Cancel => {
                        if let Some(open) = this.server_settings.as_mut() {
                            open.sticker_dialog = None;
                        }
                    }
                    EditorEvent::Submit(text) => {
                        let text = text.clone();
                        editor.update(cx, |editor, _| editor.set_text(&text));
                    }
                    EditorEvent::Edited => {
                        let text = editor.read(cx).text().to_owned();
                        if text.chars().count() > limit {
                            let cut: String = text.chars().take(limit).collect();
                            editor.update(cx, |editor, _| editor.set_text(&cut));
                        }
                    }
                }
                cx.notify();
            });
            (editor, events)
        };
        let (name, name_events) = make("ex: cat hug", 30, cx);
        let (description, description_events) = make(
            "See our Help Center for tips on writing sticker descriptions.",
            100,
            cx,
        );
        if let Some(open) = self.server_settings.as_mut() {
            open.sticker_dialog = Some(StickerDialog {
                name,
                description,
                file: None,
                file_name: String::new(),
                emoji_id: None,
                emoji_name: None,
                error: None,
                busy: false,
                editing: None,
                _events: [name_events, description_events],
            });
        }
        cx.notify();
    }

    /// The pencil: the same dialog with the sticker's values and its file fixed.
    fn edit_sticker(&mut self, guild: Id, sticker: &Value, cx: &mut Context<Self>) {
        self.open_sticker_dialog(cx);
        let name = sticker["name"].as_str().unwrap_or_default().to_owned();
        let description = sticker["description"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let tags = sticker["tags"].as_str().unwrap_or_default().to_owned();
        let custom = self
            .stores
            .guild(guild)
            .and_then(|known| {
                known
                    .emojis
                    .iter()
                    .find(|emoji| emoji.id.to_string() == tags)
            })
            .map(|emoji| (emoji.id.to_string(), emoji.name.clone()));
        let surrogates = dc_tokens::emoji_data::EMOJIS
            .iter()
            .find(|emoji| emoji.names.contains(&tags.as_str()))
            .map(|emoji| emoji.surrogates.to_owned());
        if custom.is_none() {
            self.request_emoji_image(surrogates.as_deref().unwrap_or(&tags));
        } else if let Some((id, _)) = custom.as_ref() {
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
            );
        }
        let id = sticker["id"].as_str().unwrap_or_default().to_owned();
        let format = sticker["format_type"].as_u64().unwrap_or(1);
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sticker_dialog.as_mut())
        {
            dialog.name.update(cx, |editor, _| editor.set_text(&name));
            dialog
                .description
                .update(cx, |editor, _| editor.set_text(&description));
            dialog.file_name = format!("{name}.{}", extension(format));
            dialog.editing = Some((id.clone(), (format != 3).then(|| format!("sticker:{id}"))));
            match custom {
                Some((id, name)) => {
                    dialog.emoji_id = Some(id);
                    dialog.emoji_name = Some(name);
                }
                None => {
                    dialog.emoji_id = None;
                    dialog.emoji_name = (!tags.is_empty()).then(|| surrogates.unwrap_or(tags));
                }
            }
        }
        cx.notify();
    }

    fn set_sticker_file(&mut self, guild: Id, path: &std::path::Path, cx: &mut Context<Self>) {
        let partnered = self.stores.guild(guild).is_some_and(|known| {
            known
                .features
                .iter()
                .any(|feature| feature == "PARTNERED" || feature == "VERIFIED")
        });
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let bytes = std::fs::read(path).unwrap_or_default();
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sticker_dialog.as_mut())
        {
            match prepare_file(&name, bytes, partnered, dialog.file.is_some()) {
                Ok(file) => {
                    dialog.file_name = file.name.clone();
                    dialog.file = Some(file);
                    dialog.error = None;
                }
                Err(error) => dialog.error = Some(error),
            }
        }
        cx.notify();
    }

    fn browse_sticker_file(&mut self, guild: Id, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let _ = this.update(cx, |this, cx| this.set_sticker_file(guild, path, cx));
            }
        })
        .detach();
    }

    pub(super) fn set_sticker_emoji(
        &mut self,
        id: Option<String>,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = id.as_ref() {
            self.request_image(
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
            );
        } else if let Some(name) = name.as_ref() {
            self.request_emoji_image(name);
        }
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sticker_dialog.as_mut())
        {
            dialog.emoji_id = id;
            dialog.emoji_name = name;
        }
        cx.notify();
    }

    /// `ec`: whether Upload or Update may be pressed.
    fn sticker_ready(dialog: &StickerDialog, cx: &Context<Self>) -> bool {
        !dialog.busy
            && !dialog.error.as_ref().is_some_and(|error| error.blocking)
            && dialog.name.read(cx).text().chars().count() >= 2
            && (dialog.emoji_id.is_some() || dialog.emoji_name.is_some())
            && (dialog.editing.is_some() || dialog.file.is_some())
            && description_ok(dialog.description.read(cx).text())
    }

    /// `ed`: `tags` is the emoji's id, or else its name.
    fn submit_sticker(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(dialog) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.sticker_dialog.as_ref())
        else {
            return;
        };
        if !Self::sticker_ready(dialog, cx) {
            return;
        }
        let name = dialog.name.read(cx).text().to_owned();
        let description = dialog.description.read(cx).text().to_owned();
        let tags = dialog
            .emoji_id
            .clone()
            .or_else(|| dialog.emoji_name.clone())
            .unwrap_or_default();
        let key = format!("{CHANGED_PREFIX}{guild}");
        if let Some((sticker, _)) = dialog.editing.as_ref() {
            let _ = self.requests.send(Request::Rest {
                key: Some(key),
                method: reqwest::Method::PATCH,
                route: format!("/guilds/{guild}/stickers/{sticker}"),
                body: Some(serde_json::json!({
                    "name": name,
                    "tags": tags,
                    "description": description,
                })),
            });
        } else if let Some(file) = dialog.file.as_ref() {
            let _ = self.requests.send(Request::RestForm {
                key,
                route: format!("/guilds/{guild}/stickers"),
                form: dc_http::Form {
                    fields: vec![
                        ("name".to_owned(), name),
                        ("tags".to_owned(), tags),
                        ("description".to_owned(), description),
                    ],
                    file_field: "file".to_owned(),
                    file_name: file.name.clone(),
                    file_type: file.file_type.clone(),
                    file: file.bytes.clone(),
                    headers: vec![(
                        "X-Discord-Original-MD5".to_owned(),
                        file.original_md5.clone(),
                    )],
                },
            });
        }
        if let Some(dialog) = self
            .server_settings
            .as_mut()
            .and_then(|open| open.sticker_dialog.as_mut())
        {
            dialog.busy = true;
            dialog.error = None;
        }
        cx.notify();
    }

    /// "Upload a file" (module 445002): 416 of scrolling form between two lines.
    pub(super) fn sticker_dialog_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        let dialog = open.sticker_dialog.as_ref()?;
        let guild = open.guild;
        let editing = dialog.editing.is_some();
        let ready = Self::sticker_ready(dialog, cx);
        let label = |text: &'static str, required: bool| {
            div()
                .mb(px(8.))
                .flex()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
                .when(required, |this| {
                    this.child(
                        div()
                            .font_weight(FontWeight(400.))
                            .text_color(color("--text-feedback-critical"))
                            .child("*"),
                    )
                })
        };
        let picture: Option<gpui::ImageSource> = dialog
            .file
            .as_ref()
            .and_then(|file| file.preview.clone())
            .map(Into::into)
            .or_else(|| {
                dialog
                    .editing
                    .as_ref()
                    .and_then(|(_, key)| key.as_ref())
                    .and_then(|key| self.images.get(key).cloned())
                    .map(Into::into)
            });
        let pane = |background: gpui::Rgba, picture: Option<gpui::ImageSource>| {
            div()
                .flex_1()
                .h_full()
                .bg(background)
                .flex()
                .items_center()
                .justify_center()
                .child(match picture {
                    Some(picture) => img(picture).size(px(160.)).into_any_element(),
                    None => icon("sticker_image_plus", 32., "--primary-400").into_any_element(),
                })
        };
        let emoji_picture = dialog
            .emoji_id
            .as_ref()
            .map(|id| format!("custom:{id}"))
            .or_else(|| {
                dialog
                    .emoji_name
                    .as_ref()
                    .map(|name| format!("emoji:{name}"))
            })
            .and_then(|key| self.images.get(&key).cloned());
        let emoji_button = self.images.get("emoji_button").cloned();
        // `B`: `:name:`, the unicode one by its first name.
        let emoji_text = if dialog.emoji_id.is_some() {
            dialog.emoji_name.clone()
        } else {
            dialog.emoji_name.as_ref().and_then(|surrogates| {
                dc_tokens::emoji_data::EMOJIS
                    .iter()
                    .find(|emoji| emoji.surrogates == surrogates)
                    .map(|emoji| emoji.names[0].to_owned())
            })
        };
        let file_field: AnyElement = if editing {
            div()
                .h(px(40.))
                .px(px(12.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .opacity(0.5)
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(dialog.file_name.clone())
                .into_any_element()
        } else {
            let shown = (!dialog.file_name.is_empty()).then(|| dialog.file_name.clone());
            div()
                .id("sticker-file")
                .on_drop(
                    cx.listener(move |this, paths: &gpui::ExternalPaths, _, cx| {
                        if let Some(path) = paths.paths().first() {
                            this.set_sticker_file(guild, path, cx);
                        }
                    }),
                )
                .h(px(40.))
                .pl(px(8.))
                .pr(px(4.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icon("sound_file_upload", 18., "--interactive-text-default"))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(if shown.is_some() {
                            color("--text-default")
                        } else {
                            color("--input-placeholder-text-default")
                        })
                        .child(shown.unwrap_or_else(|| "Choose a file".to_owned())),
                )
                .child(
                    div()
                        .id("sticker-browse")
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .child("Browse")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.browse_sticker_file(guild, cx)),
                        ),
                )
                .into_any_element()
        };
        let focus_field = |id: &'static str, editor: Entity<Editor>| {
            div()
                .id(id)
                .cursor_text()
                .on_click(cx.listener(move |_, _, window, cx| {
                    let handle = gpui::Focusable::focus_handle(editor.read(cx), cx);
                    window.focus(&handle, cx);
                }))
        };
        let form = div()
            .pt(px(12.))
            .pl(px(24.))
            .pb(px(12.))
            .w(px(462.))
            .flex_none()
            .child(
                div()
                    .w(px(430.))
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(
                        div().child(label("Preview", false)).child(
                            div()
                                .h(px(216.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .overflow_hidden()
                                .child(pane(color("--primary-600"), picture.clone()))
                                .child(pane(gpui::rgba(0xffffffff), picture)),
                        ),
                    )
                    .child(
                        div()
                            .child(label(
                                if editing {
                                    "File (This cannot be edited)"
                                } else {
                                    "File"
                                },
                                true,
                            ))
                            .child(file_field),
                    )
                    .child(
                        div().child(label("Related Emoji", true)).child(
                            div()
                                .id("sticker-emoji")
                                .group("sticker-emoji")
                                .relative()
                                .child(self.anchor_probe("sticker-emoji"))
                                .h(px(40.))
                                .pl(px(12.))
                                .pr(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .children(
                                    emoji_picture
                                        .or(emoji_button)
                                        .map(|picture| img(picture).size(px(18.)).flex_none()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(if emoji_text.is_some() {
                                            color("--text-default")
                                        } else {
                                            color("--text-muted")
                                        })
                                        .child(
                                            emoji_text
                                                .clone()
                                                .map(|name| format!(":{name}:"))
                                                .unwrap_or_else(|| "Click to select".to_owned()),
                                        ),
                                )
                                .when(emoji_text.is_some(), |this| {
                                    this.child(
                                        div()
                                            .id("sticker-emoji-clear")
                                            .opacity(0.)
                                            .group_hover("sticker-emoji", |style| style.opacity(1.))
                                            .child(icon(
                                                "circle_x_small",
                                                16.,
                                                "--interactive-text-default",
                                            ))
                                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.set_sticker_emoji(None, None, cx);
                                            })),
                                    )
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let anchor =
                                        this.tip_anchors.borrow().get("sticker-emoji").copied();
                                    if let Some(bounds) = anchor {
                                        this.open_picker(
                                            super::picker::Target::StickerEmoji,
                                            gpui::point(bounds.left() + px(12.), bounds.top()),
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                        ),
                    )
                    .child(
                        div().child(label("Sticker Name", true)).child(
                            focus_field("sticker-name", dialog.name.clone()).child(
                                div()
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .child(div().w_full().child(dialog.name.clone())),
                            ),
                        ),
                    )
                    .child(
                        div()
                            .child(label("Description", false))
                            .child(
                                focus_field("sticker-description", dialog.description.clone())
                                    .child(
                                        div()
                                            .h(px(89.))
                                            .pt(px(12.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--input-border-default"))
                                            .bg(color("--input-background-default"))
                                            .child(div().w_full().child(dialog.description.clone())),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-subtle"))
                                    .child("You can add a description, sometimes called alt-text, to your stickers so they’re accessible to even more people, including those who are blind or visually impaired. Good descriptions are concise, but present what’s in your sticker with as much detail as possible."),
                            ),
                    )
                    .children(dialog.error.as_ref().map(|error| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(error.message.clone())
                    })),
            );
        let body = div()
            .ml(px(-24.))
            .mr(px(-24.))
            .h(px(418.))
            .border_t_1()
            .border_b_1()
            .border_color(color("--border-subtle"))
            .child(
                div()
                    .id("sticker-dialog-scroller")
                    .h(px(416.))
                    .overflow_y_scroll()
                    .flex()
                    .items_start()
                    .child(form),
            )
            .into_any_element();
        let close = |this: &mut Root| {
            if let Some(open) = this.server_settings.as_mut() {
                open.sticker_dialog = None;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "sticker-upload-modal",
                title: "Upload a file".into(),
                subtitle: Some(
                    format!("File should be APNG, JPG, PNG, or GIF ({MAX_FILE_SIZE_TEXT} max)")
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "sticker-cancel",
                        "Never Mind",
                        super::modal::Tone::Secondary,
                        move |this, _, _| close(this),
                    ),
                    super::modal::button(
                        "sticker-upload",
                        if editing { "Update" } else { "Upload" },
                        if ready {
                            super::modal::Tone::Primary
                        } else {
                            super::modal::Tone::PrimaryDisabled
                        },
                        move |this, _, cx| this.submit_sticker(guild, cx),
                    ),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }

    /// One sticker (`fC`): picture, name, uploader; pencil over X outside the corner.
    fn sticker_tile(
        &self,
        guild: Id,
        sticker: &Value,
        disabled: bool,
        can_manage: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = sticker["id"].as_str().unwrap_or_default().to_owned();
        let group = SharedString::from(format!("server-sticker-{id}"));
        let picture = self.images.get(&format!("sticker:{id}")).cloned();
        let user = User::from_value(&sticker["user"]);
        let avatar = user
            .as_ref()
            .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
        let open = self.server_settings.as_ref();
        let removing = open.is_some_and(|open| open.sticker_removing.as_deref() == Some(&id));
        let error = open
            .and_then(|open| open.sticker_error.as_ref())
            .filter(|(failed, _)| *failed == id)
            .map(|(_, message)| message.clone());
        let tool = |key: String, glyph: &'static str, tint: &'static str| {
            div()
                .id(SharedString::from(key))
                .size(px(24.))
                .rounded_full()
                .bg(color("--background-base-low"))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph, 16., tint))
        };
        let edited = sticker.clone();
        let target = id.clone();
        div()
            .group(group.clone())
            .relative()
            .w(px(116.))
            .h(px(TILE_HEIGHT))
            .p(px(12.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .hover(|style| style.bg(color("--background-mod-normal")))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .max_w(px(108.))
                    .when(removing, |this| this.opacity(0.25))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .size(px(72.))
                            .mb(px(8.))
                            .flex_none()
                            .children(picture.map(|picture| img(picture).size(px(72.)))),
                    )
                    .child(
                        div()
                            .max_w(px(92.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(sticker["name"].as_str().unwrap_or_default().to_owned()),
                    )
                    .children(user.as_ref().map(|user| {
                        div()
                            .mt(px(4.))
                            .max_w(px(92.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(16.)).mr(px(4.)).flex_none().children(
                                avatar.map(|avatar| img(avatar).size(px(16.)).rounded_full()),
                            ))
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-default"))
                                    .child(user.display_name().to_owned()),
                            )
                    })),
            )
            .when(disabled, |this| this.opacity(0.6))
            .when(can_manage, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(-11.))
                        .right(px(-10.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .opacity(0.)
                        .group_hover(group, |style| style.opacity(1.))
                        .child(
                            tool(
                                format!("server-sticker-edit-{id}"),
                                "sound_edit",
                                "--interactive-text-default",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.edit_sticker(guild, &edited, cx);
                                },
                            )),
                        )
                        .child(
                            tool(
                                format!("server-sticker-remove-{id}"),
                                "close",
                                "--text-feedback-critical",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(open) = this.server_settings.as_mut() {
                                        open.sticker_removing = Some(target.clone());
                                        open.sticker_error = None;
                                    }
                                    let _ = this.requests.send(Request::Rest {
                                        key: Some(format!("{REMOVED_PREFIX}{guild}:{target}")),
                                        method: reqwest::Method::DELETE,
                                        route: format!("/guilds/{guild}/stickers/{target}"),
                                        body: None,
                                    });
                                    cx.notify();
                                },
                            )),
                        ),
                )
            })
            .children(error.map(|message| {
                div()
                    .absolute()
                    .bottom(px(TILE_HEIGHT + 4.))
                    .w(px(116.))
                    .p(px(8.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-highest"))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(message)
            }))
            .into_any_element()
    }

    /// The timeline's bar (`fO`): grey with its pink share, cut by a 24 wide circle around each marker.
    fn sticker_progress(
        &self,
        offsets: &[f32],
        current: usize,
        boosts: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let filled = progress_height(offsets, current, boosts);
        let total = offsets.last().copied().unwrap_or(0.) + MARKER / 2.;
        let centres: Vec<f32> = std::iter::once(0.).chain(offsets.iter().copied()).collect();
        let mut bar = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(BAR_WIDTH))
            .h(px(total));
        for pair in centres.windows(2) {
            let (from, to) = (pair[0] + BAR_WIDTH / 2., pair[1] - BAR_WIDTH / 2.);
            let segment = |top: f32, height: f32, tint: gpui::Rgba| {
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(top))
                    .w(px(4.))
                    .h(px(height))
                    .bg(tint)
            };
            bar = bar.child(segment(from, to - from, color("--background-mod-strong")));
            if filled > from {
                bar = bar.child(segment(
                    from,
                    filled.min(to) - from,
                    color("--guild-boosting-pink"),
                ));
            }
        }
        bar = bar.child(
            div()
                .absolute()
                .left(px(4.))
                .top(px(0.))
                .size(px(MARKER))
                .rounded_full()
                .bg(color("--guild-boosting-pink")),
        );
        for (tier, &offset) in offsets.iter().enumerate() {
            let reached = boosts >= REQUIRED[tier];
            let disabled = current < tier && reached;
            let marker = if tier == current && !disabled {
                div()
                    .size(px(26.))
                    .rounded_full()
                    .bg(color("--guild-boosting-pink"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(4.)),
                        blur_radius: px(8.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("sticker_tier_check", 16., "--white"))
            } else {
                div()
                    .size(px(MARKER))
                    .rounded_full()
                    .bg(if reached && !disabled {
                        color("--guild-boosting-pink")
                    } else {
                        color("--background-mod-strong")
                    })
            };
            // `getTierMarkerTooltipText`: none for the free level, the level and its Boosts for the others.
            let target: AnyElement = if tier == 0 {
                marker.into_any_element()
            } else {
                let body = if disabled {
                    Body::Text(format!("{} is disabled", TITLES[tier]).into())
                } else {
                    Body::Rich(
                        TITLES[tier].into(),
                        format!("{}/{} Boosts", boosts.min(REQUIRED[tier]), REQUIRED[tier]).into(),
                    )
                };
                self.with_tip(
                    format!("sticker-tier-{tier}"),
                    Tip::body(body, Side::Right),
                    marker,
                    cx,
                )
                .into_any_element()
            };
            let big = tier == current && !disabled;
            bar = bar.child(
                div()
                    .absolute()
                    .left(px(if big { -1. } else { 4. }))
                    .top(px(offset - if big { 13. } else { MARKER / 2. }))
                    .child(target),
            );
        }
        bar.into_any_element()
    }

    /// One level (`Nt`, `Ne`): its header, then the tiles or what they would take.
    fn sticker_tier(
        &self,
        guild: Id,
        card: &TierCard,
        stickers: &[Value],
        can_manage: &dyn Fn(&Value) -> bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tier = card.tier;
        let before = if tier == 0 { 0 } else { TOTAL[tier - 1] };
        let shown: Vec<Value> = stickers
            .iter()
            .skip(before)
            .take(card.tiles)
            .cloned()
            .collect();
        let slot = self.images.get(SLOT_ART).cloned();
        let button =
            |id: String, text: &'static str, big: bool, tint: gpui::Rgba, line: gpui::Rgba| {
                div()
                    .id(SharedString::from(id))
                    .h(px(if big { 40. } else { 32. }))
                    .px(px(if big { 15. } else { 11. }))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(line)
                    .bg(tint)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(if big { 16. } else { 14. }))
                    .line_height(px(if big { 20. } else { 18. }))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child(text)
            };
        let to_perks = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.open_perks(super::server_boost::Perks::Banner, cx);
            cx.notify();
        });
        let to_perks_unlock = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.open_perks(super::server_boost::Perks::Banner, cx);
            cx.notify();
        });
        let status: Option<AnyElement> = if card.unlock_button {
            Some(
                button(
                    format!("sticker-unlock-{tier}"),
                    "Unlock More Slots",
                    false,
                    color("--control-connected-background-default"),
                    color("--control-connected-border-default"),
                )
                .on_click(to_perks_unlock)
                .into_any_element(),
            )
        } else if card.upload {
            Some(
                button(
                    format!("sticker-upload-{tier}"),
                    "Upload Sticker",
                    false,
                    color("--control-primary-background-default"),
                    color("--control-primary-border-default"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_sticker_dialog(cx)))
                .into_any_element(),
            )
        } else if card.status && !card.locked {
            // `Ni`.
            Some(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(10.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(color("--guild-boosting-blue-for-gradients"), 0.),
                        linear_color_stop(color("--guild-boosting-purple-for-gradients"), 1.),
                    ))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child("UNLOCKED")
                    .into_any_element(),
            )
        } else if card.status {
            // `Nl`.
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(format!("{} Boosts", REQUIRED[tier])),
                    )
                    .child(icon("sticker_tier_lock", 30., "--icon-muted"))
                    .into_any_element(),
            )
        } else {
            None
        };
        let glyph = match tier {
            1 => Some("sticker_tier_1"),
            2 => Some("sticker_tier_2"),
            3 => Some("sticker_tier_3"),
            _ => None,
        };
        let header = div()
            .when(card.has_grid(), |this| this.mb(px(16.)))
            .h(px(card.header_height()))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .rounded_t(px(8.))
            .when(card.has_grid(), |this| this.rounded_b(px(8.)))
            .bg(if card.locked {
                color("--background-mod-subtle")
            } else {
                color("--background-surface-high")
            })
            .p(px(16.))
            .flex()
            .items_center()
            .children(glyph.map(|glyph| {
                div().mr(px(16.)).flex_none().child(icon(
                    glyph,
                    32.,
                    if card.locked {
                        "--primary-500"
                    } else {
                        "--guild-boosting-pink"
                    },
                ))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(TITLES[tier]),
                    )
                    .when(!card.locked, |this| {
                        let free = slots_free(stickers.len(), tier);
                        let total = ADDITIONAL[tier];
                        let word = if total == 1 { "slot" } else { "slots" };
                        this.child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-default"))
                                .child(format!("{free} of {total} {word} available")),
                        )
                    }),
            )
            .children(status);
        let body: AnyElement = if card.has_grid() {
            let locked = card.locked;
            let tiles: Vec<AnyElement> = shown
                .iter()
                .map(|sticker| self.sticker_tile(guild, sticker, locked, can_manage(sticker), cx))
                .collect();
            let placeholders: Vec<AnyElement> = (0..card.placeholders)
                .map(|_| {
                    div()
                        .w(px(116.))
                        .h(px(TILE_HEIGHT))
                        .rounded(px(8.))
                        .border_1()
                        .border_dashed()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-mod-muted"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(slot.clone().map(|slot| img(slot).w(px(70.)).h(px(55.))))
                        .into_any_element()
                })
                .collect();
            div()
                .px(px(16.))
                .pb(px(16.))
                .flex()
                .flex_wrap()
                .gap(px(TILE_GAP))
                .children(tiles)
                .children(placeholders)
                .into_any_element()
        } else if card.unused {
            div()
                .p(px(16.))
                .rounded_b(px(8.))
                .bg(color("--card-background-default"))
                .child(
                    div()
                        .py(px(4.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child("Upload more stickers to make use of this unlocked level."),
                )
                .into_any_element()
        } else {
            let added = ADDITIONAL[tier];
            let line = if tier == 1 {
                format!("+{added} Sticker Slots")
            } else {
                format!("+{added} Sticker Slots ({} total)", TOTAL[tier])
            };
            div()
                .p(px(16.))
                .rounded_b(px(8.))
                .bg(color("--card-background-default"))
                .child(
                    div()
                        .py(px(16.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .children(slot.map(|slot| img(slot).w(px(88.)).h(px(60.)).mb(px(16.))))
                        .child(
                            div()
                                .mb(px(16.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(400.))
                                .text_color(color("--text-default"))
                                .child(line),
                        )
                        .child(
                            button(
                                format!("sticker-buy-{tier}"),
                                "Buy Level",
                                true,
                                color("--control-connected-background-default"),
                                color("--control-connected-border-default"),
                            )
                            .on_click(to_perks),
                        ),
                )
                .into_any_element()
        };
        div()
            .when(tier != 3, |this| this.mb(px(24.)))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-mod-muted"))
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// The page (`Nj`): the banner on a plain server, or else the upload instructions, then the timeline.
    pub(super) fn server_stickers_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let stickers = self.stickers_of(guild);
        let known = self.stores.guild(guild);
        let premium = known.map_or(0, |known| known.premium_tier as usize).min(3);
        let more = known.is_some_and(|known| known.features.iter().any(|f| f == "MORE_STICKERS"));
        let current = if more { 3 } else { premium };
        let boosts = if more {
            REQUIRED[3]
        } else {
            known.map_or(0, |known| known.premium_subscription_count)
        };
        let me = self.stores.me();
        let can = |permission: u64| {
            known
                .zip(me)
                .is_some_and(|(known, me)| permissions::can_in_guild(known, me, permission))
        };
        let can_create = can(CREATE_EXPRESSIONS);
        let manage_all = can(MANAGE_EXPRESSIONS);
        let my_id = me.map(|me| me.id.to_string());
        let can_manage = move |sticker: &Value| {
            manage_all || (can_create && sticker["user"]["id"].as_str().map(str::to_owned) == my_id)
        };
        let intro: AnyElement = if premium != 0 || !stickers.is_empty() {
            self.sticker_instructions(cx)
        } else {
            self.sticker_upsell(cx)
        };
        let loaded = self
            .fetched
            .contains_key(&format!("{STICKERS_PREFIX}{guild}"));
        let cards: Vec<TierCard> = (0..4)
            .map(|tier| tier_card(tier, stickers.len(), current, can_create, more))
            .collect();
        let offsets = marker_offsets(&cards);
        let progress = self.sticker_progress(&offsets, current, boosts, cx);
        let tiers: Vec<AnyElement> = cards
            .iter()
            .map(|card| self.sticker_tier(guild, card, &stickers, &can_manage, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Stickers"),
            )
            .child(intro)
            .child(
                div()
                    .mt(px(40.))
                    .mb(px(40.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .when(loaded, |this| {
                this.child(
                    div()
                        .relative()
                        .child(progress)
                        .child(
                            div()
                                .ml(px(38.))
                                .w(px(654.))
                                .flex_none()
                                .child(
                                    div()
                                        .mb(px(16.))
                                        .child(
                                            div()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .child("No Server Boost"),
                                        )
                                        .child(
                                            div()
                                                .mt(px(4.))
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .text_color(color("--text-default"))
                                                .child("No one has bestowed Boosts to this server yet. See if any members would kindly bless your server for server-wide Boost Perks!"),
                                        ),
                                )
                                .children(tiers),
                        ),
                )
            })
            .into_any_element()
    }

    /// `Nh`: the banner, 660 by 240.
    fn sticker_upsell(&self, cx: &mut Context<Self>) -> AnyElement {
        let art = self.images.get(UPSELL_ART).cloned();
        let white = |id: &'static str, text: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(40.))
                .px(px(15.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::rgba(0x97979f1f))
                .bg(gpui::rgba(0xffffffff))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(gpui::rgba(0x000000ff))
                .child(text)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_perks(super::server_boost::Perks::Banner, cx);
                    cx.notify();
                }))
        };
        div()
            .relative()
            .w(px(660.))
            .h(px(240.))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(linear_gradient(
                90.,
                linear_color_stop(color("--guild-boosting-blue-for-gradients"), 0.),
                linear_color_stop(color("--guild-boosting-purple-for-gradients"), 1.),
            ))
            // `background`: repeated from the corner, so the top of a second copy shows under the 218 tall first.
            .children(art.clone().map(|art| {
                img(art)
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w(px(660.))
                    .h(px(218.))
            }))
            .children(art.map(|art| {
                img(art)
                    .absolute()
                    .top(px(218.))
                    .left(px(0.))
                    .w(px(660.))
                    .h(px(218.))
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .pt(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(24.))
                            .line_height(px(32.))
                            .font_weight(FontWeight(600.))
                            .text_color(gpui::rgba(0xffffffff))
                            .child("Get Boosted"),
                    )
                    .child(
                        div()
                            .mb(px(24.))
                            // `max-width: 400px`; GPUI's advance is slightly wider, so the lines break as captured.
                            .w(px(404.))
                            .text_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(gpui::rgba(0xffffffff))
                            .child("Enjoy more stickers and other perks by boosting your server to Level 1. Each Level unlocks more sticker slots and new benefits for everyone."),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(white("sticker-boost-server", "Boost Server", cx))
                            .child(white("sticker-learn-more", "Learn More", cx)),
                    ),
            )
            .into_any_element()
    }

    /// `Nx`.
    fn sticker_instructions(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child("Upload Instructions"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(format!("Stickers can be static (JPG, PNG) or animated (APNG, GIF). Stickers must be exactly 320 x 320 pixels and no larger than {MAX_FILE_SIZE_TEXT}. We will automatically resize static JPG, PNG and animated GIF stickers for you.")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(self.text_with_link(
                        "sticker-article",
                        "For more information about file types and tips on uploading custom stickers, check out this ",
                        "Help Center article",
                        ".",
                        cx,
                        |_, cx| cx.open_url(UPLOAD_ARTICLE),
                    )),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_tables() {
        assert_eq!(slots_free(0, 0), 5);
        assert_eq!(slots_free(3, 0), 2);
        assert_eq!(slots_free(7, 0), 0);
        assert_eq!(slots_free(7, 1), 8);
        assert_eq!(open_tier(0, false), 0);
        assert_eq!(open_tier(5, false), 1);
        assert_eq!(open_tier(60, false), 0);
        assert_eq!(open_tier(60, true), 3);
    }

    #[test]
    fn the_free_card_has_five_placeholders_and_the_rest_are_locked() {
        let free = tier_card(0, 0, 0, true, false);
        assert!(free.upload);
        assert_eq!(free.placeholders, 5);
        assert_eq!(free.height(), 243.);
        let level = tier_card(1, 0, 0, true, false);
        assert!(level.locked && level.status && !level.unlock_button);
        assert_eq!(level.height(), 283.);
        // As captured: markers at 109, 376, 683 and 990 under the first block's top.
        let cards: Vec<TierCard> = (0..4)
            .map(|tier| tier_card(tier, 0, 0, true, false))
            .collect();
        assert_eq!(marker_offsets(&cards), vec![109., 376., 683., 990.]);
        assert_eq!(progress_height(&[109., 376., 683., 990.], 0, 0), 109.);
    }

    #[test]
    fn one_sticker_leaves_four_placeholders_and_a_full_tier_offers_more() {
        let card = tier_card(0, 1, 0, true, false);
        assert_eq!((card.tiles, card.placeholders), (1, 4));
        let full = tier_card(0, 5, 0, true, false);
        assert!(!full.upload && !full.unused);
        let next = tier_card(1, 5, 0, true, false);
        assert!(next.unlock_button && !next.upload);
    }

    #[test]
    fn files_are_checked_like_on_file_select() {
        let error = prepare_file("x.txt", vec![1], false, false).err().unwrap();
        assert_eq!(error.message, "Unsupported file format");
        assert!(error.blocking);
        let error = prepare_file("x.json", b"{}".to_vec(), false, false)
            .err()
            .unwrap();
        assert_eq!(error.message, "Unsupported file format");
        let big = vec![0; MAX_FILE_SIZE + 1];
        let error = prepare_file("x.gif", big.clone(), false, true)
            .err()
            .unwrap();
        assert_eq!(
            error.message,
            "That sticker was too big! Stickers must be under 512KB."
        );
        assert!(!error.blocking);
        let file = prepare_file("x.gif", vec![0; 10], false, false)
            .ok()
            .unwrap();
        assert_eq!(file.file_type, "image/gif");
        assert_eq!(file.original_md5, "a63c90cc3684ad8b0a2176a6a8fe9005");
    }

    #[test]
    fn animated_png_is_told_by_its_chunks() {
        assert!(is_animated_png(b"..acTL..IDAT"));
        assert!(!is_animated_png(b"..IDAT..acTL"));
        assert!(!is_animated_png(b"..IDAT"));
    }

    #[test]
    fn descriptions_are_empty_or_two_to_a_hundred() {
        assert!(description_ok(""));
        assert!(!description_ok("a"));
        assert!(description_ok("ab"));
        assert!(!description_ok(&"a".repeat(101)));
    }

    #[test]
    fn sticker_urls_follow_the_format() {
        let sticker = serde_json::json!({"id": "9", "format_type": 4});
        assert_eq!(
            sticker_url(&sticker).as_deref(),
            Some("https://media.discordapp.net/stickers/9.gif?size=80")
        );
        let still = serde_json::json!({"id": "9", "format_type": 1});
        assert_eq!(
            sticker_url(&still).as_deref(),
            Some("https://media.discordapp.net/stickers/9.webp?size=80&quality=lossless")
        );
        let lottie = serde_json::json!({"id": "9", "format_type": 3});
        assert_eq!(sticker_url(&lottie), None);
    }
}
