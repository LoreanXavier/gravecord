//! The popout a sticker in a message opens (module 679636): `rU` picks `rD` for a pack sticker, `rO` for a server
//! sticker and `rG` when the sticker cannot be found. Its pieces are measured in the `popoutContainer_j9Vvzj`,
//! `popoutContent_pWj3yQ` and `emojiSection_Gl3WZ2` rules of the bundle's style sheets, and the guild card is `Q` in
//! module 767143. Strings come from `design/snapshot/dumps/STRINGS/en-US.json` (the hashes are named beside each use).
//!
//! The popout asks for what it lacks like the official one does: `GET /stickers/{id}` (`AO` in module 631576) for a
//! sticker the stores do not hold, `GET /sticker-packs/{id}` (`zk`) for a pack that is not in the premium list and
//! `GET /stickers/{id}/guild` (`STICKER_GUILD_DATA`) for the server of a sticker whose server is unknown or
//! discoverable. "Join Server" is `PUT /guilds/{id}/members/@me` (`GUILD_JOIN` in module 778858 `joinGuild`).

use super::stickers::{PACKS_KEY, PACKS_ROUTE, art_key, art_url};
use super::tooltip::{Body, Side, Tip};
use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use dc_state::stickers::{Format, Sticker, StickerPack};
use gpui::{
    AnyElement, Bounds, BoxShadow, Context, FontWeight, IntoElement, Pixels, SharedString,
    anchored, deferred, div, hsla, img, point, prelude::*, px,
};
use serde_json::Value;

/// `--custom-message-helpers-popout-container-width` (256 + 32) and `...-popout-padding-width`.
const WIDTH: f32 = 288.;
const PADDING: f32 = 16.;
/// Where the popout sits beside the sticker: the gap `Popout` leaves between it and its target.
const GAP: f32 = 8.;
/// `getStickerPack`'s sticker thumbnails (`rD`), and the other stickers of a server (`otherEmoji_Gl3WZ2`).
const PACK_THUMB: f32 = 80.;
const OTHER_THUMB: f32 = 32.;

#[derive(Clone)]
pub(super) struct StickerPopout {
    sticker: Id,
    name: String,
    format_type: u64,
    anchor: Bounds<Pixels>,
    /// `I` in `rO`: the other stickers of the server are listed.
    show_more: bool,
    /// `v` in `rO`: the other sticker under the pointer.
    hovered: Option<Id>,
}

/// The line `rO` writes under the sticker's name (`t`), by the strings' hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Note {
    /// `fZ0DiG`.
    ThisServerEverywhere,
    /// `1f6D9m`.
    YourServerTypeName,
    /// `yHmoR9`.
    JoinToUseEverywhere,
    /// `vZaScH`.
    InviteOnly,
    /// `jNphpt`.
    ThisServerNitro,
    /// `lyD5ZW`.
    YourServerNitro,
    /// `IuXYch`.
    NitroAndJoin,
    /// `hGWuxU`, with its "Learn more" link.
    InviteOnlyLearn,
}

impl Note {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::ThisServerEverywhere => {
                "This sticker is from this server. You can use it everywhere."
            }
            Self::YourServerTypeName => {
                "This sticker is from one of your servers. Type its name in the chat bar to use it."
            }
            Self::JoinToUseEverywhere => "Want to use this sticker everywhere? Join the server.",
            Self::InviteOnly => {
                "This sticker is from a server that is either invite-only or unavailable."
            }
            Self::ThisServerNitro => {
                "This sticker is from this server. Get Nitro to use it everywhere."
            }
            Self::YourServerNitro => {
                "This sticker is from one of your servers. Get Nitro to use it everywhere."
            }
            Self::NitroAndJoin => {
                "Want to use this sticker everywhere? Get Nitro and join the server."
            }
            Self::InviteOnlyLearn => {
                "This sticker is from a server that is either invite-only or unavailable. "
            }
        }
    }
}

/// What `rO` knows when it picks its line and its button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GuildFacts {
    /// `canUseCustomStickersEverywhere` (`p`).
    pub everywhere: bool,
    /// The sticker's server is one the user is in (`f`).
    pub joined: bool,
    /// ... and it is the server of the open channel (`b`).
    pub same_guild: bool,
    /// The server's public data arrived (`T`).
    pub discoverable: bool,
}

