//! The popout an emoji in a message opens: module 767143 (`MV` for a standard emoji, `iP` and `ee` for a custom one),
//! its text rules in module 450707 (`O`) and its strings in `design/snapshot/dumps/STRINGS/en-US.json` (the hashes are
//! named beside each use). Measured in `design/dom/popouts/emoji-standard-darker-cozy.json` (288 wide, 16px padding, a
//! 48px picture 4px down, the text column 10px right of it; the popout sits 8px right of the emoji, centred on it).
//!
//! The custom branch follows `O`: the line under the name depends on Nitro, on whether the emoji's server is one the
//! user is in, and on whether it is the open server. For a server the user is not in (or a discoverable one) the
//! popout looks the emoji's source up first (`GET /emojis/{id}/source`, `g_` in module 624793, answered with
//! `{type: GUILD | APPLICATION | PACK, ...}`), shows the loader meanwhile, and offers "Join Server" or "Get Nitro".

use super::sticker_popout::{shell, small_button, text};
use super::tooltip::{Body, Side, Tip};
use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, SharedString, anchored, deferred,
    div, img, point, prelude::*, px,
};

/// `otherEmoji_Gl3WZ2`: 32px tiles.
const OTHER: f32 = 32.;

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
    /// `eN` in `ee`: the other emoji of the server are listed.
    show_more: bool,
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
        let custom = matches!(kind, Kind::Custom { .. });
        self.emoji_popout = Some(EmojiPopout {
            kind,
            anchor,
            show_more: false,
        });
        self.emoji_popout_requests();
        cx.notify();
        if custom {
            // The answers land in `fetched`; this redraws and asks for what each answer names, until the popout closes.
            cx.spawn(async move |this, cx| {
                for _ in 0..40 {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(250))
                        .await;
                    let alive = this
                        .update(cx, |this, cx| {
                            let alive = this.emoji_popout.is_some();
                            if alive {
                                this.emoji_popout_requests();
                                cx.notify();
                            }
                            alive
                        })
                        .unwrap_or(false);
                    if !alive {
                        break;
                    }
                }
            })
            .detach();
        }
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
            Kind::Custom { id, name } => self.custom_emoji_card(open, &open.kind, *id, name, cx),
        };
        Some(
            deferred(
                anchored()
                    .anchor(gpui::Anchor::LeftCenter)
                    .position(popout_position(open.anchor))
                    .snap_to_window_with_margin(px(8.))
                    .child(super::keyboard::outside(
                        div().id("emoji-popout-card").occlude().child(content),
                        super::keyboard::Layer::EmojiPopout,
                        cx,
                    )),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `ee`: the emoji, its line, the button, and the card of the server it comes from.
    fn custom_emoji_card(
        &self,
        open: &EmojiPopout,
        kind: &Kind,
        id: Id,
        name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let joined_guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.emojis.iter().any(|emoji| emoji.id == id));
        let joined = joined_guild.is_some();
        let key = source_key(id);
        // Module 695282: the lookup runs for an emoji whose server the user is not in, or one that is discoverable.
        let needs_lookup = joined_guild.is_none_or(|guild| is_discoverable(&guild.features));
        let looked_up = self.fetched.contains_key(&key) || self.fetch_failures.contains_key(&key);
        if needs_lookup && !looked_up {
            return super::sticker_popout::loader();
        }
        let found = self.fetched.get(&key).and_then(Source::from_value);
        let guild: Option<SourceGuild> = match &found {
            Some(Source::Guild(guild)) => Some(guild.clone()),
            Some(_) => None,
            None => joined_guild.map(|guild| SourceGuild {
                id: guild.id,
                name: guild.name.clone(),
                icon: guild.icon.clone(),
                discoverable: is_discoverable(&guild.features),
                online: None,
                emojis: Vec::new(),
            }),
        };
        let facts = Facts {
            premium: self
                .premium()
                .has(dc_state::premium::Feature::EmojisEverywhere),
            joined,
            same_guild: self.guild.is_some()
                && (guild.as_ref().map(|guild| guild.id) == self.guild
                    || joined_guild.map(|guild| guild.id) == self.guild),
            discoverable: guild.as_ref().is_some_and(|guild| guild.discoverable),
        };
        let (line, cta) = branch(facts);
        let is_pack = matches!(found, Some(Source::Pack));
        let description: AnyElement = match &found {
            // `uERlTd`.
            Some(Source::Application(app)) => text(14., 18., 400., "--text-default")
                .child(SharedString::from(format!(
                    "This emoji is from the {app} app"
                )))
                .into_any_element(),
            // Module 137133: Nitro Emoji Pack popout uses its dedicated preview section below.
            Some(Source::Pack) => text(14., 18., 400., "--text-default")
                .child(dc_intl::t("leUgZw"))
                .into_any_element(),
            _ => text(14., 18., 400., "--text-default")
                .child(line.text())
                .into_any_element(),
        };
        let mut top = div()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .child(self.emoji_header(kind, name.to_owned(), description));
        if is_pack {
            // Module 137133 (`v`, CSS `23236`): divider + Nitro wheel + "Nitro exclusive emoji" +
            // BETA pill + 32x32 preview strip + "View Set" (`DtK5IG`) / "Get Nitro" (`gl/XHJ`).
            let is_nitro_tier2 = self.stores.account.premium_type == Some(2);
            let preview_glyphs = ["✨", "🔥", "🎉", "💜", "⚡", "🌟"];
            top = top
                .child(
                    div()
                        .my(px(16.))
                        .h(px(1.))
                        .w_full()
                        .bg(color("--border-subtle")),
                )
                .child(
                    div()
                        .mb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(super::icon("nitro_wheel", 16., "--text-default"))
                                .child(
                                    text(14., 18., 500., "--text-default")
                                        .child(dc_intl::t("leUgZw")),
                                ),
                        )
                        .child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(10.))
                                .bg(color("--background-brand"))
                                .text_size(px(10.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child("BETA"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(4.))
                        .children(preview_glyphs.into_iter().map(|g| {
                            div()
                                .size(px(32.))
                                .rounded(px(6.))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(18.))
                                .child(g)
                        })),
                )
                .child(div().mt(px(16.)).child(if is_nitro_tier2 {
                    small_button("emoji-popout-view-set", dc_intl::t("DtK5IG"), true).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.close_emoji_popout(cx);
                            this.open_picker(
                                super::picker::Target::Composer,
                                gpui::point(px(640.), px(640.)),
                                window,
                                cx,
                            );
                            if let Some(p) = this.picker.as_mut() {
                                p.show_emoji_packs = true;
                            }
                            cx.notify();
                        }),
                    )
                } else {
                    small_button("emoji-popout-pack-nitro", dc_intl::t("gl/XHJ"), true).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.close_emoji_popout(cx);
                            this.open_settings_page("nitro", cx);
                        }),
                    )
                }));
        } else if cta == Cta::GetNitro {
            top = top.child(div().mt(px(PADDING)).child(
                small_button("emoji-popout-nitro", "Get Nitro", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.close_emoji_popout(cx);
                        this.open_settings_page("nitro", cx);
                    },
                )),
            ));
        }
        if cta == Cta::JoinServer
            && let Some(guild) = guild.as_ref()
        {
            let target = guild.id;
            top = top.child(div().mt(px(PADDING)).child(
                small_button("emoji-popout-join", "Join Server", true).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.close_emoji_popout(cx);
                        this.join_emoji_guild(target);
                    },
                )),
            ));
        }
        let section = guild.as_ref().map(|guild| {
            self.emoji_source_section(open, guild, joined, cta == Cta::JoinServer, id, cx)
        });
        shell().child(top).children(section).into_any_element()
    }

    /// The darker section of `ee`: "This emoji is from" or "Get this emoji from" and the server's card `Q`, and for a
    /// server the user is not in, the toggle that lists its other emoji.
    fn emoji_source_section(
        &self,
        open: &EmojiPopout,
        guild: &SourceGuild,
        joined: bool,
        join_button: bool,
        emoji: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let target = guild.id;
        let icon = self.images.get(&format!("guild:{target}")).cloned();
        let acronym = dc_state::Guild::acronym_of(&guild.name);
        // `Q`: the picture and the name are buttons when the user is in the server or it is discoverable.
        let clickable = joined || guild.discoverable;
        let go = move |this: &mut Self, cx: &mut Context<Self>| {
            this.close_emoji_popout(cx);
            if joined {
                this.select(target, None);
                cx.notify();
            } else {
                this.join_emoji_guild(target);
            }
        };
        let second: AnyElement = match guild.online.filter(|_| guild.discoverable) {
            Some(count) => {
                let row = div()
                    .flex()
                    .items_center()
                    // `LC+S+m`.
                    .child(text(12., 16., 400., "--text-default").child(format!("{count} Online")))
                    .child(
                        div()
                            .flex_none()
                            .size(px(4.))
                            .mx(px(8.))
                            .rounded_full()
                            .bg(color("--background-mod-muted")),
                    );
                if !join_button && !joined {
                    row.child(
                        div()
                            .id("emoji-popout-guild-join")
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            // `riu2R5`.
                            .child(text(12., 16., 400., "--text-link").child("Join Server"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_emoji_popout(cx);
                                this.join_emoji_guild(target);
                            })),
                    )
                } else {
                    // `inyJqO`.
                    row.child(text(12., 16., 400., "--text-default").child("Discoverable Server"))
                }
                .into_any_element()
            }
            // `H29mx4`.
            None => text(12., 16., 400., "--text-default")
                .child("Invite-only server")
                .into_any_element(),
        };
        let picture = div()
            .id("emoji-popout-guild-icon")
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
            )
            .when(clickable, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| go(this, cx)))
            });
        let name = text(16., 20., 600., "--text-default")
            .truncate()
            .child(SharedString::from(guild.name.clone()));
        let name: AnyElement = if clickable {
            div()
                .id("emoji-popout-guild-name")
                .cursor_pointer()
                .min_w(px(0.))
                .hover(|style| style.underline())
                .child(name)
                .on_click(cx.listener(move |this, _, _, cx| go(this, cx)))
                .into_any_element()
        } else {
            name.into_any_element()
        };
        let card = div().flex().items_center().child(picture).child(
            div()
                .ml(px(10.))
                .min_w(px(0.))
                .flex_1()
                .flex()
                .flex_col()
                .child(name)
                .child(second),
        );
        let mut section = div()
            .p(px(PADDING))
            .bg(color("--background-base-lower"))
            .flex()
            .flex_col()
            .child(
                text(14., 18., 400., "--text-default")
                    .mb(px(8.))
                    // `ohTzZH` and `eLfh+a`.
                    .child(if joined {
                        "This emoji is from"
                    } else {
                        "Get this emoji from"
                    }),
            )
            .child(card);
        if !joined && guild.discoverable && guild.emojis.len() > 1 {
            let expanded = open.show_more;
            section = section.child(
                div()
                    .id("emoji-popout-show-more")
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_color(color("--interactive-text-default"))
                    .hover(|style| style.text_color(color("--interactive-text-active")))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    // `pnsAS2`.
                    .child("Show More Emoji")
                    .child(
                        div()
                            .relative()
                            .left(px(4.))
                            .size(px(16.))
                            .flex_none()
                            .child(
                                gpui::svg()
                                    .path("icons/chevron_down.svg")
                                    .size(px(16.))
                                    .when(!expanded, |this| {
                                        this.with_transformation(gpui::Transformation::rotate(
                                            gpui::radians(-std::f32::consts::FRAC_PI_2),
                                        ))
                                    }),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.emoji_popout.as_mut() {
                            open.show_more = !open.show_more;
                        }
                        cx.notify();
                    })),
            );
            if expanded {
                let tiles = others(&guild.emojis, emoji);
                section =
                    section.child(div().mt(px(4.)).flex().flex_wrap().items_center().children(
                        tiles.into_iter().map(|other| {
                            let picture = self.images.get(&format!("custom:{}", other.id)).cloned();
                            let label = if other.require_colons {
                                format!(":{}:", other.name)
                            } else {
                                other.name.clone()
                            };
                            let tile = div()
                                .id(SharedString::from(format!(
                                    "emoji-popout-other-{}",
                                    other.id
                                )))
                                .size(px(OTHER))
                                .mx(px(2.))
                                .my(px(4.))
                                .flex_none()
                                .children(picture.map(|picture| img(picture).size(px(OTHER))));
                            self.with_tip(
                                format!("emoji-popout-other-tip-{}", other.id),
                                Tip::body(Body::Text(label.into()), Side::Top),
                                tile,
                                cx,
                            )
                        }),
                    ));
            }
        }
        section.into_any_element()
    }

    /// `joinGuild` for the server an emoji came from.
    fn join_emoji_guild(&mut self, guild: Id) {
        let (method, route) = super::sticker_popout::join_request(guild);
        let _ = self.requests.send(Request::Rest {
            key: None,
            method,
            route,
            body: Some(serde_json::Value::Object(Default::default())),
        });
    }

    /// Asks for what the open custom emoji's popout lacks: the lookup, the server's icon and its other emoji.
    pub(super) fn emoji_popout_requests(&mut self) {
        let Some(Kind::Custom { id, .. }) =
            self.emoji_popout.as_ref().map(|open| open.kind.clone())
        else {
            return;
        };
        let joined = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.emojis.iter().any(|emoji| emoji.id == id));
        let needs_lookup = joined.is_none_or(|guild| is_discoverable(&guild.features));
        if needs_lookup && self.requested.insert(source_key(id)) {
            let _ = self.requests.send(Request::Fetch {
                key: source_key(id),
                route: source_route(id),
            });
        }
        let Some(Source::Guild(guild)) = self
            .fetched
            .get(&source_key(id))
            .and_then(Source::from_value)
        else {
            return;
        };
        if let Some(icon) = guild.icon.as_deref() {
            self.request_image(
                format!("guild:{}", guild.id),
                format!(
                    "https://cdn.discordapp.com/icons/{}/{icon}.webp?size=96",
                    guild.id
                ),
            );
        }
        for other in others(&guild.emojis, id) {
            let extension = if other.animated { "gif" } else { "webp" };
            self.request_image(
                format!("custom:{}", other.id),
                format!(
                    "https://cdn.discordapp.com/emojis/{}.{extension}?size=48",
                    other.id
                ),
            );
        }
    }
}

