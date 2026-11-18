//! The image upload dialogs: the select image modal (module 685663) and the image crop modal "Edit Image" (module
//! 142630) it opens, with the recent avatars of `RecentAvatarsStore` (module 685663 `B`) and the Remove Avatar
//! confirmation (module 555251).
//!
//! - The profile avatar and banner (module 468450 `XD`), the server banner (module 989433 `EK`, `GUILD_BANNER`) and
//!   the group DM icon (module 396787, "Change Icon", no recent avatars, 8 MiB) open the select image modal; its
//!   Upload Image tile takes a file and opens the cropper over it.
//! - The server icon (989433, `GUILD_ICON`) and the Server Guide banner (989433, `HOME_HEADER`) open the cropper
//!   straight from the file.
//! - Apply hands the crop to the call site (`onCrop`); Cancel closes the cropper and leaves the select modal open.
//!
//! The geometry is in `image_crop/math.rs`, the pixels in `image_crop/output.rs`.

mod math;
mod output;

pub(crate) use math::UploadType;

use super::modal::{self, Tone};
use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, SharedString, Styled,
    Window, canvas, div, img, prelude::*, px,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// The answer of `GET /users/@me/avatars` (`RECENT_AVATARS`).
pub(super) const RECENT_KEY: &str = "recent-avatars";
/// The answer of `DELETE /users/@me/avatars/{id}` (`RECENT_AVATARS_DELETE`): `recent-avatar-delete:{id}`.
const DELETE_PREFIX: &str = "recent-avatar-delete:";
/// A recent avatar's thumbnail: `recent-avatar:{id}`.
const THUMB_PREFIX: &str = "recent-avatar:";
/// A recent avatar fetched at 1024 to be cropped (`onSelectRecentAvatar`): `recent-avatar-full:{id}`.
pub(super) const FULL_PREFIX: &str = "recent-avatar-full:";
/// `recentAvatarsLimit` (module 685663).
const RECENT_LIMIT: usize = 6;
/// `shouldFetch`: the list is fetched again after five minutes.
const REFETCH_MS: u128 = 300_000;
/// The size list of module 776231 (`I`), which `kr` rounds a CDN size up to.
const CDN_SIZES: [u32; 31] = [
    16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320, 480,
    512, 600, 640, 1024, 1280, 1536, 2048, 3072, 4096,
];
/// The four pictures behind Choose GIF (`em` of module 685663: modules 203736/775788, 640237/689006, 964805/533798,
/// 322821/891814): the GIF on hover, else the PNG.
const GIF_GRID: [(&str, &str); 4] = [
    ("153f9d50b423a199.gif", "1071e0c5ceaf6ef7.png"),
    ("e629538747745c9a.gif", "6d14801ca95f25f1.png"),
    ("963f22200d58a4d8.gif", "4637e85fff87537c.png"),
    ("918ac9ba948b5e6b.gif", "ca693e3ef16951f4.png"),
];
/// `.overlay_tvT5v-{box-shadow:0 0 0 9999px rgba(47,49,54,.6)}`.
const DIM: [u8; 4] = [47, 49, 54, 153];
/// The arrow-key step of the slider (`keyboardStep: .025`).
const ZOOM_STEP: f64 = 0.025;
/// A tile of the select modal (`option_U8Q-Sg`): `(430 - 8) / 2`.
const OPTION_SIDE: f32 = 211.;
/// `.slider_tvT5v-{width:128px}`.
const SLIDER_WIDTH: f32 = 128.;

/// Where a picture goes once it is cropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    ProfileAvatar,
    ProfileBanner,
    ServerIcon,
    GuildBanner,
    HomeHeader,
    GroupIcon,
}

impl Purpose {
    fn kind(self) -> UploadType {
        match self {
            Self::ProfileAvatar | Self::GroupIcon => UploadType::Avatar,
            Self::ProfileBanner => UploadType::Banner,
            Self::ServerIcon => UploadType::GuildIcon,
            Self::GuildBanner => UploadType::GuildBanner,
            Self::HomeHeader => UploadType::HomeHeader,
        }
    }

    /// `allowRecentAvatarsSelection && uploadType === AVATAR`: the group DM icon passes `false`.
    fn recent_avatars(self) -> bool {
        self == Self::ProfileAvatar
    }

    /// The select modal's title (`modalTitle`, `DToW4e` unless given).
    fn title(self) -> String {
        match self {
            // "Change\nIcon": the title's white space collapses the line break to a space.
            Self::GroupIcon => dc_intl::t("6yrpFR").replace('\n', " "),
            _ => dc_intl::t("DToW4e").to_owned(),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::ProfileAvatar => "avatar",
            Self::ProfileBanner => "banner",
            Self::ServerIcon => "server-icon",
            Self::GuildBanner => "guild-banner",
            Self::HomeHeader => "home-header",
            Self::GroupIcon => "group-icon",
        }
    }

    #[cfg(any(test, feature = "capture"))]
    pub(super) fn parse(text: &str) -> Option<Self> {
        [
            Self::ProfileAvatar,
            Self::ProfileBanner,
            Self::ServerIcon,
            Self::GuildBanner,
            Self::HomeHeader,
            Self::GroupIcon,
        ]
        .into_iter()
        .find(|purpose| purpose.key() == text)
    }
}

/// One of `RecentAvatarsStore.getAvatars()`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RecentAvatar {
    pub id: String,
    pub storage_hash: String,
    pub description: Option<String>,
}

/// `RecentAvatarsStore` (module 685663 `B`).
#[derive(Default)]
pub(super) struct Recent {
    pub avatars: Vec<RecentAvatar>,
    pub fetching: bool,
    pub error: Option<String>,
    fetched_at: Option<std::time::Instant>,
}

impl Recent {
    /// `shouldFetch`: five minutes since the last answer, and none in flight.
    fn should_fetch(&self) -> bool {
        !self.fetching
            && self
                .fetched_at
                .is_none_or(|at| at.elapsed().as_millis() > REFETCH_MS)
    }
}

/// The select image modal.
pub(super) struct Select {
    pub purpose: Purpose,
    gif_hovered: bool,
    /// `setFetchError`: the selected recent avatar could not be fetched.
    error: Option<String>,
    /// `x`: a recent avatar is being fetched.
    busy: bool,
    /// The recent avatar under the pointer (its Remove button shows).
    hovered: Option<String>,
}

/// The dim mask kept for a crop size and shape (twice the size in pixels, the radius in percent).
type MaskCache = Option<((u32, u32, u32), Arc<gpui::RenderImage>)>;

/// The image crop modal.
pub(super) struct Crop {
    pub purpose: Purpose,
    file_name: String,
    source: Option<Arc<output::Source>>,
    pub editor: Option<math::Editor>,
    /// `eE`: the picture did not load.
    pub failed: bool,
    /// `eA`: Apply is cropping.
    pub applying: bool,
    /// The pointer's distance from the offset when a drag began (`startingCoordinates`).
    dragging: Option<math::Point>,
    slider_drag: bool,
    slider_hovered: bool,
    slider_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    preview: Option<Arc<gpui::RenderImage>>,
    /// The dim around the crop area for its size and shape.
    mask: RefCell<MaskCache>,
    area: gpui::FocusHandle,
    slider: gpui::FocusHandle,
    focus_pending: Cell<bool>,
    /// The editing area's height the editor was laid out for.
    area_height: f64,
    /// What a screen reader is told (`announce`): the rotation.
    pub announced: Option<String>,
    /// The recent avatar this picture came from (`originalAsset`).
    original: Option<RecentAvatar>,
}

