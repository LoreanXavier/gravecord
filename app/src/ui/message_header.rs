//! What follows an author's name in a message header (module 315934 `F`, called from module 549587 `eQ`): the server
//! tag (`y.Cw` of module 534400, opening the guild profile popout `191565`), the role icon (module 657048, opening the
//! role icon popout `eW`), and the app tag (module 709066: `APP` with the verified tick and its "Verified App"
//! tooltip, or `OP` with "Original Poster" in a forum post). Their styles are `clanTagChiplet_2i8Bfm`,
//! `chipletContainerInner_k8jyN7`, `roleIcon_2i8Bfm`, `botTagCozy_2i8Bfm` and `botTag_feVUtO` (stylesheets 23401, 73801
//! and 33292); the sizes of the tag and of `OP` are the ones in `design/dom/channel/forum-fixture-posted-darker-cozy.json`
//! and `pt-activity-feed-collapsed-darker-cozy.json`.

use super::sticker_popout::{shell, small_button, text};
use super::tooltip::{Body, Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::{Id, Message, RoleIcon};
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, SharedString, anchored, deferred,
    div, img, point, prelude::*, px,
};
use std::collections::HashMap;

/// The popout open beside a header chip.
pub(super) enum HeaderPopout {
    /// `eW`: the role's icon, its name and where the icon comes from.
    RoleIcon {
        guild: Id,
        role: Id,
        anchor: Bounds<Pixels>,
    },
    /// `191565`: the server behind a tag.
    GuildTag { guild: Id, anchor: Bounds<Pixels> },
}

/// Which app-style tag a message author gets (module 812299 `k`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AppTag {
    /// A bot's messages: `APP`, with the tick when the bot is verified (`isVerifiedBot`: public flag 1 << 16).
    App { verified: bool },
    /// The author of a forum post, on their own post: `OP`.
    OriginalPoster,
}

/// `isVerifiedBot`: `UserFlags.VERIFIED_BOT`.
const VERIFIED_BOT: u64 = 1 << 16;

/// `k` in module 812299: a bot is `BOT` (shown as "APP"); in a forum post its owner is `ORIGINAL_POSTER`.
pub(super) fn app_tag(bot: bool, flags: u64, forum_post_owner: bool) -> Option<AppTag> {
    if bot {
        Some(AppTag::App {
            verified: flags & VERIFIED_BOT == VERIFIED_BOT,
        })
    } else if forum_post_owner {
        Some(AppTag::OriginalPoster)
    } else {
        None
    }
}

/// `iconRoleId` (module 696451 `Y`) filtered by `fm` of module 376304: the member's highest role that has an icon,
/// shown when the server has `ROLE_ICONS` or the role belongs to a subscription.
pub(super) fn icon_role(guild: &dc_state::Guild, member_roles: &[Id]) -> Option<(Id, RoleIcon)> {
    let shown = guild.features.iter().any(|feature| feature == "ROLE_ICONS");
    pick_icon_role(shown, &guild.role_icons, &guild.role_info, member_roles)
}

fn pick_icon_role(
    shown: bool,
    icons: &HashMap<Id, RoleIcon>,
    info: &HashMap<Id, dc_state::RoleInfo>,
    member_roles: &[Id],
) -> Option<(Id, RoleIcon)> {
    member_roles
        .iter()
        .filter_map(|role| Some((*role, icons.get(role)?)))
        .filter(|(_, icon)| shown || icon.subscription)
        .max_by_key(|(role, _)| {
            (
                info.get(role).map_or(0, |one| one.position),
                std::cmp::Reverse(*role),
            )
        })
        .map(|(role, icon)| (role, icon.clone()))
}

/// The line of the role icon popout (`t0928V`, `UDDkGy`).
pub(super) fn role_icon_line(name: &str, subscription: bool) -> String {
    if subscription {
        format!("Custom Role Icon for {name}. This feature is a part of Server Subscriptions.")
    } else {
        format!("Custom Role Icon for {name}. This feature is a part of Server Boosting.")
    }
}

