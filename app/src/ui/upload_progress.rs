//! The placeholder of a message whose files are still going up: a file card with the name, a progress bar and the
//! cancel button (`file_-9z-XO` with `progressContainer_-9z-XO`, from `design/dom/upload/uploading-darker-cozy.json`).
//! It is the body of the sending row (`send_state.rs`), not a message of its own, and it has no hover bar. A message
//! that failed with a code shows the same cards without the bar.

use super::uploads::{kind_icon_key, upload_kind};
use super::{Root, color, icon};
use crate::net::Upload;
use dc_state::Message;
use gpui::{AnyElement, Context, IntoElement, SharedString, div, img, prelude::*, px, rgba};

/// The bar inside a card: 324 wide, 8 high.
const BAR_WIDTH: f32 = 324.;

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
        many => format!("Uploading {} Files", many.len()),
    }
}

impl Root {
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
        let card = self.sending_upload_card(
            message.id,
            &names,
            &send.files[0],
            sending.then_some((done, fraction)),
            cx,
        );
        Some(div().mt(px(8.)).child(card).into_any_element())
    }

    fn sending_upload_card(
        &self,
        message: dc_state::Id,
        names: &[String],
        first: &Upload,
        progress: Option<(u64, f32)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The icon follows the label, so several files take the plain one.
        let kind = if names.len() == 1 {
            upload_kind(first)
        } else {
            "unknown"
        };
        let art = self.images.get(&kind_icon_key(kind)).cloned();
        let name = label(names);
        let fill = progress.map(|(_, fraction)| fraction.min(1.));
        // "— 12 KB" after the name while bytes go up; "Processing..." in place of the bar at 100%.
        let size_note = progress
            .filter(|(done, fraction)| *done > 0 && *fraction < 1.)
            .map(|(done, _)| format!("\u{2014} {}", super::files::format_size(done)));
        let processing = progress.is_some_and(|(_, fraction)| fraction >= 1.);
        let body = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .text_color(color("--text-strong"))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name),
                    )
                    .children(size_note.map(|note| {
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(note)
                    })),
            )
            .children(fill.filter(|_| processing).map(|_| {
                div()
                    .h(px(16.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::tr("Processing..."))
            }))
            .children(fill.filter(|_| !processing).map(|fraction| {
                // `progressContainer_-9z-XO`: 16 high, the 8px track 4px down.
                div().h(px(16.)).pt(px(4.)).child(
                    div()
                        .w(px(BAR_WIDTH))
                        .h(px(8.))
                        .rounded(px(4.))
                        .overflow_hidden()
                        .bg(rgba(0x97979f33))
                        .child(
                            div()
                                .h_full()
                                .w(px(BAR_WIDTH * fraction))
                                .rounded(px(4.))
                                .bg(color("--brand-500")),
                        ),
                )
            }));
        div()
            .relative()
            .flex()
            .w(px(432.))
            .p(px(16.))
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .child(
                div()
                    .flex_none()
                    .w(px(30.))
                    .h(px(40.))
                    .children(art.map(|picture| img(picture).w(px(30.)).h(px(40.)))),
            )
            .child(body)
            .when(fill.is_some() && !processing, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("upload-cancel-{message}")))
                        .absolute()
                        .left(px(391.))
                        .top(px(22.))
                        .size(px(24.))
                        .cursor_pointer()
                        .child(icon("modal_close", 24., "--interactive-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_upload(message, cx);
                        })),
                )
            })
            .into_any_element()
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