/// The Remove Avatar confirmation (module 555251).
pub(super) struct ConfirmRemove {
    avatar: RecentAvatar,
    busy: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub select: Option<Select>,
    pub crop: Option<Crop>,
    pub confirm: Option<ConfirmRemove>,
    pub recent: Recent,
    /// The site waiting for a picked file (the file input of the Upload Image tile, or a direct site).
    pub awaiting: Option<Purpose>,
}

impl State {
    pub(super) fn open(&self) -> bool {
        self.select.is_some() || self.crop.is_some() || self.confirm.is_some()
    }
}

/// `kr` of module 776231: the CDN size for a CSS size at this pixel ratio.
fn cdn_size(css: f32, scale: f32) -> u32 {
    let wanted = (css * scale).ceil() as u32;
    CDN_SIZES
        .into_iter()
        .find(|size| wanted <= *size)
        .unwrap_or(CDN_SIZES[CDN_SIZES.len() - 1])
}

/// `Xp` of module 101058 (`ARCHIVED_AVATAR`): `/avatars/{user}/archived/{avatar}/{hash}.{ext}?size=`. A still picture
/// is asked for as WebP (`c` with `allowWebp`); an animated one (`a_` hash) as a GIF so the cropper can crop its
/// frames (the bundle asks for an animated WebP).
fn archived_avatar_url(user: dc_state::Id, avatar: &RecentAvatar, size: u32) -> String {
    let animated = avatar.storage_hash.starts_with("a_");
    let extension = if animated { "gif" } else { "webp" };
    format!(
        "https://cdn.discordapp.com/avatars/{user}/archived/{}/{}.{extension}?size={size}",
        avatar.id, avatar.storage_hash
    )
}

