//! The popout an emoji in a message opens: module 767143 (`MV` for a standard emoji, `iP` and `ee` for a custom one),
//! its text rules in module 450707 (`O`) and its strings in `design/snapshot/dumps/STRINGS/en-US.json` (the hashes are
//! named beside each use). Measured in `design/dom/popouts/emoji-standard-darker-cozy.json` (288 wide, 16px padding, a
//! 48px picture 4px down, the text column 10px right of it; the popout sits 8px right of the emoji, centred on it).
//!
//! The custom branch follows `O`: the line under the name depends on Nitro, on whether the emoji's server is one the
//! user is in, and on whether it is the open server. The server lookup the official client makes for servers the user
//! is not in (`GET /emojis/{id}/guild`) is not made here, so such an emoji reads as one from an invite-only server.

use super::sticker_popout::{shell, small_button, text};
use super::{Root, color};
use dc_state::Id;
use gpui::{
    AnyElement, Bounds, Context, IntoElement, Pixels, SharedString, anchored, deferred, div, img,
    point, prelude::*, px,
};

/// `popoutContainer` padding, shared with the sticker popout.
const PADDING: f32 = 16.;
/// The gap `Popout` leaves between the popout and the emoji.
const GAP: f32 = 8.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A unicode emoji, by its characters.
    Standard(String),
    Custom {
        id: Id,
        name: String,
    },
}

pub(super) struct EmojiPopout {
    kind: Kind,
    anchor: Bounds<Pixels>,
}

/// What `O` knows when it writes the line under the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Facts {
    pub premium: bool,
    pub joined: bool,
    pub same_guild: bool,
    pub discoverable: bool,
}

/// The line under the emoji's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    /// `hU4kIe`.
    ThisServerEverywhere,
    /// `GM0xaX`.
    YourServerTypeName,
    /// `xE9WGt`.
    JoinToUseEverywhere,
    /// `0LMpW+`.
    InviteOnly,
    /// `ICPhqa`.
    ThisServerNitro,
    /// `jQy3aM`.
    YourServerNitro,
    /// `FJ6Z01`.
    NitroAndJoin,
    /// `U6vLcA`, with its "Learn more" link.
    InviteOnlyLearn,
}

impl Line {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::ThisServerEverywhere => {
                "This emoji is from this server. You can use it everywhere."
            }
            Self::YourServerTypeName => {
                "This emoji is from one of your servers. Type its name in the chat bar to use it."
            }
            Self::JoinToUseEverywhere => "Want to use this emoji everywhere? Join the server.",
            Self::InviteOnly => {
                "This emoji is from a server that is either invite-only or unavailable."
            }
            Self::ThisServerNitro => {
                "This emoji is from this server. Get Nitro to use it everywhere."
            }
            Self::YourServerNitro => {
                "This emoji is from one of your servers. Get Nitro to use it everywhere."
            }
            Self::NitroAndJoin => {
                "Want to use this emoji everywhere? Get Nitro and join the server."
            }
            Self::InviteOnlyLearn => {
                "This emoji is from a server that is either invite-only or unavailable. "
            }
        }
    }
}

/// The button under the text (`O`'s `type`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cta {
    None,
    /// `riu2R5`.
    JoinServer,
    /// `gl/XHJ`.
    GetNitro,
}

/// `O`: the line and the button for a custom emoji.
pub(super) fn branch(facts: Facts) -> (Line, Cta) {
    let Facts {
        premium,
        joined,
        same_guild,
        discoverable,
    } = facts;
    let line = match (premium, joined, same_guild, discoverable) {
        (true, true, true, _) => Line::ThisServerEverywhere,
        (true, true, false, _) => Line::YourServerTypeName,
        (true, false, _, true) => Line::JoinToUseEverywhere,
        (true, false, _, false) => Line::InviteOnly,
        (false, true, true, _) => Line::ThisServerNitro,
        (false, true, false, _) => Line::YourServerNitro,
        (false, false, _, true) => Line::NitroAndJoin,
        (false, false, _, false) => Line::InviteOnlyLearn,
    };
    let cta = if premium && !joined && discoverable {
        Cta::JoinServer
    } else if !premium && (joined || discoverable) {
        Cta::GetNitro
    } else {
        Cta::None
    };
    (line, cta)
}

/// Where the popout's left centre goes: beside the emoji, level with its middle.
pub(super) fn popout_position(anchor: Bounds<Pixels>) -> gpui::Point<Pixels> {
    point(
        anchor.right() + px(GAP),
        anchor.top() + anchor.size.height / 2.,
    )
}

impl Root {
    pub(super) fn open_emoji_popout(
        &mut self,
        kind: Kind,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tip = None;
        self.tip_pending = None;
        self.emoji_popout = Some(EmojiPopout { kind, anchor });
        cx.notify();
    }

    pub(super) fn close_emoji_popout(&mut self, cx: &mut Context<Self>) {
        if self.emoji_popout.take().is_some() {
            cx.notify();
        }
    }

    /// The picture at 48px (`size: "jumbo"`), from the images the message list already loaded.
    fn emoji_picture(&self, kind: &Kind) -> AnyElement {
        let key = match kind {
            Kind::Standard(glyph) => format!("emoji:{glyph}"),
            Kind::Custom { id, .. } => format!("custom:{id}"),
        };
        div()
            .size(px(48.))
            .mt(px(4.))
            .flex_none()
            .children(
                self.images
                    .get(&key)
                    .cloned()
                    .map(|image| img(image).size(px(48.))),
            )
            .into_any_element()
    }