/// `Est. {date}` (`zb2Q56`): the server's age from its id, as the card writes it.
pub(super) fn established(guild: Id) -> String {
    let millis = (guild >> 22) as i64 + 1_420_070_400_000;
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|date| {
            format!(
                "Est. {}",
                date.with_timezone(&chrono::Local).format("%B %-d, %Y")
            )
        })
        .unwrap_or_default()
}

/// The `{n} Members` count (`zRl6XR`).
pub(super) fn member_count_label(count: u64) -> String {
    format!(
        "{} {}",
        super::role_popout::group_digits(count),
        if count == 1 { "Member" } else { "Members" }
    )
}

pub(super) fn profile_key(guild: Id) -> String {
    format!("guildprof:{guild}")
}

impl Root {
    /// The chips after the name, in the official order: tag, role icon, app tag.
    pub(super) fn header_decorations(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut chips = Vec::new();
        let author = &message.author;
        let id = message.id;
        // `y.Cw`: the server tag, which is a button opening the server's profile.
        if let Some(tag) = author.tag.as_ref() {
            let guild = tag.guild_id;
            let badge = tag
                .badge_url()
                .and_then(|url| self.images.get(&url).cloned());
            let key = format!("message-tag-{id}");
            let chip = div()
                .id(SharedString::from(key.clone()))
                .relative()
                // `clanTagChiplet`: 6px before it; `chipletContainerInner`: 4px radius, 0 4px padding, 17.94px tall
                // beside a 14px badge and 11.52px semibold text (the capture).
                .ml(px(6.))
                .h(px(17.9375))
                .px(px(4.))
                .rounded(px(4.))
                .bg(color("--background-mod-strong"))
                .flex()
                .flex_none()
                .items_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-muted")))
                .children(badge.map(|image| img(image).size(px(14.)).mr(px(2.))))
                .child(
                    div()
                        .max_w(px(70.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(11.52))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(SharedString::from(tag.tag.clone())),
                )
                .child(self.anchor_probe(key.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let anchor = this.tip_anchors.borrow().get(key.as_str()).copied();
                    if let Some(anchor) = anchor {
                        this.open_guild_tag_popout(guild, anchor, cx);
                    }
                }));
            chips.push(chip.into_any_element());
        }
        // `ey.A`: the role icon, 20px, 4px after the name, with the role's name in a tooltip.
        if let Some(guild) = self.current_guild()
            && let Some(member) = self.stores.members.get(guild.id, author.id)
            && let Some((role, picture)) = icon_role(guild, &member.roles)
            && let Some(name) = guild.roles.get(&role)
        {
            let guild_id = guild.id;
            let key = format!("message-role-icon-{id}");
            let art: AnyElement = match (
                picture
                    .icon_url(role, 40)
                    .and_then(|url| self.images.get(&role_icon_key(role, &url)).cloned()),
                picture.unicode_emoji.as_deref(),
            ) {
                (Some(image), _) => img(image).size(px(20.)).into_any_element(),
                (None, Some(emoji)) => self
                    .images
                    .get(&format!("emoji:{emoji}"))
                    .cloned()
                    .map_or_else(
                        || div().size(px(20.)).into_any_element(),
                        |image| img(image).size(px(20.)).into_any_element(),
                    ),
                (None, None) => div().size(px(20.)).into_any_element(),
            };
            let target = div()
                .id(SharedString::from(key.clone()))
                .ml(px(4.))
                .mt(px(1.))
                .size(px(20.))
                .cursor_pointer()
                .child(art);
            let tipped = self
                .with_tip(
                    format!("{key}-tip"),
                    Tip::body(Body::Text(name.clone().into()), Side::Top),
                    target,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let anchor = this
                        .tip_anchors
                        .borrow()
                        .get(format!("{key}-tip").as_str())
                        .copied();
                    if let Some(anchor) = anchor {
                        this.open_role_icon_popout(guild_id, role, anchor, cx);
                    }
                }));
            chips.push(tipped.into_any_element());
        }
        // `709066`: `APP` or `OP`.
        let forum_owner = self.current_thread().is_some_and(|thread| {
            thread.owner_id == Some(author.id)
                && self
                    .current_guild()
                    .and_then(|guild| guild.channels.iter().find(|one| one.id == thread.parent_id))
                    .is_some_and(|parent| {
                        matches!(parent.kind, dc_state::ChannelKind::Other(15 | 16))
                    })
        });
        if let Some(kind) = app_tag(author.bot, author.flags | author.public_flags, forum_owner) {
            chips.push(self.app_tag_chip(kind, id, cx));
        }
        chips
    }

    /// `709066 _`: `.botTagCozy` (4px before it), `.rem` sizes (15px tall, 4.4px padding, 12.8px semibold text).
    fn app_tag_chip(&self, kind: AppTag, message: Id, cx: &mut Context<Self>) -> AnyElement {
        let op = kind == AppTag::OriginalPoster;
        let verified = matches!(kind, AppTag::App { verified: true });
        let tag = div()
            .ml(px(4.))
            // `.botTag { top: .1rem }` and `.rem { margin-top: .2em }` / `.botTagOP { margin-top: .25em }` at 10px.
            .mt(px(if op { 4.1 } else { 3.6 }))
            .h(px(15.))
            .px(px(4.4))
            .rounded(px(if op { 8. } else { 4. }))
            .flex()
            .flex_none()
            .items_center()
            .bg(color(if op {
                "--brand-260"
            } else {
                "--background-brand"
            }))
            .text_color(color(if op { "--brand-560" } else { "--white" }))
            .children(verified.then(|| {
                // `botTagVerified`: a 16px tick, 3.2px into the tag's padding.
                let key = format!("message-verified-{message}");
                self.with_tip(
                    key,
                    // `g76OcH`.
                    Tip::body(Body::Text("Verified App".into()), Side::Top),
                    div()
                        .ml(px(-3.2))
                        .size(px(16.))
                        .child(icon("select_check", 16., "--white")),
                    cx,
                )
            }))
            .child(
                div()
                    .text_size(px(12.8))
                    .line_height(px(15.))
                    .font_weight(FontWeight(600.))
                    // `9RNkeF` and `fyE8sH`.
                    .child(if op { "OP" } else { "APP" }),
            );
        if op {
            // `uN6Emt`.
            return self
                .with_tip(
                    format!("message-op-{message}"),
                    Tip::body(Body::Text("Original Poster".into()), Side::Top),
                    tag,
                    cx,
                )
                .into_any_element();
        }
        tag.into_any_element()
    }

    /// The role icons the open channel's authors show.
    pub(super) fn request_role_icon_images(&mut self) {
        let Some(guild) = self.current_guild() else {
            return;
        };
        let guild_id = guild.id;
        let Some(messages) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
        else {
            return;
        };
        let mut wanted: Vec<(String, String)> = Vec::new();
        let mut emoji: Vec<String> = Vec::new();
        for message in messages {
            let Some(member) = self.stores.members.get(guild_id, message.author.id) else {
                continue;
            };
            let Some((role, picture)) = icon_role(guild, &member.roles) else {
                continue;
            };
            if let Some(url) = picture.icon_url(role, 40) {
                wanted.push((role_icon_key(role, &url), url));
            } else if let Some(glyph) = picture.unicode_emoji {
                emoji.push(glyph);
            }
        }
        for (key, url) in wanted {
            self.request_image(key, url);
        }
        for glyph in emoji {
            if let Some(file) = dc_tokens::emoji_file(&glyph) {
                self.request_image(
                    format!("emoji:{glyph}"),
                    format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
                );
            }
        }
    }

    fn open_role_icon_popout(
        &mut self,
        guild: Id,
        role: Id,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.tip = None;
        self.tip_pending = None;
        self.header_popout = Some(HeaderPopout::RoleIcon {
            guild,
            role,
            anchor,
        });
        cx.notify();
    }

    fn open_guild_tag_popout(&mut self, guild: Id, anchor: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.tip = None;
        self.tip_pending = None;
        let key = profile_key(guild);
        if self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/guilds/{guild}/profile"),
            });
        }
        self.header_popout = Some(HeaderPopout::GuildTag { guild, anchor });
        self.request_guild_profile_images(guild);
        cx.notify();
        // The answer lands in `fetched`; this redraws and asks for the pictures it names, until the popout closes.
        cx.spawn(async move |this, cx| {
            for _ in 0..40 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        let alive = matches!(
                            this.header_popout,
                            Some(HeaderPopout::GuildTag { guild: open, .. }) if open == guild
                        );
                        if alive {
                            this.request_guild_profile_images(guild);
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

    fn request_guild_profile_images(&mut self, guild: Id) {
        let Some(body) = self.fetched.get(&profile_key(guild)).cloned() else {
            return;
        };
        if let Some(icon) = body["icon_hash"].as_str().or(body["icon"].as_str()) {
            self.request_image(
                format!("guild:{guild}"),
                format!("https://cdn.discordapp.com/icons/{guild}/{icon}.webp?size=128"),
            );
        }
        if let Some(badge) = body["badge_hash"].as_str() {
            let url = format!("https://cdn.discordapp.com/clan-badges/{guild}/{badge}.png?size=16");
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn close_header_popout(&mut self, cx: &mut Context<Self>) {
        if self.header_popout.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn header_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.header_popout.as_ref()?;
        let (card, anchor): (AnyElement, Bounds<Pixels>) = match open {
            HeaderPopout::RoleIcon {
                guild,
                role,
                anchor,
            } => (self.role_icon_card(*guild, *role, cx)?, *anchor),
            HeaderPopout::GuildTag { guild, anchor, .. } => {
                (self.guild_profile_card(*guild, cx), *anchor)
            }
        };
        // `position: "right"`, `align: "center"` for the role icon (module 549587 `S`).
        let at = point(
            anchor.right() + px(8.),
            anchor.top() + anchor.size.height / 2.,
        );
        Some(
            deferred(
                div()
                    .id("header-popout-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close_header_popout(cx)),
                    )
                    .child(
                        anchored()
                            .anchor(gpui::Anchor::LeftCenter)
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(card),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// `eW`: 16px of padding, the icon at 40px with the name and the line to its right, then "Learn more".
    fn role_icon_card(&self, guild: Id, role: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let known = self.stores.guild(guild)?;
        let name = known.roles.get(&role)?.clone();
        let picture = known.role_icons.get(&role)?.clone();
        let art: AnyElement = match (
            picture
                .icon_url(role, 80)
                .and_then(|url| self.images.get(&role_icon_key(role, &url)).cloned()),
            picture.unicode_emoji.as_deref(),
        ) {
            (Some(image), _) => img(image).size(px(40.)).into_any_element(),
            (None, Some(emoji)) => self
                .images
                .get(&format!("emoji:{emoji}"))
                .cloned()
                .map_or_else(
                    || div().size(px(40.)).into_any_element(),
                    |image| img(image).size(px(40.)).into_any_element(),
                ),
            (None, None) => div().size(px(40.)).into_any_element(),
        };
        let line = role_icon_line(&name, picture.subscription);
        Some(
            shell()
                .child(
                    div()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(16.))
                                .flex()
                                .items_start()
                                .child(div().mr(px(16.)).w(px(40.)).flex_none().child(art))
                                .child(
                                    div()
                                        .min_w(px(0.))
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            text(16., 20., 600., "--text-strong")
                                                .mb(px(4.))
                                                .child(SharedString::from(name)),
                                        )
                                        .child(
                                            text(14., 18., 400., "--text-default")
                                                .child(SharedString::from(line)),
                                        ),
                                ),
                        )
                        // `H930+H`.
                        .child(
                            small_button("role-icon-learn-more", "Learn more", false).on_click(
                                cx.listener(|this, _, _, cx| this.close_header_popout(cx)),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// `191565` and `42780`: the banner, the server's picture, its name with the tag, the counts and the date, and the
    /// button under them (`461888`).
    fn guild_profile_card(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let key = profile_key(guild);
        let body = self.fetched.get(&key);
        let failed = self.fetch_failures.contains_key(&key);
        let joined = self.stores.guild(guild).is_some();
        let Some(body) = body else {
            // `d.y` spinner on the loading surface, or `R` when the request failed ("Unable to load server profile").
            return div()
                .w(px(300.))
                .rounded(px(16.))
                .overflow_hidden()
                .bg(color("--background-surface-highest"))
                .occlude()
                .p(px(16.))
                .child(text(14., 18., 400., "--text-default").child(if failed {
                    "Unable to load server profile"
                } else {
                    ""
                }))
                .into_any_element();
        };
        let name = body["name"].as_str().unwrap_or("Unknown Server").to_owned();
        let tag = body["tag"].as_str().filter(|tag| !tag.is_empty());
        let badge = body["badge_hash"].as_str().and_then(|hash| {
            self.images
                .get(&format!(
                    "https://cdn.discordapp.com/clan-badges/{guild}/{hash}.png?size=16"
                ))
                .cloned()
        });
        let online = body["online_count"].as_u64().unwrap_or(0);
        let members = body["member_count"].as_u64().unwrap_or(0);
        let discoverable = body["features"]
            .as_array()
            .is_some_and(|list| list.iter().any(|one| one == "DISCOVERABLE"));
        let banner = body["brand_color_primary"]
            .as_str()
            .and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok())
            .map_or_else(|| color("--brand-500"), gpui::rgb);
        let icon_image = self.images.get(&format!("guild:{guild}")).cloned();
        let acronym = dc_state::Guild::acronym_of(&name);
        let button = if joined {
            Some(("guild-profile-go", "Go to Server"))
        } else if discoverable {
            Some(("guild-profile-join", "Join"))
        } else {
            None
        };
        let count = |dot: &'static str, label: String| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(8.)).rounded_full().bg(color(dot)))
                .child(text(14., 18., 400., "--text-subtle").child(SharedString::from(label)))
        };
        div()
            .id("guild-profile-card")
            .w(px(300.))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-highest"))
            .overflow_hidden()
            .occlude()
            .flex()
            .flex_col()
            .child(div().h(px(120.)).w_full().bg(banner))
            .child(
                div().mt(px(-34.)).px(px(16.)).flex().child(
                    div()
                        .size(px(70.))
                        .rounded(px(20.))
                        .bg(color("--background-surface-highest"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(64.))
                                .rounded(px(16.))
                                .overflow_hidden()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(color("--background-mod-muted"))
                                .text_size(px(22.))
                                .text_color(color("--text-default"))
                                .children(icon_image.clone().map(|image| img(image).size(px(64.))))
                                .when(icon_image.is_none(), |this| this.child(acronym)),
                        ),
                ),
            )
            .child(
                div()
                    .my(px(8.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                text(16., 20., 600., "--text-strong")
                                    .min_w(px(0.))
                                    .truncate()
                                    .child(SharedString::from(name)),
                            )
                            .children(tag.map(|tag| {
                                div()
                                    .flex_none()
                                    .h(px(16.))
                                    .px(px(4.))
                                    .rounded(px(4.))
                                    .bg(color("--background-mod-strong"))
                                    .flex()
                                    .items_center()
                                    .children(
                                        badge.map(|image| img(image).size(px(14.)).mr(px(2.))),
                                    )
                                    .child(
                                        text(12., 16., 600., "--text-default")
                                            .child(SharedString::from(tag.to_owned())),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .flex()
                            .flex_wrap()
                            .gap_x(px(8.))
                            // `LC+S+m` and `zRl6XR`.
                            .child(count("--text-status-online", format!("{online} Online")))
                            .child(count("--text-status-offline", member_count_label(members))),
                    )
                    .child(
                        text(14., 18., 400., "--text-subtle")
                            .mt(px(4.))
                            .child(SharedString::from(established(guild))),
                    ),
            )
            .children(button.map(|(id, label)| {
                div().flex().w_full().px(px(16.)).pb(px(16.)).child(
                    small_button(id, label, true).w_full().on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.close_header_popout(cx);
                            if joined {
                                this.select(guild, None);
                                cx.notify();
                            }
                        },
                    )),
                )
            }))
            .into_any_element()
    }
}

/// The picture key of a role's icon: the role and the CDN address it came from.
pub(super) fn role_icon_key(role: Id, url: &str) -> String {
    format!("role-icon:{role}:{}", url.rsplit('/').next().unwrap_or(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps(icons: &[(Id, bool, i64)]) -> (HashMap<Id, RoleIcon>, HashMap<Id, dc_state::RoleInfo>) {
        let mut shown = HashMap::new();
        let mut info = HashMap::new();
        for (role, subscription, position) in icons {
            shown.insert(
                *role,
                RoleIcon {
                    icon: Some("hash".into()),
                    unicode_emoji: None,
                    subscription: *subscription,
                },
            );
            info.insert(
                *role,
                dc_state::RoleInfo {
                    position: *position,
                    ..dc_state::RoleInfo::default()
                },
            );
        }
        (shown, info)
    }

    #[test]
    fn a_bot_is_an_app_and_a_verified_bot_has_the_tick() {
        assert_eq!(
            app_tag(true, 0, false),
            Some(AppTag::App { verified: false })
        );
        assert_eq!(
            app_tag(true, VERIFIED_BOT, false),
            Some(AppTag::App { verified: true })
        );
        assert_eq!(app_tag(false, VERIFIED_BOT, false), None);
    }

    #[test]
    fn the_owner_of_a_forum_post_is_its_op_unless_they_are_an_app() {
        assert_eq!(app_tag(false, 0, true), Some(AppTag::OriginalPoster));
        assert_eq!(
            app_tag(true, 0, true),
            Some(AppTag::App { verified: false })
        );
    }

    #[test]
    fn the_role_icon_needs_the_feature_or_a_subscription_and_the_highest_role_wins() {
        let (icons, info) = maps(&[(1, false, 5)]);
        assert!(pick_icon_role(false, &icons, &info, &[1]).is_none());
        let (icons, info) = maps(&[(1, false, 5), (2, false, 9)]);
        assert_eq!(
            pick_icon_role(true, &icons, &info, &[1, 2]).map(|(role, _)| role),
            Some(2)
        );
        let (icons, info) = maps(&[(1, false, 5), (2, true, 1)]);
        assert_eq!(
            pick_icon_role(false, &icons, &info, &[1, 2]).map(|(role, _)| role),
            Some(2)
        );
        assert!(pick_icon_role(true, &icons, &info, &[]).is_none());
    }

    #[test]
    fn the_role_icon_line_names_where_the_feature_comes_from() {
        assert_eq!(
            role_icon_line("Mods", false),
            "Custom Role Icon for Mods. This feature is a part of Server Boosting."
        );
        assert!(role_icon_line("Mods", true).ends_with("Server Subscriptions."));
    }

    #[test]
    fn the_card_counts_and_age() {
        assert_eq!(member_count_label(1), "1 Member");
        assert_eq!(member_count_label(1500), "1,500 Members");
        // 2015-05-15 is where Discord's snowflake epoch plus the id's timestamp lands for this id.
        assert!(established(175_928_847_299_117_063).starts_with("Est. "));
    }

    #[test]
    fn role_icon_keys_are_per_role_and_file() {
        assert_eq!(
            role_icon_key(
                7,
                "https://cdn.discordapp.com/role-icons/7/abc.webp?size=40"
            ),
            "role-icon:7:abc.webp?size=40"
        );
    }
}
