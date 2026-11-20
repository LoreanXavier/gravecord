//! Files waiting to be sent: the composer's upload area (`channelAttachmentArea_iH8bsl`, cards `upload_JyxBSo`,
//! action bar `wrapper_Z0MHyc`) and file kinds (`I` in module 453771). Sizes from
//! `design/dom/channel/attaching-darker-cozy.json`.

use super::{Root, color, icon};
use dc_proto::pin;
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, ExternalPaths, FontWeight, Image, ImageFormat,
    IntoElement, PathPromptOptions, ScrollHandle, SharedString, canvas, div, fill, hsla, img,
    point, prelude::*, px, rgb, size,
};
use std::path::PathBuf;
use std::sync::Arc;

/// `h` in module 453771, in order: the first rule that matches decides. Type rules test the MIME type, name rules
/// the lower-cased file name.
pub(super) fn file_kind(name: &str, mime: Option<&str>) -> &'static str {
    let name = name.to_lowercase();
    let ends = |extensions: &[&str]| {
        extensions
            .iter()
            .any(|extension| name.ends_with(&format!(".{extension}")))
    };
    if let Some(mime) = mime {
        if mime.starts_with("image/vnd.adobe.photoshop") {
            return "photoshop";
        }
        if mime.starts_with("image/svg+xml") {
            return "webcode";
        }
        if mime.starts_with("image/") {
            return "image";
        }
        if mime.starts_with("video/") {
            return "video";
        }
    }
    if name.is_empty() {
        return "unknown";
    }
    if ends(&["pdf"]) {
        "acrobat"
    } else if name.contains(".ae") {
        "ae"
    } else if ends(&["sketch"]) {
        "sketch"
    } else if ends(&["ai"]) {
        "ai"
    } else if ends(&["rar", "zip", "7z", "tar", "tar.gz"]) {
        "archive"
    } else if ends(&[
        "c++",
        "cpp",
        "cc",
        "c",
        "h",
        "hpp",
        "mm",
        "m",
        "json",
        "js",
        "ts",
        "rb",
        "rake",
        "py",
        "asm",
        "fs",
        "pyc",
        "dtd",
        "cgi",
        "bat",
        "rss",
        "java",
        "graphml",
        "idb",
        "lua",
        "o",
        "gml",
        "prl",
        "sls",
        "conf",
        "cmake",
        "make",
        "sln",
        "vbe",
        "cxx",
        "wbf",
        "vbs",
        "r",
        "wml",
        "php",
        "bash",
        "applescript",
        "fcgi",
        "yaml",
        "ex",
        "exs",
        "sh",
        "ml",
        "actionscript",
    ]) {
        "code"
    } else if ends(&[
        "txt", "rtf", "doc", "docx", "md", "pages", "ppt", "pptx", "pptm", "key", "log",
    ]) {
        "document"
    } else if ends(&["xls", "xlsx", "numbers", "csv"]) {
        "spreadsheet"
    } else if ends(&["html", "xhtml", "htm", "xml", "xsd", "css", "styl"]) {
        "webcode"
    } else if ends(&["mp3", "ogg", "opus", "wav", "aiff", "flac"]) {
        "audio"
    } else {
        "unknown"
    }
}

/// The `icon_sBdlOA` background of each kind, served from the web app's assets.
pub(super) fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "video" => "98178bd2173d947a.svg",
        "acrobat" => "a42338c030af6d9c.svg",
        "ae" => "88b289225ff6142a.svg",
        "sketch" => "c9f0992f0ab98d49.svg",
        "ai" => "93636da24d7ab467.svg",
        "archive" => "f89b66e97ea52ba7.svg",
        "code" => "f4bad77442422ded.svg",
        "document" => "9ebe3ef63aafee84.svg",
        "photoshop" | "ps" => "1084490cf680dd7c.svg",
        "spreadsheet" => "b8ca4488a6d443bc.svg",
        "webcode" => "6e688b0c124e0ca8.svg",
        "audio" => "a6db1d05d35f13d0.svg",
        _ => "94660b205108a49f.svg",
    }
}

pub(super) fn kind_icon_key(kind: &str) -> String {
    format!("filekind:{kind}")
}

pub(super) fn kind_icon_url(kind: &str) -> String {
    format!("{}/assets/{}", pin::WEBAPP_ORIGIN, kind_icon(kind))
}

