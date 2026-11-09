//! Paste into the composer: a copied picture or files become pending uploads, text goes into the field, and a message
//! over the length limit opens the "Your message is too long..." dialog (module 798612) that uploads it as `message.txt`.
//! The clipboard item is passed in, so tests and the harness never touch the system clipboard.

use super::modal::{self, Spec, Tone};
use super::{Root, color};
use gpui::{
    AnyElement, ClipboardEntry, ClipboardItem, Context, FontWeight, ImageFormat, IntoElement, div,
    prelude::*, px,
};
use std::path::PathBuf;

/// `MAX_MESSAGE_LENGTH` and the Nitro one.
pub(super) const MAX_LENGTH: usize = 2000;
pub(super) const MAX_LENGTH_PREMIUM: usize = 4000;

/// The name a pasted picture gets (`image.png`), and the file a long message becomes.
const PASTED_IMAGE: &str = "image";
const LONG_MESSAGE_FILE: &str = "message.txt";

/// A fresh folder under the system temp directory, so pasted files keep their official names.
fn scratch_dir() -> Option<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let dir = std::env::temp_dir()
        .join("gravecord-paste")
        .join(nanos.to_string());
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The bytes and extension a pasted picture is uploaded as: the common web formats as they are, anything else
/// (the clipboard's bitmap) re-encoded to PNG.
fn picture_file(format: ImageFormat, bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    if matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp
    ) {
        return Some((bytes.to_vec(), format.extension()));
    }
    let decoded = image::load_from_memory(bytes).ok()?;
    let mut out = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some((out, "png"))
}

impl Root {
    /// The longest message the account may send.
    pub(super) fn max_message_length(&self) -> usize {
        self.premium().max_message_length()
    }

    /// Pastes a clipboard item. Pictures and files become pending uploads and the call returns true; text is left to
    /// the field (false), which inserts it as typed.
    pub(crate) fn paste_clipboard_item(
        &mut self,
        item: &ClipboardItem,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut files: Vec<PathBuf> = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(paths) => files.extend(paths.paths().iter().cloned()),
                ClipboardEntry::Image(picture) => {
                    if let Some((bytes, extension)) = picture_file(picture.format, &picture.bytes)
                        && let Some(dir) = scratch_dir()
                    {
                        let path = dir.join(format!("{PASTED_IMAGE}.{extension}"));
                        if std::fs::write(&path, bytes).is_ok() {
                            files.push(path);
                        }
                    }
                }
                ClipboardEntry::String(_) => {}
            }
        }
        if files.is_empty() {
            return false;
        }
        self.attach_files(files, cx);
        true
    }

    /// Enter on a message over the limit: it stays in the field and the dialog opens. True when it was held back.
    pub(super) fn hold_too_long(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        if text.chars().count() <= self.max_message_length() {
            return false;
        }
        self.too_long = Some(text.to_owned());
        let held = text.to_owned();
        self.composer
            .update(cx, |composer, _| composer.set_text(&held));
        cx.notify();
        true
    }

    /// "Upload as File": the text goes up as `message.txt` and the field empties.
    pub(super) fn upload_text_as_file(&mut self, text: &str, cx: &mut Context<Self>) {
        self.too_long = None;
        if let Some(dir) = scratch_dir() {
            let path = dir.join(LONG_MESSAGE_FILE);
            if std::fs::write(&path, text).is_ok() {
                self.composer
                    .update(cx, |composer, _| composer.set_text(""));
                self.attach_files(vec![path], cx);
            }
        }
        cx.notify();
    }

    /// The count of characters over the limit (`characterCount`, measured in the too-long golden) with the Nitro hint
    /// under it, at the composer's bottom right.
    pub(super) fn length_counter(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let max = self.max_message_length();
        let over = self
            .composer
            .read(cx)
            .text()
            .chars()
            .count()
            .checked_sub(max)?;
        if over == 0 {
            return None;
        }
        let hint = (max == MAX_LENGTH).then(|| {
            div()
                .mt(px(8.))
                .h(px(28.))
                .px(px(8.))
                .rounded(px(8.))
                .bg(color("--background-surface-highest"))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(super::icon("nitro_mark", 18., "--text-link"))
                .child("Send longer messages with ")
                .child(
                    div()
                        .text_color(color("--text-link"))
                        .child("Discord Nitro!"),
                )
        });
        Some(
            div()
                .absolute()
                .right(px(14.))
                .bottom(px(10.))
                .flex()
                .flex_col()
                .items_end()
                .child(
                    div()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-feedback-critical"))
                        .child(format!("-{over}")),
                )
                .children(hint)
                .into_any_element(),
        )
    }

    pub(super) fn too_long_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = self.too_long.clone()?;
        let max = self.max_message_length();
        let premium = max == MAX_LENGTH_PREMIUM;
        let mut subtitle = div().flex().flex_col().gap(px(8.)).child(format!(
            "You\u{2019}ve hit the {max} character count limit."
        ));
        if !premium {
            subtitle = subtitle.child(
                div()
                    .font_weight(FontWeight(400.))
                    .text_color(color("--text-default"))
                    .child("Speak your mind with 2x longer messages, send larger file uploads, ")
                    .child(
                        div()
                            .id("too-long-learn-more")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child("and more with Discord Nitro!")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.too_long = None;
                                this.open_nitro(cx);
                            })),
                    ),
            );
        }
        let mut buttons = vec![modal::button(
            "too-long-upload",
            "Upload as File",
            Tone::Secondary,
            move |this, _, cx| this.upload_text_as_file(&text, cx),
        )];
        if !premium {
            buttons.push(modal::button(
                "too-long-nitro",
                "Get Nitro",
                Tone::Primary,
                |this, _, cx| {
                    this.too_long = None;
                    this.open_nitro(cx);
                },
            ));
        }
        Some(modal::render(
            Spec {
                id: "too-long-modal",
                title: "Your message is too long...".into(),
                subtitle: Some(subtitle.into_any_element()),
                body: None,
                buttons,
                close: Box::new(|this, _, _| this.too_long = None),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_is_kept_and_a_bitmap_becomes_a_png() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (bytes, extension) = picture_file(ImageFormat::Png, &png).unwrap();
        assert_eq!((bytes, extension), (png.clone(), "png"));
        let mut bmp = Vec::new();
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        let (converted, extension) = picture_file(ImageFormat::Bmp, &bmp).unwrap();
        assert_eq!(extension, "png");
        assert_eq!(&converted[1..4], b"PNG");
        assert!(picture_file(ImageFormat::Bmp, b"not a picture").is_none());
    }
}
