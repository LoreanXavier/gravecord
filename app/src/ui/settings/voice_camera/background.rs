//! Video Background: the choice grid of Voice & Video and of the camera preview modal, the account's choice, the
//! custom backgrounds kept on the server, and handing the choice to the camera processing in `dc_media::background`.
//!
//! Read from the pinned build:
//! * the row `camera_background_setting` (module 158935 `sB`) is shown while the engine supports
//!   `VIDEO_BACKGROUND_FILTER` and a camera exists (module 463951 through 952572). Its grid is module 717359 (`el`,
//!   `en`, `Q`, `Z`, `X`, `ee`, `et`) with the classes of module 516978 (`backgroundOptions_imJCJi` and friends); the
//!   sizes come from `design/dom/settings/voice-video-darker-cozy.json` (tiles 162x94.125 with 16px gaps, the label
//!   20px with 8px under it, the grid 16px under the label's control); the error line is FormItem's (module 452027,
//!   classes of 715965): a 12px circle and `body-xs/normal` critical text, 4px apart, right under the grid;
//! * the options are None, Blur, Custom (Nitro: `canUseCustomBackgrounds`, the `VIDEO_FILTER_ASSETS` perk, module
//!   722178; without it the inline upsell that opens modal 647098), the custom backgrounds newest used first, then the
//!   presets in the order of `sl` (module 577718: 7, 8, 9, 10, 0, 1, 2, 3) with the sources and names of module 665096;
//!   each preset and custom tile has its name as a tooltip and the `video-background-context` menu (module 5752);
//! * the value is the synced `voiceAndVideo.videoBackgroundFilterDesktop` (module 965162 `R4`/`Hk`, checked by
//!   module 329551 `i`): nothing, `"blur"`, a preset number, or a custom asset. It is global, not per camera. A pick on
//!   the settings page is applied at once (`gB`, module 854325) and saved when the row goes away (`_C`, module 352430);
//!   the preview modal saves on its button. `_C` writes the setting with `FREQUENT_USER_ACTION` (10 seconds, module
//!   355097, `markDirty` of module 594061) and, for a custom asset, posts its `last-used`;
//! * custom backgrounds are the account's `VideoBackgroundStore.videoFilterAssets` (module 912630), listed by `GET
//!   /users/@me/video-filters/assets` when the grid mounts (`HR`), added by `POST` of `{type: 0, asset: <data URL>}`
//!   (`F9`; the data URL is `FileReader.readAsDataURL` of module 946274) and removed by `DELETE .../assets/<id>`
//!   (`pW`). Their pictures are on the CDN at `video-filter-assets/<user>/<id>/<hash>.<ext>` (module 486020 `Bo`): `a_`
//!   hashes are GIFs, `v_` hashes MP4s;
//! * a custom file is a JPG, PNG, GIF or MP4 of at most 10 MiB (`G9 = 0xa00000`, the filter list of `et`), at most 25
//!   of them (`maxCustomBackgrounds`); an upload refused for its size says `mrlScX`, any other failure `Mt8yDB`.
//!
//! What differs, and why (BLOCKED.md "Video: stream attenuation, hook, background"): a picked picture goes up as it
//! is, without the official crop step (the image editor of module 685663) and without the Tenor GIF picker.

mod clip;

use super::super::{Root, color, mask_icon};
use crate::net::Request;
use crate::ui::tooltip::{Body, Side, Tip};
use crate::ui::video_tiles::render_image;
use dc_media::Picture;
use dc_media::background::animated::{Animation, FrameDecoder};
use dc_media::background::{self as engine, Background};
use dc_proto::settings::VideoBackgroundFilter;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, ObjectFit, PathPromptOptions,
    Pixels, Point, RenderImage, SharedString, anchored, deferred, div, img, linear_color_stop,
    linear_gradient, prelude::*, px,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Where the choice is mirrored on this machine, for a run whose account settings have not arrived: the synced
/// setting's own name, in the official option's shape.
pub(in crate::ui) const STORED: &str = "voiceAndVideo.videoBackgroundFilterDesktop";
/// The hotspots already seen (`ANIMATED_VIDEO_BG_*_NEW`), so their NEW badges stay away.
const STORED_SEEN: &str = "voiceAndVideo.backgroundHotspotsSeen";

/// `G9` of module 577718: the largest custom file.
const MAX_FILE_BYTES: u64 = 0x00a0_0000;
/// `maxCustomBackgrounds` of module 717359.
const MAX_CUSTOM: usize = 25;
/// The extensions of the upload's filter (`et` in module 717359).
const EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "gif", "mp4"];

/// `VIDEO_FILTER_ASSETS` and `VIDEO_FILTER_ASSET(id)` of `ENDPOINTS.md`.
const ASSETS_ROUTE: &str = "/users/@me/video-filters/assets";
/// `BACKGROUND` of `yZ` (module 577718), the only asset type the grid lists.
const TYPE_BACKGROUND: u64 = 0;
/// `FREQUENT_USER_ACTION` of module 355097: how long `_C`'s settings write waits (`markDirty`, module 594061).
pub(in crate::ui) const SAVE_DELAY: Duration = Duration::from_secs(10);

/// Keys of the answers `video_background_fetched` takes.
const KEY_PREFIX: &str = "video-background:";
const ASSETS_KEY: &str = "video-background:assets";
const UPLOAD_KEY: &str = "video-background:upload:";
const DELETE_KEY: &str = "video-background:delete:";
const LAST_USED_KEY: &str = "video-background:last-used";

/// The Blur tile's picture (`backgroundOptionBlurBackground_imJCJi`), drawn through `filter: blur(14px)`.
const BLUR_ART: &str = "https://cdn.discordapp.com/assets/content/9b1bb92a5685e3f1d67378826197687bb24f8cf1319a4ab2375fbc8a3fb7d0fe.png";
const BLUR_ART_KEY: &str = "video-background:blur-art";
const BLUR_RADIUS: f32 = 14.;
/// The upsell modal's art (`artURL` of module 647098).
const UPSELL_ART: &str = "https://cdn.discordapp.com/assets/content/2d563ce5f261ca4793e70ca87ccd10fd2b224ba302a5cd36414a5e4b4a374725.svg";
const UPSELL_ART_KEY: &str = "video-background:upsell-art";

/// The settings page's measured tile: 162x94.125 in a 696px grid.
const TILE_RATIO: f32 = 94.125 / 162.;
/// `minmax(132px, 1fr)` and `gap: var(--space-16)`.
const TILE_MIN: f32 = 132.;
const GAP: f32 = 16.;

const CDN: &str = "https://cdn.discordapp.com/assets/content/";
/// `window.GLOBAL_ENV.CDN_HOST` of the desktop build.
const CDN_HOST: &str = "cdn.discordapp.com";
/// `E.Im` of module 577718: the canvas a still background is drawn into, and the size the engine asks the CDN for.
const CANVAS: (u32, u32) = (1280, 720);
/// The size the grid's custom tiles ask the CDN for (`size: 720` in `en`).
const TILE_SIZE: u32 = 720;

/// One preset of module 665096.
struct Preset {
    id: u32,
    file: &'static str,
    video: bool,
    /// The name (`name`), as its message key.
    name: &'static str,
    /// The hotspot whose NEW badge it carries (`hotspotLocation`).
    hotspot: Option<&'static str>,
}

