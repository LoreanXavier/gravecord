//! Video Background: the choice grid of Voice & Video and of the camera preview modal, the stored choice, and handing
//! it to the camera processing in `dc_media::background`.
//!
//! Read from the pinned build:
//! * the row `camera_background_setting` (module 158935 `sB`) is shown while the engine supports
//!   `VIDEO_BACKGROUND_FILTER` and a camera exists (module 463951 through 952572). Its grid is module 717359 (`el`,
//!   `en`, `Q`, `Z`, `X`, `ee`, `et`) with the classes of module 516978 (`backgroundOptions_imJCJi` and friends); the
//!   sizes come from `design/dom/settings/voice-video-darker-cozy.json` (tiles 162x94.125 with 16px gaps, the label
//!   20px with 8px under it, the grid 16px under the label's control);
//! * the options are None, Blur, Custom (Nitro: `canUseCustomBackgrounds`, the `VIDEO_FILTER_ASSETS` perk, module
//!   722178; without it the inline upsell), the custom pictures, then the presets in the order of `sl` (module
//!   577718: 7, 8, 9, 10, 0, 1, 2, 3) with the sources and names of module 665096;
//! * the value is the synced `voiceAndVideo.videoBackgroundFilterDesktop` (module 965162 `R4`/`Hk`): nothing,
//!   `"blur"`, a preset number, or a custom asset. It is global, not per camera;
//! * a custom file is a JPG, PNG, GIF or MP4 of at most 10 MiB (`G9 = 0xa00000`, the filter list of `et`), at most 25
//!   of them (`maxCustomBackgrounds`).
//!
//! What differs, and why (BLOCKED.md "Video: stream attenuation, hook, background"): the choice is stored on this
//! machine and read from the synced settings when there is none here, but not written back to the account (the
//! settings writer has no entry for this oneof yet); a custom picture is kept as a file on this machine instead of
//! being uploaded to `/users/@me/video-filters/assets`; GIF and MP4 files and the four animated presets cannot be
//! decoded here, so they end in the official "Something went wrong" message and the camera shows no background.

use super::super::{Root, color, mask_icon};
use crate::ui::video_tiles::render_image;
use dc_media::Picture;
use dc_media::background::{self as engine, Background};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, PathPromptOptions, RenderImage,
    SharedString, deferred, div, img, linear_color_stop, linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the choice is kept on this machine: the synced setting's own name.
pub(in crate::ui) const STORED: &str = "voiceAndVideo.videoBackgroundFilterDesktop";
/// The custom pictures added on this machine, newest first.
const STORED_CUSTOM: &str = "voiceAndVideo.customBackgrounds";
/// The hotspots already seen (`ANIMATED_VIDEO_BG_*_NEW`), so their NEW badges stay away.
const STORED_SEEN: &str = "voiceAndVideo.backgroundHotspotsSeen";

/// `G9` of module 577718: the largest custom file.
const MAX_FILE_BYTES: u64 = 0x00a0_0000;
/// `maxCustomBackgrounds` of module 717359.
const MAX_CUSTOM: usize = 25;
/// The extensions of the upload's filter (`et` in module 717359).
const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "gif", "mp4"];

/// The Blur tile's picture (`backgroundOptionBlurBackground_imJCJi`), drawn through `filter: blur(14px)`.
const BLUR_ART: &str = "https://cdn.discordapp.com/assets/content/9b1bb92a5685e3f1d67378826197687bb24f8cf1319a4ab2375fbc8a3fb7d0fe.png";
const BLUR_ART_KEY: &str = "video-background:blur-art";
const BLUR_RADIUS: f32 = 14.;

/// The settings page's measured tile: 162x94.125 in a 696px grid.
const TILE_RATIO: f32 = 94.125 / 162.;
/// `minmax(132px, 1fr)` and `gap: var(--space-16)`.
const TILE_MIN: f32 = 132.;
const GAP: f32 = 16.;

const CDN: &str = "https://cdn.discordapp.com/assets/content/";

/// One preset of module 665096.
struct Preset {
    id: u32,
    file: &'static str,
    video: bool,
    /// The hotspot whose NEW badge it carries (`hotspotLocation`).
    hotspot: Option<&'static str>,
}

