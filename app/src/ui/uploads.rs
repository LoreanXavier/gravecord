//! Files waiting to be sent: the composer's upload area (`channelAttachmentArea_iH8bsl`, cards `upload_JyxBSo`,
//! action bar `wrapper_Z0MHyc`) and file kinds (`I` in module 453771). Sizes from
//! `design/dom/channel/attaching-darker-cozy.json`.

use super::{Root, color, icon};
use dc_proto::pin;
use gpui::{
    AnyElement, BoxShadow, Context, ExternalPaths, Image, ImageFormat, IntoElement,
    PathPromptOptions, SharedString, div, hsla, img, point, prelude::*, px,
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
}

impl Pending {
    fn new(path: PathBuf) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let mime = mime_of(&path);
        let kind = file_kind(&name, mime);
        let (preview, picture) = if kind == "image" {
            let bytes = std::fs::read(&path).ok()?;
            let size = image::load_from_memory(&bytes)
                .ok()
                .map(|decoded| (decoded.width(), decoded.height()));
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
        })
    }
}

/// The 200px media box of a card: a picture fitted to its width, centred above the name.
fn fitted(picture: (u32, u32)) -> (f32, f32) {
    let (width, height) = (picture.0 as f32, picture.1 as f32);
    let scale = (200. / width).min(174. / height);
    (width * scale, height * scale)
}

impl Root {
    pub(crate) fn attach_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            if let Some(pending) = Pending::new(path) {
                if pending.preview.is_none() {
                    self.request_image(kind_icon_key(pending.kind), kind_icon_url(pending.kind));
                }
                self.pending_files.push(pending);
            }
        }
        cx.notify();
    }

    /// The attach button: the system file dialog, several files allowed.
    pub(super) fn pick_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
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
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(width))
                            .h(px(height))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .child(
                                img(preview.clone())
                                    .w(px(width))
                                    .h(px(height))
                                    .rounded(px(8.)),
                            ),
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
                            .child(pending.name.clone()),
                    ),
            )
            // `actionBar_JyxBSo`: pinned to the corner and pushed out by a quarter of its size.
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .right(px(-16.))
                    .h(px(32.))
                    .rounded(px(4.))
                    .bg(color("--background-surface-highest"))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.08))
                    .flex()
                    .child(action(
                        format!("upload-spoiler-{index}"),
                        "upload_spoiler",
                        "--interactive-text-default",
                    ))
                    .child(action(
                        format!("upload-modify-{index}"),
                        "upload_modify",
                        "--interactive-text-default",
                    ))
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
        if self.pending_files.is_empty() {
            return None;
        }
        let cards: Vec<AnyElement> = self
            .pending_files
            .iter()
            .enumerate()
            .map(|(index, pending)| self.upload_card(index, pending, cx))
            .collect();
        Some(
            div()
                .flex()
                .gap(px(24.))
                .pt(px(20.))
                .px(px(10.))
                .pb(px(10.))
                .pr(px(30.))
                .children(cards)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::file_kind;

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
