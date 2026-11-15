//! The placeholder of a message whose files are still going up: a file card with the name, a progress bar and the
//! cancel button (`file_-9z-XO` with `progressContainer_-9z-XO`, from `design/dom/upload/uploading-darker-cozy.json`).
//! It is the body of the sending row (`send_state.rs`), not a message of its own, and it has no hover bar. A message
//! that failed with a code shows the same cards without the bar.

use super::motion::{self, Ease, css};
use super::uploads::{file_kind, kind_icon_key};
use super::{Root, color, theme};
use crate::net::Upload;
use crate::parity::Parity;
use dc_state::Message;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, SharedString, canvas, div, img, point,
    prelude::*, px, relative, size, svg,
};

/// What the one card of an upload shows (`I` in module 905394): the bytes gone up over all files so far, and the
/// share of the total they make. File `current` has sent `sent`; earlier files are whole, later ones have not begun.
pub(super) fn overall(sizes: &[u64], current: usize, sent: u64) -> (u64, f32) {
    let total: u64 = sizes.iter().sum();
    let done: u64 = sizes.iter().take(current).sum::<u64>()
        + sent.min(sizes.get(current).copied().unwrap_or(sent));
    if total == 0 {
        return (done, 0.);
    }
    (done, (done as f32 / total as f32).clamp(0., 1.))
}

/// The card's label: the file's name for one file, "Uploading N Files" for more.
pub(super) fn label(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        many => dc_intl::t!("D0noUt", count = many.len().to_string()),
    }
}

fn upload_content(caption: Option<AnyElement>, card: AnyElement) -> AnyElement {
    div()
        .children(caption.map(|caption| {
            div()
                .parity("upload-file-caption")
                .mb(px(2.))
                .opacity(0.5)
                .child(caption)
        }))
        .child(card)
        .into_any_element()
}

impl Root {
    /// Pending uploads are custom message content (891522), outside ordinary attachment spacing.
    pub(super) fn upload_message_content(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.send_ui_send(message.id)?.state != super::send_state::SendState::Sending {
            return None;
        }
        let card = self.upload_placeholder(message, cx)?;
        let caption =
            (!message.content.trim().is_empty()).then(|| self.content(message).into_any_element());
        Some(upload_content(caption, card))
    }

    /// The card of a message that is sending or has failed with files, in place of its attachments: one card for the
    /// whole upload, as the official `UploadProgress` draws it.
    pub(super) fn upload_placeholder(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let send = self.send_ui_send(message.id)?;
        if send.files.is_empty() {
            return None;
        }
        let sending = send.state == super::send_state::SendState::Sending;
        let (current, sent, _) = self.send_ui_progress(message.id).unwrap_or((0, 0, 0));
        let sizes: Vec<u64> = send
            .files
            .iter()
            .map(|file| std::fs::metadata(&file.path).map_or(0, |meta| meta.len()))
            .collect();
        let names: Vec<String> = send.files.iter().map(Upload::filename).collect();
        let (done, fraction) = overall(&sizes, current, sent);
        let card =
            self.sending_upload_card(message.id, &names, sending.then_some((done, fraction)), cx);
        Some(
            div()
                .when(!sending, |this| this.mt(px(8.)))
                .child(card)
                .into_any_element(),
        )
    }