/// In the grid's order (`sl` of module 577718).
const PRESETS: [Preset; 8] = [
    Preset {
        id: 7,
        file: "0f96e9ef2cf5c9d2eef831bd1e4155973a608f9662cb9a2bf91a2523b918c803.mp4",
        video: true,
        hotspot: Some("ANIMATED_VIDEO_BG_CAPERNITE_DAY_NEW"),
    },
    Preset {
        id: 8,
        file: "52f91129995158682c465310f61e64cd61fbf227f0dc6b43313c5e8226818661.mp4",
        video: true,
        hotspot: Some("ANIMATED_VIDEO_BG_CAPERNITE_NIGHT_NEW"),
    },
    Preset {
        id: 9,
        file: "67802e4ea1da5f87f7610d4143626979e6337d37d4e16c1eff2d45276dd70a89.mp4",
        video: true,
        hotspot: Some("ANIMATED_VIDEO_BG_HACKER_DEN_NEW"),
    },
    Preset {
        id: 10,
        file: "39743155a132962e7f660f16833725034975458ec167d30d9c6971f93b439422.mp4",
        video: true,
        hotspot: Some("ANIMATED_VIDEO_BG_WUMPICE_NEW"),
    },
    Preset {
        id: 0,
        file: "e321ddf99270fad41d4c5b21de89a82984d8469c892a3f9a924f5996764d33b4.png",
        video: false,
        hotspot: None,
    },
    Preset {
        id: 1,
        file: "435961b527cedf836cfdfb34cf7c4c7353b248f93be15c30a9a3b778b7d20c76.png",
        video: false,
        hotspot: None,
    },
    Preset {
        id: 2,
        file: "cc4247f477caba753bfda3b66dd480a417c59ccd741b3809fedbfc3c8bdd49fd.png",
        video: false,
        hotspot: None,
    },
    Preset {
        id: 3,
        file: "857242c637737acc689e8e06fdde1ce120fc595fd65833f4f86944eaea46527e.png",
        video: false,
        hotspot: None,
    },
];

/// The Custom tile's hotspot (`ANIMATED_VIDEO_BG_CUSTOM_TOOLTIP_NEW`).
const CUSTOM_HOTSPOT: &str = "ANIMATED_VIDEO_BG_CUSTOM_TOOLTIP_NEW";

fn preset(id: u32) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

fn preset_key(id: u32) -> String {
    format!("video-background:preset:{id}")
}

/// The background option (`selectedBackgroundOption`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum Choice {
    #[default]
    None,
    Blur,
    Preset(u32),
    /// A picture added on this machine.
    Custom(PathBuf),
}

impl Choice {
    /// The stored form, in the official option's shape: null, `"blur"`, a number, or the custom picture.
    pub(in crate::ui) fn to_json(&self) -> Value {
        match self {
            Self::None => Value::Null,
            Self::Blur => json!("blur"),
            Self::Preset(id) => json!(id),
            Self::Custom(path) => json!({ "path": path.to_string_lossy() }),
        }
    }

    /// The stored form read back; anything unknown is None, like the official `i()` (module 329551).
    pub(in crate::ui) fn from_json(value: &Value) -> Self {
        match value {
            Value::String(text) if text == "blur" => Self::Blur,
            Value::Number(number) => number
                .as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .filter(|id| preset(*id).is_some())
                .map_or(Self::None, Self::Preset),
            Value::Object(object) => object
                .get("path")
                .and_then(Value::as_str)
                .map_or(Self::None, |path| Self::Custom(PathBuf::from(path))),
            _ => Self::None,
        }
    }

    /// `voice_and_video` (5) of the synced settings: `blur` (1) holds `use_blur` (1), `preset_option` is 2. The
    /// settings reader answers 0 for a missing varint, so a stored preset 0 (Cybercity) reads as no background.
    fn from_synced(use_blur: Option<u64>, preset_option: Option<u64>) -> Self {
        if use_blur == Some(1) {
            return Self::Blur;
        }
        preset_option
            .filter(|id| *id != 0)
            .and_then(|id| u32::try_from(id).ok())
            .filter(|id| preset(*id).is_some())
            .map_or(Self::None, Self::Preset)
    }
}

/// Why a picked file was refused, as the official messages say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Refusal {
    /// `mrlScX`: over 10 MB.
    TooLarge,
    /// `Mt8yDB`: the upload failed (here: the file could not be read as a picture).
    Unreadable,
}

