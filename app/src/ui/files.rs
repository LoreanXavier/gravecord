//! Attachments that are not pictures. Kinds from `i` in module 949244; the plaintext preview card from
//! `container_BOOBu-` and `newMosaicStyle_BOOBu-`; sizes from `up` in module 255438. Measured in
//! `design/dom/channel/after-upload.json`.

use super::plaintext_extensions::PLAINTEXT;
use super::tooltip::{Side, Tip};
use super::{MONO, Root, color, icon};
use crate::parity::Parity;
use dc_state::{Attachment, Message};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, SharedString, div, img, prelude::*,
    px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Image,
    Video,
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
    if attachment.is_video() {
        return Kind::Video;
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
        .filter_map(|attachment| match kind(attachment) {
            Kind::Plaintext => Some((text_key(attachment), attachment.url.clone())),
            Kind::Other | Kind::Image | Kind::Video => {
                let kind = super::uploads::file_kind(&attachment.display_name(), None);
                Some((
                    super::uploads::kind_icon_key(kind),
                    super::uploads::kind_icon_url(kind),
                ))
            }
            Kind::Audio => None,
        })
        .collect()
}

/// `--custom-plaintext-preview-collapsed-lines` is not reached by short files: they take their own lines.
const COLLAPSED_LINES: usize = 6;

/// `em` of module 383055: the languages a text file is drawn as Markdown in.
const MARKDOWN_LANGUAGES: [&str; 4] = ["markdown", "md", "mkd", "mkdown"];

/// The language a text file is read in: the one chosen, else the last part of its name (`n.split(".").slice(-1)`).
pub(super) fn file_language(chosen: Option<&str>, filename: &str) -> String {
    chosen.map_or_else(
        || filename.rsplit('.').next().unwrap_or_default().to_owned(),
        str::to_owned,
    )
}

/// Whether the file is drawn as Markdown: its language is one of `em` and Render Markdown is on (`j=h&&em.has(c)`).
pub(super) fn renders_markdown(language: &str, on: bool) -> bool {
    on && MARKDOWN_LANGUAGES.contains(&language)
}

impl Root {
    /// Module 456262 in the card (`markup_ku1OHH markdownView_KFT5TX`: 8px by 16px, the message markup's styles),
    /// then the notice of what is not shown (`markdownEllipsis_KFT5TX`, body-sm in `--text-subtle`, 16px in, 8px
    /// under). The text is read by the message parser, not markdown-it: tables, soft line breaks and link
    /// references are not drawn as the official preview draws them.
    pub(super) fn markdown_preview(
        &self,
        attachment: &Attachment,
        text: &str,
        notice: Option<String>,
    ) -> AnyElement {
        let body = Message::from_value(&serde_json::json!({
            "id": attachment.id.to_string(), "channel_id": "0", "content": text,
            "timestamp": "1970-01-01T00:00:00+00:00", "author": {"id": "0", "username": ""},
        }));
        div()
            .flex()
            .flex_col()
            .children(body.map(|body| {
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .child(self.content_tinted(&body, "--text-default"))
            }))
            .children(notice.map(|notice| {
                div()
                    .px(px(16.))
                    .pb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(notice)
            }))
            .into_any_element()
    }