    pub(super) fn sending_upload_card(
        &self,
        message: dc_state::Id,
        names: &[String],
        progress: Option<(u64, f32)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The icon follows the label, so several files take the plain one.
        let name = label(names);
        let kind = file_kind(&name, None);
        let art = self.images.get(&kind_icon_key(kind)).cloned();
        let fill = progress.map(|(_, fraction)| fraction.min(1.));
        // "— 12 KB" after the name while bytes go up; "Processing..." in place of the bar at 100%.
        let size_note = progress
            .filter(|(done, fraction)| *done > 0 && *fraction < 1.)
            .map(|(done, _)| format!("\u{2014} {}", super::components::humanize_filesize(done)));
        let processing = progress.is_some_and(|(_, fraction)| fraction >= 1.);
        let shown = fill.filter(|_| !processing).map(|fraction| {
            motion::value(
                motion::key(&("upload-progress", message)),
                fraction,
                css(200., Ease::EaseOut),
            )
        });
        let body = div()
            .parity("upload-file-inner")
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .parity("upload-file-name-row")
                    .h(px(22.))
                    .flex()
                    .items_end()
                    .gap(px(4.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(
                                if theme() == dc_tokens::Theme::Light && self.ui_scale.get() <= 1. {
                                    FontWeight::MEDIUM
                                } else {
                                    FontWeight::NORMAL
                                },
                            )
                            .text_color(color("--interactive-text-active"))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name),
                    )
                    .children(size_note.map(|note| {
                        div()
                            .flex_none()
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(color("--text-muted"))
                            .child(note)
                    })),
            )
            .children(fill.filter(|_| processing).map(|_| {
                div()
                    .parity("upload-file-progress")
                    .mr(px(8.))
                    .h(px(16.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .font_weight(FontWeight::NORMAL)
                    .child(dc_intl::t!("jfKTes"))
            }))
            .children(shown.map(|fraction| {
                // `progressContainer_-9z-XO`: 16 high, the 8px track 4px down.
                div()
                    .parity("upload-file-progress")
                    .h(px(16.))
                    .mr(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .parity("upload-file-track")
                            .relative()
                            .w_full()
                            .h(px(8.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--progressbar-track-background"))
                            .child(
                                div()
                                    .parity("upload-file-fill")
                                    .absolute()
                                    .left(relative(fraction - 1.))
                                    .h_full()
                                    .w_full()
                                    .child(canvas(
                                        |_, _, _| (),
                                        move |bounds, (), window, _| {
                                            // GPUI's content mask is rectangular. Paint the intersection with the
                                            // rounded track, retaining the translated element's layout bounds.
                                            if fraction > 0. {
                                                let width = bounds.size.width * fraction;
                                                let visible = Bounds::new(
                                                    point(bounds.origin.x + bounds.size.width - width, bounds.origin.y),
                                                    size(width, bounds.size.height),
                                                );
                                                window.paint_quad(gpui::fill(visible, color("--progressbar-indicator-background"))
                                                    .corner_radii(px(4.)));
                                            }
                                        },
                                    ).size_full()),
                            ),
                    )
            }));
        div()
            .parity_with_border("upload-file-card", 1.)
            .relative()
            .flex()
            .w(px(432.))
            .max_w_full()
            .p(px(16.))
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![super::hover_bar::popover_shadow(0.)])
            .child(
                div()
                    .parity("upload-file-icon")
                    .flex_none()
                    .w(px(30.))
                    .h(px(40.))
                    .mr(px(8.))
                    .children(art.map(|picture| img(picture).w(px(30.)).h(px(40.)))),
            )
            .child(body)
            .when(fill.is_some() && !processing, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("upload-cancel-{message}")))
                        .flex_none()
                        .w(px(28.))
                        .h(px(30.))
                        .pl(px(4.))
                        .cursor_pointer()
                        .text_color(color("--interactive-text-default"))
                        .hover(|style| style.text_color(color("--interactive-text-hover")))
                        .child(
                            div()
                                .parity("upload-file-cancel")
                                .size(px(24.))
                                .child(svg().path("icons/x_small.svg").size_full()),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_upload(message, cx);
                        })),
                )
            })
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn script_upload_card(&mut self, value: &str, cx: &mut Context<Self>) -> String {
        if !crate::inert::on() {
            return "upload-card requires inert capture".into();
        }
        if value == "off" {
            self.capture_upload_card = None;
        } else {
            let (number, caption) = value
                .strip_suffix("-caption")
                .map_or((value, false), |number| (number, true));
            let Ok(percent) = number.parse::<f32>() else {
                return "upload-card expects0..100".into();
            };
            if !percent.is_finite() || !(0. ..=100.).contains(&percent) {
                return "upload-card expects0..100".into();
            }
            self.capture_file_card = None;
            self.capture_upload_card = Some((
                (75_497_472. * f64::from(percent) / 100.) as u64,
                percent / 100.,
                caption,
            ));
            self.request_image(
                kind_icon_key("archive"),
                super::uploads::kind_icon_url("archive"),
            );
        }
        cx.notify();
        format!("{value}; local fixture only")
    }

    pub(super) fn capture_upload_card_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (done, fraction, caption) = self.capture_upload_card?;
        let card = self.sending_upload_card(
            100000000000000012,
            &["reference-package.zip".into()],
            Some((done, fraction)),
            cx,
        );
        let caption = caption.then(|| {
            div()
                .text_size(px(16.))
                .line_height(px(22.))
                .font_weight(FontWeight::NORMAL)
                .text_color(color("--text-default"))
                .child("A caption above the file")
                .into_any_element()
        });
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
                        .child(upload_content(caption, card)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{label, overall};

    #[test]
    fn the_bar_follows_the_whole_upload() {
        assert_eq!(overall(&[10, 10], 0, 5), (5, 0.25));
        assert_eq!(overall(&[10, 10], 1, 5), (15, 0.75));
        assert_eq!(overall(&[10], 0, 20), (10, 1.));
        assert_eq!(overall(&[0], 0, 0), (0, 0.));
    }

    #[test]
    fn one_file_goes_by_name_and_more_by_count() {
        assert_eq!(label(&["a.txt".to_owned()]), "a.txt");
        assert_eq!(
            label(&["a".to_owned(), "b".to_owned()]),
            "Uploading 2 Files"
        );
    }
}
