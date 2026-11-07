//! Direct messages: the welcome at the start of the history (`container_w6h4R2`) and system messages
//! (`container_bLMKT5`). Measured in `design/dom/dm/loreanxavier-darker-cozy.json`.

use super::{Root, avatar_source, color, header_time, icon};
use dc_state::Message;
use gpui::{AnyElement, FontWeight, IntoElement, div, img, prelude::*, px};

/// `FRIEND_REQUEST_ACCEPTED` in the message type enum.
pub(super) const FRIEND_REQUEST_ACCEPTED: u64 = 67;

/// System messages this client draws; the rest still render as ordinary messages.
pub(super) fn is_system(message: &Message) -> bool {
    message.kind == FRIEND_REQUEST_ACCEPTED
}

/// A `sm` secondary button: 32 tall, 11px inside a 1px border, 14px medium text.
fn small_button(label: &'static str) -> impl IntoElement {
    div()
        .h(px(32.))
        .px(px(11.))
        .rounded(px(8.))
        .border_1()
        .border_color(color("--control-secondary-border-default"))
        .bg(color("--control-secondary-background-default"))
        .flex()
        .items_center()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--control-secondary-text-default"))
        .child(label)
}

impl Root {
    /// The start of a DM: the other person large, the opening line, mutual servers and the friend buttons.
    pub(super) fn dm_welcome(&self) -> Option<AnyElement> {
        let channel = self.current_dm()?;
        let user = self.stores.dm_recipient(channel)?.clone();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let mutual = self
            .fetched
            .get(&format!("profile:{}", user.id))
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        let friend = self
            .stores
            .relationships
            .get(&user.id)
            .is_some_and(|kind| *kind == 1);
        let first_mutual = self
            .fetched
            .get(&format!("profile:{}", user.id))
            .and_then(|body| {
                body["mutual_guilds"][0]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok())
            })
            .and_then(|guild| self.images.get(&format!("guild:{guild}")).cloned());
        let name = user.display_name().to_owned();
        Some(
            div()
                .mt(px(16.))
                .ml(px(16.))
                .mr(px(16.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .size(px(80.))
                        .rounded_full()
                        .children(avatar.map(|picture| img(picture).size(px(80.)).rounded_full())),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text_size(px(32.))
                        .line_height(px(40.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(name.clone()),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text_size(px(24.))
                        .line_height(px(32.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(user.username.clone()),
                )
                .child(
                    div()
                        .mt(px(19.))
                        .flex()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child("This is the beginning of your direct message history with\u{a0}")
                        .child(div().font_weight(FontWeight(600.)).child(name))
                        .child("."),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .when(mutual > 0, |this| {
                            this.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(24.))
                                            .rounded(px(8.))
                                            .overflow_hidden()
                                            .children(
                                                first_mutual
                                                    .map(|picture| img(picture).size(px(24.))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-subtle"))
                                            .child(if mutual == 1 {
                                                "1 Mutual Server".to_owned()
                                            } else {
                                                format!("{mutual} Mutual Servers")
                                            }),
                                    )
                                    // The dot between the servers and the buttons.
                                    .child(
                                        div()
                                            .mx(px(4.))
                                            .size(px(4.))
                                            .rounded_full()
                                            .bg(color("--background-mod-strong")),
                                    ),
                            )
                        })
                        .when(friend, |this| {
                            this.child(
                                div()
                                    .ml(px(4.))
                                    .flex()
                                    .gap(px(8.))
                                    .child(small_button("Remove Friend"))
                                    .child(small_button("Block")),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// A system line: an 18px icon centred in the 72px gutter, then who and what, and the time.
    pub(super) fn system_message(&self, message: &Message) -> AnyElement {
        let (icon_name, tail) = match message.kind {
            FRIEND_REQUEST_ACCEPTED => ("system_friend_accepted", " accepted your friend request."),
            _ => ("system_friend_accepted", ""),
        };
        div()
            .pt(px(17.))
            .pr(px(16.))
            .child(
                div()
                    .relative()
                    .pl(px(72.))
                    .pr(px(24.))
                    .py(px(2.))
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(4.))
                            .w(px(72.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(icon_name, 18., "--status-positive")),
                    )
                    .child(
                        div()
                            .py(px(2.))
                            .flex()
                            .items_center()
                            .line_height(px(22.))
                            .text_color(color("--channels-default"))
                            .child(
                                div()
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(message.author.display_name().to_owned()),
                            )
                            .child(tail)
                            .child(
                                div()
                                    .ml(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .child(header_time(message.local_time(), chrono::Local::now())),
                            ),
                    ),
            )
            .into_any_element()
    }
}
