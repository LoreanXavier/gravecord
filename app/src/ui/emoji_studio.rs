//! Emoji Studio modal (module 894399 `EmojiStudioModal`, `eN`, `eh`, `z`, `ee`, `W`, `U`;
//! constants module 80569; styles modules 162555, 740243 and 77634).
//!
//! Opens from Server Settings > Emoji when picking a single image file (or via the capture harness
//! `emoji-studio` command). All image decoding, cropping, rotation, horizontal flipping and
//! preview generation happen off the UI thread (`cx.background_executor().spawn`).

use super::image_crop::output::{self, Cropped, Kind, Source};
use super::server_emoji::{EMOJIS_PREFIX, sanitize_emoji_name};
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Guild, Id, permissions};
use gpui::{
    AnyElement, Bounds, Context, Entity, FocusHandle, FontWeight, ImageFormat, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, SharedString,
    Subscription, div, img, prelude::*, px,
};
use image::{AnimationDecoder, DynamicImage, Frame, RgbaImage, imageops, imageops::FilterType};
use serde_json::Value;
use std::cell::Cell;
use std::io::Cursor;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Rest reply key prefix for Emoji Studio `POST /guilds/{guild}/emojis`.
pub(super) const STUDIO_UPLOAD_PREFIX: &str = "emoji-studio:upload:";
/// `eg` of module 894399: 288x288 crop square (`--custom-image-editor-size: 288px`).
pub(super) const EDITOR_SIZE: f64 = 288.;
/// Max emoji dimension (`128` in `eh`).
const MAX_EMOJI_DIM: u32 = 128;
/// Max emoji payload size (`256 KiB` = `262144` in `894399` `eh` & `690521` `isDataTooBig`).
pub(super) const MAX_EMOJI_BYTES: usize = 262_144;
/// Minimum zoom (`1.0`) and maximum zoom (`4.0`) of the Emoji Studio slider.
pub(super) const MIN_ZOOM: f64 = 1.;
pub(super) const MAX_ZOOM: f64 = 4.;
const ZOOM_STEP: f64 = 0.025;

/// Base emoji slot limits per boost tier (`sN` of module 473145).
pub(super) fn guild_emoji_limit(guild: &Guild) -> usize {
    let tier = match guild.premium_tier {
        1 => 100,
        2 => 150,
        3 => 250,
        _ => 50,
    };
    let has_more = guild.features.iter().any(|f| f == "MORE_EMOJI");
    if has_more { tier.max(200) } else { tier }
}

/// All error codes handled by `W` and `U` of module 894399 (constants from module 80569 & 652215).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StudioError {
    TooManyEmoji,
    TooManyAnimatedEmoji,
    TooBig,
    ResizeAnimated,
    MissingImageData,
    MissingGuild,
    AnimatedCropping,
    ImageLoad,
    NoPermissions,
    RateLimited,
    Unknown,
}

impl StudioError {
    /// `U` of module 894399: maps HTTP status and Discord JSON error code.
    pub(crate) fn from_response(status: u64, body: &Value) -> Self {
        if status == 429 {
            return Self::RateLimited;
        }
        match body["code"].as_u64() {
            Some(30008) => Self::TooManyEmoji,
            Some(30018) => Self::TooManyAnimatedEmoji,
            Some(50045) | Some(50035) => Self::TooBig,
            Some(50138) => Self::ResizeAnimated,
            _ => Self::Unknown,
        }
    }

    /// `W` of module 894399: localized error string via `dc_intl`.
    pub(crate) fn message(self) -> String {
        match self {
            Self::TooManyEmoji | Self::TooManyAnimatedEmoji => dc_intl::t("FtKH49").to_owned(),
            Self::TooBig => dc_intl::t!("kIO9jy", maxSize = 256),
            Self::ResizeAnimated => dc_intl::t("6WN/qk").to_owned(),
            Self::MissingImageData => dc_intl::t("41/Kbh").to_owned(),
            Self::MissingGuild => dc_intl::t("8RCtpD").to_owned(),
            Self::AnimatedCropping => dc_intl::t("yoVkHN").to_owned(),
            Self::ImageLoad => dc_intl::t("xZLPcF").to_owned(),
            Self::NoPermissions => dc_intl::t("QY7ZFZ").to_owned(),
            Self::RateLimited => dc_intl::t("4rjikl").to_owned(),
            Self::Unknown => dc_intl::t("iufib1").to_owned(),
        }
    }
}

/// `ef` of module 894399: scales `(w, h)` so the longer side equals `288 * zoom`.
pub(super) fn scaled_dimensions(width: f64, height: f64, zoom: f64) -> (f64, f64) {
    let target = EDITOR_SIZE * zoom;
    let ratio = width / height.max(1.);
    if width > height {
        (target, target / ratio)
    } else {
        (target * ratio, target)
    }
}

/// Initial zoom on load / reset (`eh`): fills the 288px square by aspect ratio, clamped to `[1, 4]`.
pub(super) fn initial_zoom(width: f64, height: f64) -> f64 {
    let min_side = width.min(height).max(1.);
    let max_side = width.max(height).max(1.);
    (max_side / min_side).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Drag limits `(left, right, bottom, top)` (`ej` of module 894399).
pub(super) fn drag_bounds(
    width: f64,
    height: f64,
    zoom: f64,
    animated: bool,
) -> (f64, f64, f64, f64) {
    let (w, h) = scaled_dimensions(width, height, zoom);
    if animated && (w < EDITOR_SIZE || h < EDITOR_SIZE) {
        return (0., 0., 0., 0.);
    }
    let rx = (EDITOR_SIZE - w).abs() / 2.;
    let ry = (EDITOR_SIZE - h).abs() / 2.;
    (-rx, rx, -ry, ry)
}

/// Transform state of the Emoji Studio image editor (`eh` of module 894399).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Transform {
    /// Display-oriented dimensions (swapped on 90/270 rotation).
    pub width: f64,
    pub height: f64,
    /// Original upright dimensions before any rotation.
    pub natural_width: f64,
    pub natural_height: f64,
    pub rotation: u32,
    pub flip_horizontal: bool,
    pub zoom: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub animated: bool,
}

impl Transform {
    pub(super) fn new(natural_width: f64, natural_height: f64, animated: bool) -> Self {
        let zoom = initial_zoom(natural_width, natural_height);
        Self {
            width: natural_width,
            height: natural_height,
            natural_width,
            natural_height,
            rotation: 0,
            flip_horizontal: false,
            zoom,
            offset_x: 0.,
            offset_y: 0.,
            animated,
        }
    }

    pub(super) fn drawn(&self) -> (f64, f64) {
        scaled_dimensions(self.width, self.height, self.zoom)
    }