/// Checks a picked file before it becomes a custom background.
pub(in crate::ui) fn check_file(path: &Path, size: u64) -> Result<(), Refusal> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !EXTENSIONS.contains(&extension.as_str()) {
        return Err(Refusal::Unreadable);
    }
    if size > MAX_FILE_BYTES {
        return Err(Refusal::TooLarge);
    }
    Ok(())
}

/// A still picture decoded to RGBA, or None for what cannot be decoded here (GIF, MP4, damaged files).
pub(in crate::ui) fn decode_still(bytes: &[u8]) -> Option<Picture> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    Picture::rgba(width, height, decoded.into_raw())
}

/// The Video Background state kept with the camera page.
#[derive(Default)]
pub(in crate::ui) struct BackgroundPage {
    /// The saved choice: what a camera that turns on uses.
    pub choice: Choice,
    /// The preview modal's own choice while it is open (module 308229 `P`).
    pub pending: Option<Choice>,
    /// `errorMessage` of the grid.
    pub error: Option<&'static str>,
    restored: bool,
    custom: Vec<PathBuf>,
    seen: Vec<String>,
    requested: Cell<bool>,
    blur_art: RefCell<Option<Arc<RenderImage>>>,
    /// Bumped on every choice so a slow decode of an older one is dropped.
    serial: u64,
}

fn error_text(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::TooLarge => dc_intl::tr(
            "Uh oh! That file size was too large. Please use a different image under 10 MB.",
        ),
        Refusal::Unreadable => dc_intl::tr(
            "Uh oh! Something went wrong when uploading your background. Please try again or use a different image.",
        ),
    }
}

fn apply_failed() -> &'static str {
    dc_intl::tr(
        "Uh oh! Something went wrong when setting your background. Please try again or use a different background.",
    )
}

/// The folders searched for the segmentation model: next to the executable, in this run's data folder, and the one
/// `GRAVECORD_SEGMENTATION_MODELS` names.
fn model_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("GRAVECORD_SEGMENTATION_MODELS") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("models"));
    }
    if let Some(data) = crate::storage::data_dir() {
        dirs.push(data.join("models"));
    }
    dirs
}

/// Capture builds only: `GRAVECORD_VIDEO_BACKGROUND_DEMO=1` counts as a camera being there, so the harness can draw
/// the grid on a machine without one. No device is opened either way.
fn demo_camera() -> bool {
    cfg!(feature = "capture")
        && std::env::var("GRAVECORD_VIDEO_BACKGROUND_DEMO").as_deref() == Ok("1")
}

/// A list of strings kept in storage.
fn stored_list(key: &str) -> Vec<String> {
    crate::storage::get(key)
        .and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Points the engine at the model folders, once per run.
fn ensure_model_dirs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| engine::set_model_dirs(model_dirs()));
}

impl Root {
    /// The saved choice, read from storage (or the synced settings) until [`Self::video_background_restore`] ran.
    fn saved_choice(&self) -> Choice {
        let page = &self.voice.camera.background;
        if page.restored {
            return page.choice.clone();
        }
        match crate::storage::get(STORED) {
            Some(value) => Choice::from_json(&value),
            None => Choice::from_synced(
                self.stores.proto_varint(&[5, 1], 1),
                self.stores.proto_varint(&[5], 2),
            ),
        }
    }

    /// Reads the stored choice once and hands the current one to the camera engine. Called before a camera starts,
    /// when the preview modal opens and when the settings page shows the row.
    pub(in crate::ui) fn video_background_restore(&mut self, cx: &mut Context<Self>) {
        ensure_model_dirs();
        if !self.voice.camera.background.restored {
            let choice = self.saved_choice();
            let page = &mut self.voice.camera.background;
            page.restored = true;
            page.choice = choice;
            page.custom = stored_list(STORED_CUSTOM)
                .into_iter()
                .map(PathBuf::from)
                .collect();
            page.seen = stored_list(STORED_SEEN);
        }
        let choice = self.voice.camera.background.choice.clone();
        self.video_background_engage(choice, cx);
    }

    /// Whether the grid shows: `supports(VIDEO_BACKGROUND_FILTER)` (the model is here) and a camera exists.
    pub(in crate::ui) fn video_background_shown(&self) -> bool {
        ensure_model_dirs();
        engine::available()
            && (demo_camera()
                || !self.voice.camera.devices.is_empty()
                || !dc_voice::camera_devices().is_empty())
    }