/// `rO`'s answer: the line, whether "Get Nitro" shows (`S`) and whether "Join Server" shows (`R`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Branch {
    pub note: Note,
    pub upsell: bool,
    pub join: bool,
}

pub(super) fn guild_branch(facts: GuildFacts) -> Branch {
    let GuildFacts {
        everywhere,
        joined,
        same_guild,
        discoverable,
    } = facts;
    let (note, upsell) = if everywhere {
        let note = if joined {
            if same_guild {
                Note::ThisServerEverywhere
            } else {
                Note::YourServerTypeName
            }
        } else if discoverable {
            Note::JoinToUseEverywhere
        } else {
            Note::InviteOnly
        };
        (note, false)
    } else if joined {
        let note = if same_guild {
            Note::ThisServerNitro
        } else {
            Note::YourServerNitro
        };
        (note, true)
    } else if discoverable {
        (Note::NitroAndJoin, true)
    } else {
        (Note::InviteOnlyLearn, false)
    };
    Branch {
        note,
        upsell,
        join: !upsell && !joined && discoverable && everywhere,
    }
}

/// The three other stickers of a pack `rD` lists: from the pack's first four, those that are not the open one,
/// until there are three.
pub(super) fn pack_others(stickers: &[Sticker], open: Id) -> Vec<&Sticker> {
    stickers.iter().take(4).fold(Vec::new(), |mut list, one| {
        if list.len() != 3 && one.id != open {
            list.push(one);
        }
        list
    })
}

/// The other stickers `rO` offers under "Show More Stickers": up to thirteen of the server's, without the open
/// one, at most twelve.
pub(super) fn guild_others(stickers: &[Value], open: Id) -> Vec<Sticker> {
    stickers
        .iter()
        .take(13)
        .filter_map(Sticker::from_value)
        .filter(|one| one.id != open)
        .take(12)
        .collect()
}

/// The request `joinGuild` makes: `PUT /guilds/{id}/members/@me` with an empty body (a joiner, not a lurker).
pub(super) fn join_request(guild: Id) -> (reqwest::Method, String) {
    (reqwest::Method::PUT, format!("/guilds/{guild}/members/@me"))
}

/// `ij.jE`'s fields the popout reads from `GET /stickers/{id}/guild`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PublicGuild {
    id: Id,
    name: String,
    icon: Option<String>,
    discoverable: bool,
    online: Option<u64>,
    stickers: Vec<Value>,
}

impl PublicGuild {
    pub(super) fn from_value(body: &Value) -> Option<Self> {
        Some(Self {
            id: body["id"].as_str()?.parse().ok()?,
            name: body["name"].as_str()?.to_owned(),
            icon: body["icon"].as_str().map(str::to_owned),
            discoverable: body["features"]
                .as_array()
                .is_some_and(|list| list.iter().any(|one| one == "DISCOVERABLE")),
            online: body["approximate_presence_count"].as_u64(),
            stickers: body["stickers"].as_array().cloned().unwrap_or_default(),
        })
    }
}

fn sticker_key(id: Id) -> String {
    format!("sticker-info:{id}")
}

fn pack_key(id: Id) -> String {
    format!("sticker-pack:{id}")
}

fn guild_key(id: Id) -> String {
    format!("sticker-guild:{id}")
}

enum Resolved {
    Loading,
    Standard(Sticker),
    Guild(Sticker),
    /// `rG`: the lookup ended and found nothing.
    Unavailable,
}

/// The container every branch sits in: `popoutContainer_j9Vvzj`.
pub(super) fn shell() -> gpui::Div {
    let mut background = color("--background-surface-highest");
    background.a *= 0.9;
    div()
        .w(px(WIDTH))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(background)
        .occlude()
        .shadow(vec![
            BoxShadow {
                color: hsla(0., 0., 1., 0.08),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
            BoxShadow {
                color: hsla(0., 0., 0., 0.14),
                offset: point(px(0.), px(1.)),
                blur_radius: px(4.),
                spread_radius: px(0.),
                inset: false,
            },
        ])
}

pub(super) fn text(size: f32, line: f32, weight: f32, token: &str) -> gpui::Div {
    div()
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(weight))
        .text_color(color(token))
}