    fn plaintext_card(&self, attachment: &Attachment, cx: &mut Context<Self>) -> AnyElement {
        let text = self
            .texts
            .get(&text_key(attachment))
            .cloned()
            .unwrap_or_default();
        let shown: Vec<&str> = text.lines().take(COLLAPSED_LINES).collect();
        let language = self.file_language.get(&attachment.id).cloned();
        let wrap = !self.file_unwrapped.contains(&attachment.id);
        let shown_text = shown.join("\n");
        let markdown = renders_markdown(
            &file_language(language.as_deref(), &attachment.filename),
            !self.file_markdown_off.contains(&attachment.id),
        );
        // `eE`: past the lines shown, "..." and how many lines are left (`DQnFp2`).
        let left = text.lines().count().saturating_sub(COLLAPSED_LINES);
        let notice = (left > 0).then(|| format!("... {}", dc_intl::t!("DQnFp2", lines = left)));
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
                    // The Markdown view is not clamped to the collapsed lines (`KQ` only without it).
                    .when(!markdown, |code| code.h(px(height)))
                    .overflow_hidden()
                    .rounded_t(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .when(markdown, |card| {
                        card.child(self.markdown_preview(attachment, &shown_text, notice.clone()))
                    })
                    .when(!markdown, |card| {
                        card.child(
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
                        )
                    }),
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
    fn file_card(
        &self,
        message: &Message,
        attachment: &Attachment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = attachment.display_name();
        let group = SharedString::from(format!("generic-file-{}-{}", message.id, attachment.id));
        let name_click = attachment.clone();
        let hit_url = attachment.url.clone();
        let hit_label = name.clone();
        let kind = super::uploads::file_kind(&name, None);
        let art = self
            .images
            .get(&super::uploads::kind_icon_key(kind))
            .cloned();
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|me| me.id == message.author.id);
        let removable = own && (message.attachments.len() > 1 || !message.content.is_empty());
        let card = div()
            .parity_with_border("attachment-file-card", 1.)
            .w_full()
            .p(px(16.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![super::hover_bar::popover_shadow(0.)])
            .flex()
            .items_center()
            .child(
                div()
                    .parity("attachment-file-icon")
                    .w(px(30.))
                    .h(px(40.))
                    .flex_none()
                    .mr(px(8.))
                    .when_some(art, |this, art| this.child(img(art).size_full())),
            )
            .child(
                div()
                    .parity("attachment-file-inner")
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "file-card-name-{}",
                                attachment.id
                            )))
                            .parity("attachment-file-name")
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(color("--text-link"))
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.download_attachment(&name_click, cx);
                            }))
                            .on_mouse_up(
                                MouseButton::Right,
                                cx.listener(move |this, event: &gpui::MouseUpEvent, _, _| {
                                    this.menu_hit = Some((
                                        event.position,
                                        super::menu::Hit::Link {
                                            url: hit_url.clone(),
                                            label: hit_label.clone(),
                                        },
                                    ));
                                }),
                            )
                            .child(name),
                    )
                    .child(
                        div()
                            .parity("attachment-file-size")
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(color("--text-muted"))
                            .child(super::components::humanize_filesize(attachment.size)),
                    ),
            );
        // Not in the pinned client: ZipPreview's row under a `.zip` file (`flex-wrap`, a full-width row after the name;
        // `plugin_hooks/zip_preview.rs`).
        let card = match self.zip_preview_inline(message.id, attachment, cx) {
            Some(row) => card.flex_wrap().child(row),
            None => card,
        };
        let button = |id: &'static str, glyph: &'static str| {
            div()
                .id((id, attachment.id))
                .flex()
                .p(px(6.))
                .cursor_pointer()
                .text_color(color("--interactive-text-default"))
                .hover(|style| {
                    style
                        .bg(color("--interactive-background-hover"))
                        .text_color(color("--interactive-text-hover"))
                })
                .active(|style| {
                    style
                        .bg(color("--interactive-background-active"))
                        .text_color(color("--interactive-text-active"))
                })
                .child(
                    gpui::svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(20.))
                        .flex_none(),
                )
        };
        let mut buttons = div()
            .id(("file-buttons", attachment.id))
            .parity("attachment-file-buttons")
            .absolute()
            .top(px(-8.))
            .right(px(-8.))
            .rounded(px(5.))
            .overflow_hidden()
            .flex()
            .bg(color("--background-base-low"))
            .shadow(vec![gpui::BoxShadow {
                color: color("--background-base-lower").into(),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
            .opacity(0.)
            .group_hover(group.clone(), |style| style.opacity(1.))
            .hover(|style| style.opacity(1.));
        if own {
            let (message, edit_attachment) = (message.clone(), attachment.clone());
            let target =
                button("file-edit", "clips_pencil").on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_modify_sent(&message, &edit_attachment, cx);
                }));
            buttons = buttons.child(self.with_tip(
                format!("file-edit-tip-{}", attachment.id),
                Tip::text("Modify Attachment", Side::Top),
                target,
                cx,
            ));
        }
        if removable {
            let (message, remove_attachment) = (message.clone(), attachment.clone());
            let target = button("file-remove", "clips_trash").on_click(cx.listener(
                move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_remove_sent(&message, &remove_attachment, cx);
                },
            ));
            buttons = buttons.child(self.with_tip(
                format!("file-remove-tip-{}", attachment.id),
                Tip::text("Remove Message Attachment", Side::Top),
                target,
                cx,
            ));
        }
        let download = attachment.clone();
        let target = button("file-download", "lb_download")
            .parity("attachment-file-download")
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.download_attachment(&download, cx);
            }));
        buttons = buttons.child(self.with_tip(
            format!("file-download-tip-{}", attachment.id),
            Tip::text("Download", Side::Top),
            target,
            cx,
        ));
        div()
            .id(group.clone())
            .group(group)
            .relative()
            .w(px(432.))
            .max_w_full()
            .child(card)
            .child(buttons)
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
                Kind::Image | Kind::Video if self.pref_on("chat_inline_media_uploads", true) => {
                    None
                }
                Kind::Image | Kind::Video => Some(self.file_card(message, attachment, cx)),
                Kind::Plaintext => Some(self.plaintext_card(attachment, cx)),
                Kind::Audio => Some(self.audio_card(message, attachment, cx)),
                Kind::Other => Some(self.file_card(message, attachment, cx)),
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