fn mime_of(path: &std::path::Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    Some(match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "psd" => "image/vnd.adobe.photoshop",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => return None,
    })
}

pub(crate) struct Pending {
    pub path: PathBuf,
    pub name: String,
    kind: &'static str,
    preview: Option<Arc<Image>>,
    /// The picture's own size, for fitting it into the card.
    picture: Option<(u32, u32)>,
    /// The picture as a card shows it under the spoiler mark: blurred, over the spoiler background.
    blurred: Option<Arc<Image>>,
    /// The alt text set in Modify Attachment.
    pub description: Option<String>,
    /// Not in the pinned client: the card's AnonymiseFileNames switch, `None` until pressed (`plugin_hooks.rs`).
    pub anonymise: Option<bool>,
}

/// The accessible name of an upload card's picture (module 914905): `8TRAzR` with the alt text, else `lXoOEZ`; a
/// file without a name is "uploaded image" (`lduvqL`).
pub(super) fn media_label(name: &str, alt: Option<&str>) -> String {
    let file = if name.is_empty() {
        dc_intl::t("lduvqL").to_owned()
    } else {
        name.to_owned()
    };
    match alt.filter(|alt| !alt.is_empty()) {
        Some(alt) => dc_intl::t!("8TRAzR", filename = file, alt = alt).to_string(),
        None => dc_intl::t!("lXoOEZ", filename = file).to_string(),
    }
}

/// The prefix that marks an attachment as a spoiler (`SPOILER_` in module 453771).
pub(super) const SPOILER_PREFIX: &str = "SPOILER_";

/// The name with the spoiler mark switched: added when absent, taken off when present.
pub(super) fn toggle_spoiler(name: &str) -> String {
    match name.strip_prefix(SPOILER_PREFIX) {
        Some(plain) => plain.to_owned(),
        None => format!("{SPOILER_PREFIX}{name}"),
    }
}

impl Pending {
    pub(super) fn from_upload(upload: crate::net::Upload) -> Option<Self> {
        let mut pending = Self::new(upload.path)?;
        pending.name = upload.name;
        pending.description = upload.description;
        Some(pending)
    }

    pub(super) fn new(path: PathBuf) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let mime = mime_of(&path);
        let kind = file_kind(&name, mime);
        let mut blurred = None;
        let (preview, picture) = if kind == "image" {
            let bytes = std::fs::read(&path).ok()?;
            let decoded = image::load_from_memory(&bytes).ok();
            let size = decoded
                .as_ref()
                .map(|decoded| (decoded.width(), decoded.height()));
            blurred = decoded.as_ref().and_then(spoiler_bake);
            let format = match mime {
                Some("image/jpeg") => ImageFormat::Jpeg,
                Some("image/gif") => ImageFormat::Gif,
                Some("image/webp") => ImageFormat::Webp,
                Some("image/bmp") => ImageFormat::Bmp,
                _ => ImageFormat::Png,
            };
            (Some(Arc::new(Image::from_bytes(format, bytes))), size)
        } else {
            (None, None)
        };
        Some(Self {
            path,
            name,
            kind,
            preview,
            picture,
            blurred,
            description: None,
            anonymise: None,
        })
    }

    /// The picture and the size it is drawn at in Modify Attachment: its own size, at most 150 tall and the 430px
    /// column less 2 x 16 wide (`imageFilePreviewImg_TLgD1o`).
    pub(super) fn preview_image(&self) -> Option<(Arc<Image>, f32, f32)> {
        let (width, height) = (self.picture?.0 as f32, self.picture?.1 as f32);
        let scale = (398. / width).min(150. / height).min(1.);
        Some((self.preview.clone()?, width * scale, height * scale))
    }

    /// Not in the pinned client: the card as PreviewMessage's preview shows it (`plugin_hooks/preview_message.rs`).
    pub(in crate::ui) fn preview_file(&self) -> super::plugin_hooks::PreviewFile {
        super::plugin_hooks::PreviewFile {
            name: self.name.clone(),
            size: std::fs::metadata(&self.path).map_or(0, |meta| meta.len()),
            content_type: mime_of(&self.path),
            picture: self.picture,
        }
    }

    /// Not in the pinned client: the card's picture for PreviewMessage's preview.
    pub(in crate::ui) fn preview_picture(&self) -> Option<Arc<Image>> {
        self.preview.clone()
    }

    pub fn is_spoiler(&self) -> bool {
        self.name.starts_with(SPOILER_PREFIX)
    }

    /// What the message sends for this card.
    pub fn upload(self) -> crate::net::Upload {
        // Not in the pinned client: AnonymiseFileNames renames the file as it is sent (`plugin_hooks.rs`).
        let name =
            super::plugin_hooks::anonymised_name(&self.name, self.anonymise).unwrap_or(self.name);
        crate::net::Upload {
            path: self.path,
            name,
            description: self.description,
        }
    }
}