/// `GET /emojis/{id}/source` (`EMOJI_SOURCE_DATA`, read by `g_` in module 624793).
pub(super) fn source_route(id: Id) -> String {
    format!("/emojis/{id}/source")
}

pub(super) fn source_key(id: Id) -> String {
    format!("emoji-source:{id}")
}

fn is_discoverable(features: &[String]) -> bool {
    features.iter().any(|feature| feature == "DISCOVERABLE")
}

/// What `g_` returns: where an emoji comes from.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Source {
    Guild(SourceGuild),
    Application(String),
    Pack,
}

/// `_.createFromServer`: the fields of the server the popout reads.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SourceGuild {
    pub id: Id,
    pub name: String,
    pub icon: Option<String>,
    pub discoverable: bool,
    /// `approximate_presence_count`.
    pub online: Option<u64>,
    pub emojis: Vec<OtherEmoji>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct OtherEmoji {
    pub id: Id,
    pub name: String,
    pub animated: bool,
    pub require_colons: bool,
}

impl Source {
    pub(super) fn from_value(body: &serde_json::Value) -> Option<Self> {
        match body["type"].as_str()? {
            "GUILD" => {
                let guild = &body["guild"];
                Some(Self::Guild(SourceGuild {
                    id: guild["id"].as_str()?.parse().ok()?,
                    name: guild["name"].as_str()?.to_owned(),
                    icon: guild["icon"].as_str().map(str::to_owned),
                    discoverable: guild["features"]
                        .as_array()
                        .is_some_and(|list| list.iter().any(|one| one == "DISCOVERABLE")),
                    online: guild["approximate_presence_count"].as_u64(),
                    emojis: guild["emojis"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|emoji| {
                            Some(OtherEmoji {
                                id: emoji["id"].as_str()?.parse().ok()?,
                                name: emoji["name"].as_str()?.to_owned(),
                                animated: emoji["animated"].as_bool().unwrap_or(false),
                                require_colons: emoji["require_colons"].as_bool().unwrap_or(true),
                            })
                        })
                        .collect(),
                }))
            }
            "APPLICATION" => Some(Self::Application(
                body["application"]["name"].as_str()?.to_owned(),
            )),
            "PACK" => Some(Self::Pack),
            _ => None,
        }
    }
}