/// `Y0`: the placeholder while the data is on its way, from the mask in `popoutLoadingBackground_j9Vvzj`.
fn loader() -> AnyElement {
    let block = |left: f32, top: f32, width: f32, height: f32| {
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .rounded(px(4.))
            .bg(color("--primary-600"))
    };
    shell()
        .child(
            div()
                .relative()
                .m(px(PADDING))
                .h(px(78.))
                .child(block(0., 0., 58., 58.))
                .child(block(74., 0., 77., 18.))
                .child(block(74., 30., 182., 18.))
                .child(block(74., 60., 121., 18.)),
        )
        .into_any_element()
}

pub(super) fn small_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
) -> gpui::Stateful<gpui::Div> {
    let (background, hover, border, ink) = if primary {
        (
            "--control-primary-background-default",
            "--control-primary-background-hover",
            "--control-primary-border-default",
            "--white",
        )
    } else {
        (
            "--control-secondary-background-default",
            "--control-secondary-background-hover",
            "--border-muted",
            "--control-secondary-text-default",
        )
    };
    div()
        .id(id)
        .w_full()
        .h(px(32.))
        .px(px(11.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .border_1()
        .border_color(color(border))
        .bg(color(background))
        .hover(move |style| style.bg(color(hover)))
        .cursor_pointer()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color(ink))
        .child(label)
}

impl Root {
    /// The sticker in a message, as `rH` builds it: a pointer cursor, the name in a tooltip while the popout is
    /// closed, and a click that opens the popout beside it.
    pub(super) fn sticker_target(
        &self,
        id: Id,
        name: &str,
        format_type: u64,
        interacting: bool,
        size: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .sticker_popout
            .as_ref()
            .is_some_and(|open| open.sticker == id);
        let known = self.stores.stickers.sticker(id).map(|one| one.name.clone());
        let label = SharedString::from(known.unwrap_or_else(|| name.to_owned()));
        let picture = self.sticker_art(
            id,
            Format::from_wire(format_type),
            size,
            interacting,
            false,
            cx,
        );
        let probe = format!("message-sticker-{id}");
        let clicked = probe.clone();
        let target = div()
            .id(SharedString::from(probe.clone()))
            .relative()
            .size(px(size))
            .flex_none()
            .cursor_pointer()
            .children(picture.map(|picture| picture.size(px(size))))
            .child(self.anchor_probe(probe))
            .on_click(cx.listener({
                let name = label.to_string();
                move |this, _, _, cx| {
                    let anchor = this.tip_anchors.borrow().get(clicked.as_str()).copied();
                    if let Some(anchor) = anchor {
                        this.open_sticker_popout(id, name.clone(), format_type, anchor, cx);
                    }
                }
            }));
        if open {
            return target.into_any_element();
        }
        self.with_tip(
            format!("message-sticker-tip-{id}"),
            Tip::body(Body::Sticker(label), Side::Top),
            target,
            cx,
        )
        .into_any_element()
    }

    fn open_sticker_popout(
        &mut self,
        id: Id,
        name: String,
        format_type: u64,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tip = None;
        self.sticker_popout = Some(StickerPopout {
            sticker: id,
            name,
            format_type,
            anchor,
            show_more: false,
            hovered: None,
        });
        self.sticker_popout_requests();
        cx.notify();
        // The answers land in `fetched`; this redraws and asks for what each answer names, until the popout closes.
        cx.spawn(async move |this, cx| {
            for _ in 0..40 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        let alive = this
                            .sticker_popout
                            .as_ref()
                            .is_some_and(|open| open.sticker == id);
                        if alive {
                            this.sticker_popout_requests();
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

    /// "View More Stickers": the composer's picker on its Stickers tab, scrolled to the pack.
    fn view_sticker_pack(&mut self, pack: Id, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let composer = self.tip_anchors.borrow().get("composer-box").copied();
        let Some(bounds) = composer else {
            return;
        };
        self.picker = None;
        self.open_picker_tab(
            super::picker::Tab::Stickers,
            point(bounds.left(), bounds.top() - px(9.)),
            window,
            cx,
        );
        self.show_sticker_pack(pack, cx);
    }

    pub(super) fn close_sticker_popout(&mut self, cx: &mut Context<Self>) {
        if self.sticker_popout.take().is_some() {
            cx.notify();
        }
    }

    /// The sticker `rh.Zq` hands the branches: the stores' copy, else the one `GET /stickers/{id}` returned.
    fn resolve_popout_sticker(&self, open: &StickerPopout) -> Resolved {
        let found = self
            .stores
            .stickers
            .sticker(open.sticker)
            .cloned()
            .or_else(|| {
                self.fetched
                    .get(&sticker_key(open.sticker))
                    .and_then(Sticker::from_value)
            });
        match found {
            Some(one) if one.is_standard() => Resolved::Standard(one),
            Some(one) if one.is_guild() => Resolved::Guild(one),
            Some(_) => Resolved::Unavailable,
            None if self.fetch_failures.contains_key(&sticker_key(open.sticker)) => {
                Resolved::Unavailable
            }
            None if self.fetched.contains_key(&sticker_key(open.sticker)) => Resolved::Unavailable,
            None => Resolved::Loading,
        }
    }

    /// Sends the lookups the popout still needs, once each.
    fn sticker_popout_requests(&mut self) {
        let Some(open) = self.sticker_popout.clone() else {
            return;
        };
        let known = self.stores.stickers.sticker(open.sticker).cloned();
        if known.is_none() && self.requested.insert(sticker_key(open.sticker)) {
            let _ = self.requests.send(Request::Fetch {
                key: sticker_key(open.sticker),
                route: format!("/stickers/{}", open.sticker),
            });
        }
        let one = known.or_else(|| {
            self.fetched
                .get(&sticker_key(open.sticker))
                .and_then(Sticker::from_value)
        });
        let Some(one) = one else {
            return;
        };
        self.request_image(art_key(one.id), art_url(&one));
        if one.is_standard() {
            if !self.stores.stickers.has_loaded_packs()
                && self.requested.insert(PACKS_KEY.to_owned())
            {
                let _ = self.requests.send(Request::Fetch {
                    key: PACKS_KEY.to_owned(),
                    route: PACKS_ROUTE.to_owned(),
                });
            }
            let pack = one.pack_id;
            if let Some(pack) = pack
                && self.stores.stickers.has_loaded_packs()
                && self.stores.stickers.pack(pack).is_none()
                && self.requested.insert(pack_key(pack))
            {
                let _ = self.requests.send(Request::Fetch {
                    key: pack_key(pack),
                    route: format!("/sticker-packs/{pack}"),
                });
            }
            let siblings: Vec<Sticker> = self
                .pack_for_popout(&one)
                .map(|(pack, _)| pack.stickers.into_iter().take(4).collect())
                .unwrap_or_default();
            for other in siblings {
                self.requested_art(other);
            }
        } else if one.is_guild() {
            let source = one.guild_id.and_then(|guild| self.stores.guild(guild));
            let fetch =
                source.is_none_or(|guild| guild.features.iter().any(|f| f == "DISCOVERABLE"));
            if fetch && self.requested.insert(guild_key(one.id)) {
                let _ = self.requests.send(Request::Fetch {
                    key: guild_key(one.id),
                    route: format!("/stickers/{}/guild", one.id),
                });
            }
            if let Some(public) = self.public_guild(one.id) {
                if let Some(icon) = public.icon.as_deref() {
                    self.request_image(
                        format!("guild:{}", public.id),
                        format!(
                            "https://cdn.discordapp.com/icons/{}/{icon}.webp?size=96",
                            public.id
                        ),
                    );
                }
                for other in guild_others(&public.stickers, one.id) {
                    self.requested_art(other);
                }
            }
        }
    }

    fn requested_art(&mut self, sticker: Sticker) {
        self.request_image(art_key(sticker.id), art_url(&sticker));
    }

    fn public_guild(&self, sticker: Id) -> Option<PublicGuild> {
        self.fetched
            .get(&guild_key(sticker))
            .and_then(PublicGuild::from_value)
    }

    /// The pack a pack sticker belongs to: the premium list's, else the one fetched for it. The flag is `isPremiumPack`.
    fn pack_for_popout(&self, one: &Sticker) -> Option<(StickerPack, bool)> {
        let id = one.pack_id?;
        if let Some(pack) = self.stores.stickers.pack(id) {
            return Some((pack.clone(), true));
        }
        self.fetched
            .get(&pack_key(id))
            .and_then(StickerPack::from_value)
            .map(|pack| (pack, false))
    }

    pub(super) fn sticker_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.sticker_popout.as_ref()?;
        let content = match self.resolve_popout_sticker(open) {
            Resolved::Loading => loader(),
            Resolved::Unavailable => self.unavailable_card(open, cx),
            Resolved::Standard(one) => self.pack_card(&one, cx),
            Resolved::Guild(one) => self.guild_card(open, &one, cx),
        };
        let at = point(
            open.anchor.right() + px(GAP),
            open.anchor.top() + open.anchor.size.height / 2.,
        );
        Some(
            deferred(
                div()
                    .id("sticker-popout-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close_sticker_popout(cx)),
                    )
                    .child(
                        anchored()
                            .anchor(gpui::Anchor::LeftCenter)
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(content),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The sticker's picture at `size`, still or animated as the message list draws it.
    fn popout_art(&self, one: &Sticker, size: f32, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size(px(size))
            .flex_none()
            .children(
                self.sticker_art(one.id, one.format(), size, true, false, cx)
                    .map(|picture| picture.size(px(size))),
            )
            .into_any_element()
    }

    /// `rP`: the sticker at 48px beside its name and a line of description; the text starts 10px from the picture
    /// (the margin a horizontal `Flex` puts on its children).
    fn sticker_header(
        &self,
        one: &Sticker,
        description: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .child(self.popout_art(one, 48., cx))
            .child(
                div()
                    .ml(px(10.))
                    .min_w(px(0.))
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        text(16., 20., 600., "--text-default")
                            .child(SharedString::from(one.name.clone())),
                    )
                    .child(description),
            )
            .into_any_element()
    }

    /// `rD`.
    fn pack_card(&self, one: &Sticker, cx: &mut Context<Self>) -> AnyElement {
        let Some((pack, premium)) = self.pack_for_popout(one) else {
            return loader();
        };
        let first = text(14., 18., 400., "--text-default");
        let line = if premium {
            vec![
                ("This is a ", false),
                (pack.name.as_str(), true),
                (" sticker.", false),
            ]
        } else {
            vec![
                ("This is a ", false),
                (pack.name.as_str(), true),
                (" sticker. They are no longer available.", false),
            ]
        };
        let description =
            first
                .flex()
                .flex_wrap()
                .children(line.into_iter().map(|(part, bold)| {
                    div()
                        .font_weight(FontWeight(if bold { 700. } else { 400. }))
                        .child(SharedString::from(part.to_owned()))
                }));
        let others = pack_others(&pack.stickers, one.id);
        let pack_id = pack.id;
        shell()
            .child(
                div()
                    .p(px(PADDING))
                    .flex()
                    .flex_col()
                    .child(
                        text(16., 20., 600., "--text-default")
                            .child(SharedString::from(one.name.clone())),
                    )
                    .child(description)
                    .child(
                        div().my(px(8.)).flex().justify_between().children(
                            others
                                .into_iter()
                                .map(|other| self.popout_art(other, PACK_THUMB, cx)),
                        ),
                    )
                    .when(premium, |this| {
                        this.child(
                            div().flex().flex_col().gap(px(8.)).child(
                                small_button("sticker-popout-more", "View More Stickers", false)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.close_sticker_popout(cx);
                                        this.view_sticker_pack(pack_id, window, cx);
                                    })),
                            ),
                        )
                    }),
            )
            .into_any_element()
    }

    /// `rG`: the sticker is in no store and the lookup ended. The line is `hGWuxU` with its link.
    fn unavailable_card(&self, open: &StickerPopout, cx: &mut Context<Self>) -> AnyElement {
        let stand_in = Sticker {
            id: open.sticker,
            name: open.name.clone(),
            description: None,
            tags: None,
            kind: 0,
            format_type: open.format_type,
            guild_id: None,
            pack_id: None,
            available: true,
            user_id: None,
        };
        let description = self.learn_more_line(cx);
        shell()
            .child(
                div()
                    .p(px(PADDING))
                    .child(self.sticker_header(&stand_in, description, cx)),
            )
            .into_any_element()
    }

    /// `hGWuxU`: the line and its "Learn more" link, which opens the Nitro settings page (`rM`). The link starts a
    /// row of its own, where the official line flows on from the sentence before it.
    fn learn_more_line(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .child(
                text(14., 18., 400., "--text-default")
                    .child(Note::InviteOnlyLearn.text().trim_end()),
            )
            .child(
                text(14., 18., 400., "--text-default")
                    .flex()
                    .flex_wrap()
                    .child(
                        div()
                            .id("sticker-popout-learn-more")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .hover(|style| style.underline())
                            .child("Learn more")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_sticker_popout(cx);
                                this.open_settings_page("nitro", cx);
                            })),
                    )
                    .child(div().ml(px(4.)).child("about using custom stickers.")),
            )
            .into_any_element()
    }

    /// `rO`.
    fn guild_card(
        &self,
        open: &StickerPopout,
        one: &Sticker,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let joined_guild = one.guild_id.and_then(|guild| self.stores.guild(guild));
        let public = self.public_guild(one.id);
        let looked_up = self.fetched.contains_key(&guild_key(one.id))
            || self.fetch_failures.contains_key(&guild_key(one.id));
        let needs_lookup =
            joined_guild.is_none_or(|guild| guild.features.iter().any(|f| f == "DISCOVERABLE"));
        if needs_lookup && !looked_up {
            return loader();
        }
        let joined = joined_guild.is_some();
        let facts = GuildFacts {
            everywhere: self
                .premium()
                .has(dc_state::premium::Feature::StickersEverywhere),
            joined,
            same_guild: one.guild_id.is_some() && one.guild_id == self.guild,
            discoverable: public.is_some(),
        };
        let branch = guild_branch(facts);
        let description = if branch.note == Note::InviteOnlyLearn {
            self.learn_more_line(cx)
        } else {
            text(14., 18., 400., "--text-default")
                .child(branch.note.text())
                .into_any_element()
        };
        let mut top = div()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .child(self.sticker_header(one, description, cx));
        if branch.upsell {
            top = top.child(div().mt(px(PADDING)).child(
                small_button("sticker-popout-nitro", "Get Nitro", true).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.close_sticker_popout(cx);
                        this.open_settings_page("nitro", cx);
                    },
                )),
            ));
        }
        if branch.join
            && let Some(public) = public.as_ref()
        {
            let guild = public.id;
            top = top.child(div().mt(px(PADDING)).child(
                small_button("sticker-popout-join", "Join Server", true).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.close_sticker_popout(cx);
                        this.join_sticker_guild(guild);
                    },
                )),
            ));
        }
        let card = joined_guild
            .map(|guild| (guild.id, guild.name.clone(), guild.icon.is_some()))
            .or_else(|| {
                public
                    .as_ref()
                    .map(|public| (public.id, public.name.clone(), public.icon.is_some()))
            });
        let source = card.map(|(id, name, has_icon)| {
            let discoverable_online = public
                .as_ref()
                .filter(|public| public.discoverable)
                .and_then(|public| public.online);
            let others = public
                .as_ref()
                .map(|public| guild_others(&public.stickers, one.id))
                .unwrap_or_default();
            self.popout_source_section(
                open,
                (id, name, has_icon),
                joined,
                branch.join,
                discoverable_online,
                others,
                cx,
            )
        });
        shell().child(top).children(source).into_any_element()
    }

    /// The darker section of `rO`: "This sticker is from" and the server's card, and for a server the user is not
    /// in, the toggle that lists its other stickers.
    #[allow(clippy::too_many_arguments)]
    fn popout_source_section(
        &self,
        open: &StickerPopout,
        (id, name, has_icon): (Id, String, bool),
        joined: bool,
        join_button: bool,
        online: Option<u64>,
        others: Vec<Sticker>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon_image = self.images.get(&format!("guild:{id}")).cloned();
        let acronym = dc_state::Guild::acronym_of(&name);
        let second: AnyElement = match online {
            Some(count) => {
                let mut row = div()
                    .flex()
                    .items_center()
                    .child(text(12., 16., 400., "--text-default").child(format!("{count} Online")))
                    .child(
                        div()
                            .flex_none()
                            .size(px(4.))
                            .mx(px(8.))
                            .rounded_full()
                            .bg(color("--background-mod-muted")),
                    );
                row = if !join_button && !joined {
                    row.child(
                        div()
                            .id("sticker-popout-guild-join")
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .child(text(12., 16., 400., "--text-link").child("Join Server"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_sticker_popout(cx);
                                this.join_sticker_guild(id);
                            })),
                    )
                } else {
                    row.child(text(12., 16., 400., "--text-default").child("Discoverable Server"))
                };
                row.into_any_element()
            }
            None => text(12., 16., 400., "--text-default")
                .child("Invite-only server")
                .into_any_element(),
        };
        let card = div()
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
                    .when(!has_icon || icon_image.is_none(), |this| {
                        this.bg(color("--background-mod-muted"))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(acronym.clone())
                    })
                    .children(
                        icon_image
                            .filter(|_| has_icon)
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
                            .child(SharedString::from(name)),
                    )
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
                    .child(if joined {
                        "This sticker is from"
                    } else {
                        "Get this sticker from"
                    }),
            )
            .child(card);
        if !joined {
            let expanded = open.show_more;
            section = section.child(
                div()
                    .id("sticker-popout-show-more")
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_color(color("--interactive-text-default"))
                    .hover(|style| style.text_color(color("--interactive-text-active")))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .child("Show More Stickers")
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
                        if let Some(open) = this.sticker_popout.as_mut() {
                            open.show_more = !open.show_more;
                        }
                        cx.notify();
                    })),
            );
            if expanded {
                let hovered = open.hovered;
                section =
                    section.child(div().mt(px(4.)).flex().flex_wrap().items_center().children(
                        others.into_iter().map(|other| {
                            let id = other.id;
                            let dim = hovered.is_some_and(|hovered| hovered != id);
                            let name = SharedString::from(other.name.clone());
                            let tile = div()
                                .id(SharedString::from(format!("sticker-popout-other-{id}")))
                                .size(px(OTHER_THUMB))
                                .mx(px(2.))
                                .my(px(4.))
                                .flex_none()
                                .when(dim, |this| this.opacity(0.5))
                                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                                    if let Some(open) = this.sticker_popout.as_mut() {
                                        open.hovered = inside.then_some(id);
                                    }
                                    cx.notify();
                                }))
                                .child(self.popout_art(&other, OTHER_THUMB, cx));
                            self.with_tip(
                                format!("sticker-popout-other-tip-{id}"),
                                Tip::body(Body::Text(name), Side::Top),
                                tile,
                                cx,
                            )
                        }),
                    ));
            }
        }
        section.into_any_element()
    }

    /// `joinGuild` for the server a sticker came from; the answer arrives with the guild's own dispatches.
    fn join_sticker_guild(&mut self, guild: Id) {
        let (method, route) = join_request(guild);
        let _ = self.requests.send(Request::Rest {
            key: None,
            method,
            route,
            body: Some(Value::Object(Default::default())),
        });
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only: `stickerpopout <open|closed|joined|gone>` opens the popout for a made-up server sticker with
    /// the answers a server would have given, so the branches can be framed without a sticker from a real server.
    pub(crate) fn script_sticker_popout(
        &mut self,
        words: &[&str],
        cx: &mut Context<Self>,
    ) -> String {
        use serde_json::json;
        let [kind] = words else {
            return "stickerpopout: usage `stickerpopout <open|closed|joined|gone>`".to_owned();
        };
        let id: Id = 900_001;
        let guild = if *kind == "joined" {
            self.guild.unwrap_or(900_002)
        } else {
            900_002
        };
        let sticker = json!({
            "id": id.to_string(), "name": "Wave", "type": 2, "format_type": 1,
            "guild_id": guild.to_string(), "description": "A hand waving",
        });
        let key = sticker_key(id);
        match *kind {
            "gone" => {
                self.fetch_failures.insert(key, std::time::Instant::now());
            }
            _ => {
                self.fetched.insert(key, sticker);
            }
        }
        match *kind {
            "open" => {
                let others: Vec<Value> = (1..=5)
                    .map(|n| json!({"id": (900_100 + n).to_string(), "name": format!("Other {n}"), "format_type": 1}))
                    .collect();
                self.fetched.insert(
                    guild_key(id),
                    json!({
                        "id": guild.to_string(), "name": "Sticker Land", "icon": null,
                        "features": ["COMMUNITY", "DISCOVERABLE"],
                        "approximate_presence_count": 1234, "stickers": others,
                    }),
                );
            }
            "closed" | "joined" => {
                self.fetch_failures
                    .insert(guild_key(id), std::time::Instant::now());
            }
            _ => {}
        }
        self.requested.insert(sticker_key(id));
        self.requested.insert(guild_key(id));
        self.tip = None;
        self.sticker_popout = Some(StickerPopout {
            sticker: id,
            name: "Wave".to_owned(),
            format_type: 1,
            anchor: Bounds {
                origin: point(px(440.), px(300.)),
                size: gpui::size(px(160.), px(160.)),
            },
            show_more: false,
            hovered: None,
        });
        cx.notify();
        format!("stickerpopout: {kind}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts(everywhere: bool, joined: bool, same_guild: bool, discoverable: bool) -> GuildFacts {
        GuildFacts {
            everywhere,
            joined,
            same_guild,
            discoverable,
        }
    }

    #[test]
    fn a_nitro_user_reads_where_the_sticker_is_from() {
        let this = guild_branch(facts(true, true, true, false));
        assert_eq!(this.note, Note::ThisServerEverywhere);
        assert!(!this.upsell && !this.join);
        let other = guild_branch(facts(true, true, false, false));
        assert_eq!(other.note, Note::YourServerTypeName);
        assert!(!other.upsell && !other.join);
    }

    #[test]
    fn a_stranger_sees_join_only_when_the_server_is_discoverable() {
        let open = guild_branch(facts(true, false, false, true));
        assert_eq!(open.note, Note::JoinToUseEverywhere);
        assert!(open.join && !open.upsell);
        let closed = guild_branch(facts(true, false, false, false));
        assert_eq!(closed.note, Note::InviteOnly);
        assert!(!closed.join);
    }

    #[test]
    fn without_nitro_the_upsell_replaces_join() {
        let member = guild_branch(facts(false, true, true, false));
        assert_eq!(member.note, Note::ThisServerNitro);
        assert!(member.upsell && !member.join);
        let stranger = guild_branch(facts(false, false, false, true));
        assert_eq!(stranger.note, Note::NitroAndJoin);
        assert!(stranger.upsell && !stranger.join);
        let hidden = guild_branch(facts(false, false, false, false));
        assert_eq!(hidden.note, Note::InviteOnlyLearn);
        assert!(!hidden.upsell && !hidden.join);
    }

    fn sticker(id: Id) -> Sticker {
        Sticker {
            id,
            name: format!("s{id}"),
            description: None,
            tags: None,
            kind: 1,
            format_type: 1,
            guild_id: None,
            pack_id: Some(9),
            available: true,
            user_id: None,
        }
    }

    #[test]
    fn a_pack_lists_up_to_three_others_from_its_first_four() {
        let all: Vec<Sticker> = (1..=8).map(sticker).collect();
        let ids = |list: Vec<&Sticker>| list.into_iter().map(|one| one.id).collect::<Vec<_>>();
        assert_eq!(ids(pack_others(&all, 2)), vec![1, 3, 4]);
        assert_eq!(ids(pack_others(&all, 7)), vec![1, 2, 3]);
        assert_eq!(ids(pack_others(&all[..2], 1)), vec![2]);
    }

    #[test]
    fn a_server_lists_twelve_others_from_its_first_thirteen() {
        let list: Vec<Value> = (1..=20)
            .map(|id| json!({"id": id.to_string(), "name": "n", "format_type": 1}))
            .collect();
        let others = guild_others(&list, 3);
        assert_eq!(others.len(), 12);
        assert!(others.iter().all(|one| one.id != 3));
        assert_eq!(others.last().map(|one| one.id), Some(13));
    }

    #[test]
    fn joining_is_a_put_on_the_member_route_and_is_never_sent_by_the_tests() {
        let (method, route) = join_request(42);
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(route, "/guilds/42/members/@me");
    }

    #[test]
    fn public_guild_data_reads_discoverability_and_presence() {
        let body = json!({
            "id": "5", "name": "Open", "icon": "abc",
            "features": ["COMMUNITY", "DISCOVERABLE"],
            "approximate_presence_count": 1234,
            "stickers": [{"id": "1", "name": "a", "format_type": 1}],
        });
        let guild = PublicGuild::from_value(&body).unwrap();
        assert!(guild.discoverable);
        assert_eq!(guild.online, Some(1234));
        assert_eq!(guild.stickers.len(), 1);
        let hidden = PublicGuild::from_value(&json!({"id": "5", "name": "Closed"})).unwrap();
        assert!(!hidden.discoverable && hidden.online.is_none());
    }
}