    fn emoji_header(&self, kind: &Kind, name: String, description: AnyElement) -> AnyElement {
        div()
            .flex()
            .child(self.emoji_picture(kind))
            .child(
                div()
                    .ml(px(10.))
                    .min_w(px(0.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(text(16., 20., 600., "--text-default").child(SharedString::from(name)))
                    .child(description),
            )
            .into_any_element()
    }

    pub(super) fn emoji_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.emoji_popout.as_ref()?;
        let content = match &open.kind {
            Kind::Standard(glyph) => {
                let name = super::reaction_tip::emoji_label(glyph, false);
                shell()
                    .child(div().p(px(PADDING)).child(self.emoji_header(
                        &open.kind,
                        name,
                        // `sXdH8c`.
                        text(14., 18., 400., "--text-default")
                            .child("A default emoji. You can use this emoji everywhere on Discord.")
                            .into_any_element(),
                    )))
                    .into_any_element()
            }
            Kind::Custom { id, name } => self.custom_emoji_card(&open.kind, *id, name, cx),
        };
        Some(
            deferred(
                div()
                    .id("emoji-popout-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close_emoji_popout(cx)),
                    )
                    .child(
                        anchored()
                            .anchor(gpui::Anchor::LeftCenter)
                            .position(popout_position(open.anchor))
                            .snap_to_window_with_margin(px(8.))
                            .child(content),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `ee`: the emoji, its line, the button, and for a server the user is in the card of that server.
    fn custom_emoji_card(
        &self,
        kind: &Kind,
        id: Id,
        name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.emojis.iter().any(|emoji| emoji.id == id));
        let facts = Facts {
            premium: self
                .premium()
                .has(dc_state::premium::Feature::EmojisEverywhere),
            joined: source.is_some(),
            same_guild: source.is_some_and(|guild| Some(guild.id) == self.guild),
            discoverable: source.is_some_and(|guild| {
                guild
                    .features
                    .iter()
                    .any(|feature| feature == "DISCOVERABLE")
            }),
        };
        let (line, cta) = branch(facts);
        let mut top = div().p(px(PADDING)).flex().flex_col().child(
            self.emoji_header(
                kind,
                name.to_owned(),
                text(14., 18., 400., "--text-default")
                    .child(line.text())
                    .into_any_element(),
            ),
        );
        if cta == Cta::GetNitro {
            top = top.child(div().mt(px(PADDING)).child(
                small_button("emoji-popout-nitro", "Get Nitro", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.close_emoji_popout(cx);
                        this.open_settings_page("nitro", cx);
                    },
                )),
            ));
        }
        let section = source.map(|guild| {
            let icon = self.images.get(&format!("guild:{}", guild.id)).cloned();
            let acronym = dc_state::Guild::acronym_of(&guild.name);
            div()
                .p(px(PADDING))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .child(
                    // `ohTzZH`.
                    text(14., 18., 400., "--text-default")
                        .mb(px(8.))
                        .child("This emoji is from"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .size(px(32.))
                                .flex_none()
                                .rounded(px(10.))
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(guild.icon.is_none() || icon.is_none(), |this| {
                                    this.bg(color("--background-mod-muted"))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(acronym.clone())
                                })
                                .children(
                                    icon.filter(|_| guild.icon.is_some())
                                        .map(|picture| img(picture).size(px(32.))),
                                ),
                        )
                        .child(
                            div()
                                .ml(px(10.))
                                .min_w(px(0.))
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(
                                    text(16., 20., 600., "--text-default")
                                        .truncate()
                                        .child(SharedString::from(guild.name.clone())),
                                )
                                // `H29mx4`: the presence count of a discoverable server is not fetched.
                                .child(
                                    text(12., 16., 400., "--text-default")
                                        .child("Invite-only server"),
                                ),
                        ),
                )
        });
        shell().child(top).children(section).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(premium: bool, joined: bool, same_guild: bool, discoverable: bool) -> Facts {
        Facts {
            premium,
            joined,
            same_guild,
            discoverable,
        }
    }

    #[test]
    fn a_nitro_user_reads_where_a_joined_emoji_is_from() {
        assert_eq!(
            branch(facts(true, true, true, false)),
            (Line::ThisServerEverywhere, Cta::None)
        );
        assert_eq!(
            branch(facts(true, true, false, false)),
            (Line::YourServerTypeName, Cta::None)
        );
    }

    #[test]
    fn a_stranger_is_offered_the_join_button_only_when_the_server_is_discoverable() {
        assert_eq!(
            branch(facts(true, false, false, true)),
            (Line::JoinToUseEverywhere, Cta::JoinServer)
        );
        assert_eq!(
            branch(facts(true, false, false, false)),
            (Line::InviteOnly, Cta::None)
        );
    }

    #[test]
    fn without_nitro_the_upsell_replaces_join() {
        assert_eq!(
            branch(facts(false, true, true, false)),
            (Line::ThisServerNitro, Cta::GetNitro)
        );
        assert_eq!(
            branch(facts(false, true, false, false)),
            (Line::YourServerNitro, Cta::GetNitro)
        );
        assert_eq!(
            branch(facts(false, false, false, true)),
            (Line::NitroAndJoin, Cta::GetNitro)
        );
        assert_eq!(
            branch(facts(false, false, false, false)),
            (Line::InviteOnlyLearn, Cta::None)
        );
    }

    #[test]
    fn the_popout_opens_beside_the_emoji_level_with_its_middle() {
        // The golden: the emoji's right edge at 505.5 and middle at 376, the popout's left edge at 513.5.
        let anchor = Bounds::new(point(px(483.5), px(365.)), gpui::size(px(22.), px(22.)));
        let at = popout_position(anchor);
        assert_eq!(f32::from(at.x), 513.5);
        assert_eq!(f32::from(at.y), 376.);
    }
}