    /// `canUseCustomBackgrounds`: the `VIDEO_FILTER_ASSETS` perk.
    fn can_use_custom_backgrounds(&self) -> bool {
        dc_state::premium::PremiumType::from_wire(self.stores.account.premium_type)
            .has(dc_state::premium::Feature::VideoFilterAssets)
    }

    /// The choice the grid shows: the modal's own while it is open, else the saved one.
    fn video_background_shown_choice(&self, in_modal: bool) -> Choice {
        let page = &self.voice.camera.background;
        match (&page.pending, in_modal) {
            (Some(pending), true) => pending.clone(),
            _ => self.saved_choice(),
        }
    }

    /// A tile was picked. On the settings page it is saved and applied at once (`gB` then `_C` of module 352430);
    /// in the modal it only previews until Turn On Camera or Change Background (`S1`, module 854325).
    pub(in crate::ui) fn video_background_pick(
        &mut self,
        choice: Choice,
        in_modal: bool,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.voice.camera.background;
        page.error = None;
        if in_modal {
            page.pending = Some(choice.clone());
        } else {
            page.choice = choice.clone();
            crate::storage::set(STORED, choice.to_json());
        }
        self.video_background_engage(choice, cx);
        cx.notify();
    }

    /// The modal's confirm: its choice becomes the saved one.
    pub(in crate::ui) fn video_background_commit_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(choice) = self.voice.camera.background.pending.take() {
            self.voice.camera.background.choice = choice.clone();
            crate::storage::set(STORED, choice.to_json());
            self.video_background_engage(choice, cx);
        }
    }

    /// The modal closed without confirming: the saved choice is back on the camera.
    pub(in crate::ui) fn video_background_drop_pending(&mut self, cx: &mut Context<Self>) {
        let page = &mut self.voice.camera.background;
        page.error = None;
        if page.pending.take().is_some() {
            let choice = page.choice.clone();
            self.video_background_engage(choice, cx);
        }
    }

    /// Hands `choice` to the camera engine; a picture is decoded first, off the UI thread.
    fn video_background_engage(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let page = &mut self.voice.camera.background;
        page.serial += 1;
        let serial = page.serial;
        match choice {
            Choice::None => engine::set_background(Background::None),
            Choice::Blur => engine::set_background(Background::Blur),
            Choice::Preset(id) => {
                let Some(preset) = preset(id) else {
                    engine::set_background(Background::None);
                    return;
                };
                if preset.video {
                    // The animated presets are MP4: nothing here decodes them.
                    engine::set_background(Background::None);
                    page.error = Some(apply_failed());
                    return;
                }
                let key = preset_key(id);
                self.request_image(key.clone(), format!("{CDN}{}", preset.file));
                cx.spawn(async move |this, cx| {
                    // The picture arrives through the image requests; wait for it a while.
                    for _ in 0..150 {
                        let bytes = this
                            .update(cx, |this, _| {
                                (this.voice.camera.background.serial == serial)
                                    .then(|| this.images.get(&key).map(|image| image.bytes.clone()))
                            })
                            .ok()
                            .flatten();
                        match bytes {
                            None => return,
                            Some(Some(bytes)) => {
                                let decoded = cx
                                    .background_executor()
                                    .spawn(async move { decode_still(&bytes) })
                                    .await;
                                let _ = this.update(cx, |this, cx| {
                                    this.video_background_decoded(serial, decoded, cx)
                                });
                                return;
                            }
                            Some(None) => {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(100))
                                    .await;
                            }
                        }
                    }
                })
                .detach();
            }
            Choice::Custom(path) => {
                cx.spawn(async move |this, cx| {
                    let decoded = cx
                        .background_executor()
                        .spawn(async move {
                            std::fs::read(&path)
                                .ok()
                                .and_then(|bytes| decode_still(&bytes))
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.video_background_decoded(serial, decoded, cx)
                    });
                })
                .detach();
            }
        }
    }

    fn video_background_decoded(
        &mut self,
        serial: u64,
        decoded: Option<Picture>,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.voice.camera.background;
        if page.serial != serial {
            return;
        }
        match decoded {
            Some(picture) => engine::set_background(Background::Image(Arc::new(picture))),
            None => {
                engine::set_background(Background::None);
                page.error = Some(apply_failed());
            }
        }
        cx.notify();
    }

    /// The Custom tile with the perk: pick a file, check it, add it and select it (`onAddBackgroundImage`).
    fn video_background_add_custom(&mut self, in_modal: bool, cx: &mut Context<Self>) {
        if crate::inert::skip("video background file picker") {
            return;
        }
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let size = std::fs::metadata(&path)
                .map(|meta| meta.len())
                .unwrap_or(u64::MAX);
            let _ = this.update(cx, |this, cx| {
                if let Err(refusal) = check_file(&path, size) {
                    this.voice.camera.background.error = Some(error_text(refusal));
                    cx.notify();
                    return;
                }
                let mut custom = this.custom_list();
                custom.retain(|known| known != &path);
                custom.insert(0, path.clone());
                custom.truncate(MAX_CUSTOM);
                let list: Vec<Value> = custom
                    .iter()
                    .map(|path| Value::String(path.to_string_lossy().into_owned()))
                    .collect();
                crate::storage::set(STORED_CUSTOM, Value::Array(list));
                this.voice.camera.background.custom = custom;
                this.video_background_pick(Choice::Custom(path), in_modal, cx);
            });
        })
        .detach();
    }

    fn video_background_seen(&mut self, hotspot: &'static str) {
        if !self.voice.camera.background.restored {
            let page = &mut self.voice.camera.background;
            page.seen = stored_list(STORED_SEEN);
            page.custom = stored_list(STORED_CUSTOM)
                .into_iter()
                .map(PathBuf::from)
                .collect();
        }
        let page = &mut self.voice.camera.background;
        if page.seen.iter().any(|seen| seen == hotspot) {
            return;
        }
        page.seen.push(hotspot.to_owned());
        let list: Vec<Value> = page.seen.iter().map(|seen| json!(seen)).collect();
        crate::storage::set(STORED_SEEN, Value::Array(list));
    }

    fn hotspot_new(&self, hotspot: Option<&'static str>) -> bool {
        let page = &self.voice.camera.background;
        let stored;
        let seen = if page.restored {
            &page.seen
        } else {
            stored = stored_list(STORED_SEEN);
            &stored
        };
        hotspot.is_some_and(|hotspot| !seen.iter().any(|seen| seen == hotspot))
    }

    /// The custom pictures, newest first.
    fn custom_list(&self) -> Vec<PathBuf> {
        let page = &self.voice.camera.background;
        if page.restored {
            page.custom.clone()
        } else {
            stored_list(STORED_CUSTOM)
                .into_iter()
                .map(PathBuf::from)
                .collect()
        }
    }

    /// The pictures the tiles show, asked for once.
    fn video_background_request_art(&self) {
        let page = &self.voice.camera.background;
        if page.requested.replace(true) {
            return;
        }
        let send = |key: String, url: String| {
            let _ = self.requests.send(crate::net::Request::Image { key, url });
        };
        send(BLUR_ART_KEY.to_owned(), BLUR_ART.to_owned());
        for preset in PRESETS.iter().filter(|preset| !preset.video) {
            send(preset_key(preset.id), format!("{CDN}{}", preset.file));
        }
    }

    /// The Blur tile's picture blurred by 14px at the tile's size, made once when the picture is here.
    fn blur_art(&self, width: f32, height: f32) -> Option<Arc<RenderImage>> {
        let page = &self.voice.camera.background;
        if let Some(art) = page.blur_art.borrow().clone() {
            return Some(art);
        }
        let source = self.images.get(BLUR_ART_KEY)?;
        let decoded = image::load_from_memory(&source.bytes).ok()?.to_rgba8();
        let (w, h) = (width.round().max(1.) as u32, height.round().max(1.) as u32);
        let fitted = dc_media::background::composite::cover(
            decoded.as_raw(),
            decoded.width() as usize,
            decoded.height() as usize,
            w as usize,
            h as usize,
        );
        let buffer = image::RgbaImage::from_raw(w, h, fitted)?;
        let blurred = image::imageops::blur(&buffer, BLUR_RADIUS);
        let picture = Picture::rgba(w, h, blurred.into_raw())?;
        let art = render_image(&picture)?;
        *page.blur_art.borrow_mut() = Some(art.clone());
        Some(art)
    }

    /// `camera_background_setting`: the label and the grid, 696 wide on the page.
    pub(in crate::ui) fn video_background_row(
        &self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::tr("Video Background")),
            )
            .child(self.video_background_grid(width, false, cx))
            .into_any_element()
    }

    /// The grid alone (`en` of module 717359) with the error line under it.
    pub(in crate::ui) fn video_background_grid(
        &self,
        width: f32,
        in_modal: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.video_background_request_art();
        let columns = ((width + GAP) / (TILE_MIN + GAP)).floor().max(1.);
        let tile_w = (width - GAP * (columns - 1.)) / columns;
        let tile_h = tile_w * TILE_RATIO;
        let selected = self.video_background_shown_choice(in_modal);
        let page = &self.voice.camera.background;
        let mut tiles: Vec<AnyElement> = Vec::new();
        let size = (tile_w, tile_h);
        tiles.push(self.icon_tile(
            TileSpec {
                id: "video-background-none".into(),
                selected: selected == Choice::None,
                disabled: false,
                size,
            },
            "deny_circle",
            dc_intl::tr("None"),
            None,
            cx.listener(move |this, _, _, cx| {
                this.video_background_pick(Choice::None, in_modal, cx)
            }),
        ));
        let blur_art = self.blur_art(tile_w, tile_h);
        tiles.push(
            self.icon_tile(
                TileSpec {
                    id: "video-background-blur".into(),
                    selected: selected == Choice::Blur,
                    disabled: false,
                    size,
                },
                "vbg_blur",
                dc_intl::tr("Blur"),
                Some(match blur_art {
                    Some(art) => img(art)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                    None => div().into_any_element(),
                }),
                cx.listener(move |this, _, _, cx| {
                    this.video_background_pick(Choice::Blur, in_modal, cx)
                }),
            ),
        );
        tiles.push(self.custom_tile(in_modal, size, cx));
        if self.can_use_custom_backgrounds() {
            for (index, path) in self.custom_list().into_iter().enumerate() {
                let choice = Choice::Custom(path.clone());
                let picked = selected == choice;
                tiles.push(picture_tile(
                    TileSpec {
                        id: SharedString::from(format!("video-background-custom-{index}")),
                        selected: picked,
                        disabled: false,
                        size,
                    },
                    Some(
                        img(path)
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                    ),
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        this.video_background_pick(choice.clone(), in_modal, cx)
                    }),
                ));
            }
        }
        for preset in &PRESETS {
            let picture = (!preset.video)
                .then(|| self.images.get(&preset_key(preset.id)).cloned())
                .flatten()
                .map(|image| {
                    img(image)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                });
            let id = preset.id;
            let hotspot = preset.hotspot;
            tiles.push(picture_tile(
                TileSpec {
                    id: SharedString::from(format!("video-background-preset-{id}")),
                    selected: selected == Choice::Preset(id),
                    disabled: false,
                    size,
                },
                picture,
                preset.video,
                self.hotspot_new(hotspot),
                cx.listener(move |this, _, _, cx| {
                    if let Some(hotspot) = hotspot {
                        this.video_background_seen(hotspot);
                    }
                    this.video_background_pick(Choice::Preset(id), in_modal, cx)
                }),
            ));
        }
        let per_row = columns as usize;
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            let row: Vec<AnyElement> = tiles.by_ref().take(per_row).collect();
            rows.push(div().flex().gap(px(GAP)).children(row).into_any_element());
        }
        div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .children(rows)
            .children(page.error.map(|message| {
                div()
                    .mt(px(4. - GAP))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(message)
            }))
            .into_any_element()
    }

    /// `Q`: an icon over its label, on a plain or pictured background.
    fn icon_tile(
        &self,
        spec: TileSpec,
        glyph: &'static str,
        label: &'static str,
        backdrop: Option<AnyElement>,
        on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> AnyElement {
        let inner = div()
            .relative()
            .size_full()
            .children(backdrop.map(|backdrop| div().absolute().inset_0().child(backdrop)))
            .child(icon_content(glyph, div().child(label).into_any_element()));
        tile_frame(spec, inner.into_any_element(), Vec::new(), on_click)
    }

    /// The Custom tile: with the perk it adds a file (`et`), without it opens the upsell (`ee`).
    fn custom_tile(&self, in_modal: bool, size: (f32, f32), cx: &mut Context<Self>) -> AnyElement {
        let can = self.can_use_custom_backgrounds();
        let full = can && self.custom_list().len() >= MAX_CUSTOM;
        let label = div()
            .flex()
            .items_center()
            .child(
                div()
                    .mr(px(4.))
                    .child(mask_icon("nitro_wheel", 14., "--white")),
            )
            .child(div().truncate().child(dc_intl::tr("Custom")))
            .into_any_element();
        // `backgroundCustomInlineUpsellBackground`: the tier-2 gradient in three stops, drawn as two halves.
        let gradient = div()
            .absolute()
            .inset_0()
            .flex()
            .when(full, |this| this.opacity(0.2))
            .child(div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(color("--premium-tier-2-purple-for-gradients"), 0.),
                linear_color_stop(color("--premium-tier-2-purple-for-gradients-2"), 1.),
            )))
            .child(div().flex_1().h_full().bg(linear_gradient(
                90.,
                linear_color_stop(color("--premium-tier-2-purple-for-gradients-2"), 0.),
                linear_color_stop(color("--premium-tier-2-pink-for-gradients"), 1.),
            )));
        // `backgroundCustomInlineUpsellBackgroundDarkener`: neutral-45 at 0.2 (0.04 when the list is full).
        let darkener = div()
            .absolute()
            .inset_0()
            .bg(color("--neutral-45"))
            .opacity(if full { 0.04 } else { 0.2 });
        let inner = div()
            .relative()
            .size_full()
            .child(gradient)
            .child(darkener)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .when(full, |this| this.opacity(0.2))
                    .child(icon_content("clips_image_plus", label)),
            );
        let badge = self
            .hotspot_new(Some(CUSTOM_HOTSPOT))
            .then(new_badge)
            .into_iter()
            .collect();
        tile_frame(
            TileSpec {
                id: "video-background-custom".into(),
                selected: false,
                disabled: full,
                size,
            },
            inner.into_any_element(),
            badge,
            cx.listener(move |this, _, _, cx| {
                this.video_background_seen(CUSTOM_HOTSPOT);
                if this.can_use_custom_backgrounds() {
                    this.video_background_add_custom(in_modal, cx);
                } else {
                    // The upsell modal of module 647098 is not built; the Nitro page stands in.
                    this.open_nitro(cx);
                }
                cx.notify();
            }),
        )
    }
}

