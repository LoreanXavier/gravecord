//! Direct messages: the welcome at the start of the history (`container_w6h4R2`) and system messages
//! (`container_bLMKT5`). Measured in `design/dom/dm/loreanxavier-darker-cozy.json`.

use super::{Root, avatar_source, color, header_time, icon};
use dc_state::Message;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// `FRIEND_REQUEST_ACCEPTED` in the message type enum.
pub(super) const FRIEND_REQUEST_ACCEPTED: u64 = 67;
/// `CHANNEL_PINNED_MESSAGE`.
pub(super) const CHANNEL_PINNED_MESSAGE: u64 = 6;

/// System messages this client draws; the rest still render as ordinary messages.
pub(super) fn is_system(message: &Message) -> bool {
    matches!(
        message.kind,
        FRIEND_REQUEST_ACCEPTED | CHANNEL_PINNED_MESSAGE
    )
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
    /// `yIDvPL`: "{name} pinned **a message** to this channel. See all **pinned messages**.", each part a link:
    /// the name to the profile, the message to jump to, the pins popout.
    fn pin_notice(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let link = |id: String, text: String| {
            div()
                .id(SharedString::from(id))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .cursor_pointer()
                .child(text)
        };
        let target = message.reference_id;
        let channel = message.channel_id;
        div()
            .py(px(2.))
            .flex()
            .flex_wrap()
            .items_center()
            .line_height(px(22.))
            .text_color(color("--channels-default"))
            .child(
                self.profile_trigger(
                    format!("pin-notice-name-{}", message.id),
                    message.author.id,
                    div()
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(message.author.display_name().to_owned()),
                    cx,
                ),
            )
            .child("\u{a0}pinned\u{a0}")
            .child(
                link(
                    format!("pin-notice-message-{}", message.id),
                    "a message".to_owned(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let found = target.and_then(|target| {
                        this.stores
                            .messages
                            .get(&channel)
                            .and_then(|list| list.iter().find(|known| known.id == target))
                            .cloned()
                    });
                    if let Some(found) = found {
                        this.jump_to_message(&found, cx);
                    }
                })),
            )
            .child("\u{a0}to this channel. See all\u{a0}")
            .child(
                link(
                    format!("pin-notice-pins-{}", message.id),
                    "pinned messages".to_owned(),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    let key = if this.current_dm().is_some() {
                        "dm-pins"
                    } else {
                        "pins"
                    };
                    if this.pins_open.is_none() {
                        this.toggle_pins(key, cx);
                    }
                })),
            )
            .child(".")
            .child(
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .child(header_time(message.local_time(), chrono::Local::now())),
            )
            .into_any_element()
    }

    pub(super) fn system_message(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let (icon_name, tail, tint) = match message.kind {
            FRIEND_REQUEST_ACCEPTED => (
                "system_friend_accepted",
                " accepted your friend request.",
                "--status-positive",
            ),
            CHANNEL_PINNED_MESSAGE => ("system_pinned", "", "--channels-default"),
            _ => ("system_friend_accepted", "", "--status-positive"),
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
                            .child(icon(icon_name, 18., tint)),
                    )
                    .when(message.kind == CHANNEL_PINNED_MESSAGE, |this| {
                        this.child(self.pin_notice(message, cx))
                    })
                    .when(message.kind != CHANNEL_PINNED_MESSAGE, |this| {
                        this.child(
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
                                        .child(header_time(
                                            message.local_time(),
                                            chrono::Local::now(),
                                        )),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}