/// In the grid's order (`sl` of module 577718).
const PRESETS: [Preset; 8] = [
    Preset {
        id: 7,
        file: "0f96e9ef2cf5c9d2eef831bd1e4155973a608f9662cb9a2bf91a2523b918c803.mp4",
        video: true,
        name: "PM+DU1",
        hotspot: Some("ANIMATED_VIDEO_BG_CAPERNITE_DAY_NEW"),
    },
    Preset {
        id: 8,
        file: "52f91129995158682c465310f61e64cd61fbf227f0dc6b43313c5e8226818661.mp4",
        video: true,
        name: "3zoBh1",
        hotspot: Some("ANIMATED_VIDEO_BG_CAPERNITE_NIGHT_NEW"),
    },
    Preset {
        id: 9,
        file: "67802e4ea1da5f87f7610d4143626979e6337d37d4e16c1eff2d45276dd70a89.mp4",
        video: true,
        name: "1gKhX9",
        hotspot: Some("ANIMATED_VIDEO_BG_HACKER_DEN_NEW"),
    },
    Preset {
        id: 10,
        file: "39743155a132962e7f660f16833725034975458ec167d30d9c6971f93b439422.mp4",
        video: true,
        name: "TSTgyL",
        hotspot: Some("ANIMATED_VIDEO_BG_WUMPICE_NEW"),
    },
    Preset {
        id: 0,
        file: "e321ddf99270fad41d4c5b21de89a82984d8469c892a3f9a924f5996764d33b4.png",
        video: false,
        name: "SHUTBj",
        hotspot: None,
    },
    Preset {
        id: 1,
        file: "435961b527cedf836cfdfb34cf7c4c7353b248f93be15c30a9a3b778b7d20c76.png",
        video: false,
        name: "UxTcIq",
        hotspot: None,
    },
    Preset {
        id: 2,
        file: "cc4247f477caba753bfda3b66dd480a417c59ccd741b3809fedbfc3c8bdd49fd.png",
        video: false,
        name: "HFBsc8",
        hotspot: None,
    },
    Preset {
        id: 3,
        file: "857242c637737acc689e8e06fdde1ce120fc595fd65833f4f86944eaea46527e.png",
        video: false,
        name: "/Dl3+Z",
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

/// `kr` of module 776231: the CDN size ladder, the first step at least `size`.
fn cdn_size(size: u32) -> u32 {
    const LADDER: [u32; 31] = [
        16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320,
        480, 512, 600, 640, 1024, 1280, 1536, 2048, 3072, 4096,
    ];
    LADDER
        .iter()
        .copied()
        .find(|step| size <= *step)
        .unwrap_or(4096)
}

/// A custom background of the account (`VideoFilterAsset` of the REST API).
#[derive(Clone, Debug)]
pub(in crate::ui) struct Asset {
    pub id: u64,
    pub user_id: u64,
    /// `asset`: the picture's hash, `a_` for a GIF, `v_` for an MP4.
    pub hash: String,
    pub kind: u64,
    /// `last_used`, ISO 8601, when the server has one.
    pub last_used: Option<String>,
}

impl PartialEq for Asset {
    /// The grid tells assets apart by id (`i.id === e.id`).
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Asset {}

fn snowflake(value: &Value) -> Option<u64> {
    value
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| value.as_u64())
}

impl Asset {
    pub(in crate::ui) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: snowflake(&value["id"])?,
            user_id: snowflake(&value["user_id"]).unwrap_or(0),
            hash: value["asset"].as_str()?.to_owned(),
            kind: value["type"].as_u64().unwrap_or(TYPE_BACKGROUND),
            last_used: value["last_used"].as_str().map(str::to_owned),
        })
    }

    /// `VI`: an animated GIF.
    fn is_gif(&self) -> bool {
        self.hash.starts_with("a_")
    }

    /// `q6`: an MP4.
    fn is_video(&self) -> bool {
        self.hash.starts_with("v_")
    }

    /// `getVideoFilterAssetURL` (module 486020 `Bo` through `g`, with `CDN_HOST` set and `canWebP` off): a GIF hash
    /// is `.gif` when it may animate, an MP4 hash `.mp4` with no query, anything else `.png`, at the ladder size.
    fn url(&self, size: u32, can_animate: bool) -> String {
        let extension = if can_animate && self.is_video() {
            "mp4"
        } else if can_animate && self.is_gif() {
            "gif"
        } else {
            "png"
        };
        let base = format!(
            "https://{CDN_HOST}/video-filter-assets/{}/{}/{}.{extension}",
            self.user_id, self.id, self.hash
        );
        if extension == "mp4" {
            return base;
        }
        format!("{base}?size={}", cdn_size(size))
    }

    fn engine_key(&self) -> String {
        format!("video-background:asset:{}:{}", self.id, self.hash)
    }

    fn tile_key(&self) -> String {
        format!("video-background:asset-tile:{}:{}", self.id, self.hash)
    }
}

/// What `VideoBackgroundStore` (module 912630) does with the asset list.
#[derive(Debug)]
pub(in crate::ui) enum AssetsEvent {
    /// `VIDEO_FILTER_ASSETS_FETCH_SUCCESS`: the list replaces the store's, keyed by id.
    Fetched(Vec<Asset>),
    /// `VIDEO_FILTER_ASSET_UPLOAD_SUCCESS`: added, or replacing the one with its id.
    Uploaded(Asset),
    /// `VIDEO_FILTER_ASSET_DELETE_SUCCESS`.
    Deleted(u64),
}

pub(in crate::ui) fn reduce_assets(assets: &mut Vec<Asset>, event: AssetsEvent) {
    match event {
        AssetsEvent::Fetched(list) => {
            assets.clear();
            for asset in list {
                assets.retain(|known| known.id != asset.id);
                assets.push(asset);
            }
        }
        AssetsEvent::Uploaded(asset) => {
            assets.retain(|known| known.id != asset.id);
            assets.push(asset);
        }
        AssetsEvent::Deleted(id) => assets.retain(|asset| asset.id != id),
    }
}

/// `en`'s order of the custom backgrounds: the most recently used first; when either has never been used, the newer
/// id first.
fn sort_assets(assets: &mut [Asset]) {
    let time = |asset: &Asset| {
        asset
            .last_used
            .as_deref()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .map(|time| time.timestamp_millis())
    };
    assets.sort_by(|a, b| match (time(a), time(b)) {
        (Some(a_time), Some(b_time)) => b_time.cmp(&a_time),
        _ => b.id.cmp(&a.id),
    });
}

/// The background option (`selectedBackgroundOption`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum Choice {
    #[default]
    None,
    Blur,
    Preset(u32),
    Custom(Asset),
}

impl Choice {
    /// The stored form, in the official option's shape: null, `"blur"`, a number, or the asset object.
    pub(in crate::ui) fn to_json(&self) -> Value {
        match self {
            Self::None => Value::Null,
            Self::Blur => json!("blur"),
            Self::Preset(id) => json!(id),
            Self::Custom(asset) => json!({
                "type": asset.kind,
                "id": asset.id.to_string(),
                "user_id": asset.user_id.to_string(),
                "asset": asset.hash,
            }),
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
            Value::Object(_) => Asset::from_value(value)
                .filter(|asset| asset.kind == TYPE_BACKGROUND)
                .map_or(Self::None, Self::Custom),
            _ => Self::None,
        }
    }

    /// `Hk` of module 965162: the synced oneof as an option. `me` is the account (`user_id` of a custom asset).
    pub(in crate::ui) fn from_filter(filter: Option<&VideoBackgroundFilter>, me: u64) -> Self {
        match filter {
            None => Self::None,
            Some(VideoBackgroundFilter::Blur { use_blur }) => {
                if *use_blur {
                    Self::Blur
                } else {
                    Self::None
                }
            }
            Some(VideoBackgroundFilter::Preset(id)) => Self::Preset(*id),
            Some(VideoBackgroundFilter::Custom { id, asset_hash }) => Self::Custom(Asset {
                id: *id,
                user_id: me,
                hash: asset_hash.clone(),
                kind: TYPE_BACKGROUND,
                last_used: None,
            }),
        }
    }