/// The spoiler cover of a picture: the picture fitted to the card's media box, blurred with a 28px radius and laid at
/// 28% over `--__spoiler-background-color--hidden` (oklab 0.516 0.0027 -0.0139, rgb 102 103 112). The radius and the
/// share are fitted to the pixels of `design/golden/windows/upload/row-image-spoiler` (mean error under 2 levels).
fn spoiler_bake(decoded: &image::DynamicImage) -> Option<Arc<Image>> {
    const COVER: [f32; 3] = [102., 103., 112.];
    const SHARE: f32 = 0.28;
    let (width, height) = fitted((decoded.width(), decoded.height()));
    let small = decoded
        .resize_exact(
            width.round().max(1.) as u32,
            height.round().max(1.) as u32,
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    let mut blurred = image::imageops::blur(&small, 28.);
    for pixel in blurred.pixels_mut() {
        for (channel, cover) in pixel.0.iter_mut().zip(COVER) {
            *channel = (cover * (1. - SHARE) + f32::from(*channel) * SHARE).round() as u8;
        }
        pixel.0[3] = 255;
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(blurred)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

/// What follows the last card in the row: the action bar hangs 15px past the card's edge and counts as content
/// (`design/dom/upload/row-three-scroll`: the bar of the last card ends at 1111, the row starts at 384).
const ROW_TAIL: f32 = 15.;

/// The scrollable width of a row of `count` cards (`upload_JyxBSo` 218 wide, 24 apart, 10 in front).
fn row_content_width(count: usize) -> f32 {
    10. + count as f32 * 218. + count.saturating_sub(1) as f32 * 24. + ROW_TAIL
}

/// `scrollbarGhost_s8JExU`: a 14px bar under the row whose track (8% black) and thumb (40% black) keep a 3px margin
/// (module 415695's CSS); the thumb is the viewport's share of the content.
fn upload_scrollbar(handle: ScrollHandle) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let max = handle.max_offset().x;
            if max <= px(0.) {
                return;
            }
            let track = Bounds::new(
                point(bounds.left() + px(3.), bounds.top() + px(3.)),
                size(bounds.size.width - px(6.), px(8.)),
            );
            let viewport = bounds.size.width;
            let thumb = (track.size.width * (viewport / (viewport + max))).max(px(40.));
            let scrolled = (-f32::from(handle.offset().x) / f32::from(max)).clamp(0., 1.);
            let left = track.left() + (track.size.width - thumb) * scrolled;
            window.paint_quad(fill(track, hsla(0., 0., 0., 0.08)).corner_radii(px(4.)));
            window.paint_quad(
                fill(
                    Bounds::new(point(left, track.top()), size(thumb, px(8.))),
                    hsla(0., 0., 0., 0.4),
                )
                .corner_radii(px(4.)),
            );
        },
    )
    .mr(px(12.))
    .h(px(14.))
}

/// The 200px media box of a card: a picture fitted to its width, centred above the name.
fn fitted(picture: (u32, u32)) -> (f32, f32) {
    let (width, height) = (picture.0 as f32, picture.1 as f32);
    let scale = (200. / width).min(174. / height);
    (width * scale, height * scale)
}

struct VideoPreview {
    preview: Arc<Image>,
    size: (u32, u32),
    blurred: Option<Arc<Image>>,
}

fn decode_video_preview(path: &std::path::Path) -> Option<VideoPreview> {
    const MAX_PREVIEW_FILE_BYTES: u64 = 256 * 1024 * 1024;
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > MAX_PREVIEW_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let rgba = crate::video_clip::mp4_first_frame_rgba(&bytes)?;
    let size = (rgba.width(), rgba.height());
    let decoded = image::DynamicImage::ImageRgba8(rgba);
    let blurred = spoiler_bake(&decoded);
    let (fw, fh) = fitted(size);
    let thumb = decoded.resize_exact(
        (fw.round().max(1.) as u32).saturating_mul(2),
        (fh.round().max(1.) as u32).saturating_mul(2),
        image::imageops::FilterType::Triangle,
    );
    let mut png = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(VideoPreview {
        preview: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        size,
        blurred,
    })
}