/// `Z`: a picture (or, for an animated preset, the empty tile) with the play mark and the NEW badge.
fn picture_tile(
    spec: TileSpec,
    picture: Option<AnyElement>,
    video: bool,
    new: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let inner = div().size_full().children(picture).into_any_element();
    let mut overlay = Vec::new();
    if video {
        // `playIcon_imJCJi`: 16px, 4px from the top and the end, black at 0.4 behind a 12px play glyph.
        overlay.push(
            div()
                .absolute()
                .top(px(4.))
                .right(px(4.))
                .size(px(16.))
                .rounded_full()
                .bg(gpui::hsla(0., 0., 0., 0.4))
                .flex()
                .items_center()
                .justify_center()
                .child(mask_icon("vbg_play", 12., "--white"))
                .into_any_element(),
        );
    }
    if new {
        overlay.push(new_badge());
    }
    tile_frame(spec, inner, overlay, on_click)
}

/// What every tile is told: its id, whether it is selected or disabled, and its size.
struct TileSpec {
    id: SharedString,
    selected: bool,
    disabled: bool,
    size: (f32, f32),
}

/// The outer tile: the selected ring around it, the hover lift (`backgroundOption_imJCJi`).
fn tile_frame(
    TileSpec {
        id,
        selected,
        disabled,
        size: (width, height),
    }: TileSpec,
    inner: AnyElement,
    overlay: Vec<AnyElement>,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .when(!disabled, |this| this.cursor_pointer())
        .when(!selected && !disabled, |this| {
            this.hover(|style| {
                style.mt(px(-1.)).mb(px(1.)).shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.16),
                    offset: gpui::point(px(0.), px(4.)),
                    blur_radius: px(4.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
        })
        .when(!disabled, |this| this.on_click(on_click))
        .when(selected, |this| {
            this.child(
                div()
                    .absolute()
                    .top(px(-4.))
                    .left(px(-4.))
                    .right(px(-4.))
                    .bottom(px(-4.))
                    .rounded(px(4.))
                    .border_2()
                    .border_color(color("--brand-500")),
            )
        })
        .child(
            div()
                .relative()
                .size_full()
                .rounded(px(4.))
                .overflow_hidden()
                .bg(color("--primary-700"))
                .child(inner),
        )
        .children(overlay)
        .into_any_element()
}

/// `backgroundOptionContent`: a 24px-high icon box (18px glyph) over a `body-sm/normal` label, centred, white.
fn icon_content(glyph: &'static str, label: AnyElement) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .text_color(color("--white"))
        .child(
            div()
                .h(px(24.))
                .flex()
                .items_center()
                .child(mask_icon(glyph, 18., "--white")),
        )
        .child(
            div()
                .max_w_full()
                .px(px(4.))
                .text_size(px(14.))
                .line_height(px(18.))
                .whitespace_nowrap()
                .overflow_hidden()
                .child(label),
        )
        .into_any_element()
}

/// `newBadgeCutout`: the white NEW pill in a 3px ring of the page colour, 6px left of and 3px above the tile.
fn new_badge() -> AnyElement {
    deferred(
        div()
            .absolute()
            .top(px(-3.))
            .left(px(-6.))
            .p(px(3.))
            .rounded_full()
            .bg(color("--background-base-low"))
            .child(
                div()
                    .h(px(16.))
                    .px(px(8.))
                    .rounded_full()
                    .bg(color("--white"))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(gpui::black())
                    .child(dc_intl::tr("NEW")),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_through_the_stored_form() {
        for choice in [
            Choice::None,
            Choice::Blur,
            Choice::Preset(0),
            Choice::Preset(10),
            Choice::Custom(PathBuf::from(r"C:\Pictures\room.png")),
        ] {
            let stored = choice.to_json();
            let text = serde_json::to_string(&stored).unwrap();
            let back: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(Choice::from_json(&back), choice, "{text}");
        }
        assert_eq!(Choice::to_json(&Choice::Blur), json!("blur"));
        assert_eq!(Choice::to_json(&Choice::Preset(7)), json!(7));
        assert_eq!(Choice::to_json(&Choice::None), Value::Null);
    }

    #[test]
    fn unknown_stored_values_mean_none() {
        for value in [
            json!(5),
            json!(6),
            json!(11),
            json!(-1),
            json!("sharp"),
            json!({"id": "1"}),
            json!(true),
        ] {
            assert_eq!(Choice::from_json(&value), Choice::None, "{value}");
        }
    }

    #[test]
    fn the_synced_setting_is_read_when_nothing_is_stored_here() {
        assert_eq!(Choice::from_synced(Some(1), None), Choice::Blur);
        assert_eq!(Choice::from_synced(None, Some(3)), Choice::Preset(3));
        assert_eq!(Choice::from_synced(Some(0), Some(9)), Choice::Preset(9));
        assert_eq!(Choice::from_synced(None, Some(5)), Choice::None);
        assert_eq!(Choice::from_synced(None, None), Choice::None);
        // A missing field reads as 0: nothing is chosen.
        assert_eq!(Choice::from_synced(Some(0), Some(0)), Choice::None);
    }

    #[test]
    fn presets_follow_the_official_order_and_kinds() {
        let ids: Vec<u32> = PRESETS.iter().map(|preset| preset.id).collect();
        assert_eq!(ids, [7, 8, 9, 10, 0, 1, 2, 3]);
        assert!(
            PRESETS
                .iter()
                .all(|preset| preset.video == preset.file.ends_with(".mp4"))
        );
    }

    #[test]
    fn custom_files_are_checked_like_the_upload() {
        assert_eq!(check_file(Path::new("a.PNG"), 10), Ok(()));
        assert_eq!(check_file(Path::new("a.jpeg"), MAX_FILE_BYTES), Ok(()));
        assert_eq!(
            check_file(Path::new("a.png"), MAX_FILE_BYTES + 1),
            Err(Refusal::TooLarge)
        );
        assert_eq!(check_file(Path::new("a.bmp"), 10), Err(Refusal::Unreadable));
        assert_eq!(check_file(Path::new("noext"), 10), Err(Refusal::Unreadable));
    }

    #[test]
    fn stills_decode_and_junk_does_not() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let picture = decode_still(&png).unwrap();
        assert_eq!((picture.width, picture.height), (3, 2));
        assert_eq!(&picture.data[..4], &[10, 20, 30, 255]);
        assert!(decode_still(b"GIF89a....").is_none());
    }
}
