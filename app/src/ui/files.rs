//! Attachments that are not pictures. Kinds from `i` in module 949244; the plaintext preview card from
//! `container_BOOBu-` and `newMosaicStyle_BOOBu-`; sizes from `up` in module 255438. Measured in
//! `design/dom/channel/after-upload.json`.

use super::plaintext_extensions::PLAINTEXT;
use super::{MONO, Root, color, icon};
use dc_state::{Attachment, Message};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Image,
    Audio,
    Plaintext,
    Other,
}

/// `i` in module 949244, for attachments without a picture size: audio by name, then the plaintext list by the last
/// dot-separated part of the name, exactly as written.
pub(super) fn kind(attachment: &Attachment) -> Kind {
    if attachment.is_image() {
        return Kind::Image;
    }
    let name = &attachment.filename;
    let lower = name.to_lowercase();
    if [".mp3", ".m4a", ".ogg", ".opus", ".wav", ".flac"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        return Kind::Audio;
    }
    let last = name.rsplit('.').next().unwrap_or_default();
    if name.contains('.') && PLAINTEXT.contains(&last)
        || !name.contains('.') && PLAINTEXT.contains(&name.as_str())
    {
        Kind::Plaintext
    } else {
        Kind::Other
    }
}

/// `up` with the default options: decimal units, "N KB" rounded up below a megabyte, "N MB" rounded up below a
/// gigabyte, and gigabytes to one decimal.
pub(super) fn format_size(bytes: u64) -> String {
    let kilobytes = bytes as f64 / 1000.;
    if kilobytes / 1000. < 1. {
        return format!("{} KB", kilobytes.ceil() as u64);
    }
    let megabytes = (kilobytes / 1000.).ceil();
    if megabytes < 1000. {
        return format!("{} MB", megabytes as u64);
    }
    let gigabytes = (megabytes / 1000. * 10.).round() / 10.;
    format!("{gigabytes} GB")
}

pub(super) fn text_key(attachment: &Attachment) -> String {
    format!("text:{}", attachment.id)
}

/// What to fetch for the previews of a message's attachments.
pub(super) fn wanted(message: &Message) -> Vec<(String, String)> {
    message
        .attachments
        .iter()
        .filter(|attachment| kind(attachment) == Kind::Plaintext)
        .map(|attachment| (text_key(attachment), attachment.url.clone()))
        .collect()
}

/// `--custom-plaintext-preview-collapsed-lines` is not reached by short files: they take their own lines.
const COLLAPSED_LINES: usize = 6;