    pub(super) fn bounds(&self) -> (f64, f64, f64, f64) {
        drag_bounds(self.width, self.height, self.zoom, self.animated)
    }

    pub(super) fn clamp_offset(&mut self) {
        let (left, right, bottom, top) = self.bounds();
        self.offset_x = self.offset_x.clamp(left, right);
        self.offset_y = self.offset_y.clamp(bottom, top);
    }

    pub(super) fn set_offset(&mut self, x: f64, y: f64) {
        self.offset_x = x;
        self.offset_y = y;
        self.clamp_offset();
    }

    pub(super) fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.clamp_offset();
    }

    pub(super) fn rotate_cw(&mut self) {
        self.rotation = (self.rotation + 90) % 360;
        std::mem::swap(&mut self.width, &mut self.height);
        self.clamp_offset();
    }

    pub(super) fn toggle_flip(&mut self) {
        self.flip_horizontal = !self.flip_horizontal;
    }

    pub(super) fn reset(&mut self) {
        self.width = self.natural_width;
        self.height = self.natural_height;
        self.rotation = 0;
        self.flip_horizontal = false;
        self.zoom = initial_zoom(self.natural_width, self.natural_height);
        self.offset_x = 0.;
        self.offset_y = 0.;
    }

    pub(super) fn is_untouched_square(&self, byte_len: usize) -> bool {
        self.rotation == 0
            && !self.flip_horizontal
            && (self.zoom - 1.).abs() < 1e-6
            && self.offset_x.abs() < 1e-6
            && self.offset_y.abs() < 1e-6
            && (self.natural_width - self.natural_height).abs() < 1e-6
            && byte_len <= MAX_EMOJI_BYTES
    }
}

/// Applies horizontal flip (if set) and clockwise rotation to an RGBA image, matching CSS
/// `rotate(A deg) scaleX(-1)` transform order.
fn transform_rgba(image: &RgbaImage, rotation: u32, flip_horizontal: bool) -> RgbaImage {
    let flipped = if flip_horizontal {
        imageops::flip_horizontal(image)
    } else {
        image.clone()
    };
    match rotation {
        90 => imageops::rotate90(&flipped),
        180 => imageops::rotate180(&flipped),
        270 => imageops::rotate270(&flipped),
        _ => flipped,
    }
}

/// Converts RGBA pixels into a GPUI `RenderImage` (BGRA byte order).
fn rgba_to_render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(rgba)]))
}

/// Cuts the 288x288 crop window from a transformed image and resizes to `<= 128x128`.
fn crop_transformed_frame(frame: &RgbaImage, transform: &Transform) -> RgbaImage {
    let transformed = transform_rgba(frame, transform.rotation, transform.flip_horizontal);
    let (drawn_w, drawn_h) = transform.drawn();
    let img_w = f64::from(transformed.width().max(1));
    let img_h = f64::from(transformed.height().max(1));
    let scale_x = img_w / drawn_w.max(1.);
    let scale_y = img_h / drawn_h.max(1.);
    let src_w = (EDITOR_SIZE * scale_x).round().max(1.) as u32;
    let src_h = (EDITOR_SIZE * scale_y).round().max(1.) as u32;
    let center_x = img_w / 2. - transform.offset_x * scale_x;
    let center_y = img_h / 2. - transform.offset_y * scale_y;
    let x0 = (center_x - f64::from(src_w) / 2.).round() as i64;
    let y0 = (center_y - f64::from(src_h) / 2.).round() as i64;

    let mut cut = RgbaImage::new(src_w, src_h);
    for (dx, dy, out_px) in cut.enumerate_pixels_mut() {
        let sx = x0 + i64::from(dx);
        let sy = y0 + i64::from(dy);
        if sx >= 0
            && sy >= 0
            && sx < i64::from(transformed.width())
            && sy < i64::from(transformed.height())
        {
            *out_px = *transformed.get_pixel(sx as u32, sy as u32);
        }
    }
    let target = MAX_EMOJI_DIM.min(src_w.max(src_h)).max(1);
    if cut.width() == target && cut.height() == target {
        cut
    } else {
        imageops::resize(&cut, target, target, FilterType::Triangle)
    }
}

/// Result of off-thread crop + preview rendering.
struct CropResult {
    cropped: Cropped,
    preview_small: Arc<RenderImage>,
    editor_image: Arc<RenderImage>,
}

/// Runs off the UI thread: builds the cropped emoji payload + its preview and editor `RenderImage`.
fn render_cropped(source: &Source, transform: &Transform) -> Result<CropResult, StudioError> {
    let editor_rgba = transform_rgba(
        &source.upright,
        transform.rotation,
        transform.flip_horizontal,
    );
    let editor_image =
        rgba_to_render_image(if editor_rgba.width().max(editor_rgba.height()) > 1024 {
            let s = 1024. / f64::from(editor_rgba.width().max(editor_rgba.height()));
            imageops::resize(
                &editor_rgba,
                ((f64::from(editor_rgba.width()) * s).round() as u32).max(1),
                ((f64::from(editor_rgba.height()) * s).round() as u32).max(1),
                FilterType::Triangle,
            )
        } else {
            editor_rgba
        });

    if transform.is_untouched_square(source.bytes.len()) {
        let preview_rgba = if source.upright.width() > MAX_EMOJI_DIM {
            imageops::resize(
                &source.upright,
                MAX_EMOJI_DIM,
                MAX_EMOJI_DIM,
                FilterType::Triangle,
            )
        } else {
            source.upright.clone()
        };
        let mime = match source.kind {
            Kind::Gif => "image/gif",
            Kind::AnimatedWebp => "image/webp",
            Kind::Still => "image/png",
        };
        return Ok(CropResult {
            cropped: Cropped {
                mime,
                bytes: source.bytes.clone(),
            },
            preview_small: rgba_to_render_image(preview_rgba),
            editor_image,
        });
    }

    match source.kind {
        Kind::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&source.bytes))
                .map_err(|_| StudioError::AnimatedCropping)?;
            let frames = decoder
                .into_frames()
                .collect_frames()
                .map_err(|_| StudioError::AnimatedCropping)?;
            let mut preview_first = None;
            let mut out_frames = Vec::with_capacity(frames.len());
            for frame in frames {
                let delay = frame.delay();
                let cropped_frame = crop_transformed_frame(&frame.into_buffer(), transform);
                if preview_first.is_none() {
                    preview_first = Some(cropped_frame.clone());
                }
                out_frames.push(Frame::from_parts(cropped_frame, 0, 0, delay));
            }
            let mut out = Vec::new();
            {
                let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut out, 10);
                encoder
                    .set_repeat(image::codecs::gif::Repeat::Infinite)
                    .map_err(|_| StudioError::AnimatedCropping)?;
                encoder
                    .encode_frames(out_frames)
                    .map_err(|_| StudioError::AnimatedCropping)?;
            }
            let preview_rgba =
                preview_first.unwrap_or_else(|| crop_transformed_frame(&source.upright, transform));
            Ok(CropResult {
                cropped: Cropped {
                    mime: "image/gif",
                    bytes: out,
                },
                preview_small: rgba_to_render_image(preview_rgba),
                editor_image,
            })
        }
        Kind::AnimatedWebp => {
            let preview_rgba = crop_transformed_frame(&source.upright, transform);
            Ok(CropResult {
                cropped: Cropped {
                    mime: "image/webp",
                    bytes: source.bytes.clone(),
                },
                preview_small: rgba_to_render_image(preview_rgba),
                editor_image,
            })
        }
        Kind::Still => {
            let cropped_rgba = crop_transformed_frame(&source.upright, transform);
            let mut bytes = Vec::new();
            DynamicImage::ImageRgba8(cropped_rgba.clone())
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
                .map_err(|_| StudioError::ImageLoad)?;
            Ok(CropResult {
                cropped: Cropped {
                    mime: "image/png",
                    bytes,
                },
                preview_small: rgba_to_render_image(cropped_rgba),
                editor_image,
            })
        }
    }
}