impl Root {
    pub(crate) fn attach_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // A batch the official client refuses (empty, over the limit, too many) adds none of its files.
        let sizes: Vec<u64> = paths
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .collect();
        if self.refuse_upload(&sizes) {
            cx.notify();
            return;
        }
        let mut video_paths = Vec::new();
        for path in paths {
            if let Some(pending) = Pending::new(path) {
                // The art of the kind also draws the card of the message that sends this file.
                self.request_image(kind_icon_key(pending.kind), kind_icon_url(pending.kind));
                if pending.kind == "video" && pending.preview.is_none() {
                    video_paths.push(pending.path.clone());
                }
                self.pending_files.push(pending);
            }
        }
        for path in video_paths {
            cx.spawn(async move |this, cx| {
                let worker_path = path.clone();
                let decoded = cx
                    .background_executor()
                    .spawn(async move { decode_video_preview(&worker_path) })
                    .await;
                if let Some(decoded) = decoded {
                    let _ = this.update(cx, |root, cx| {
                        let mut updated = false;
                        for item in &mut root.pending_files {
                            if item.path == path && item.preview.is_none() {
                                item.preview = Some(decoded.preview.clone());
                                item.picture = Some(decoded.size);
                                item.blurred = decoded.blurred.clone();
                                updated = true;
                            }
                        }
                        if updated {
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// The attach button: the system file dialog, several files allowed.
    pub(super) fn pick_files(&mut self, cx: &mut Context<Self>) {
        let picked = super::native_dialog::open_paths(
            PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: None,
            },
            super::native_dialog::ALL_FILTERS,
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |this, cx| this.attach_files(paths, cx));
            }
        })
        .detach();
    }

    pub(super) fn on_drop_files(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        self.attach_files(paths.paths().to_vec(), cx);
    }

    fn upload_card(&self, index: usize, pending: &Pending, cx: &mut Context<Self>) -> AnyElement {
        let media = match (&pending.preview, pending.picture) {
            (Some(preview), Some(picture)) => {
                let (width, height) = fitted(picture);
                let cover = pending
                    .is_spoiler()
                    .then(|| pending.blurred.clone())
                    .flatten();
                let shown = cover.clone().unwrap_or_else(|| preview.clone());
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id(SharedString::from(format!("upload-media-{index}")))
                            // Module 914905: "Open expanded image view for {filename}", or "... with description
                            // {alt}" when the file has alt text.
                            .aria_label(media_label(&pending.name, pending.description.as_deref()))
                            .w(px(width))
                            .h(px(height))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .relative()
                            .child(img(shown).w(px(width)).h(px(height)).rounded(px(8.)))
                            // `spoilerWarning_0_7_Rr`: a 20px-radius pill (60% black, padding 8 by 12) centred on the cover.
                            .when(cover.is_some(), |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .px(px(12.))
                                                .py(px(8.))
                                                .rounded(px(20.))
                                                .bg(hsla(0., 0., 0., 0.6))
                                                .text_size(px(15.))
                                                .line_height(px(15.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(rgb(0xe4e4e6))
                                                .child("SPOILER"),
                                        ),
                                )
                            }),
                    )
                    .into_any_element()
            }
            _ => {
                let icon_image = self.images.get(&kind_icon_key(pending.kind)).cloned();
                div()
                    .h(px(144.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(icon_image.map(|picture| img(picture).w(px(72.)).h(px(96.))))
                    .into_any_element()
            }
        };
        let action = |id: String, name: &str, tint: &str| {
            div()
                .id(SharedString::from(id))
                .size(px(32.))
                .p(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(icon(name, 20., tint))
        };
        div()
            .relative()
            .flex_none()
            .w(px(218.))
            .h(px(218.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-normal"))
            .bg(color("--background-surface-high"))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.14),
                offset: point(px(0.), px(1.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(media)
                    .when(pending.preview.is_none(), |this| this.child(div().flex_1()))
                    .child(
                        div()
                            .h(px(26.))
                            .pt(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(
                                pending
                                    .name
                                    .strip_prefix(SPOILER_PREFIX)
                                    .unwrap_or(&pending.name)
                                    .to_owned(),
                            ),
                    ),
            )
            // `actionBar_JyxBSo`: pinned to the corner and pushed out by a quarter of its size.
            .child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .right(px(-17.))
                    .h(px(34.))
                    .items_center()
                    .rounded(px(4.))
                    .bg(color("--background-surface-highest"))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.08))
                    .flex()
                    // Not in the pinned client: AnonymiseFileNames' switch, first in the bar (`plugin_hooks.rs`).
                    .children(super::plugin_hooks::anonymise_default().map(|default| {
                        let on = pending.anonymise.unwrap_or(default);
                        action(
                            format!("upload-anonymise-{index}"),
                            if on {
                                "plugin_anon_on"
                            } else {
                                "plugin_anon_off"
                            },
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(pending) = this.pending_files.get_mut(index) {
                                pending.anonymise = Some(!on);
                            }
                            cx.notify();
                        }))
                    }))
                    .child(
                        action(
                            format!("upload-spoiler-{index}"),
                            // The eye becomes the slashed eye while the card is a spoiler.
                            if pending.is_spoiler() {
                                "eye_slash"
                            } else {
                                "upload_spoiler"
                            },
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(pending) = this.pending_files.get_mut(index) {
                                pending.name = toggle_spoiler(&pending.name);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        action(
                            format!("upload-modify-{index}"),
                            "upload_modify",
                            "--interactive-text-default",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_modify_attachment(index, window, cx)
                            },
                        )),
                    )
                    .child(
                        action(
                            format!("upload-remove-{index}"),
                            "upload_remove",
                            "--control-critical-secondary-text-default",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if index < this.pending_files.len() {
                                this.pending_files.remove(index);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn upload_area(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let staged = self.staged_sticker_card(cx);
        if self.pending_files.is_empty() && staged.is_none() {
            return None;
        }
        let cards: Vec<AnyElement> = self
            .pending_files
            .iter()
            .enumerate()
            .map(|(index, pending)| self.upload_card(index, pending, cx))
            .chain(staged)
            .collect();
        let handle = self.upload_scroll.clone();
        let count = cards.len();
        let viewport = handle.bounds().size.width;
        let overflows = viewport > px(0.) && px(row_content_width(count)) > viewport;
        Some(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("upload-row")
                        .flex()
                        .gap(px(24.))
                        .pt(px(20.))
                        .pl(px(10.))
                        .pb(px(10.))
                        .pr(px(ROW_TAIL))
                        // The area is 14px narrower than the composer's outer box: 612 in a 626 box
                        // (`scrollableContainer_2x8-Qn` and `channelAttachmentArea_iH8bsl`, row-three-scroll).
                        .mr(px(12.))
                        .overflow_x_scroll()
                        .track_scroll(&handle)
                        .children(cards),
                )
                .when(overflows, |this| {
                    this.child(upload_scrollbar(handle.clone()))
                })
                // The scrollbar takes room only when the row overflows: ask for another frame once that changes.
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, window, _| {
                            if (handle.max_offset().x > px(0.)) != overflows {
                                window.refresh();
                            }
                        },
                    )
                    .absolute()
                    .size_0(),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{file_kind, toggle_spoiler};

    #[test]
    fn the_spoiler_mark_switches_on_and_off() {
        assert_eq!(toggle_spoiler("cat.png"), "SPOILER_cat.png");
        assert_eq!(toggle_spoiler("SPOILER_cat.png"), "cat.png");
    }

    #[test]
    fn kinds_follow_the_official_table() {
        assert_eq!(file_kind("notes.txt", None), "document");
        assert_eq!(file_kind("main.rs.bak", None), "unknown");
        assert_eq!(file_kind("build.sh", None), "code");
        assert_eq!(file_kind("shot.png", Some("image/png")), "image");
        assert_eq!(file_kind("logo.svg", Some("image/svg+xml")), "webcode");
        assert_eq!(file_kind("pack.tar.gz", None), "archive");
        assert_eq!(file_kind("clip.aep", None), "ae");
    }
}

#[cfg(test)]
mod media_label_tests {
    /// Module 914905's accessible names of an upload card's picture.
    #[test]
    fn the_picture_is_named_by_its_file_and_alt_text() {
        assert_eq!(
            super::media_label("cat.png", None),
            "Open expanded image view for cat.png"
        );
        assert_eq!(
            super::media_label("", Some("")),
            "Open expanded image view for uploaded image"
        );
        assert!(super::media_label("cat.png", Some("a cat")).contains("a cat"));
    }
}