    /// `R4` of module 965162: the option as the synced oneof.
    pub(in crate::ui) fn to_filter(&self) -> Option<VideoBackgroundFilter> {
        match self {
            Self::None => None,
            Self::Blur => Some(VideoBackgroundFilter::Blur { use_blur: true }),
            Self::Preset(id) => Some(VideoBackgroundFilter::Preset(*id)),
            Self::Custom(asset) => Some(VideoBackgroundFilter::Custom {
                id: asset.id,
                asset_hash: asset.hash.clone(),
            }),
        }
    }

    /// `i` of module 329551: a custom asset needs the perk, a number must be a preset; anything else is None.
    pub(in crate::ui) fn checked(self, can_use_custom: bool) -> Self {
        match self {
            Self::Custom(_) if !can_use_custom => Self::None,
            Self::Preset(id) if preset(id).is_none() => Self::None,
            other => other,
        }
    }
}

/// Why a picked file was refused, as the official messages say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Refusal {
    /// `mrlScX`: over 10 MB.
    TooLarge,
    /// `Mt8yDB`: the upload failed.
    Unreadable,
}

/// Checks a picked file before it is uploaded (`maxFileSizeBytes` and the filter of `et`).
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

/// `readAsDataURL`: the file as `data:<type>;base64,...`, its type as the browser names it from the extension.
pub(in crate::ui) fn data_url(path: &Path, bytes: &[u8]) -> String {
    use base64::Engine;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mime = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// `F9` of module 352430: the upload's request.
pub(in crate::ui) fn upload_request(asset: String, in_modal: bool) -> Request {
    Request::Rest {
        key: Some(format!("{UPLOAD_KEY}{}", u8::from(in_modal))),
        method: reqwest::Method::POST,
        route: ASSETS_ROUTE.to_owned(),
        body: Some(json!({"type": TYPE_BACKGROUND, "asset": asset})),
    }
}

/// `pW` of module 352430: the delete's request.
pub(in crate::ui) fn delete_request(id: u64) -> Request {
    Request::Rest {
        key: Some(format!("{DELETE_KEY}{id}")),
        method: reqwest::Method::DELETE,
        route: format!("{ASSETS_ROUTE}/{id}"),
        body: None,
    }
}

/// `_C` of module 352430 for a custom asset: `VIDEO_FILTER_ASSET_LAST_USED`.
pub(in crate::ui) fn last_used_request(id: u64) -> Request {
    Request::Rest {
        key: Some(LAST_USED_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("{ASSETS_ROUTE}/{id}/last-used"),
        body: None,
    }
}

/// `HR` of module 352430: the list's request.
pub(in crate::ui) fn list_request() -> Request {
    Request::Rest {
        key: Some(ASSETS_KEY.to_owned()),
        method: reqwest::Method::GET,
        route: ASSETS_ROUTE.to_owned(),
        body: None,
    }
}

/// The class `u` of module 352430: what a failed upload says. `BINARY_TYPE_MAX_SIZE` on the `asset` field is the
/// size message, anything else (the 25-item limit too) the general one.
pub(in crate::ui) fn upload_refusal(answer: &Value) -> Refusal {
    let code = &answer["body"]["errors"]["asset"]["_errors"][0]["code"];
    if code == "BINARY_TYPE_MAX_SIZE" {
        Refusal::TooLarge
    } else {
        Refusal::Unreadable
    }
}

/// A still picture decoded to RGBA, or None for what cannot be decoded as one.
pub(in crate::ui) fn decode_still(bytes: &[u8]) -> Option<Picture> {
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    Picture::rgba(width, height, decoded.into_raw())
}

/// The canvas of module 854325 `m`: the picture scaled to the canvas's height, centred across its width (cut at the
/// sides when wider, transparent beside it when narrower), as `drawImage(img, (w - h / r) / 2, 0, h / r, h)` draws it.
pub(in crate::ui) fn official_canvas(picture: &Picture) -> Option<Picture> {
    let source = image::RgbaImage::from_raw(picture.width, picture.height, picture.to_rgba()?)?;
    if picture.width == 0 || picture.height == 0 {
        return None;
    }
    let (cw, ch) = CANVAS;
    let width = ((f64::from(ch) * f64::from(picture.width) / f64::from(picture.height)).round()
        as u32)
        .max(1);
    let scaled = image::imageops::resize(&source, width, ch, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::new(cw, ch);
    let left = (i64::from(cw) - i64::from(width)) / 2;
    image::imageops::replace(&mut canvas, &scaled, left, 0);
    Picture::rgba(cw, ch, canvas.into_raw())
}

/// The kind of file behind an option, for the engine (`isVideo`, `VI`, `q6` in module 854325).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Media {
    Still,
    Gif,
    Mp4,
}

/// A decoded background for the engine, or None when the file is not one.
fn engine_background(bytes: Vec<u8>, media: Media) -> Option<Background> {
    match media {
        Media::Still => {
            let picture = official_canvas(&decode_still(&bytes)?)?;
            Some(Background::Image(Arc::new(picture)))
        }
        Media::Gif => {
            let bytes: Arc<[u8]> = Arc::from(bytes);
            // Read here first so a file that is no GIF ends in the error line, not in a silent black room.
            clip::Gif::read(bytes.clone()).ok()?;
            Some(Background::Animated(Animation::start(move || {
                let decoder: Box<dyn FrameDecoder> = Box::new(clip::Gif::read(bytes).ok()?);
                Some(decoder)
            })))
        }
        Media::Mp4 => {
            crate::spinner::H264Track::read(&bytes).ok()?;
            Some(Background::Animated(Animation::start(move || {
                let decoder: Box<dyn FrameDecoder> = Box::new(clip::Mp4::read(&bytes).ok()?);
                Some(decoder)
            })))
        }
    }
}

/// The `video-background-context` menu open on a tile (module 5752).
#[derive(Clone)]
struct Menu {
    at: Point<Pixels>,
    option: Choice,
    in_use: bool,
}

/// The Video Background state kept with the camera page.
#[derive(Default)]
pub(in crate::ui) struct BackgroundPage {
    /// The settings row's choice (`sB`'s state): applied at once, saved when the row goes.
    pub choice: Choice,
    /// The preview modal's own choice while it is open (module 308229 `P`).
    pub pending: Option<Choice>,
    /// `errorMessage` of the grid.
    pub error: Option<&'static str>,
    restored: bool,
    /// The settings row changed the choice since it was shown (`t.current`).
    dirty: bool,
    /// `_C`'s settings write is waiting out [`SAVE_DELAY`].
    save_scheduled: bool,
    /// `VideoBackgroundStore.videoFilterAssets`.
    assets: Vec<Asset>,
    /// The grid asked for the list since it was shown.
    assets_requested: Cell<bool>,
    seen: Vec<String>,
    /// `seen` was read from storage.
    seen_loaded: bool,
    requested: Cell<bool>,
    blur_art: RefCell<Option<Arc<RenderImage>>>,
    /// The first frames of the animated tiles, by image key, and the ones being decoded.
    first_frames: RefCell<HashMap<String, Arc<RenderImage>>>,
    decoding: RefCell<HashSet<String>>,
    menu: Option<Menu>,
    /// Modal 647098 is open.
    pub upsell: bool,
    /// Bumped on every choice so a slow decode of an older one is dropped.
    serial: u64,
    /// Settings writes sent, for tests.
    writes_sent: u64,
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
fn model_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("GRAVECORD_SEGMENTATION_MODELS") {
        dirs.push(std::path::PathBuf::from(dir));
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
    fn me_id(&self) -> u64 {
        self.stores.me().map_or(0, |me| me.id)
    }

    /// `i(currentUser)` of module 329551: the account's choice, checked. Before the account's settings arrive, the
    /// copy kept on this machine.
    fn saved_choice(&self) -> Choice {
        let can = self.can_use_custom_backgrounds();
        if self.stores.settings_loaded() {
            return Choice::from_filter(self.stores.video_background().as_ref(), self.me_id())
                .checked(can);
        }
        crate::storage::get(STORED)
            .map(|value| Choice::from_json(&value))
            .unwrap_or_default()
            .checked(can)
    }

    /// Reads what is stored once and hands the saved choice to the camera engine. Called before a camera starts and
    /// when the preview modal opens.
    pub(in crate::ui) fn video_background_restore(&mut self, cx: &mut Context<Self>) {
        ensure_model_dirs();
        if !self.voice.camera.background.restored {
            let choice = self.saved_choice();
            let page = &mut self.voice.camera.background;
            page.restored = true;
            page.choice = choice;
            if !page.seen_loaded {
                page.seen = stored_list(STORED_SEEN);
                page.seen_loaded = true;
            }
        }
        let choice = if self.voice.camera.background.dirty {
            self.voice.camera.background.choice.clone()
        } else {
            self.saved_choice()
        };
        self.video_background_engage(choice, cx);
    }

    /// The Voice & Video page came into view: the row mounts with the saved choice (`useState(i(user))`) and the
    /// grid asks for the custom backgrounds again (`HR` on mount).
    pub(in crate::ui) fn video_background_page_shown(&mut self) {
        let choice = self.saved_choice();
        let page = &mut self.voice.camera.background;
        page.choice = choice;
        page.dirty = false;
        page.assets_requested.set(false);
        page.menu = None;
    }

    /// The preview modal opened: its own choice starts from the saved one and its grid asks for the list again.
    pub(in crate::ui) fn video_background_modal_opened(&mut self) {
        let page = &mut self.voice.camera.background;
        page.pending = None;
        page.error = None;
        page.menu = None;
        page.assets_requested.set(false);
    }

    /// The Voice & Video page went: the row unmounts and saves what it picked (`_C(s.current)` when `t.current`).
    pub(in crate::ui) fn video_background_page_left(&mut self, cx: &mut Context<Self>) {
        let page = &mut self.voice.camera.background;
        page.menu = None;
        page.assets_requested.set(false);
        if !std::mem::take(&mut page.dirty) {
            return;
        }
        let choice = page.choice.clone();
        self.video_background_save(choice, cx);
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
        dc_state::premium::PremiumType::from_wire(self.premium_wire())
            .has(dc_state::premium::Feature::VideoFilterAssets)
    }

    /// The choice the grid shows: the modal's own while it is open, else the settings row's.
    fn video_background_shown_choice(&self, in_modal: bool) -> Choice {
        let page = &self.voice.camera.background;
        match (&page.pending, in_modal) {
            (Some(pending), true) => pending.clone(),
            (None, true) => self.saved_choice(),
            _ if page.restored => page.choice.clone(),
            _ => self.saved_choice(),
        }
    }

    /// A tile was picked. On the settings page it is applied at once (`gB`) and saved when the row goes; in the
    /// modal it only previews until Turn On Camera or Change Background (`S1`, module 854325).
    pub(in crate::ui) fn video_background_pick(
        &mut self,
        choice: Choice,
        in_modal: bool,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.voice.camera.background;
        page.error = None;
        page.menu = None;
        if in_modal {
            page.pending = Some(choice.clone());
        } else {
            page.restored = true;
            page.choice = choice.clone();
            page.dirty = true;
        }
        self.video_background_engage(choice, cx);
        cx.notify();
    }

    /// The modal's button (`B`): its choice is applied and saved.
    pub(in crate::ui) fn video_background_commit_pending(&mut self, cx: &mut Context<Self>) {
        let choice = self
            .voice
            .camera
            .background
            .pending
            .take()
            .unwrap_or_else(|| self.saved_choice());
        let page = &mut self.voice.camera.background;
        page.choice = choice.clone();
        page.dirty = false;
        self.video_background_engage(choice.clone(), cx);
        self.video_background_save(choice, cx);
    }

    /// The modal closed without its button: the saved choice is back on the camera.
    pub(in crate::ui) fn video_background_drop_pending(&mut self, cx: &mut Context<Self>) {
        let page = &mut self.voice.camera.background;
        page.error = None;
        page.menu = None;
        page.assets_requested.set(false);
        if page.pending.take().is_some() {
            let choice = if page.dirty {
                page.choice.clone()
            } else {
                self.saved_choice()
            };
            self.video_background_engage(choice, cx);
        }
    }

    /// `_C` of module 352430: the account's setting is changed at once and written after [`SAVE_DELAY`] (a write
    /// already waiting is not pushed back); a custom asset's `last-used` is posted.
    pub(in crate::ui) fn video_background_save(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let filter = choice.to_filter();
        let _ = self.stores.set_video_background(filter.as_ref());
        if !crate::storage::settings_writes_blocked() {
            crate::storage::set(STORED, choice.to_json());
        }
        if let Choice::Custom(asset) = &choice {
            let _ = self.requests.send(last_used_request(asset.id));
        }
        let page = &mut self.voice.camera.background;
        if page.save_scheduled {
            return;
        }
        page.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, _| this.video_background_write());
        })
        .detach();
    }

    /// The delayed write: the whole `voice_and_video` message as it is now.
    pub(in crate::ui) fn video_background_write(&mut self) {
        self.voice.camera.background.save_scheduled = false;
        if crate::storage::settings_writes_blocked() {
            crate::inert::note("held video background settings write");
            return;
        }
        let current = self.stores.video_background();
        let patch = self.stores.set_video_background(current.as_ref());
        if patch.is_empty() {
            return;
        }
        self.voice.camera.background.writes_sent += 1;
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".to_owned(),
            body: Some(json!({"settings": patch})),
        });
    }

    /// Hands `choice` to the camera engine; a file is fetched through the picture pipeline and decoded off the UI
    /// thread (module 854325 `m`).
    fn video_background_engage(&mut self, choice: Choice, cx: &mut Context<Self>) {
        let page = &mut self.voice.camera.background;
        page.serial += 1;
        let serial = page.serial;
        let (key, url, media) = match choice {
            Choice::None => {
                engine::set_background(Background::None);
                return;
            }
            Choice::Blur => {
                engine::set_background(Background::Blur);
                return;
            }
            Choice::Preset(id) => {
                let Some(preset) = preset(id) else {
                    engine::set_background(Background::None);
                    return;
                };
                let media = if preset.video {
                    Media::Mp4
                } else {
                    Media::Still
                };
                (preset_key(id), format!("{CDN}{}", preset.file), media)
            }
            Choice::Custom(asset) => {
                let media = if asset.is_video() {
                    Media::Mp4
                } else if asset.is_gif() {
                    Media::Gif
                } else {
                    Media::Still
                };
                (asset.engine_key(), asset.url(CANVAS.0, true), media)
            }
        };
        self.request_image(key.clone(), url);
        cx.spawn(async move |this, cx| {
            // The file arrives through the picture requests; wait for it a while.
            for _ in 0..300 {
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
                            .spawn(async move { engine_background(bytes, media) })
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            this.video_background_decoded(serial, decoded, cx)
                        });
                        return;
                    }
                    Some(None) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                    }
                }
            }
        })
        .detach();
    }

    fn video_background_decoded(
        &mut self,
        serial: u64,
        decoded: Option<Background>,
        cx: &mut Context<Self>,
    ) {
        let page = &mut self.voice.camera.background;
        if page.serial != serial {
            return;
        }
        match decoded {
            Some(background) => engine::set_background(background),
            None => {
                engine::set_background(Background::None);
                page.error = Some(apply_failed());
            }
        }
        cx.notify();
    }

    /// The Custom tile with the perk: pick a file, check it and upload it (`onAddBackgroundImage`), then select what
    /// the server answers.
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
            if let Err(refusal) = check_file(&path, size) {
                let _ = this.update(cx, |this, cx| {
                    this.voice.camera.background.error = Some(error_text(refusal));
                    cx.notify();
                });
                return;
            }
            let read = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(&path).ok()?;
                    // `JF` of module 946274: a picture that does not load is refused before it is sent.
                    let still = !matches!(
                        path.extension().and_then(|ext| ext.to_str()),
                        Some(ext) if ext.eq_ignore_ascii_case("mp4") || ext.eq_ignore_ascii_case("gif")
                    );
                    if still && decode_still(&bytes).is_none() {
                        return None;
                    }
                    Some(data_url(&path, &bytes))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match read {
                    Some(asset) => {
                        let _ = this.requests.send(upload_request(asset, in_modal));
                    }
                    None => {
                        this.voice.camera.background.error = Some(error_text(Refusal::Unreadable));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// An answer to one of this module's requests. True when the key was one.
    pub(in crate::ui) fn video_background_fetched(
        &mut self,
        key: &str,
        answer: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(KEY_PREFIX) {
            return false;
        }
        let ok = answer["ok"].as_bool().unwrap_or(false);
        if key == ASSETS_KEY {
            // `VIDEO_FILTER_ASSETS_FETCH_SUCCESS`.
            if ok && let Some(items) = answer["body"].as_array() {
                let list = items.iter().filter_map(Asset::from_value).collect();
                reduce_assets(
                    &mut self.voice.camera.background.assets,
                    AssetsEvent::Fetched(list),
                );
            }
        } else if let Some(in_modal) = key.strip_prefix(UPLOAD_KEY) {
            let asset = ok.then(|| Asset::from_value(&answer["body"])).flatten();
            match asset {
                Some(asset) => {
                    // `VIDEO_FILTER_ASSET_UPLOAD_SUCCESS`, then `D(s)`: the new background is picked.
                    reduce_assets(
                        &mut self.voice.camera.background.assets,
                        AssetsEvent::Uploaded(asset.clone()),
                    );
                    self.video_background_pick(Choice::Custom(asset), in_modal == "1", cx);
                }
                None => {
                    self.voice.camera.background.error = Some(error_text(upload_refusal(answer)));
                }
            }
        } else if let Some(id) = key
            .strip_prefix(DELETE_KEY)
            .and_then(|id| id.parse::<u64>().ok())
            && ok
        {
            // `pW`: a deleted background that is the account's choice clears it, then
            // `VIDEO_FILTER_ASSET_DELETE_SUCCESS`.
            if matches!(self.saved_choice(), Choice::Custom(asset) if asset.id == id) {
                self.video_background_save(Choice::None, cx);
            }
            reduce_assets(
                &mut self.voice.camera.background.assets,
                AssetsEvent::Deleted(id),
            );
        }
        cx.notify();
        true
    }

    fn video_background_seen(&mut self, hotspot: &'static str) {
        let page = &mut self.voice.camera.background;
        if !page.seen_loaded {
            page.seen = stored_list(STORED_SEEN);
            page.seen_loaded = true;
        }
        if page.seen.iter().any(|seen| seen == hotspot) {
            return;
        }
        page.seen.push(hotspot.to_owned());
        let list: Vec<Value> = page.seen.iter().map(|seen| json!(seen)).collect();
        if !crate::storage::settings_writes_blocked() {
            crate::storage::set(STORED_SEEN, Value::Array(list));
        }
    }

    fn hotspot_new(&self, hotspot: Option<&'static str>) -> bool {
        let page = &self.voice.camera.background;
        let stored;
        let seen = if page.seen_loaded {
            &page.seen
        } else {
            stored = stored_list(STORED_SEEN);
            &stored
        };
        hotspot.is_some_and(|hotspot| !seen.iter().any(|seen| seen == hotspot))
    }

    /// The custom backgrounds the grid lists (`y` of `el`, sorted by `en`), only with the perk.
    fn custom_list(&self) -> Vec<Asset> {
        if !self.can_use_custom_backgrounds() {
            return Vec::new();
        }
        let mut assets: Vec<Asset> = self
            .voice
            .camera
            .background
            .assets
            .iter()
            .filter(|asset| asset.kind == TYPE_BACKGROUND)
            .cloned()
            .collect();
        sort_assets(&mut assets);
        assets
    }

    /// What the grid needs from the network, asked for once: the tiles' pictures and the list (`HR`, once per mount).
    fn video_background_request_art(&self) {
        let page = &self.voice.camera.background;
        let send = |key: String, url: String| {
            let _ = self.requests.send(Request::Image { key, url });
        };
        if !page.requested.replace(true) {
            send(BLUR_ART_KEY.to_owned(), BLUR_ART.to_owned());
            for preset in &PRESETS {
                send(preset_key(preset.id), format!("{CDN}{}", preset.file));
            }
        }
        if !page.assets_requested.replace(true) {
            let _ = self.requests.send(list_request());
        }
        let wanted: Vec<(String, String)> = self
            .custom_list()
            .iter()
            .map(|asset| {
                if asset.is_video() {
                    (asset.engine_key(), asset.url(TILE_SIZE, true))
                } else {
                    (asset.tile_key(), asset.url(TILE_SIZE, false))
                }
            })
            .collect();
        let requested = &page.decoding;
        for (key, url) in wanted {
            if !self.images.contains_key(&key)
                && requested.borrow_mut().insert(format!("req:{key}"))
            {
                send(key, url);
            }
        }
    }

    /// The first frame of an animated tile's file at the tile's size, decoded once off the UI thread.
    fn first_frame(
        &self,
        key: &str,
        gif: bool,
        size: (f32, f32),
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let page = &self.voice.camera.background;
        if let Some(frame) = page.first_frames.borrow().get(key) {
            return Some(frame.clone());
        }
        let bytes = self.images.get(key)?.bytes.clone();
        if !page.decoding.borrow_mut().insert(key.to_owned()) {
            return None;
        }
        let key = key.to_owned();
        let (width, height) = (
            size.0.round().max(1.) as usize,
            size.1.round().max(1.) as usize,
        );
        cx.spawn(async move |this, cx| {
            let frame = cx
                .background_executor()
                .spawn(async move {
                    let picture = clip::first_frame(&bytes, gif)?;
                    let fitted = dc_media::background::composite::cover(
                        &picture.to_rgba()?,
                        picture.width as usize,
                        picture.height as usize,
                        width,
                        height,
                    );
                    Picture::rgba(width as u32, height as u32, fitted)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(image) = frame.as_ref().and_then(render_image) {
                    this.voice
                        .camera
                        .background
                        .first_frames
                        .borrow_mut()
                        .insert(key, image);
                    cx.notify();
                }
            });
        })
        .detach();
        None
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
            .child(video_background_label())
            .child(self.video_background_grid(width, false, cx))
            .into_any_element()
    }

    /// `el` of module 717359 in the preview modal: the FormItem's label over the grid, 24px under the camera select
    /// (`spacingTop24_V8Qcsn`, module 357063).
    pub(in crate::ui) fn video_background_modal_section(
        &self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .mt(px(24.))
            .flex()
            .flex_col()
            .child(video_background_label())
            .child(self.video_background_grid(width, true, cx))
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
        for asset in self.custom_list() {
            let choice = Choice::Custom(asset.clone());
            let picked = selected == choice;
            let picture = if asset.is_video() {
                self.first_frame(&asset.engine_key(), false, size, cx)
                    .map(|frame| img(frame).size_full().into_any_element())
            } else {
                self.images.get(&asset.tile_key()).cloned().map(|image| {
                    img(image)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                })
            };
            let id = SharedString::from(format!("video-background-custom-{}", asset.id));
            let tile = self.picture_tile(
                TileSpec {
                    id: id.clone(),
                    selected: picked,
                    disabled: false,
                    size,
                },
                PictureTile {
                    picture,
                    animated: asset.is_video() || asset.is_gif(),
                    new: false,
                    option: choice.clone(),
                    in_modal,
                    hotspot: None,
                },
                cx,
            );
            // `$`: the Nitro wheel and "Custom Background" (the wheel is not drawn by the shared tooltip).
            tiles.push(
                self.with_tip(
                    id,
                    Tip::text(dc_intl::tr("Custom Background"), Side::Top),
                    tile,
                    cx,
                )
                .into_any_element(),
            );
        }
        for preset in &PRESETS {
            let key = preset_key(preset.id);
            let picture = if preset.video {
                self.first_frame(&key, false, size, cx)
                    .map(|frame| img(frame).size_full().into_any_element())
            } else {
                self.images.get(&key).cloned().map(|image| {
                    img(image)
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                })
            };
            let id = SharedString::from(format!("video-background-preset-{}", preset.id));
            let tile = self.picture_tile(
                TileSpec {
                    id: id.clone(),
                    selected: selected == Choice::Preset(preset.id),
                    disabled: false,
                    size,
                },
                PictureTile {
                    picture,
                    animated: preset.video,
                    new: self.hotspot_new(preset.hotspot),
                    option: Choice::Preset(preset.id),
                    in_modal,
                    hotspot: preset.hotspot,
                },
                cx,
            );
            tiles.push(
                self.with_tip(id, Tip::text(dc_intl::t(preset.name), Side::Top), tile, cx)
                    .into_any_element(),
            );
        }
        let per_row = columns as usize;
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            let row: Vec<AnyElement> = tiles.by_ref().take(per_row).collect();
            rows.push(div().flex().gap(px(GAP)).children(row).into_any_element());
        }
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .children(rows),
            )
            .children(page.error.map(error_line))
            .children(self.video_background_menu(cx))
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
        tile_frame(spec, inner.into_any_element(), Vec::new())
            .on_click(on_click)
            .into_any_element()
    }

    /// The Custom tile: with the perk it adds a file (`et`), without it opens the upsell (`ee`). Its tooltip (`X`)
    /// announces the animated backgrounds until the pointer leaves it once.
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
        let new = self.hotspot_new(Some(CUSTOM_HOTSPOT));
        let badge = new.then(new_badge).into_iter().collect();
        let tip: SharedString = if new {
            dc_intl::t("5TUJOv").into()
        } else if !can {
            dc_intl::t("IqE0T/").into()
        } else if full {
            dc_intl::t!("ykGFeD", maxCustomBackgrounds = MAX_CUSTOM).into()
        } else {
            dc_intl::t("Qx0tFc").into()
        };
        let tile = tile_frame(
            TileSpec {
                id: "video-background-custom".into(),
                selected: false,
                disabled: full,
                size,
            },
            inner.into_any_element(),
            badge,
        )
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.video_background_seen(CUSTOM_HOTSPOT);
                cx.notify();
            }
        }))
        .when(!full, |this| {
            this.on_click(cx.listener(move |this, _, _, cx| {
                if this.can_use_custom_backgrounds() {
                    this.video_background_add_custom(in_modal, cx);
                } else {
                    // `onUpsellClick`: module 647098.
                    this.video_background_open_upsell(cx);
                }
                cx.notify();
            }))
        });
        self.with_tip(
            "video-background-custom",
            Tip::body(Body::Text(tip), Side::Top),
            tile,
            cx,
        )
        .into_any_element()
    }

    /// `Z`: a picture (or, for an animated file, its first frame) with the play mark and the NEW badge. A click picks
    /// it; the pointer entering it clears its hotspot (`v`); a right click opens `video-background-context`.
    fn picture_tile(
        &self,
        spec: TileSpec,
        tile: PictureTile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let in_use = spec.selected;
        let inner = div().size_full().children(tile.picture).into_any_element();
        let mut overlay = Vec::new();
        if tile.animated {
            // `playIcon_imJCJi`: 16px, 4px from the top and the end, black at 0.4 behind the play glyph.
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
        if tile.new {
            overlay.push(new_badge());
        }
        let (pick, menu) = (tile.option.clone(), tile.option);
        let (in_modal, hotspot) = (tile.in_modal, tile.hotspot);
        tile_frame(spec, inner, overlay)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.video_background_pick(pick.clone(), in_modal, cx)
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && let Some(hotspot) = hotspot {
                    this.video_background_seen(hotspot);
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    this.voice.camera.background.menu = Some(Menu {
                        at: event.position,
                        option: menu.clone(),
                        in_use,
                    });
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// `video-background-context` (module 5752): one danger row, "remove", only for a custom background not in use.
    fn video_background_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.voice.camera.background.menu.clone()?;
        let (label, disabled) = menu_row(&menu.option, menu.in_use);
        let target = match &menu.option {
            Choice::Custom(asset) => Some(asset.id),
            _ => None,
        };
        let row = div()
            .id("video-background-context-remove")
            .min_h(px(34.))
            .flex_none()
            .p(px(8.))
            .rounded(px(2.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-feedback-critical"))
            .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |this| {
                this.cursor_pointer().hover(|style| {
                    style
                        .bg(color("--background-feedback-mod-critical"))
                        .rounded(px(4.))
                })
            })
            .child(div().truncate().child(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.voice.camera.background.menu = None;
                if let (false, Some(id)) = (disabled, target) {
                    // `pW` and `Eo`.
                    let _ = this.requests.send(delete_request(id));
                }
                cx.notify();
            }));
        Some(
            deferred(
                anchored()
                    .position(menu.at)
                    .flip_horizontal_snap_with_margin(px(8.))
                    .child(
                        div()
                            .id("video-background-context")
                            .occlude()
                            .rounded(px(8.))
                            .bg(color("--background-surface-higher"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.24),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .max_w(px(320.))
                            .p(px(8.))
                            .flex()
                            .flex_col()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.voice.camera.background.menu = None;
                                cx.notify();
                            }))
                            .child(row),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Opens modal 647098.
    fn video_background_open_upsell(&mut self, cx: &mut Context<Self>) {
        self.request_image(UPSELL_ART_KEY.to_owned(), UPSELL_ART.to_owned());
        self.voice.camera.background.upsell = true;
        cx.notify();
    }

    /// Closes modal 647098. True when it was open.
    pub(in crate::ui) fn video_background_close_upsell(&mut self, cx: &mut Context<Self>) -> bool {
        if !std::mem::take(&mut self.voice.camera.background.upsell) {
            return false;
        }
        cx.notify();
        true
    }

    /// Module 647098 in the premium upsell modal of module 945240 (classes of module 777133): the art 225x154
    /// (`videoBackgroundArt_PxLVP4`) 48px above the box's top edge, "Wanna upload your own background?" in
    /// `heading-xl/semibold` 24px over the body text, then the footer's subscribe button and Back.
    pub(in crate::ui) fn video_background_upsell_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.voice.camera.background.upsell {
            return None;
        }
        let art = self
            .images
            .get(UPSELL_ART_KEY)
            .cloned()
            .map(|image| img(image).w(px(225.)).h(px(154.)).into_any_element());
        let runs = dc_intl::format_runs("hDMlqD", &[("onLearnMore", dc_intl::Arg::from(true))]);
        // One centred paragraph (`root_PwiJiV` is `text-align: center`) with the `$link` run in the link colour.
        let mut text = String::new();
        let mut link = None;
        for (run, tag) in runs {
            let start = text.len();
            text.push_str(&run);
            if tag.as_deref().is_some_and(|tag| tag.starts_with('$')) {
                link = Some(start..text.len());
            }
        }
        let highlights: Vec<_> = link
            .iter()
            .map(|range| {
                (
                    range.clone(),
                    gpui::HighlightStyle {
                        color: Some(color("--text-link").into()),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let entity = cx.entity().downgrade();
        let body = div()
            .w_full()
            .text_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(
                gpui::InteractiveText::new(
                    "video-background-upsell-body",
                    gpui::StyledText::new(text).with_highlights(highlights),
                )
                .on_click(link.into_iter().collect(), move |_, _, cx| {
                    // `onLearnMore`: the modal closes and the Nitro page opens.
                    let _ = entity.update(cx, |this, cx| {
                        this.voice.camera.background.upsell = false;
                        this.open_nitro(cx);
                    });
                }),
            );
        let cta = self.subscribe_cta(Some(dc_state::premium::sku::TIER_2));
        let disabled = cta.disabled;
        let subscribe = div()
            .id("video-background-upsell-subscribe")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            // `Colors.GREEN` (module 725807 for tier 2 without the gradient).
            .bg(color("--control-connected-background-default"))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-connected-text-default"))
            .when(!cta.disabled, |this| this.cursor_pointer())
            .when(cta.disabled, |this| this.opacity(0.5))
            .child(mask_icon(
                "nitro_wheel",
                16.,
                "--control-connected-text-default",
            ))
            .child(dc_intl::tr(cta.text))
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                this.voice.camera.background.upsell = false;
                this.open_purchase(false, cx);
            }));
        let back = div()
            .id("video-background-upsell-back")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--control-secondary-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(dc_intl::tr("Back"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.video_background_close_upsell(cx);
            }));
        // Over the settings layer too, which draws after the voice dialogs.
        Some(
            deferred(
                div()
                    .id("video-background-upsell-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.video_background_close_upsell(cx);
                    }))
                    .child(
                        div()
                            .id("video-background-upsell")
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .relative()
                            .w(px(440.))
                            .mt(px(48.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                // `artContainer_PwiJiV`: 225 wide, 8px corners, 48px above the box, 24px over the text.
                                div()
                                    .mt(px(-48.))
                                    .mb(px(24.))
                                    .w(px(225.))
                                    .h(px(154.))
                                    .rounded(px(8.))
                                    .overflow_hidden()
                                    .children(art),
                            )
                            .child(
                                div()
                                    .px(px(16.))
                                    .mb(px(32.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(
                                        div()
                                            .mb(px(24.))
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("fHqiDW")),
                                    )
                                    .child(body),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p(px(16.))
                                    .flex()
                                    .flex_row_reverse()
                                    .items_center()
                                    .justify_between()
                                    .child(subscribe)
                                    .child(back),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

/// The FormItem label (`body-md/medium`, `text-strong`, 8px under it).
fn video_background_label() -> AnyElement {
    div()
        .mb(px(8.))
        .text_size(px(16.))
        .line_height(px(20.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .child(dc_intl::tr("Video Background"))
        .into_any_element()
}

/// The FormItem's error line (`statusMessageContainer_VhFsFg`): the 12px circle, 4px, then `body-xs/normal`.
fn error_line(message: &'static str) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(4.))
        .child(div().h(px(16.)).flex().items_center().child(mask_icon(
            "shop_circle_error",
            12.,
            "--text-feedback-critical",
        )))
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-feedback-critical"))
                .child(message),
        )
        .into_any_element()
}

/// The remove row's label and whether it is disabled (module 5752).
fn menu_row(option: &Choice, in_use: bool) -> (&'static str, bool) {
    match option {
        Choice::Custom(_) if in_use => (dc_intl::t("xh/P/8"), true),
        Choice::Custom(_) => (dc_intl::t("oKuPwe"), false),
        _ => (dc_intl::t("hFYUDN"), true),
    }
}

/// What a `Z` tile shows and does besides its frame.
struct PictureTile {
    picture: Option<AnyElement>,
    animated: bool,
    new: bool,
    option: Choice,
    in_modal: bool,
    hotspot: Option<&'static str>,
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
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .when(!disabled, |this| this.cursor_pointer())
        .when(disabled, |this| this.cursor_not_allowed())
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
    use crate::net::Inert;

    fn asset(id: u64, hash: &str, last_used: Option<&str>) -> Asset {
        Asset {
            id,
            user_id: 42,
            hash: hash.to_owned(),
            kind: TYPE_BACKGROUND,
            last_used: last_used.map(str::to_owned),
        }
    }

    #[test]
    fn choices_round_trip_through_the_stored_form() {
        for choice in [
            Choice::None,
            Choice::Blur,
            Choice::Preset(0),
            Choice::Preset(10),
            Choice::Custom(asset(1_100_000_000_000_000_000, "v_abc", None)),
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
            json!({"path": "C:\\room.png"}),
            json!(true),
        ] {
            assert_eq!(Choice::from_json(&value), Choice::None, "{value}");
        }
    }

    #[test]
    fn the_synced_oneof_round_trips_and_preset_zero_is_cybercity() {
        let me = 42;
        for choice in [
            Choice::None,
            Choice::Blur,
            Choice::Preset(0),
            Choice::Preset(9),
            Choice::Custom(asset(77, "a_gif", None)),
        ] {
            let filter = choice.to_filter();
            // Through the bytes the account holds.
            let (_, settings) = dc_proto::settings::set_video_background(&[], filter.as_ref());
            let read = dc_proto::settings::video_background_in(&settings);
            assert_eq!(Choice::from_filter(read.as_ref(), me), choice);
        }
        assert_eq!(
            Choice::from_filter(Some(&VideoBackgroundFilter::Preset(0)), me).checked(false),
            Choice::Preset(0),
            "Cybercity is a choice, not nothing"
        );
        // `i`: unknown numbers and, without the perk, custom assets are nothing.
        assert_eq!(Choice::Preset(5).checked(true), Choice::None);
        assert_eq!(
            Choice::Custom(asset(1, "h", None)).checked(false),
            Choice::None
        );
        assert_eq!(
            Choice::from_filter(Some(&VideoBackgroundFilter::Blur { use_blur: false }), me),
            Choice::None
        );
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
        assert!(PRESETS.iter().all(|preset| dc_intl::has_key(preset.name)));
    }

    #[test]
    fn custom_files_are_checked_like_the_upload() {
        assert_eq!(check_file(Path::new("a.PNG"), 10), Ok(()));
        assert_eq!(check_file(Path::new("a.jpeg"), MAX_FILE_BYTES), Ok(()));
        assert_eq!(check_file(Path::new("a.mp4"), 10), Ok(()));
        assert_eq!(
            check_file(Path::new("a.png"), MAX_FILE_BYTES + 1),
            Err(Refusal::TooLarge)
        );
        assert_eq!(check_file(Path::new("a.bmp"), 10), Err(Refusal::Unreadable));
        assert_eq!(check_file(Path::new("noext"), 10), Err(Refusal::Unreadable));
    }

    #[test]
    fn the_upload_is_a_json_post_of_a_data_url() {
        let url = data_url(Path::new("room.PNG"), b"\x89PNG");
        assert_eq!(url, "data:image/png;base64,iVBORw==");
        assert!(data_url(Path::new("a.mp4"), b"").starts_with("data:video/mp4;base64,"));
        assert!(data_url(Path::new("a.jpg"), b"").starts_with("data:image/jpeg;base64,"));
        assert!(data_url(Path::new("a.gif"), b"").starts_with("data:image/gif;base64,"));
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = upload_request(url.clone(), true)
        else {
            panic!("a REST request");
        };
        assert_eq!(method, reqwest::Method::POST);
        assert_eq!(route, "/users/@me/video-filters/assets");
        // `last_used` is undefined in `F9` and so not in the JSON.
        assert_eq!(body, Some(json!({"type": 0, "asset": url})));
        assert_eq!(key.as_deref(), Some("video-background:upload:1"));
    }

    #[test]
    fn every_write_is_refused_in_an_inert_run_and_the_list_is_read() {
        let writes = [
            upload_request("data:image/png;base64,".to_owned(), false),
            delete_request(5),
            last_used_request(5),
            Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(json!({"settings": "KgA="})),
            },
        ];
        for request in &writes {
            assert_eq!(request.inert(), Inert::Deny);
        }
        assert_eq!(list_request().inert(), Inert::Allow);
        let Request::Rest { method, route, .. } = delete_request(5) else {
            panic!("a REST request");
        };
        assert_eq!(
            (method, route.as_str()),
            (reqwest::Method::DELETE, "/users/@me/video-filters/assets/5")
        );
        let Request::Rest { route, .. } = last_used_request(5) else {
            panic!("a REST request");
        };
        assert_eq!(route, "/users/@me/video-filters/assets/5/last-used");
    }

    #[test]
    fn upload_failures_say_what_the_official_class_says() {
        let too_large = json!({"ok": false, "status": 400, "body": {"code": 50035, "errors": {"asset": {"_errors": [
            {"code": "BINARY_TYPE_MAX_SIZE", "message": "File cannot be larger than 10240.0 kb."}
        ]}}}});
        assert_eq!(upload_refusal(&too_large), Refusal::TooLarge);
        // The server's limit of 25 and anything else are the general message.
        let limit = json!({"ok": false, "status": 400, "body": {"code": 30061, "message": "Maximum number of video filter assets reached"}});
        assert_eq!(upload_refusal(&limit), Refusal::Unreadable);
        let refused = json!({"ok": false, "status": 0, "body": null, "refused": true});
        assert_eq!(upload_refusal(&refused), Refusal::Unreadable);
        assert_eq!(MAX_CUSTOM, 25);
        assert_eq!(
            dc_intl::t!("ykGFeD", maxCustomBackgrounds = MAX_CUSTOM),
            "You\u{2019}ve hit the limit of 25 backgrounds! Remove a background before adding another."
        );
    }

    #[test]
    fn asset_urls_follow_the_cdn_route() {
        let still = asset(9, "abc", None);
        assert_eq!(
            still.url(TILE_SIZE, true),
            "https://cdn.discordapp.com/video-filter-assets/42/9/abc.png?size=1024"
        );
        assert_eq!(
            still.url(CANVAS.0, true),
            "https://cdn.discordapp.com/video-filter-assets/42/9/abc.png?size=1280"
        );
        let gif = asset(9, "a_abc", None);
        assert_eq!(
            gif.url(CANVAS.0, true),
            "https://cdn.discordapp.com/video-filter-assets/42/9/a_abc.gif?size=1280"
        );
        assert_eq!(
            gif.url(TILE_SIZE, false),
            "https://cdn.discordapp.com/video-filter-assets/42/9/a_abc.png?size=1024"
        );
        let video = asset(9, "v_abc", None);
        assert_eq!(
            video.url(TILE_SIZE, true),
            "https://cdn.discordapp.com/video-filter-assets/42/9/v_abc.mp4"
        );
        assert_eq!(cdn_size(1), 16);
        assert_eq!(cdn_size(5000), 4096);
    }

    #[test]
    fn assets_are_listed_newest_used_first() {
        let mut assets = vec![
            asset(1, "a", Some("2026-01-01T00:00:00.000000+00:00")),
            asset(2, "b", Some("2026-03-01T00:00:00.000000+00:00")),
            asset(3, "c", None),
        ];
        sort_assets(&mut assets);
        let ids: Vec<u64> = assets.iter().map(|asset| asset.id).collect();
        assert_eq!(
            ids[0], 3,
            "an unused one against a used one: the newer id first"
        );
        let mut used = vec![
            asset(1, "a", Some("2026-01-01T00:00:00+00:00")),
            asset(2, "b", Some("2026-03-01T00:00:00+00:00")),
        ];
        sort_assets(&mut used);
        assert_eq!(used[0].id, 2);
        let parsed = Asset::from_value(&json!({
            "id": "1234", "user_id": "42", "asset": "v_x", "type": 0, "last_used": null
        }))
        .unwrap();
        assert_eq!(
            (parsed.id, parsed.user_id, parsed.is_video()),
            (1234, 42, true)
        );
    }

    #[test]
    fn the_store_keeps_the_list_by_id() {
        let mut assets = Vec::new();
        reduce_assets(
            &mut assets,
            AssetsEvent::Fetched(vec![
                asset(1, "a", None),
                asset(2, "b", None),
                asset(1, "c", None),
            ]),
        );
        assert_eq!(assets.len(), 2);
        reduce_assets(&mut assets, AssetsEvent::Uploaded(asset(3, "v_d", None)));
        reduce_assets(
            &mut assets,
            AssetsEvent::Uploaded(asset(3, "v_d", Some("2026-01-01T00:00:00Z"))),
        );
        assert_eq!(assets.len(), 3);
        reduce_assets(&mut assets, AssetsEvent::Deleted(2));
        let ids: Vec<u64> = assets.iter().map(|asset| asset.id).collect();
        assert_eq!(ids, [1, 3]);
        reduce_assets(&mut assets, AssetsEvent::Fetched(Vec::new()));
        assert!(assets.is_empty());
    }

    #[test]
    fn the_remove_row_follows_the_menu_module() {
        let custom = Choice::Custom(asset(1, "h", None));
        assert_eq!(menu_row(&custom, false), ("Remove Background", false));
        assert_eq!(
            menu_row(&custom, true),
            ("Cannot remove currently selected background", true)
        );
        assert_eq!(
            menu_row(&Choice::Preset(0), false),
            ("Cannot remove default backgrounds", true)
        );
    }

    #[test]
    fn stills_go_on_the_official_canvas() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let picture = decode_still(&png).unwrap();
        assert_eq!((picture.width, picture.height), (3, 2));
        assert_eq!(&picture.data[..4], &[10, 20, 30, 255]);
        assert!(decode_still(b"GIF89a....").is_none());
        // A square picture is 720 high and centred: transparent at the sides, the picture in the middle.
        let square = Picture::rgba(2, 2, [200, 0, 0, 255].repeat(4)).unwrap();
        let canvas = official_canvas(&square).unwrap();
        assert_eq!((canvas.width, canvas.height), CANVAS);
        let at = |x: u32, y: u32| &canvas.data[((y * CANVAS.0 + x) * 4) as usize..][..4];
        assert_eq!(at(100, 360), &[0, 0, 0, 0]);
        assert_eq!(at(640, 360), &[200, 0, 0, 255]);
        assert_eq!(at(1200, 360), &[0, 0, 0, 0]);
        // A wide picture fills the height and is cut at the sides.
        let wide = Picture::rgba(4, 1, [0, 200, 0, 255].repeat(4)).unwrap();
        let canvas = official_canvas(&wide).unwrap();
        assert!(
            canvas
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 200, 0, 255])
        );
    }
}