/// Checks if a file's magic bytes indicate an animated AVIF (`AjdEvM` non-editable notice).
fn is_avif(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && (&bytes[8..12] == b"avis" || &bytes[8..12] == b"avif")
}

/// Open Emoji Studio modal state held on `Root`.
pub(crate) struct EmojiStudio {
    generation: u64,
    crop_generation: u64,
    /// `true` when editing an existing emoji (`FOYn8U`), `false` when adding (`iMJO37`).
    editing_existing: bool,
    selected_guild: Option<Id>,
    name: String,
    name_field: Entity<Editor>,
    _name_sub: Subscription,
    source: Option<Arc<Source>>,
    transform: Option<Transform>,
    /// Non-editable animated AVIF notice (`AjdEvM`).
    non_editable_animated: bool,
    /// Latest rendered editor image (with rotation + horizontal flip applied).
    editor_image: Option<Arc<RenderImage>>,
    /// Latest cropped 128x128 preview for the aside reaction & jumbo tiles.
    preview_image: Option<Arc<RenderImage>>,
    /// Latest cropped payload ready to POST.
    cropped: Option<Cropped>,
    /// Upload/editor error (`Z`).
    error: Option<StudioError>,
    /// Whether `Finish` is currently posting.
    uploading: bool,
    /// Reset options menu popout open (`eg`).
    reset_menu_open: bool,
    /// Upload-to server dropdown open.
    guild_menu_open: bool,
    /// Active pan drag: `(start_mouse_x, start_mouse_y, start_offset_x, start_offset_y)`.
    dragging: Option<(f64, f64, f64, f64)>,
    /// Active zoom slider drag.
    slider_drag: bool,
    slider_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Checkerboard tile SVG image for the left canvas.
    checkerboard: Arc<gpui::Image>,
    focus: FocusHandle,
}

/// Dark/light subtle 16x16 SVG checkerboard pattern matching `checkerboard_LuYMto`.
fn checkerboard_svg() -> Vec<u8> {
    br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="8" height="8" fill="#ffffff" fill-opacity="0.04"/><rect x="8" y="8" width="8" height="8" fill="#ffffff" fill-opacity="0.04"/></svg>"##
        .to_vec()
}

/// Sanitizes an in-progress name edit as `z` of module 894399 does: whitespace becomes `_`, and
/// once at least 2 characters are present, runs `sanitize_emoji_name`.
pub(super) fn filter_studio_name(raw: &str) -> String {
    let underscored: String = raw
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect();
    if underscored.chars().count() < 2 {
        underscored
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    } else {
        sanitize_emoji_name(&underscored)
    }
}

/// Derives the initial emoji name from a file name (`894399` `eN`).
pub(super) fn initial_emoji_name(file_name: &str) -> String {
    let stem = file_name
        .rfind('.')
        .map_or(file_name, |dot| &file_name[..dot]);
    if stem.is_empty() {
        String::new()
    } else {
        sanitize_emoji_name(stem)
    }
}

impl Root {
    /// Whether Emoji Studio is currently open (for keyboard focus trapping).
    pub(super) fn emoji_studio_open(&self) -> bool {
        self.emoji_studio.is_some()
    }

    /// Eligible guilds where the current user holds `CREATE_GUILD_EXPRESSIONS` (`ee` of 894399).
    fn emoji_studio_guilds(&self) -> Vec<(Id, String, Option<String>, usize)> {
        let Some(me) = self.stores.me() else {
            return Vec::new();
        };
        let animated = self.emoji_studio.as_ref().is_some_and(|s| {
            s.source.as_ref().is_some_and(|src| src.kind != Kind::Still) || s.non_editable_animated
        });
        self.stores
            .guilds
            .iter()
            .filter(|guild| {
                permissions::can_in_guild(guild, me, permissions::CREATE_GUILD_EXPRESSIONS)
            })
            .map(|guild| {
                let limit = guild_emoji_limit(guild);
                let used = self
                    .fetched
                    .get(&format!("{EMOJIS_PREFIX}{}", guild.id))
                    .and_then(|v| v.as_array())
                    .map_or(0, |list| {
                        list.iter()
                            .filter(|e| {
                                e["animated"].as_bool().unwrap_or(false) == animated
                                    && e["managed"].as_bool() != Some(true)
                            })
                            .count()
                    });
                (
                    guild.id,
                    guild.name.clone(),
                    guild.icon.clone(),
                    limit.saturating_sub(used),
                )
            })
            .collect()
    }

    /// Opens Emoji Studio from a local file path (reads bytes off the UI thread).
    pub(super) fn open_emoji_studio_path(
        &mut self,
        guild_id: Option<Id>,
        path: &Path,
        cx: &mut Context<Self>,
    ) {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("emoji.png")
            .to_owned();
        let path_owned = path.to_path_buf();
        let bytes_task = cx
            .background_executor()
            .spawn(async move { std::fs::read(&path_owned).ok() });
        cx.spawn(async move |this, cx| {
            let bytes = bytes_task.await;
            let _ = this.update(cx, |this, cx| match bytes {
                Some(bytes) => this.open_emoji_studio_bytes(guild_id, file_name, bytes, cx),
                None => {
                    this.open_emoji_studio_bytes(guild_id, file_name, Vec::new(), cx);
                }
            });
        })
        .detach();
    }