/// The avatars of a `RECENT_AVATARS_FETCH_SUCCESS` (`storage_hash` renamed `storageHash`).
pub(super) fn parse_recent(body: &serde_json::Value) -> Vec<RecentAvatar> {
    body["avatars"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|avatar| {
                    Some(RecentAvatar {
                        id: avatar["id"].as_str()?.to_owned(),
                        storage_hash: avatar["storage_hash"].as_str()?.to_owned(),
                        description: avatar["description"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The dim outside the crop area's shape (`overlay_tvT5v-`'s spread shadow), `scale` pixels per CSS pixel: `radius`
/// is the corner radius as a fraction of the shorter side (`50%` avatar, `25%` server icon, none for a banner).
fn dim_mask(width: f64, height: f64, radius: f64, scale: f64) -> Arc<gpui::RenderImage> {
    let (pw, ph) = (
        (width * scale).ceil().max(1.) as u32,
        (height * scale).ceil().max(1.) as u32,
    );
    let r = radius * width.min(height) * scale;
    let (w, h) = (width * scale, height * scale);
    let mut pixels = image::RgbaImage::new(pw, ph);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        // The distance outside the rounded rectangle; coverage over one pixel.
        let qx = (cx - w / 2.).abs() - (w / 2. - r);
        let qy = (cy - h / 2.).abs() - (h / 2. - r);
        let outside = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt() + qx.max(qy).min(0.) - r;
        let alpha = (outside + 0.5).clamp(0., 1.);
        let a = (f64::from(DIM[3]) * alpha).round() as u8;
        // BGRA for the atlas.
        *pixel = image::Rgba([DIM[2], DIM[1], DIM[0], a]);
    }
    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(pixels)]))
}

/// The editing area's height: `.editingContainer_tvT5v-{height:350px}`, 200px in a window at most 800px high.
fn area_height(window: &Window) -> f64 {
    if f32::from(window.viewport_size().height) <= 800. {
        200.
    } else {
        350.
    }
}

impl Root {
    /// Opens the select image modal for a site (module 685663), and fetches the recent avatars when it lists them.
    pub(super) fn open_image_select(&mut self, purpose: Purpose, cx: &mut Context<Self>) {
        self.image_crop.select = Some(Select {
            purpose,
            gif_hovered: false,
            error: None,
            busy: false,
            hovered: None,
        });
        if purpose.recent_avatars() && self.image_crop.recent.should_fetch() {
            self.image_crop.recent.fetching = true;
            self.image_crop.recent.error = None;
            let _ = self.requests.send(Request::Rest {
                key: Some(RECENT_KEY.to_owned()),
                method: reqwest::Method::GET,
                route: "/users/@me/avatars".to_owned(),
                body: None,
            });
        }
        tracing::info!("image-crop: select {}", purpose.key());
        cx.notify();
    }

    /// The file input of a site: the system file dialog, then the cropper. A capture run never opens the dialog; its
    /// `image-pick <path>` step hands the file in.
    pub(super) fn pick_crop_file(&mut self, purpose: Purpose, cx: &mut Context<Self>) {
        self.image_crop.awaiting = Some(purpose);
        tracing::info!("image-crop: waiting for a file for {}", purpose.key());
        if crate::storage::is_capture_run() {
            return;
        }
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
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
            let _ = this.update(cx, |this, cx| this.crop_file_picked(&path, cx));
        })
        .detach();
    }

    /// A file was picked for the waiting site: a picture type the API takes opens the cropper (`onChange` of the
    /// file input, then `handleOpenImageEditingModal`).
    pub(crate) fn crop_file_picked(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(purpose) = self.image_crop.awaiting.take() else {
            return;
        };
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        if let Some(refused) = self.crop_file_refused(purpose, bytes.len(), cx) {
            tracing::info!("image-crop: {} refused: {refused}", purpose.key());
            return;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        self.open_crop(purpose, bytes, name, None, cx);
    }

    /// The file inputs' `maxFileSizeBytes`: the profile editor says so (`/tGlcj`), the others drop the file as they
    /// did before the cropper.
    fn crop_file_refused(
        &mut self,
        purpose: Purpose,
        size: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        match purpose {
            Purpose::ProfileAvatar | Purpose::ProfileBanner => {
                self.profile_editor_too_big(size, cx)
            }
            // `maxFileSizeBytes: 8388608` (module 396787).
            Purpose::GroupIcon => (size > 8_388_608).then(|| "The file is too large".to_owned()),
            _ => None,
        }
    }

    /// Opens the cropper over a picture; it decodes off the main thread and appears once loaded (`onLoad`).
    fn open_crop(
        &mut self,
        purpose: Purpose,
        bytes: Vec<u8>,
        file_name: String,
        original: Option<RecentAvatar>,
        cx: &mut Context<Self>,
    ) {
        self.image_crop.crop = Some(Crop {
            purpose,
            file_name,
            source: None,
            editor: None,
            failed: false,
            applying: false,
            dragging: None,
            slider_drag: false,
            slider_hovered: false,
            slider_bounds: Rc::new(Cell::new(None)),
            preview: None,
            mask: RefCell::new(None),
            area: cx.focus_handle(),
            slider: cx.focus_handle(),
            focus_pending: Cell::new(true),
            area_height: 0.,
            announced: None,
            original,
        });
        tracing::info!("image-crop: crop {} ({} bytes)", purpose.key(), bytes.len());
        let task = cx
            .background_executor()
            .spawn(async move { output::decode(bytes) });
        cx.spawn(async move |this, cx| {
            let decoded = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(crop) = this.image_crop.crop.as_mut() else {
                    return;
                };
                match decoded {
                    Some(source) => {
                        crop.preview = Some(source.preview(0));
                        crop.source = Some(Arc::new(source));
                    }
                    None => {
                        tracing::info!("image-crop: the picture did not load");
                        crop.failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Cancel (`e6`): the cropper closes; a select modal under it stays.
    fn cancel_crop(&mut self, cx: &mut Context<Self>) {
        self.image_crop.crop = None;
        cx.notify();
    }

    /// Apply (`e4`): the crop is made off the main thread, handed to the site, and both dialogs close.
    fn apply_crop(&mut self, cx: &mut Context<Self>) {
        let Some(crop) = self.image_crop.crop.as_mut() else {
            return;
        };
        let (Some(source), Some(editor)) = (crop.source.clone(), crop.editor.as_ref()) else {
            return;
        };
        if crop.applying || crop.failed {
            return;
        }
        crop.applying = true;
        let draw = editor.draw();
        // `assetOrigin` (module 469054): a recent avatar kept as it was is the archived asset, an edited one its
        // edit, any other picture a new asset. GAP: the save sends every origin as a new picture.
        let origin = match (&crop.original, editor.has_edits) {
            (Some(_), false) => "ARCHIVED_ASSET",
            (Some(_), true) => "EDITED_ARCHIVED_ASSET",
            (None, _) => "NEW_ASSET",
        };
        tracing::info!("image-crop: asset origin {origin}");
        tracing::info!(
            "image-crop: apply {} rotation {} zoom {:.3} offset ({:.1}, {:.1}) source ({:.1}, {:.1}, {:.1} x {:.1}) canvas {} x {}",
            crop.purpose.key(),
            editor.rotation,
            editor.zoom,
            editor.offset.x,
            editor.offset.y,
            draw.source_x,
            draw.source_y,
            draw.source_width,
            draw.source_height,
            draw.canvas_width,
            draw.canvas_height
        );
        let task = cx
            .background_executor()
            .spawn(async move { output::crop(&source, draw) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(crop) = this.image_crop.crop.take() else {
                    return;
                };
                match result {
                    Ok(cropped) => {
                        tracing::info!(
                            "image-crop: cropped {} {} ({} bytes)",
                            crop.purpose.key(),
                            cropped.mime,
                            cropped.bytes.len()
                        );
                        // `onCrop` closes the select modal (`O`: `r()` before `onComplete`).
                        this.image_crop.select = None;
                        this.image_crop_done(crop.purpose, &crop.file_name, cropped, cx);
                    }
                    Err(error) => {
                        tracing::warn!("image-crop: cropping failed: {error}");
                        this.image_crop.crop = Some(Crop {
                            applying: false,
                            ..crop
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// `onCrop` / `onComplete` of each site.
    fn image_crop_done(
        &mut self,
        purpose: Purpose,
        file_name: &str,
        cropped: output::Cropped,
        cx: &mut Context<Self>,
    ) {
        let uri = cropped.data_uri();
        match purpose {
            Purpose::ProfileAvatar | Purpose::ProfileBanner => {
                let stem = std::path::Path::new(file_name)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
                self.profile_editor_cropped(
                    purpose == Purpose::ProfileBanner,
                    uri,
                    cropped.gpui_image(),
                    &stem,
                    cx,
                );
            }
            Purpose::ServerIcon => self.server_icon_cropped(uri, cx),
            Purpose::GuildBanner => self.boost_banner_cropped(uri, cx),
            Purpose::HomeHeader => self.onboarding_home_header(Some(uri), cx),
            Purpose::GroupIcon => self.group_icon_cropped(uri, cropped.gpui_image(), cx),
        }
    }

    /// The editor once the picture has loaded and the area's height is known (`eK`), and again when it changes.
    fn crop_layout(&mut self, height: f64) {
        let Some(crop) = self.image_crop.crop.as_mut() else {
            return;
        };
        let Some(source) = crop.source.as_ref() else {
            return;
        };
        if crop.area_height == height && crop.editor.is_some() {
            return;
        }
        crop.area_height = height;
        match crop.editor.as_mut() {
            Some(editor) => editor.resized(height),
            None => {
                crop.editor = Some(math::Editor::new(
                    crop.purpose.kind(),
                    source.natural(),
                    height,
                ));
            }
        }
    }

    fn crop_editor_mut(&mut self) -> Option<&mut math::Editor> {
        let crop = self.image_crop.crop.as_mut()?;
        if crop.applying || crop.failed {
            return None;
        }
        crop.editor.as_mut()
    }

    /// The Rotate button (`eZ`): a quarter turn, announced as "Rotated to {degrees} degrees".
    fn rotate_crop(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.crop_editor_mut() else {
            return;
        };
        let degrees = editor.rotate();
        let crop = self.image_crop.crop.as_mut().expect("the crop is open");
        crop.preview = crop.source.as_ref().map(|source| source.preview(degrees));
        // GAP: no screen reader announcer exists yet; the text is kept for the harness (module 765178 `announce`).
        crop.announced = Some(dc_intl::t!("uYhsHT", degrees = degrees));
        tracing::info!("image-crop: rotated to {degrees}");
        cx.notify();
    }

    /// Reset (`eq`).
    fn reset_crop(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.crop_editor_mut() else {
            return;
        };
        if !editor.has_edits {
            return;
        }
        editor.reset();
        let crop = self.image_crop.crop.as_mut().expect("the crop is open");
        crop.preview = crop.source.as_ref().map(|source| source.preview(0));
        cx.notify();
    }

    fn set_crop_zoom(&mut self, zoom: f64, cx: &mut Context<Self>) {
        if let Some(editor) = self.crop_editor_mut() {
            editor.set_zoom(zoom.clamp(1., 2.));
            cx.notify();
        }
    }

    fn crop_slider_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self
            .image_crop
            .crop
            .as_ref()
            .and_then(|crop| crop.slider_bounds.get())
        else {
            return;
        };
        let track = f32::from(bounds.size.width) - 10.;
        let fraction = ((f32::from(x - bounds.left()) - 5.) / track).clamp(0., 1.);
        self.set_crop_zoom(1. + f64::from(fraction), cx);
    }

    /// Keys while a dialog of this module is open, before the window's own handlers: Escape closes the top one; the
    /// arrows move the picture while the editing area has focus (`e5`) and step the zoom on the focused slider.
    pub(super) fn image_crop_key(
        &mut self,
        keys: &gpui::Keystroke,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.image_crop.open() {
            return false;
        }
        if keys.key == "escape" {
            // The top dialog closes: the confirmation, else the cropper (Cancel), else the select modal.
            if self.image_crop.confirm.is_some() {
                self.image_crop.confirm = None;
            } else if self.image_crop.crop.is_some() {
                self.cancel_crop(cx);
            } else {
                self.image_crop.select = None;
            }
            cx.notify();
            return true;
        }
        let Some(crop) = self.image_crop.crop.as_ref() else {
            return false;
        };
        if crop.slider.is_focused(window) {
            let zoom = crop.editor.as_ref().map_or(1., |editor| editor.zoom);
            let next = match keys.key.as_str() {
                "left" | "down" => zoom - ZOOM_STEP,
                "right" | "up" => zoom + ZOOM_STEP,
                "home" => 1.,
                "end" => 2.,
                _ => return false,
            };
            self.set_crop_zoom(next, cx);
            return true;
        }
        if !crop.area.is_focused(window) {
            return false;
        }
        // Home, End, Page Up and Page Down stop there (`stopPropagation`).
        if matches!(keys.key.as_str(), "home" | "end" | "pageup" | "pagedown") {
            return true;
        }
        let shift = keys.modifiers.shift;
        let key = keys.key.clone();
        let taken = self
            .crop_editor_mut()
            .is_some_and(|editor| editor.nudge(&key, shift));
        if taken {
            cx.notify();
        }
        taken
    }

    /// The recent avatars' answers.
    pub(super) fn image_crop_fetched(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if key == RECENT_KEY {
            let recent = &mut self.image_crop.recent;
            recent.fetching = false;
            if body["ok"].as_bool().unwrap_or(false) {
                recent.avatars = parse_recent(&body["body"]);
                recent.fetched_at = Some(std::time::Instant::now());
            } else {
                // `RECENT_AVATARS_FETCH_FAILURE` with an `APIError` (module 181658): the answer's message, else
                // "Oops! You've caught an ultra rare error." (`aTVNes`).
                recent.error = Some(
                    body["body"]["message"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            dc_intl::format(
                                "aTVNes",
                                &[(
                                    "statusPageURL",
                                    dc_intl::Arg::from("https://discordstatus.com"),
                                )],
                            )
                        }),
                );
            }
            cx.notify();
            return true;
        }
        if let Some(id) = key.strip_prefix(DELETE_PREFIX) {
            if body["ok"].as_bool().unwrap_or(false) {
                // `RECENT_AVATAR_DELETE`, then "Avatar removed" (`YJPieI`) is announced.
                self.image_crop
                    .recent
                    .avatars
                    .retain(|avatar| avatar.id != id);
                tracing::info!("image-crop: {}", dc_intl::t("YJPieI"));
            } else {
                self.chat_extra.alert = Some(super::chat_notices::Alert::show(
                    "recent-avatar-remove-failed",
                    dc_intl::t("iufib1"),
                    Some(dc_intl::t("/ZNT+0").to_owned()),
                    None,
                    None,
                    super::chat_notices::Then::Close,
                ));
            }
            if self
                .image_crop
                .confirm
                .as_ref()
                .is_some_and(|confirm| confirm.avatar.id == id)
            {
                self.image_crop.confirm = None;
            }
            cx.notify();
            return true;
        }
        false
    }

    /// A recent avatar fetched at 1024 opens the cropper (`onSelectRecentAvatar`); the file is named by `yM`.
    pub(super) fn image_crop_full_fetched(
        &mut self,
        key: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = key.strip_prefix(FULL_PREFIX) else {
            return;
        };
        let Some(select) = self.image_crop.select.as_mut() else {
            return;
        };
        select.busy = false;
        let purpose = select.purpose;
        let Some(avatar) = self
            .image_crop
            .recent
            .avatars
            .iter()
            .find(|avatar| avatar.id == id)
            .cloned()
        else {
            return;
        };
        let stem = avatar
            .description
            .as_deref()
            .map(|description| description.split(',').next().unwrap_or_default().to_owned())
            .unwrap_or_else(|| dc_intl::t("lqaIxI").to_owned());
        let extension = if avatar.storage_hash.starts_with("a_") {
            "gif"
        } else {
            "webp"
        };
        self.open_crop(
            purpose,
            bytes,
            format!("{stem}.{extension}"),
            Some(avatar),
            cx,
        );
    }

    /// A recent avatar's download failed: "Something went wrong getting your file." (`fZRH9P`).
    pub(super) fn image_crop_full_failed(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !key.starts_with(FULL_PREFIX) {
            return false;
        }
        if let Some(select) = self.image_crop.select.as_mut() {
            select.busy = false;
            select.error = Some(dc_intl::t("fZRH9P").to_owned());
        }
        cx.notify();
        true
    }

    fn select_recent_avatar(&mut self, avatar: &RecentAvatar, cx: &mut Context<Self>) {
        let Some(user) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let Some(select) = self.image_crop.select.as_mut() else {
            return;
        };
        if select.busy {
            return;
        }
        select.busy = true;
        select.error = None;
        let key = format!("{FULL_PREFIX}{}", avatar.id);
        // Asked for afresh each time: the bytes go to the cropper, not the picture store.
        self.requested.remove(&key);
        self.request_image(key, archived_avatar_url(user, avatar, 1024));
        cx.notify();
    }

    /// The Remove button (`v`): with Shift it removes at once (`W`), else it asks first (module 555251).
    fn remove_recent_avatar(&mut self, avatar: RecentAvatar, shift: bool, cx: &mut Context<Self>) {
        if shift {
            self.send_remove_recent(&avatar.id);
        } else {
            self.image_crop.confirm = Some(ConfirmRemove {
                avatar,
                busy: false,
            });
        }
        cx.notify();
    }

    fn send_remove_recent(&mut self, id: &str) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{DELETE_PREFIX}{id}")),
            method: reqwest::Method::DELETE,
            route: format!("/users/@me/avatars/{id}"),
            body: None,
        });
    }

    /// The dialogs of this module, over the layers of the sites (the profile editor's are at 2 and 3).
    pub(super) fn image_crop_layers(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.image_crop_request_images(window);
        let mut layers = Vec::new();
        if let Some(select) = self.select_modal(window, cx) {
            layers.push(select);
        }
        if self.image_crop.crop.is_some() {
            self.crop_layout(area_height(window));
            if let Some(crop) = self.crop_modal(window, cx) {
                layers.push(crop);
            }
        }
        if let Some(confirm) = self.remove_avatar_modal(window, cx) {
            layers.push(confirm);
        }
        layers
    }

    /// Module 685663: the upload options and, for an avatar, the recent avatars.
    fn select_modal(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let select = self.image_crop.select.as_ref()?;
        let purpose = select.purpose;
        let kind = purpose.kind();
        let premium_kind = matches!(kind, UploadType::Avatar | UploadType::Banner);
        let light = super::theme() == dc_tokens::Theme::Light;
        let option = |id: &'static str| {
            div()
                .id(id)
                // `aspect-ratio:1` in the two-column grid: half of the 430px body less the 8px gap.
                .size(px(OPTION_SIDE))
                .relative()
                .overflow_hidden()
                .rounded(px(8.))
                .bg(color("--background-mod-normal"))
                .p(px(16.))
                .flex()
                .justify_center()
                .items_center()
                .cursor_pointer()
                .when(light, |this| {
                    this.hover(|style| style.bg(color("--interactive-background-selected")))
                })
        };
        let label = |text: String, premium: bool, icon_name: &'static str| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .child(icon(icon_name, 24., "--interactive-icon-default"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        // `hasPremiumIcon`: 2px after the label; the wheel sits 3px low (`premiumWheel`).
                        .when(premium, |this| {
                            this.child(div().mt(px(3.)).child(icon(
                                "nitro_wheel",
                                16.,
                                "--text-strong",
                            )))
                        })
                        .child(div().when(premium, |this| this.mr(px(2.))).child(text)),
                )
        };
        // Upload Image: the file input under `imageUploadOption` (its hover darkens it by `--opacity-black-8`).
        let upload = option("image-select-upload")
            .child(div().absolute().inset_0().when(!light, |this| {
                this.hover(|style| style.bg(color("--opacity-black-8")))
            }))
            .child(label(
                dc_intl::t("MsUY/S").to_owned(),
                kind == UploadType::Banner,
                "clips_image_plus",
            ))
            .on_click(cx.listener(move |this, _, _, cx| this.pick_crop_file(purpose, cx)));
        // Choose GIF over the four pictures, darkened (`gifGrid:after`, 64% black, 68% on hover in the dark theme).
        let hovered = select.gif_hovered;
        let shade = match (light, hovered) {
            (false, false) => "--opacity-black-64",
            (false, true) => "--opacity-black-68",
            (true, false) => "--opacity-white-64",
            (true, true) => "--opacity-white-56",
        };
        let grid =
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_wrap()
                .children(GIF_GRID.iter().map(|(gif, png)| {
                    let url = format!(
                        "https://discord.com/assets/{}",
                        if hovered { gif } else { png }
                    );
                    let picture = self.images.get(&url).cloned();
                    div()
                        .w(gpui::relative(0.5))
                        .h(gpui::relative(0.5))
                        .overflow_hidden()
                        .children(picture.map(|picture| {
                            img(picture).size_full().object_fit(gpui::ObjectFit::Cover)
                        }))
                }))
                .child(div().absolute().inset_0().bg(color(shade)));
        let gif = option("image-select-gif")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if let Some(select) = this.image_crop.select.as_mut() {
                    select.gif_hovered = *hovered;
                }
                cx.notify();
            }))
            .child(grid)
            .child(div().relative().child(label(
                dc_intl::t("xsC+/y").to_owned(),
                premium_kind,
                "crop_gif",
            )))
            // GAP: the Choose GIF picker (module 491981, Tenor search) is not built; the tile opens nothing yet.
            ;
        let mut body = div().flex().flex_col().gap(px(24.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().flex().gap(px(8.)).child(upload).child(gif))
                // `imageSpecifications` (the profile banner's `IhzZlo`).
                .when(purpose == Purpose::ProfileBanner, |this| {
                    this.child(
                        div()
                            .px(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("IhzZlo")),
                    )
                }),
        );
        if purpose.recent_avatars() {
            body = body.child(self.recent_avatars_section(select, window, cx));
        }
        let spec = modal::Spec {
            id: "image-select",
            title: purpose.title().into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            buttons: Vec::new(),
            close: Box::new(|this, _, _| this.image_crop.select = None),
        };
        Some(modal::render_over_layers(spec, cx))
    }

    /// The recent avatars (`ea` and `en` of module 685663): the header, the avatars and the empty slots.
    fn recent_avatars_section(
        &self,
        select: &Select,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let recent = &self.image_crop.recent;
        let scale = window.scale_factor();
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let header = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("Bnq9zK")),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t!("+CyJu3", recentAvatarsLimit = RECENT_LIMIT)),
            );
        let count = recent.avatars.len().min(RECENT_LIMIT);
        let empty = RECENT_LIMIT.saturating_sub(count);
        let slots = if recent.fetching {
            // GAP: the design system spinner (module 289873) is drawn as the three dots.
            div()
                .h(px(56.))
                .flex()
                .items_center()
                .child(super::components::dots(
                    "recent-avatars-spinner",
                    "--text-muted",
                ))
                .into_any_element()
        } else {
            div()
                .mt(px(4.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .relative()
                        .flex()
                        .gap(px(8.))
                        .children(recent.avatars.iter().take(RECENT_LIMIT).map(|avatar| {
                            let picture = user.and_then(|user| {
                                let url = archived_avatar_url(user, avatar, cdn_size(56., scale));
                                self.images.get(&format!("{THUMB_PREFIX}{url}")).cloned()
                            });
                            let hovered = select.hovered.as_deref() == Some(avatar.id.as_str());
                            let (chosen, removed, hover_id) =
                                (avatar.clone(), avatar.clone(), avatar.id.clone());
                            div()
                                .id(SharedString::from(format!("recent-avatar-{}", avatar.id)))
                                .relative()
                                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                                    if let Some(select) = this.image_crop.select.as_mut() {
                                        if *inside {
                                            select.hovered = Some(hover_id.clone());
                                        } else if select.hovered.as_deref() == Some(hover_id.as_str()) {
                                            select.hovered = None;
                                        }
                                    }
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("recent-avatar-pick-{}", avatar.id)))
                                        .size(px(56.))
                                        .rounded_full()
                                        .overflow_hidden()
                                        .cursor_pointer()
                                        .bg(color("--background-mod-normal"))
                                        .children(picture.map(|picture| {
                                            img(picture).size_full().object_fit(gpui::ObjectFit::Cover)
                                        }))
                                        .when(hovered, |this| {
                                            this.child(div().absolute().inset_0().rounded_full().bg(color("--opacity-black-20")))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.select_recent_avatar(&chosen, cx);
                                        })),
                                )
                                .when(hovered, |this| {
                                    this.child(
                                        self.with_tip(
                                            format!("recent-avatar-remove-{}", removed.id),
                                            Tip::text(dc_intl::t("N86XcP"), Side::Top),
                                            div()
                                                .id(SharedString::from(format!("recent-avatar-remove-{}", removed.id)))
                                                .size(px(28.))
                                                .rounded_full()
                                                .bg(color("--control-overlay-secondary-background-default"))
                                                .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon("clips_trash", 16., "--control-overlay-secondary-text-default"))
                                                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                                    this.remove_recent_avatar(removed.clone(), event.modifiers().shift, cx);
                                                })),
                                            cx,
                                        )
                                        .absolute()
                                        .top(px(-6.))
                                        .right(px(-6.)),
                                    )
                                })
                        }))
                        .when(empty > 0, |this| {
                            this.child(
                                div()
                                    .flex()
                                    .gap(px(12.))
                                    .children((0..empty).map(|_| {
                                        gpui::svg()
                                            .path("icons/recent_avatar_placeholder.svg")
                                            .size(px(56.))
                                            .flex_none()
                                            .text_color(color("--background-mod-normal"))
                                    })),
                            )
                        }),
                )
                .into_any_element()
        };
        let error = select.error.clone().or_else(|| recent.error.clone());
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(header)
            .child(slots)
            .children(error.map(|message| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(4.))
                    .child(icon("shop_circle_error", 16., "--text-feedback-critical"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-feedback-critical"))
                            .child(message),
                    )
            }))
            .into_any_element()
    }

    /// Module 142630: the editing area, the zoom slider and Rotate, Reset in the action bar, Cancel and Apply.
    fn crop_modal(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let crop = self.image_crop.crop.as_ref()?;
        if crop.focus_pending.replace(false) {
            window.focus(&crop.area, cx);
        }
        let height = area_height(window);
        let disabled = crop.applying || crop.failed || crop.source.is_none();
        let editor = crop.editor.as_ref();
        let (sideways, upright) = editor.map_or((false, false), math::Editor::movable);
        let movable = sideways || upright;
        let area = self.crop_area(crop, height, disabled, cx);
        let keyboard = self.keys.keyboard_mode_is_on();
        let area_focused = crop.area.is_focused(window) || crop.slider.is_focused(window);
        // `keyboardHint`: shown in keyboard mode; faded while the area is not focused and the picture can move.
        let hint = (keyboard && !crop.failed).then(|| {
            let line = if movable {
                // `YIfE3a` with its hooks drawn as the key combos of module 475358.
                let runs = dc_intl::format_runs(
                    "YIfE3a",
                    &[
                        ("arrowKeys", dc_intl::Arg::from("Arrow keys")),
                        ("shiftKey", dc_intl::Arg::from("Shift")),
                    ],
                );
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .children(runs.into_iter().map(|(text, tag)| match tag.as_deref() {
                        Some("arrowKeysHook") => {
                            super::keyboard::help::combo("up+down+left+right").into_any_element()
                        }
                        Some("shiftKeyHook") => {
                            super::keyboard::help::combo("shift").into_any_element()
                        }
                        _ => div().child(text.trim().to_owned()).into_any_element(),
                    }))
            } else {
                div().child(dc_intl::t("jUze6F"))
            };
            div()
                .pt(px(8.))
                .mb(px(-4.))
                .flex()
                .justify_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-muted"))
                .when(!area_focused && movable, |this| this.opacity(0.3))
                .child(line)
        });
        let zoom = editor.map_or(1., |editor| editor.zoom);
        let tools = div()
            .mx(px(-6.))
            .py(px(16.))
            .pl(px(36.))
            .flex()
            .items_center()
            .child(
                div()
                    .mx_auto()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().w(px(24.)).flex().justify_center().child(icon(
                        "clips_image",
                        12.,
                        "--interactive-text-default",
                    )))
                    .child(self.crop_slider(crop, zoom, disabled, cx))
                    .child(div().w(px(24.)).flex().justify_center().child(icon(
                        "clips_image",
                        24.,
                        "--interactive-text-default",
                    ))),
            )
            .child(
                self.with_tip(
                    "image-crop-rotate",
                    Tip::text(dc_intl::t("E36Wd4"), Side::Top),
                    super::keyboard::ring_when_focused(div().id("image-crop-rotate"))
                        .focusable()
                        .tab_index(0)
                        .p(px(6.))
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(!disabled, |this| this.cursor_pointer())
                        .child(icon(
                            "rotate",
                            24.,
                            if disabled {
                                "--icon-muted"
                            } else {
                                "--icon-subtle"
                            },
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.rotate_crop(cx))),
                    cx,
                ),
            );
        let body = div()
            .pt(px(8.))
            .flex()
            .flex_col()
            .child(area)
            // The text under the area when it cannot move (`jUze6F`) is for screen readers only (`h.A`).
            .children(hint)
            .child(tools);
        let edits = editor.is_some_and(|editor| editor.has_edits);
        let reset = div()
            .id("image-crop-reset")
            .flex()
            .items_center()
            .h(px(20.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-brand"))
            .when(!edits, |this| this.opacity(0.5))
            .when(edits, |this| {
                this.cursor_pointer().hover(|style| style.underline())
            })
            .child(dc_intl::t("yBZMsQ"))
            .on_click(cx.listener(|this, _, _, cx| this.reset_crop(cx)));
        let apply_tone = if crop.applying {
            Tone::PrimaryLoading
        } else if disabled {
            Tone::PrimaryDisabled
        } else {
            Tone::Primary
        };
        let apply = modal::button(
            "image-crop-apply",
            dc_intl::t("ZSHmKD"),
            apply_tone,
            |this, _, cx| this.apply_crop(cx),
        );
        let spec = modal::Spec {
            id: "image-crop",
            title: dc_intl::t("DxAYCF").into(),
            subtitle: None,
            body: Some(body.into_any_element()),
            buttons: vec![
                modal::button(
                    "image-crop-cancel",
                    dc_intl::t("9TG40l"),
                    Tone::Secondary,
                    |this, _, cx| this.cancel_crop(cx),
                ),
                apply,
            ],
            close: Box::new(|this, _, cx| this.cancel_crop(cx)),
        };
        Some(modal::render_with_input_over_layers(
            spec,
            reset.into_any_element(),
            cx,
        ))
    }

    /// `editingContainer`: the picture on black, moved by dragging, under the dimmed crop shape.
    fn crop_area(
        &self,
        crop: &Crop,
        height: f64,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let area = super::keyboard::ring_when_focused(div().id("image-crop-area"))
            .track_focus(&crop.area.clone().tab_index(0).tab_stop(true))
            .relative()
            .w_full()
            .h(px(height as f32))
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--black"));
        if crop.failed {
            // `errorContainer`: the error icon 8px from "Failed to load the image." (`+ITMYX`).
            return area
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(icon("shop_circle_error", 18., "--text-feedback-critical"))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-feedback-critical"))
                        .child(dc_intl::t("+ITMYX").replace('\n', " ")),
                )
                .into_any_element();
        }
        let (Some(editor), Some(preview)) = (crop.editor.as_ref(), crop.preview.clone()) else {
            // `opacity: imageDimensions != null`: nothing shows until the picture has loaded.
            return area.into_any_element();
        };
        let drawn = editor.drawn();
        let crop_size = editor.crop;
        let centred = || {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
        };
        let picture = div()
            .id("image-crop-picture")
            .flex_none()
            .relative()
            .left(px(editor.offset.x as f32))
            .top(px(editor.offset.y as f32))
            .w(px(drawn.width as f32))
            .h(px(drawn.height as f32))
            .when(!disabled, |this| this.cursor(gpui::CursorStyle::OpenHand))
            .when(disabled, |this| this.cursor_not_allowed())
            .child(img(preview).size_full())
            .when(!disabled, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        let Some(crop) = this.image_crop.crop.as_mut() else {
                            return;
                        };
                        window.focus(&crop.area, cx);
                        if let Some(editor) = crop.editor.as_ref() {
                            crop.dragging = Some(math::Point {
                                x: f64::from(f32::from(event.position.x)) - editor.offset.x,
                                y: f64::from(f32::from(event.position.y)) - editor.offset.y,
                            });
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
            });
        // The dim with its hole and the 5px white ring (`overlay` and its shape class).
        let radius = match crop.purpose.kind() {
            UploadType::Avatar => 0.5,
            UploadType::GuildIcon => 0.25,
            _ => 0.,
        };
        let mask = {
            let key = (
                (crop_size.width * 2.).round() as u32,
                (crop_size.height * 2.).round() as u32,
                (radius * 100.) as u32,
            );
            let mut cached = crop.mask.borrow_mut();
            match cached.as_ref() {
                Some((known, mask)) if *known == key => mask.clone(),
                _ => {
                    let mask = dim_mask(crop_size.width, crop_size.height, radius, 2.);
                    *cached = Some((key, mask.clone()));
                    mask
                }
            }
        };
        let dim = gpui::Rgba {
            r: f32::from(DIM[0]) / 255.,
            g: f32::from(DIM[1]) / 255.,
            b: f32::from(DIM[2]) / 255.,
            a: f32::from(DIM[3]) / 255.,
        };
        let (cw, ch) = (crop_size.width as f32, crop_size.height as f32);
        let far = 2000.;
        let overlay = div()
            .flex_none()
            .relative()
            .w(px(cw))
            .h(px(ch))
            .child(img(mask).absolute().inset_0().size_full())
            .child(
                div()
                    .absolute()
                    .left(px(-far))
                    .top(px(-far))
                    .w(px(cw + 2. * far))
                    .h(px(far))
                    .bg(dim),
            )
            .child(
                div()
                    .absolute()
                    .left(px(-far))
                    .top(px(ch))
                    .w(px(cw + 2. * far))
                    .h(px(far))
                    .bg(dim),
            )
            .child(
                div()
                    .absolute()
                    .left(px(-far))
                    .top_0()
                    .w(px(far))
                    .h(px(ch))
                    .bg(dim),
            )
            .child(
                div()
                    .absolute()
                    .left(px(cw))
                    .top_0()
                    .w(px(far))
                    .h(px(ch))
                    .bg(dim),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border(px(5.))
                    .border_color(color("--white"))
                    .rounded(px((radius * crop_size.width.min(crop_size.height)) as f32)),
            );
        let dragging = crop.dragging.is_some();
        area.child(centred().child(picture))
            .child(centred().child(overlay))
            // Moves and the release over the area itself, before a frame has put the window-wide listener in place.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.dragging() {
                    this.crop_drag_to(event.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.crop_drag_end(cx)),
            )
            .when(dragging, |this| this.child(self.crop_drag_listener()))
            .into_any_element()
    }

    /// `eW`: the picture follows the pointer while it is dragged.
    fn crop_drag_to(&mut self, position: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(crop) = self.image_crop.crop.as_mut() else {
            return;
        };
        let (Some(start), Some(editor)) = (crop.dragging, crop.editor.as_mut()) else {
            return;
        };
        editor.move_to(
            f64::from(f32::from(position.x)) - start.x,
            f64::from(f32::from(position.y)) - start.y,
        );
        cx.notify();
    }

    /// `eY` (`mouseup`): the drag ends and its offset is committed.
    fn crop_drag_end(&mut self, cx: &mut Context<Self>) {
        if let Some(crop) = self.image_crop.crop.as_mut()
            && crop.dragging.take().is_some()
        {
            if let Some(editor) = crop.editor.as_mut() {
                editor.commit();
            }
            cx.notify();
        }
    }

    /// While the picture is dragged, moves and the release anywhere in the window reach it (`mousemove` and
    /// `mouseup` on `window`).
    fn crop_drag_listener(&self) -> impl IntoElement {
        let weak = self.weak.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = weak.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Bubble {
                        return;
                    }
                    let _ = moved.update(cx, |this, cx| this.crop_drag_to(event.position, cx));
                });
                let released = weak.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    let _ = released.update(cx, |this, cx| this.crop_drag_end(cx));
                });
            },
        )
        .absolute()
        .size_0()
    }

    /// The zoom slider (module 205513 with `equidistant` and `hideBubble`): 1 to 2, 128 wide.
    fn crop_slider(
        &self,
        crop: &Crop,
        zoom: f64,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fraction = (zoom - 1.).clamp(0., 1.) as f32;
        let active = !disabled && (crop.slider_drag || crop.slider_hovered);
        let bar_height = if active { 6. } else { 4. };
        let grabber = if active { 20. } else { 16. };
        let track = SLIDER_WIDTH - 10.;
        let bounds = crop.slider_bounds.clone();
        let weak = self.weak.clone();
        div()
            .id("image-crop-slider")
            .track_focus(&crop.slider.clone().tab_index(0).tab_stop(!disabled))
            .relative()
            .w(px(SLIDER_WIDTH))
            .h(px(24.))
            .when(disabled, |this| this.opacity(0.6))
            .child(
                canvas(move |rect, _, _| bounds.set(Some(rect)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top(px(12. - bar_height / 2.))
                    .left_0()
                    .w_full()
                    .h(px(bar_height))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(color("--slider-track-background"))
                    .child(
                        div()
                            .h_full()
                            .w(px(SLIDER_WIDTH * fraction))
                            .bg(color("--background-brand")),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(12. - grabber / 2.))
                    .left(px(5. + track * fraction - grabber / 2.))
                    .size(px(grabber))
                    .rounded_full()
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(gpui::white()),
            )
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if let Some(crop) = this.image_crop.crop.as_mut() {
                            crop.slider_hovered = *hovered;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            if let Some(crop) = this.image_crop.crop.as_mut() {
                                crop.slider_drag = true;
                                window.focus(&crop.slider, cx);
                            }
                            this.crop_slider_to(event.position.x, cx);
                            cx.stop_propagation();
                        }),
                    )
                    // Moves and the release over the slider itself, before the window-wide listener is in place.
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if event.dragging()
                            && this
                                .image_crop
                                .crop
                                .as_ref()
                                .is_some_and(|crop| crop.slider_drag)
                        {
                            this.crop_slider_to(event.position.x, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if let Some(crop) = this.image_crop.crop.as_mut() {
                                crop.slider_drag = false;
                            }
                            cx.notify();
                        }),
                    )
            })
            .when(crop.slider_drag, |this| {
                this.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            let moved = weak.clone();
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase == gpui::DispatchPhase::Bubble {
                                    let _ = moved.update(cx, |this, cx| {
                                        this.crop_slider_to(event.position.x, cx);
                                    });
                                }
                            });
                            let released = weak.clone();
                            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                if phase == gpui::DispatchPhase::Bubble
                                    && event.button == MouseButton::Left
                                {
                                    let _ = released.update(cx, |this, cx| {
                                        if let Some(crop) = this.image_crop.crop.as_mut() {
                                            crop.slider_drag = false;
                                        }
                                        cx.notify();
                                    });
                                }
                            });
                        },
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .into_any_element()
    }

    /// Module 555251: "Remove Avatar", the avatar at 120, the protip, and Cancel / "Remove it please!".
    fn remove_avatar_modal(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.image_crop.confirm.as_ref()?;
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let picture = user.and_then(|user| {
            let url =
                archived_avatar_url(user, &confirm.avatar, cdn_size(120., window.scale_factor()));
            self.images.get(&format!("{THUMB_PREFIX}{url}")).cloned()
        });
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .self_center()
                    .mb(px(12.))
                    .size(px(120.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(color("--background-mod-normal"))
                    .children(picture.map(|picture| img(picture).size_full())),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-feedback-positive"))
                    .child(dc_intl::t("2Ld8DM")),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child(dc_intl::t("Oe0xqt")),
            );
        let id = confirm.avatar.id.clone();
        let spec = modal::Spec {
            id: "recent-avatar-remove",
            title: dc_intl::t("rfaLTH").into(),
            subtitle: Some(div().child(dc_intl::t("EKmxKc")).into_any_element()),
            body: Some(body.into_any_element()),
            buttons: vec![
                modal::button(
                    "recent-avatar-remove-cancel",
                    dc_intl::t("ETE/oC"),
                    Tone::Secondary,
                    |this, _, _| {
                        this.image_crop.confirm = None;
                    },
                ),
                modal::button(
                    "recent-avatar-remove-confirm",
                    dc_intl::t("Lh0uyy"),
                    if confirm.busy {
                        Tone::PrimaryLoading
                    } else {
                        Tone::Critical
                    },
                    move |this, _, _| {
                        if let Some(confirm) = this.image_crop.confirm.as_mut() {
                            confirm.busy = true;
                        }
                        this.send_remove_recent(&id);
                    },
                ),
            ],
            close: Box::new(|this, _, _| this.image_crop.confirm = None),
        };
        Some(modal::render_small_over_popouts(spec, cx))
    }

    /// Capture only: the open image dialogs and the cropper's transform, for the harness's state lines.
    #[cfg(feature = "capture")]
    pub(crate) fn image_crop_state(&self) -> String {
        let state = &self.image_crop;
        let crop = state.crop.as_ref().map(|crop| {
            let editor = crop.editor.as_ref().map(|editor| {
                let draw = editor.draw();
                format!(
                    "image {:.1}x{:.1} crop {:.1}x{:.1} bounds x {:.1}..{:.1} y {:.1}..{:.1} offset ({:.1}, {:.1}) rotation {} zoom {:.3} edits {} source ({:.1}, {:.1}, {:.1}x{:.1}) canvas {}x{}",
                    editor.image.width,
                    editor.image.height,
                    editor.crop.width,
                    editor.crop.height,
                    editor.bounds.left,
                    editor.bounds.right,
                    editor.bounds.bottom,
                    editor.bounds.top,
                    editor.offset.x,
                    editor.offset.y,
                    editor.rotation,
                    editor.zoom,
                    editor.has_edits,
                    draw.source_x,
                    draw.source_y,
                    draw.source_width,
                    draw.source_height,
                    draw.canvas_width,
                    draw.canvas_height
                )
            });
            format!(
                "crop {} failed {} applying {} {} announced {:?}",
                crop.purpose.key(),
                crop.failed,
                crop.applying,
                editor.unwrap_or_else(|| "loading".to_owned()),
                crop.announced
            )
        });
        format!(
            "image-crop: select {:?} crop {:?} confirm {} recent {} awaiting {:?}",
            state.select.as_ref().map(|select| select.purpose.key()),
            crop,
            state.confirm.is_some(),
            state.recent.avatars.len(),
            state.awaiting.map(Purpose::key)
        )
    }

    /// Capture only: opens the select image modal of a site by its key (`avatar`, `banner`, `guild-banner`,
    /// `group-icon`) or waits for the file of a direct site (`server-icon`, `home-header`).
    #[cfg(feature = "capture")]
    pub(crate) fn harness_open_image_select(&mut self, purpose: &str, cx: &mut Context<Self>) {
        match Purpose::parse(purpose) {
            Some(direct @ (Purpose::ServerIcon | Purpose::HomeHeader)) => {
                self.pick_crop_file(direct, cx)
            }
            Some(purpose) => self.open_image_select(purpose, cx),
            None => {}
        }
    }

    /// The pictures the select modal shows: the GIF tile's and the recent avatars' thumbnails.
    pub(super) fn image_crop_request_images(&mut self, window: &Window) {
        let Some(select) = self.image_crop.select.as_ref() else {
            return;
        };
        let hovered = select.gif_hovered;
        let recent = select.purpose.recent_avatars();
        for (gif, png) in GIF_GRID {
            let url = format!(
                "https://discord.com/assets/{}",
                if hovered { gif } else { png }
            );
            self.request_image(url.clone(), url);
        }
        if !recent {
            return;
        }
        let Some(user) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let scale = window.scale_factor();
        let mut wanted: Vec<String> = self
            .image_crop
            .recent
            .avatars
            .iter()
            .take(RECENT_LIMIT)
            .map(|avatar| archived_avatar_url(user, avatar, cdn_size(56., scale)))
            .collect();
        if let Some(confirm) = self.image_crop.confirm.as_ref() {
            wanted.push(archived_avatar_url(
                user,
                &confirm.avatar,
                cdn_size(120., scale),
            ));
        }
        for url in wanted {
            self.request_image(format!("{THUMB_PREFIX}{url}"), url);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cdn_size_rounds_up_to_the_list() {
        assert_eq!(cdn_size(56., 1.), 56);
        assert_eq!(cdn_size(56., 1.5), 96);
        assert_eq!(cdn_size(120., 2.), 240);
        assert_eq!(cdn_size(5000., 1.), 4096);
    }

    #[test]
    fn recent_avatars_are_read_from_the_answer() {
        let body = serde_json::json!({"avatars": [
            {"id": "11", "storage_hash": "abc", "description": "me, added today"},
            {"id": "12", "storage_hash": "a_def"},
            {"id": 13}
        ]});
        let avatars = parse_recent(&body);
        assert_eq!(avatars.len(), 2);
        assert_eq!(avatars[0].description.as_deref(), Some("me, added today"));
        assert_eq!(avatars[1].description, None);
        assert_eq!(
            archived_avatar_url(7, &avatars[1], 1024),
            "https://cdn.discordapp.com/avatars/7/archived/12/a_def.gif?size=1024"
        );
    }

    #[test]
    fn only_the_profile_avatar_lists_recent_avatars() {
        assert!(Purpose::ProfileAvatar.recent_avatars());
        assert!(!Purpose::GroupIcon.recent_avatars());
        assert_eq!(Purpose::GroupIcon.kind(), UploadType::Avatar);
        assert_eq!(Purpose::GroupIcon.title(), "Change Icon");
        assert_eq!(Purpose::parse("home-header"), Some(Purpose::HomeHeader));
    }

    #[test]
    fn the_dim_is_clear_inside_the_circle_and_dark_outside() {
        let mask = dim_mask(100., 100., 0.5, 1.);
        let frame = &mask.as_bytes(0).expect("one frame");
        let at = |x: usize, y: usize| frame[(y * 100 + x) * 4 + 3];
        assert_eq!(at(50, 50), 0);
        assert_eq!(at(0, 0), DIM[3]);
        assert_eq!(at(99, 99), DIM[3]);
        let square = dim_mask(100., 50., 0., 1.);
        let frame = &square.as_bytes(0).expect("one frame");
        assert!(frame.chunks(4).all(|pixel| pixel[3] == 0));
    }
}