/// `en`: the server's first thirteen emoji without the open one, at most twelve.
pub(super) fn others(emojis: &[OtherEmoji], open: Id) -> Vec<&OtherEmoji> {
    emojis
        .iter()
        .take(13)
        .filter(|emoji| emoji.id != open)
        .take(12)
        .collect()
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

    #[test]
    fn a_guild_source_reads_its_card_and_other_emoji() {
        let body = serde_json::json!({
            "type": "GUILD",
            "guild": {
                "id": "5", "name": "Blob Land", "icon": null,
                "features": ["COMMUNITY", "DISCOVERABLE"],
                "approximate_presence_count": 1234,
                "emojis": [
                    {"id": "10", "name": "a", "animated": false, "require_colons": true},
                    {"id": "11", "name": "b", "animated": true, "require_colons": false},
                ],
            },
        });
        let Some(Source::Guild(guild)) = Source::from_value(&body) else {
            panic!("a guild source");
        };
        assert!(guild.discoverable);
        assert_eq!(guild.online, Some(1234));
        assert_eq!(guild.emojis.len(), 2);
        assert!(guild.emojis[1].animated && !guild.emojis[1].require_colons);
    }

    #[test]
    fn an_application_or_pack_source_has_no_guild() {
        let app = serde_json::json!({"type": "APPLICATION", "application": {"id": "1", "name": "Helper"}});
        assert_eq!(
            Source::from_value(&app),
            Some(Source::Application("Helper".into()))
        );
        assert_eq!(
            Source::from_value(&serde_json::json!({"type": "PACK", "pack": null})),
            Some(Source::Pack)
        );
        assert_eq!(
            Source::from_value(&serde_json::json!({"type": "OTHER"})),
            None
        );
    }

    #[test]
    fn the_other_emoji_are_the_first_thirteen_without_the_open_one_at_most_twelve() {
        let list: Vec<OtherEmoji> = (1..=20)
            .map(|id| OtherEmoji {
                id,
                name: format!("e{id}"),
                animated: false,
                require_colons: true,
            })
            .collect();
        let shown = others(&list, 3);
        assert_eq!(shown.len(), 12);
        assert!(shown.iter().all(|emoji| emoji.id != 3 && emoji.id <= 13));
        assert_eq!(others(&list[..2], 1).len(), 1);
    }

    #[test]
    fn the_lookup_route_is_the_source_route() {
        assert_eq!(source_route(77), "/emojis/77/source");
        // Module 137133 Nitro Emoji Pack popout keys.
        assert_eq!(dc_intl::t("leUgZw"), "Nitro exclusive emoji");
        assert_eq!(dc_intl::t("DtK5IG"), "View Set");
        assert_eq!(dc_intl::t("gl/XHJ"), "Get Nitro");
    }
}
