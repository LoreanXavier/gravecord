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

/// How full the bar of file `index` is, when file `current` has sent `sent` of `total`: earlier files are done,
/// later ones have not begun.
pub(super) fn fill_of(index: usize, current: usize, sent: u64, total: u64) -> f32 {
    if index < current {
        1.
    } else if index > current || total == 0 {
        0.
    } else {
        (sent as f32 / total as f32).clamp(0., 1.)
    }
}

impl Root {
    /// The cards of a message that is sending or has failed with files, in place of its attachments.
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
        let (current, sent, total) = self.send_ui_progress(message.id).unwrap_or((0, 0, 0));
        let id = message.id;
        let cards: Vec<AnyElement> = send
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                self.sending_upload_card(
                    id,
                    index,
                    file,
                    sending.then(|| fill_of(index, current, sent, total)),
                    cx,
                )
            })
            .collect();
        Some(
            div()
                .mt(px(8.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(cards)
                .into_any_element(),
        )
    }

    fn sending_upload_card(
        &self,
        message: dc_state::Id,
        index: usize,
        file: &Upload,
        fill: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = upload_kind(file);
        let art = self.images.get(&kind_icon_key(kind)).cloned();
        let name = file.filename();
        let body = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .h(px(22.))
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .text_color(color("--text-strong"))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(name),
            )
            .children(fill.map(|fraction| {
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
            .when(fill.is_some(), |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!(
                            "upload-cancel-{message}-{index}"
                        )))
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
    use super::fill_of;

    #[test]
    fn the_bar_follows_the_file_being_sent() {
        assert_eq!(fill_of(0, 1, 5, 10), 1.);
        assert_eq!(fill_of(2, 1, 5, 10), 0.);
        assert_eq!(fill_of(1, 1, 5, 10), 0.5);
        assert_eq!(fill_of(0, 0, 0, 0), 0.);
        assert_eq!(fill_of(0, 0, 20, 10), 1.);
    }
}