impl Root {
    fn plaintext_card(&self, attachment: &Attachment, cx: &mut Context<Self>) -> AnyElement {
        let text = self
            .texts
            .get(&text_key(attachment))
            .cloned()
            .unwrap_or_default();
        let shown: Vec<&str> = text.lines().take(COLLAPSED_LINES).collect();
        let language = self.file_language.get(&attachment.id).cloned();
        let wrap = self.file_wrap.contains(&attachment.id);
        let shown_text = shown.join("\n");
        let (for_language, for_expand, for_more) =
            (attachment.clone(), attachment.clone(), attachment.clone());
        // `max(lines * 16px, 100px) + 26px`.
        let height = (shown.len() as f32 * 16.).max(100.) + 26.;
        div()
            .flex()
            .flex_col()
            .min_w(px(302.))
            .child(
                div()
                    .h(px(height))
                    .overflow_hidden()
                    .rounded_t(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .child(
                        div()
                            .mt(px(8.))
                            .mx(px(8.))
                            .p(px(8.))
                            .font_family(MONO)
                            .text_size(px(14.))
                            .line_height(px(16.))
                            .text_color(color("--text-code"))
                            .when(!wrap, |this| this.whitespace_nowrap())
                            // A chosen language highlights the preview; plain text otherwise.
                            .when_some(language.clone(), |this, language| {
                                this.children(super::code_lines(Some(&language), &shown_text))
                            })
                            .when(language.is_none(), |this| {
                                this.children(
                                    shown.iter().map(|line| div().child((*line).to_owned())),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .h(px(59.))
                    .p(px(12.))
                    .rounded_b(px(8.))
                    .border_1()
                    .border_t_0()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(attachment.filename.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-subtle"))
                                    .child(format_size(attachment.size)),
                            ),
                    )
                    .child(div().flex_1().min_w(px(16.)))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "file-language-{}",
                                attachment.id
                            )))
                            .cursor_pointer()
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.open_file_menu(
                                        for_language.clone(),
                                        super::file_actions::FileMenuKind::Language,
                                        event.position(),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .child(icon("file_language", 18., "--text-default")),
                    )
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "file-expand-{}",
                                attachment.id
                            )))
                            .ml(px(16.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.file_expanded = Some(for_expand.clone());
                                cx.notify();
                            }))
                            .child(icon("file_expand", 18., "--text-default")),
                    )
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "file-more-{}",
                                attachment.id
                            )))
                            .ml(px(16.))
                            .mr(px(4.))
                            .cursor_pointer()
                            .on_click(cx.listener(
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    this.open_file_menu(
                                        for_more.clone(),
                                        super::file_actions::FileMenuKind::More,
                                        event.position(),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .child(icon("file_more", 18., "--text-default")),
                    ),
            )
            .into_any_element()
    }

    /// Generic file card for kinds without a preview.
    fn file_card(&self, attachment: &Attachment, cx: &mut Context<Self>) -> AnyElement {
        let download = attachment.clone();
        div()
            .min_w(px(302.))
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("file-card-{}", attachment.id)))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-link"))
                    .cursor_pointer()
                    // The name is the card's link: it saves the file.
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.download_attachment(&download, cx);
                    }))
                    .child(attachment.filename.clone()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child(format_size(attachment.size)),
            )
            .into_any_element()
    }

    /// `nonVisualMediaItemContainer_idVBIj`: 8px under the pictures, each item as wide as it needs.
    pub(super) fn files(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        // A message still uploading (or failed) shows the cards of its uploads.
        if let Some(placeholder) = self.upload_placeholder(message, cx) {
            return Some(placeholder);
        }
        let cards: Vec<AnyElement> = message
            .attachments
            .iter()
            .filter_map(|attachment| match kind(attachment) {
                // A voice message draws its player in place of a card (`renderAudioComponent`, module 679636).
                Kind::Audio if message.is_voice_message() => {
                    Some(self.voice_pill(message, attachment))
                }
                Kind::Image if self.pref_on("chat_inline_media_uploads", true) => None,
                Kind::Image => Some(self.file_card(attachment, cx)),
                Kind::Plaintext => Some(self.plaintext_card(attachment, cx)),
                Kind::Audio => Some(self.audio_card(message, attachment, cx)),
                Kind::Other => Some(self.file_card(attachment, cx)),
            })
            .map(|card| div().flex().child(card).into_any_element())
            .collect();
        (!cards.is_empty()).then(|| {
            div()
                .mt(px(8.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(cards)
                .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, format_size, kind};
    use dc_state::Attachment;

    fn named(name: &str) -> Attachment {
        Attachment {
            id: 1,
            filename: name.to_owned(),
            url: String::new(),
            proxy_url: String::new(),
            content_type: None,
            width: None,
            height: None,
            size: 25,
            flags: 0,
            description: None,
            duration_secs: None,
            waveform: None,
        }
    }

    #[test]
    fn kinds_follow_the_classifier() {
        assert_eq!(kind(&named("notes.txt")), Kind::Plaintext);
        assert_eq!(kind(&named("main.rs")), Kind::Plaintext);
        assert_eq!(kind(&named("song.MP3")), Kind::Audio);
        assert_eq!(kind(&named("pack.zip")), Kind::Other);
    }

    #[test]
    fn sizes_round_up_in_decimal_units() {
        assert_eq!(format_size(25), "1 KB");
        assert_eq!(format_size(999_999), "1000 KB");
        assert_eq!(format_size(1_500_000), "2 MB");
        assert_eq!(format_size(2_340_000_000), "2.3 GB");
    }
}