#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_file_card(&mut self, variant: &str, cx: &mut Context<Self>) -> String {
        if !crate::inert::on() {
            return "file-card requires inert capture".into();
        }
        self.capture_upload_card = None;
        if variant == "off" {
            self.capture_file_card = None;
        } else {
            let (filename, title) = match variant {
                "archive" => ("reference-package.zip", None),
                "unknown" => ("reference-package.apk", None),
                "title" => ("reference-package.tar.gz", Some("Renamed package")),
                _ => return "unknown file-card fixture".into(),
            };
            self.capture_file_card = Message::from_value(&serde_json::json!({
                "id": "100000000000000011", "channel_id": "1568692565136375900",
                "type": 0, "content": "", "timestamp": "2026-11-15T12:00:00Z",
                "author": {"id": "100000000000000001", "username": "Reference"},
                "attachments": [{"id": "100000000000000010", "filename": filename,
                    "title": title, "size": 75497472,
                    "url": "https://example.invalid/reference-package.zip"}],
            }));
            if let Some(message) = &self.capture_file_card {
                for (key, url) in wanted(message) {
                    self.request_image(key, url);
                }
            }
        }
        cx.notify();
        format!("{variant}; local fixture only")
    }

    pub(super) fn capture_file_card_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.capture_file_card.as_ref()?;
        let attachment = message.attachments.first()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .bg(color("--background-scrim"))
                .child(
                    div()
                        .absolute()
                        .left(px(400.))
                        .top(px(240.))
                        .child(self.file_card(message, attachment, cx)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, file_language, format_size, kind, renders_markdown, wanted};

    #[test]
    fn markdown_files_render_until_turned_off() {
        assert_eq!(file_language(None, "notes.md"), "md");
        assert_eq!(file_language(Some("rust"), "notes.md"), "rust");
        assert!(renders_markdown("md", true));
        assert!(renders_markdown("markdown", true));
        assert!(!renders_markdown("md", false));
        assert!(!renders_markdown("txt", true));
    }
    use dc_state::Attachment;

    fn named(name: &str) -> Attachment {
        Attachment {
            id: 1,
            filename: name.to_owned(),
            title: None,
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
            ..Attachment::default()
        }
    }

    #[test]
    fn kinds_follow_the_classifier() {
        assert_eq!(kind(&named("notes.txt")), Kind::Plaintext);
        assert_eq!(kind(&named("main.rs")), Kind::Plaintext);
        assert_eq!(kind(&named("song.MP3")), Kind::Audio);
        assert_eq!(kind(&named("pack.zip")), Kind::Other);

        let mut clip = named("clip.mp4");
        assert_eq!(kind(&clip), Kind::Other);
        clip.proxy_url = "https://cdn.example/clip.mp4".to_owned();
        clip.width = Some(1280);
        clip.height = Some(720);
        assert_eq!(kind(&clip), Kind::Video);
    }

    #[test]
    fn sizes_round_up_in_decimal_units() {
        assert_eq!(format_size(25), "1 KB");
        assert_eq!(format_size(999_999), "1000 KB");
        assert_eq!(format_size(1_500_000), "2 MB");
        assert_eq!(format_size(2_340_000_000), "2.3 GB");
    }

    #[test]
    fn file_art_is_requested_through_the_shared_message_asset_path() {
        let mut message = dc_state::Message::from_value(&serde_json::json!({
            "id": "1", "channel_id": "2", "type": 0, "content": "",
            "timestamp": "2026-11-15T12:00:00Z",
            "author": {"id": "3", "username": "Reference"},
        }))
        .expect("message");
        message.attachments = vec![
            named("archive.zip"),
            named("installer.apk"),
            named("notes.txt"),
        ];
        let assets = wanted(&message);
        assert_eq!(
            assets[0],
            (
                "filekind:archive".to_owned(),
                format!(
                    "{}/assets/f89b66e97ea52ba7.svg",
                    dc_proto::pin::WEBAPP_ORIGIN
                ),
            )
        );
        assert_eq!(assets[1].0, "filekind:unknown");
        assert_eq!(assets[2].0, "text:1");
        message.attachments[0].title = Some("Renamed archive".into());
        assert_eq!(wanted(&message)[0], assets[0]);
    }
}