    /// Opens Emoji Studio from raw image bytes (decodes and crops off the UI thread).
    pub(super) fn open_emoji_studio_bytes(
        &mut self,
        guild_id: Option<Id>,
        file_name: String,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        let default_guild = guild_id.or_else(|| {
            let active = self.guild?;
            let me = self.stores.me()?;
            let guild = self.stores.guild(active)?;
            permissions::can_in_guild(guild, me, permissions::CREATE_GUILD_EXPRESSIONS)
                .then_some(active)
        });
        let initial_name = initial_emoji_name(&file_name);
        let name_field = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(dc_intl::t("U2JFHZ").to_owned());
            editor.set_text(&initial_name);
            editor
        });
        let name_sub = cx.subscribe(
            &name_field,
            |this, editor, event: &EditorEvent, cx| match event {
                EditorEvent::Edited => {
                    let raw = editor.read(cx).text().to_owned();
                    let cleaned = filter_studio_name(&raw);
                    if cleaned != raw {
                        editor.update(cx, |ed, _| ed.set_text(&cleaned));
                    }
                    if let Some(studio) = this.emoji_studio.as_mut() {
                        studio.name = cleaned;
                    }
                    cx.notify();
                }
                EditorEvent::Submit(text) => {
                    let cleaned = filter_studio_name(text);
                    editor.update(cx, |ed, _| ed.set_text(&cleaned));
                    if let Some(studio) = this.emoji_studio.as_mut() {
                        studio.name = cleaned;
                    }
                    this.submit_emoji_studio(cx);
                }
                EditorEvent::Cancel => {
                    this.close_emoji_studio(cx);
                }
            },
        );
        self.emoji_studio_generation += 1;
        let generation = self.emoji_studio_generation;
        let non_editable_animated = is_avif(&bytes);
        let checkerboard = Arc::new(gpui::Image::from_bytes(
            ImageFormat::Svg,
            checkerboard_svg(),
        ));
        self.emoji_studio = Some(EmojiStudio {
            generation,
            crop_generation: 0,
            editing_existing: false,
            selected_guild: default_guild,
            name: initial_name,
            name_field,
            _name_sub: name_sub,
            source: None,
            transform: None,
            non_editable_animated,
            editor_image: None,
            preview_image: None,
            cropped: None,
            error: None,
            uploading: false,
            reset_menu_open: false,
            guild_menu_open: false,
            dragging: None,
            slider_drag: false,
            slider_bounds: Rc::new(Cell::new(None)),
            checkerboard,
            focus: cx.focus_handle(),
        });
        // Pick first eligible guild if none was preselected.
        let eligible = self.emoji_studio_guilds();
        if let Some(studio) = self.emoji_studio.as_mut()
            && studio.selected_guild.is_none()
        {
            studio.selected_guild = eligible.first().map(|(id, _, _, _)| *id);
        }
        // Decode off the UI thread.
        let decode_task = cx
            .background_executor()
            .spawn(async move { output::decode(bytes) });
        cx.spawn(async move |this, cx| {
            let decoded = decode_task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(studio) = this
                    .emoji_studio
                    .as_mut()
                    .filter(|s| s.generation == generation)
                else {
                    return;
                };
                match decoded {
                    Some(source) => {
                        let natural = source.natural();
                        let animated = source.kind != Kind::Still;
                        studio.transform =
                            Some(Transform::new(natural.width, natural.height, animated));
                        studio.source = Some(Arc::new(source));
                        this.schedule_emoji_studio_crop(cx);
                    }
                    None => {
                        studio.error = Some(StudioError::ImageLoad);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Re-crops the image and updates the reaction/jumbo previews off the UI thread.
    fn schedule_emoji_studio_crop(&mut self, cx: &mut Context<Self>) {
        let Some(studio) = self.emoji_studio.as_mut() else {
            return;
        };
        let (Some(source), Some(transform)) = (studio.source.clone(), studio.transform.clone())
        else {
            return;
        };
        let generation = studio.generation;
        studio.crop_generation += 1;
        let crop_generation = studio.crop_generation;
        let task = cx
            .background_executor()
            .spawn(async move { render_cropped(&source, &transform) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ =
                this.update(cx, |this, cx| {
                    let Some(studio) = this.emoji_studio.as_mut().filter(|s| {
                        s.generation == generation && s.crop_generation == crop_generation
                    }) else {
                        return;
                    };
                    match result {
                        Ok(res) => {
                            let too_big = res.cropped.bytes.len() > MAX_EMOJI_BYTES;
                            studio.editor_image = Some(res.editor_image);
                            studio.preview_image = Some(res.preview_small);
                            studio.cropped = Some(res.cropped);
                            studio.error = if too_big {
                                Some(StudioError::TooBig)
                            } else {
                                None
                            };
                        }
                        Err(err) => {
                            studio.error = Some(err);
                        }
                    }
                    cx.notify();
                });
        })
        .detach();
    }

    pub(super) fn close_emoji_studio(&mut self, cx: &mut Context<Self>) {
        self.emoji_studio = None;
        cx.notify();
    }

    fn reset_emoji_studio(&mut self, cx: &mut Context<Self>) {
        if let Some(studio) = self.emoji_studio.as_mut() {
            if let Some(transform) = studio.transform.as_mut() {
                transform.reset();
            }
            studio.error = None;
            studio.reset_menu_open = false;
        }
        self.schedule_emoji_studio_crop(cx);
        cx.notify();
    }

    /// `Finish` (`eA` of module 894399): posts `POST /guilds/{guild}/emojis`.
    fn submit_emoji_studio(&mut self, cx: &mut Context<Self>) {
        let Some(studio) = self.emoji_studio.as_mut() else {
            return;
        };
        if studio.uploading {
            return;
        }
        let Some(guild) = studio.selected_guild else {
            studio.error = Some(StudioError::MissingGuild);
            cx.notify();
            return;
        };
        let Some(cropped) = studio.cropped.as_ref() else {
            studio.error = Some(StudioError::MissingImageData);
            cx.notify();
            return;
        };
        let name = sanitize_emoji_name(&studio.name);
        if name.chars().count() < 2 {
            return;
        }
        let data_uri = cropped.data_uri();
        studio.uploading = true;
        studio.error = None;
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{STUDIO_UPLOAD_PREFIX}{guild}:{name}")),
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/emojis"),
            body: Some(serde_json::json!({
                "image": data_uri,
                "name": name,
            })),
        });
        cx.notify();
    }

    /// Handles the REST reply to `STUDIO_UPLOAD_PREFIX`.
    pub(super) fn emoji_studio_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(rest) = key.strip_prefix(STUDIO_UPLOAD_PREFIX) else {
            return;
        };
        let answer = self.fetched.get(key).cloned().unwrap_or(Value::Null);
        let Some((guild_str, fallback_name)) = rest.split_once(':') else {
            return;
        };
        let guild_id = guild_str.parse::<Id>().ok();
        if answer["ok"].as_bool() == Some(true) {
            let emoji_name = answer["body"]["name"]
                .as_str()
                .unwrap_or(fallback_name)
                .to_owned();
            let guild_name = guild_id
                .and_then(|id| self.stores.guild(id))
                .map(|g| g.name.clone())
                .unwrap_or_default();
            self.emoji_studio = None;
            if let Some(id) = guild_id {
                self.request_emojis(id);
            }
            let toast = dc_intl::t!(
                "BaxFf8",
                emojiName = emoji_name.as_str(),
                guildName = guild_name.as_str()
            );
            self.put_app_toast(toast, true, cx);
            cx.notify();
        } else {
            let status = answer["status"].as_u64().unwrap_or(0);
            let err = StudioError::from_response(status, &answer["body"]);
            if let Some(studio) = self.emoji_studio.as_mut() {
                studio.uploading = false;
                studio.error = Some(err);
            }
            cx.notify();
        }
    }

    /// Keyboard handling while Emoji Studio is open.
    pub(super) fn emoji_studio_key(
        &mut self,
        key: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.emoji_studio.is_none() {
            return false;
        }
        if key.key == "escape" {
            if let Some(studio) = self.emoji_studio.as_mut()
                && (studio.reset_menu_open || studio.guild_menu_open)
            {
                studio.reset_menu_open = false;
                studio.guild_menu_open = false;
                cx.notify();
                return true;
            }
            self.close_emoji_studio(cx);
            return true;
        }
        false
    }

    /// Capture helper: `emoji-studio [path]` opens Emoji Studio on a file or a generated 320x200 test badge.
    #[cfg(feature = "capture")]
    pub(crate) fn script_open_emoji_studio(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let arg = arg.trim();
        let guild = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id));
        if !arg.is_empty() {
            self.open_emoji_studio_path(guild, Path::new(arg), cx);
            return format!("emoji-studio: opening {arg}");
        }
        // Generate a crisp 320x200 synthetic gradient + star PNG so captures work offline without external files.
        let img = RgbaImage::from_fn(320, 200, |x, y| {
            let dx = x as f32 - 160.;
            let dy = y as f32 - 100.;
            let r = (dx * dx + dy * dy).sqrt();
            if r < 68. {
                image::Rgba([255, 224, 92, 255])
            } else if r < 82. {
                image::Rgba([88, 101, 242, 255])
            } else {
                let t = (x as f32 / 320.).clamp(0., 1.);
                image::Rgba([
                    (46. + 60. * t) as u8,
                    (52. + 40. * (1. - t)) as u8,
                    (128. + 90. * t) as u8,
                    255,
                ])
            }
        });
        let mut bytes = Vec::new();
        let _ = DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png);
        self.open_emoji_studio_bytes(guild, "party_star.png".to_owned(), bytes, cx);
        "emoji-studio: opened synthetic party_star.png".to_owned()
    }

    /// Renders the Emoji Studio modal overlay (`EmojiStudioModal` / `eE` of module 894399).
    pub(super) fn emoji_studio_layer(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let studio = self.emoji_studio.as_ref()?;
        let viewport = window.viewport_size();
        let modal_w = (f32::from(viewport.width) - 80.).clamp(600., 960.);
        let modal_h = (f32::from(viewport.height) - 80.).clamp(480., 672.);
        let title_text = if studio.editing_existing {
            dc_intl::t("FOYn8U")
        } else {
            dc_intl::t("iMJO37")
        };

        let eligible_guilds = self.emoji_studio_guilds();
        let guild_error = if eligible_guilds.is_empty() {
            Some(StudioError::NoPermissions)
        } else if let Some(sel) = studio.selected_guild {
            eligible_guilds
                .iter()
                .find(|(id, _, _, _)| *id == sel)
                .and_then(|(_, _, _, slots)| (*slots == 0).then_some(StudioError::TooManyEmoji))
        } else {
            None
        };

        let selected_guild_row = studio
            .selected_guild
            .and_then(|sel| eligible_guilds.iter().find(|(id, _, _, _)| *id == sel));

        let transform = studio.transform.clone();
        let zoom_val = transform.as_ref().map_or(MIN_ZOOM, |t| t.zoom);
        let zoom_pct = ((zoom_val - MIN_ZOOM) / (MAX_ZOOM - MIN_ZOOM)).clamp(0., 1.) as f32;
        let is_dragging = studio.dragging.is_some();
        let non_editable = studio.non_editable_animated;
        let editor_img = studio.editor_image.clone();
        let preview_img = studio.preview_image.clone();
        let slider_bounds = studio.slider_bounds.clone();
        let slider_bounds_for_click = studio.slider_bounds.clone();
        let slider_bounds_for_move = studio.slider_bounds.clone();

        // Left editor area (`editor_LuYMto` + `imageEditor_wNVtHP`).
        let editor_canvas = {
            let mut canvas = div()
                .id("emoji-studio-editor")
                .relative()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .bg(color("--background-base-lowest"))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    let Some(studio) = this.emoji_studio.as_mut() else {
                        return;
                    };
                    if let Some((sx, sy, ox, oy)) = studio.dragging {
                        let nx = ox + (f64::from(event.position.x) - sx);
                        let ny = oy + (f64::from(event.position.y) - sy);
                        if let Some(t) = studio.transform.as_mut() {
                            t.set_offset(nx, ny);
                        }
                        cx.notify();
                    } else if studio.slider_drag
                        && let Some(bounds) = slider_bounds_for_move.get()
                    {
                        let width = f64::from(bounds.size.width).max(1.);
                        let rel = ((f64::from(event.position.x) - f64::from(bounds.origin.x))
                            / width)
                            .clamp(0., 1.);
                        let next_zoom = MIN_ZOOM + rel * (MAX_ZOOM - MIN_ZOOM);
                        if let Some(t) = studio.transform.as_mut() {
                            t.set_zoom(next_zoom);
                        }
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, cx| {
                        let needs_recrop = if let Some(studio) = this.emoji_studio.as_mut() {
                            let changed = studio.dragging.is_some() || studio.slider_drag;
                            studio.dragging = None;
                            studio.slider_drag = false;
                            changed
                        } else {
                            false
                        };
                        if needs_recrop {
                            this.schedule_emoji_studio_crop(cx);
                            cx.notify();
                        }
                    }),
                );

            // Subtle checkerboard backdrop layer.
            canvas = canvas.child(
                div().absolute().inset_0().bg(gpui::rgba(0x00000022)).child(
                    img(studio.checkerboard.clone())
                        .w_full()
                        .h_full()
                        .opacity(0.35),
                ),
            );

            // Editing container (288x288 center crop frame + positioned image).
            let crop_box = {
                let mut container = div()
                    .relative()
                    .w(px(EDITOR_SIZE as f32))
                    .h(px(EDITOR_SIZE as f32))
                    .flex()
                    .items_center()
                    .justify_center();

                if let (Some(img_render), Some(t)) = (editor_img, transform.as_ref()) {
                    let (dw, dh) = t.drawn();
                    let left = (EDITOR_SIZE - dw) / 2. + t.offset_x;
                    let top = (EDITOR_SIZE - dh) / 2. + t.offset_y;
                    container = container.child(
                        div()
                            .id("emoji-studio-image")
                            .absolute()
                            .left(px(left as f32))
                            .top(px(top as f32))
                            .w(px(dw as f32))
                            .h(px(dh as f32))
                            .cursor(if is_dragging {
                                gpui::CursorStyle::ClosedHand
                            } else {
                                gpui::CursorStyle::OpenHand
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    if let Some(studio) = this.emoji_studio.as_mut()
                                        && let Some(t) = studio.transform.as_ref()
                                    {
                                        studio.dragging = Some((
                                            f64::from(event.position.x),
                                            f64::from(event.position.y),
                                            t.offset_x,
                                            t.offset_y,
                                        ));
                                        studio.reset_menu_open = false;
                                        studio.guild_menu_open = false;
                                        cx.notify();
                                    }
                                }),
                            )
                            .child(img(img_render).w_full().h_full()),
                    );
                }

                // Crop overlay border (`overlay_wNVtHP`: 2px solid white + 4 outer dim curtains leaving the 288x288
                // crop square at full brightness) + pan hint (`oBPhdN`).
                if !non_editable && transform.is_some() {
                    let curtain = gpui::hsla(0., 0., 0., if is_dragging { 0.4 } else { 0.68 });
                    let side = EDITOR_SIZE as f32;
                    container = container
                        // Top curtain
                        .child(
                            div()
                                .absolute()
                                .bottom(px(side))
                                .left(px(-999.))
                                .w(px(side + 1998.))
                                .h(px(999.))
                                .bg(curtain),
                        )
                        // Bottom curtain
                        .child(
                            div()
                                .absolute()
                                .top(px(side))
                                .left(px(-999.))
                                .w(px(side + 1998.))
                                .h(px(999.))
                                .bg(curtain),
                        )
                        // Left curtain
                        .child(
                            div()
                                .absolute()
                                .top(px(0.))
                                .right(px(side))
                                .w(px(999.))
                                .h(px(side))
                                .bg(curtain),
                        )
                        // Right curtain
                        .child(
                            div()
                                .absolute()
                                .top(px(0.))
                                .left(px(side))
                                .w(px(999.))
                                .h(px(side))
                                .bg(curtain),
                        )
                        // Crisp 2px white crop frame
                        .child(
                            div()
                                .absolute()
                                .left(px(-2.))
                                .top(px(-2.))
                                .w(px(side + 4.))
                                .h(px(side + 4.))
                                .border_2()
                                .border_color(gpui::rgb(0xffffff)),
                        );
                    if !is_dragging {
                        container = container.child(
                            div()
                                .absolute()
                                .top(px(side + 24.))
                                .left(px(-80.))
                                .w(px(side + 160.))
                                .flex()
                                .justify_center()
                                .opacity(0.6)
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("oBPhdN")),
                        );
                    }
                }
                container
            };

            canvas = canvas.child(crop_box);

            // Top title bar (`titleBar_LuYMto`: 40px high, top 20px, centered).
            canvas = canvas.child(
                div()
                    .absolute()
                    .top(px(20.))
                    .left(px(0.))
                    .right(px(0.))
                    .h(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(title_text),
                    ),
            );

            // Top-left Close button (`closeButton_LuYMto`: top 20, left 20).
            canvas = canvas.child(
                div()
                    .id("emoji-studio-close")
                    .absolute()
                    .top(px(20.))
                    .left(px(20.))
                    .size(px(36.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 20., "--interactive-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| this.close_emoji_studio(cx))),
            );

            // Top-right Reset options menu button (`resetButton_LuYMto`: top 20, right 20).
            let reset_menu = studio.reset_menu_open.then(|| {
                div()
                    .id("emoji-studio-reset-menu")
                    .absolute()
                    .top(px(62.))
                    .right(px(20.))
                    .min_w(px(180.))
                    .p(px(6.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-highest"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .shadow_lg()
                    .child(
                        div()
                            .id("emoji-studio-reset-action")
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-feedback-critical")))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-feedback-critical"))
                            .child(dc_intl::t("ka3Yhm"))
                            .on_click(cx.listener(|this, _, _, cx| this.reset_emoji_studio(cx))),
                    )
            });

            canvas = canvas
                .child(
                    div()
                        .id("emoji-studio-reset-btn")
                        .absolute()
                        .top(px(20.))
                        .right(px(20.))
                        .size(px(36.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("unsend", 20., "--interactive-icon-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(studio) = this.emoji_studio.as_mut() {
                                studio.reset_menu_open = !studio.reset_menu_open;
                                studio.guild_menu_open = false;
                            }
                            cx.notify();
                        })),
                )
                .children(reset_menu);

            // Floating bottom toolbar (`toolbar_wNVtHP`: bottom 24px, max-w 480px) or `AjdEvM` notice.
            let bottom_bar: AnyElement = if non_editable {
                div()
                    .absolute()
                    .bottom(px(24.))
                    .px(px(16.))
                    .py(px(12.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("AjdEvM"))
                    .into_any_element()
            } else {
                let toolbar_btn = |id: &'static str,
                                   icon_name: &'static str,
                                   tip: &'static str,
                                   cx: &mut Context<Self>| {
                    self.with_tip(
                        format!("{id}-tip"),
                        super::tooltip::Tip::body(
                            super::tooltip::Body::Text(dc_intl::t(tip).into()),
                            super::tooltip::Side::Top,
                        ),
                        div()
                            .id(id)
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(icon_name, 20., "--interactive-icon-default")),
                        cx,
                    )
                };

                let rotate_btn = div()
                    .id("emoji-studio-rotate")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.emoji_studio.as_mut()
                            && let Some(t) = studio.transform.as_mut()
                        {
                            t.rotate_cw();
                        }
                        this.schedule_emoji_studio_crop(cx);
                        cx.notify();
                    }))
                    .child(toolbar_btn(
                        "emoji-studio-rotate-ic",
                        "rotate",
                        "FEIIO9",
                        cx,
                    ));

                let flip_btn = div()
                    .id("emoji-studio-flip")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.emoji_studio.as_mut()
                            && let Some(t) = studio.transform.as_mut()
                        {
                            t.toggle_flip();
                        }
                        this.schedule_emoji_studio_crop(cx);
                        cx.notify();
                    }))
                    .child(toolbar_btn(
                        "emoji-studio-flip-ic",
                        "flip_horizontal",
                        "4LRS2p",
                        cx,
                    ));

                let zoom_out_btn = div()
                    .id("emoji-studio-zoom-out")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.emoji_studio.as_mut()
                            && let Some(t) = studio.transform.as_mut()
                        {
                            t.set_zoom(t.zoom - ZOOM_STEP * 4.);
                        }
                        this.schedule_emoji_studio_crop(cx);
                        cx.notify();
                    }))
                    .child(toolbar_btn(
                        "emoji-studio-zoom-out-ic",
                        "lb_zoom_out",
                        "QlArhK",
                        cx,
                    ));

                let zoom_in_btn = div()
                    .id("emoji-studio-zoom-in")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(studio) = this.emoji_studio.as_mut()
                            && let Some(t) = studio.transform.as_mut()
                        {
                            t.set_zoom(t.zoom + ZOOM_STEP * 4.);
                        }
                        this.schedule_emoji_studio_crop(cx);
                        cx.notify();
                    }))
                    .child(toolbar_btn(
                        "emoji-studio-zoom-in-ic",
                        "lb_zoom_in",
                        "Ch32tT",
                        cx,
                    ));

                // Interactive zoom slider (`1.0 .. 4.0`).
                let slider_track = div()
                    .id("emoji-studio-zoom-slider")
                    .relative()
                    .flex_1()
                    .h(px(24.))
                    .mx(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(
                        gpui::canvas(
                            move |bounds, _, _| {
                                slider_bounds.set(Some(bounds));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(6.))
                            .rounded_full()
                            .bg(color("--background-mod-strong"))
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(zoom_pct))
                                    .rounded_full()
                                    .bg(color("--brand-500")),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(gpui::relative(zoom_pct))
                            .ml(px(-7.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(gpui::rgb(0xffffff))
                            .border_1()
                            .border_color(color("--border-strong")),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if let Some(studio) = this.emoji_studio.as_mut()
                                && let Some(bounds) = slider_bounds_for_click.get()
                            {
                                studio.slider_drag = true;
                                let width = f64::from(bounds.size.width).max(1.);
                                let rel = ((f64::from(event.position.x)
                                    - f64::from(bounds.origin.x))
                                    / width)
                                    .clamp(0., 1.);
                                let next_zoom = MIN_ZOOM + rel * (MAX_ZOOM - MIN_ZOOM);
                                if let Some(t) = studio.transform.as_mut() {
                                    t.set_zoom(next_zoom);
                                }
                                cx.notify();
                            }
                        }),
                    );

                div()
                    .absolute()
                    .bottom(px(24.))
                    .w(px((modal_w - 288. - 48.).clamp(280., 480.)))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .p(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(rotate_btn)
                            .child(flip_btn),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(px(6.))
                            .border_l_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(zoom_out_btn)
                            .child(slider_track)
                            .child(zoom_in_btn),
                    )
                    .into_any_element()
            };

            canvas.child(bottom_bar)
        };

        // Right aside (`aside_LuYMto`: 288px, padding 24px, gap 24px).
        let required_star = || {
            div()
                .ml(px(3.))
                .text_color(color("--text-feedback-critical"))
                .child("*")
        };
        let field_label = |label: &'static str, required: bool| {
            div()
                .mb(px(8.))
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(label)
                .when(required, |d| d.child(required_star()))
        };

        let reaction_preview = div()
            .flex_1()
            .h(px(110.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .h(px(30.))
                    .px(px(6.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(match preview_img.as_ref() {
                        Some(p) => img(p.clone()).size(px(20.)).into_any_element(),
                        None => div()
                            .size(px(20.))
                            .rounded(px(4.))
                            .bg(color("--background-mod-strong"))
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child("6"),
                    ),
            );

        let jumbo_preview = div()
            .flex_1()
            .h(px(110.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_center()
            .child(match preview_img.as_ref() {
                Some(p) => img(p.clone()).size(px(48.)).into_any_element(),
                None => div()
                    .size(px(48.))
                    .rounded(px(8.))
                    .bg(color("--background-mod-strong"))
                    .into_any_element(),
            });

        let name_section = div()
            .flex()
            .flex_col()
            .child(field_label(dc_intl::t("m0YV7M"), true))
            .child(
                div()
                    .h(px(40.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(studio.name_field.clone()),
                    )
                    .when(!studio.name.is_empty(), |d| {
                        d.child(
                            div()
                                .id("emoji-studio-clear-name")
                                .size(px(20.))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 12., "--icon-muted"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(studio) = this.emoji_studio.as_mut() {
                                        studio.name.clear();
                                        let field = studio.name_field.clone();
                                        field.update(cx, |ed, _| ed.set_text(""));
                                    }
                                    cx.notify();
                                })),
                        )
                    }),
            );

        let guild_section = (!studio.editing_existing).then(|| {
            let disabled = eligible_guilds.is_empty();
            let trigger_text = match selected_guild_row {
                Some((_, name, _, _)) => name.clone(),
                None if disabled => dc_intl::t("jHpxwo").to_owned(),
                None => dc_intl::t("4mqeQO").to_owned(),
            };
            let trigger_slots =
                selected_guild_row.map(|(_, _, _, slots)| dc_intl::t!("WkK72v", count = *slots));

            let dropdown = studio.guild_menu_open.then(|| {
                let mut list = div()
                    .id("emoji-studio-guild-dropdown")
                    .occlude()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(44.))
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        let on_trigger = this
                            .tip_anchors
                            .borrow()
                            .get("emoji-studio-guild-trigger")
                            .is_some_and(|b| b.contains(&event.position));
                        if !on_trigger {
                            if let Some(studio) = this.emoji_studio.as_mut() {
                                studio.guild_menu_open = false;
                            }
                            cx.notify();
                        }
                    }))
                    .p(px(4.))
                    .rounded(px(8.))
                    .bg(color("--background-surface-highest"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .shadow(vec![super::shadow_high()])
                    .flex()
                    .flex_col()
                    .gap(px(2.));
                for (gid, gname, _, slots) in &eligible_guilds {
                    let gid_copy = *gid;
                    let selected = studio.selected_guild == Some(*gid);
                    let slots_text = dc_intl::t!("WkK72v", count = *slots);
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("emoji-studio-guild-{gid}")))
                            .flex_none()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .when(selected, |d| d.bg(color("--background-mod-normal")))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-strong"))
                                    .child(gname.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(slots_text),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(studio) = this.emoji_studio.as_mut() {
                                    studio.selected_guild = Some(gid_copy);
                                    studio.guild_menu_open = false;
                                }
                                this.request_emojis(gid_copy);
                                cx.notify();
                            })),
                    );
                }
                gpui::deferred(list).with_priority(99)
            });

            div()
                .flex()
                .flex_col()
                .child(field_label(dc_intl::t("9uKafS"), true))
                .child(
                    div()
                        .relative()
                        .child(
                            div()
                                .id("emoji-studio-guild-trigger")
                                .relative()
                                .h(px(40.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .bg(color("--input-background-default"))
                                .border_1()
                                .border_color(if guild_error.is_some() {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--border-subtle")
                                })
                                .when(!disabled, |d| d.cursor_pointer())
                                .when(disabled, |d| d.opacity(0.5))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(trigger_text),
                                )
                                .children(trigger_slots.map(|slots| {
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(slots)
                                }))
                                .child(icon("automod_chevron_down", 16., "--icon-muted"))
                                .child(self.anchor_probe("emoji-studio-guild-trigger"))
                                .when(!disabled, |d| {
                                    d.on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(studio) = this.emoji_studio.as_mut() {
                                            studio.guild_menu_open = !studio.guild_menu_open;
                                            studio.reset_menu_open = false;
                                        }
                                        cx.notify();
                                    }))
                                }),
                        )
                        .children(dropdown),
                )
                .children(guild_error.map(|err| {
                    div()
                        .mt(px(4.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-feedback-critical"))
                        .child(err.message())
                }))
        });

        let can_finish = !studio.uploading
            && studio.cropped.is_some()
            && studio.selected_guild.is_some()
            && studio.name.chars().count() >= 2
            && guild_error.is_none()
            && studio.error != Some(StudioError::TooBig);

        let aside = div()
            .w(px(288.))
            .flex_none()
            .h_full()
            .p(px(24.))
            .bg(color("--background-surface-high"))
            .border_l_1()
            .border_color(color("--border-muted"))
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(field_label(dc_intl::t("JmuIb5"), false))
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(reaction_preview)
                            .child(jumbo_preview),
                    ),
            )
            .child(name_section)
            .children(guild_section)
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(studio.error.map(|err| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err.message())
                    }))
                    .child(
                        div()
                            .id("emoji-studio-finish")
                            .h(px(40.))
                            .w_full()
                            .rounded(px(8.))
                            .bg(color("--brand-500"))
                            .when(can_finish, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.bg(color("--brand-560")))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_emoji_studio(cx);
                                    }))
                            })
                            .when(!can_finish, |d| d.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(gpui::rgb(0xffffff))
                            .child(dc_intl::t("Q7UP6F")),
                    ),
            );

        // Backdrop + modal container (`modalRoot_emg1jO`).
        Some(
            div()
                .id("emoji-studio-backdrop")
                .track_focus(&studio.focus)
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_emoji_studio(cx)),
                )
                .child(
                    div()
                        .id("emoji-studio-modal")
                        .w(px(modal_w))
                        .h(px(modal_h))
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow_2xl()
                        .flex()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(editor_canvas)
                        .child(aside),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn studio_transform_matches_module_894399_geometry() {
        // 320x200 image: aspect ratio 1.6 -> initial zoom 1.6, scaled 460.8 x 288.0.
        let mut t = Transform::new(320., 200., false);
        assert!((t.zoom - 1.6).abs() < 1e-6);
        let (w, h) = t.drawn();
        assert!((w - 460.8).abs() < 1e-3);
        assert!((h - 288.).abs() < 1e-3);
        let (left, right, bottom, top) = t.bounds();
        assert!((right - 86.4).abs() < 1e-3);
        assert!((left + 86.4).abs() < 1e-3);
        assert!(top.abs() < 1e-6 && bottom.abs() < 1e-6);

        // Pan clamps to bounds.
        t.set_offset(500., 50.);
        assert!((t.offset_x - 86.4).abs() < 1e-3);
        assert!(t.offset_y.abs() < 1e-6);

        // Rotate 90deg swaps width and height.
        t.rotate_cw();
        assert_eq!(t.rotation, 90);
        assert_eq!((t.width, t.height), (200., 320.));
        t.toggle_flip();
        assert!(t.flip_horizontal);

        // Reset restores initial state.
        t.reset();
        assert_eq!(t.rotation, 0);
        assert!(!t.flip_horizontal);
        assert!((t.zoom - 1.6).abs() < 1e-6);
        assert_eq!((t.offset_x, t.offset_y), (0., 0.));
    }

    #[test]
    fn studio_name_filtering_and_error_messages_resolve() {
        assert_eq!(initial_emoji_name("my cool emoji!.png"), "mycoolemoji");
        assert_eq!(filter_studio_name("hi there"), "hi_there");
        assert_eq!(filter_studio_name("a"), "a");
        assert_eq!(
            StudioError::from_response(429, &json!({})),
            StudioError::RateLimited
        );
        assert_eq!(
            StudioError::from_response(400, &json!({"code": 30008})),
            StudioError::TooManyEmoji
        );
        assert_eq!(
            StudioError::from_response(400, &json!({"code": 30018})),
            StudioError::TooManyAnimatedEmoji
        );
        assert_eq!(
            StudioError::from_response(400, &json!({"code": 50045})),
            StudioError::TooBig
        );
        assert_eq!(
            StudioError::from_response(400, &json!({"code": 50138})),
            StudioError::ResizeAnimated
        );
        for err in [
            StudioError::TooManyEmoji,
            StudioError::TooManyAnimatedEmoji,
            StudioError::TooBig,
            StudioError::ResizeAnimated,
            StudioError::MissingImageData,
            StudioError::MissingGuild,
            StudioError::AnimatedCropping,
            StudioError::ImageLoad,
            StudioError::NoPermissions,
            StudioError::RateLimited,
            StudioError::Unknown,
        ] {
            assert!(!err.message().is_empty(), "{err:?} message empty");
        }
        assert!(StudioError::TooBig.message().contains("256"));
    }

    #[test]
    fn crop_and_flip_produce_valid_png_within_128px() {
        let img = RgbaImage::from_fn(256, 128, |x, _| {
            if x < 128 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("png");
        let source = output::decode(png).expect("decode");
        let mut transform = Transform::new(256., 128., false);
        transform.toggle_flip();
        let res = render_cropped(&source, &transform).expect("render");
        assert_eq!(res.cropped.mime, "image/png");
        assert!(res.cropped.data_uri().starts_with("data:image/png;base64,"));
        let out = image::load_from_memory(&res.cropped.bytes)
            .expect("out")
            .to_rgba8();
        assert_eq!(out.dimensions(), (128, 128));
        // Flipped horizontally: left half of centre crop is blue, right half is red.
        assert_eq!(out.get_pixel(10, 64).0, [0, 0, 255, 255]);
        assert_eq!(out.get_pixel(118, 64).0, [255, 0, 0, 255]);
    }
}
