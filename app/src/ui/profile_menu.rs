//! The profile's overflow menu (module 987307, `user-profile-overflow-menu`, "Profile Actions") and its banner
//! friend button (module 508882). Groups: Share Profile and Invite to Server; Ignore, Block and Report User Profile.
//! The menu opens 8px left of the button, level with its top. Measured in
//! `design/dom/menus/dm-profile-more-darker-cozy.json` and `dm-profile-invite-darker-cozy.json`.
//!
//! Invite to Server (module 889460) lists the servers the user may invite from, by `CREATE_INSTANT_INVITE` or a
//! vanity code, except the one in view: a one-use unique invite on the server's first channel that allows it, sent
//! in the DM, and the row then reads "Invite Sent".

use super::friends::{RELATIONSHIP_FRIEND, RELATIONSHIP_INCOMING, RELATIONSHIP_OUTGOING};
use super::remediation::Remedy;
use super::{Root, color};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::permissions::{self, CREATE_INSTANT_INVITE, VIEW_CHANNEL};
use dc_state::{ChannelKind, Id};
use gpui::{
    Anchor, AnyElement, Context, FontWeight, IntoElement, SharedString, Window, anchored, deferred,
    div, point, prelude::*, px, svg,
};
use serde_json::json;
use std::collections::HashSet;

/// Fetched-key prefix of the one-use invite made for a person: `profile-invite:{user}`.
pub(super) const INVITE_KEY: &str = "profile-invite:";

#[derive(Clone, Debug, Default)]
pub(super) struct ProfileMenu {
    pub user: Id,
    /// The Invite to Server submenu is open.
    pub invite: bool,
    /// Servers whose invite went out from this menu.
    pub sent: HashSet<Id>,
    /// The probe of the button it hangs off, and whether it opens to that button's right (the full profile) rather
    /// than its left (the DM panel's banner).
    pub anchor: &'static str,
    pub right: bool,
}

/// Which menu hangs off a profile button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Which {
    Overflow,
    Friend,
}

impl Root {
    /// `can(permission, channel)` for the current user.
    pub(super) fn can_in_channel(&self, channel: &dc_state::Channel, permission: u64) -> bool {
        let Some(me) = self.stores.me() else {
            return false;
        };
        self.stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel.id))
            .is_some_and(|guild| permissions::can_in_channel(guild, channel, me, permission))
    }

    /// Servers to invite this person to, in rail order: inviting allowed (or a vanity code), not the one in view.
    fn invite_guilds(&self) -> Vec<(Id, String)> {
        let Some(me) = self.stores.me() else {
            return Vec::new();
        };
        self.stores
            .guilds
            .iter()
            .filter(|guild| Some(guild.id) != self.guild)
            .filter(|guild| {
                permissions::can_in_guild(guild, me, CREATE_INSTANT_INVITE)
                    || guild.vanity_url_code.is_some()
            })
            .map(|guild| (guild.id, guild.name.clone()))
            .collect()
    }

    /// `getDefaultChannel(guild, true, CREATE_INSTANT_INVITE)`: the first text channel by position that can be seen
    /// and invited from.
    fn invite_channel(&self, guild: Id) -> Option<Id> {
        let me = self.stores.me()?;
        let guild = self.stores.guilds.iter().find(|known| known.id == guild)?;
        let mut channels: Vec<_> = guild
            .channels
            .iter()
            .filter(|channel| channel.kind == ChannelKind::Text)
            .filter(|channel| {
                permissions::can_in_channel(
                    guild,
                    channel,
                    me,
                    VIEW_CHANNEL | CREATE_INSTANT_INVITE,
                )
            })
            .collect();
        channels.sort_by_key(|channel| (channel.position, channel.id));
        channels.first().map(|channel| channel.id)
    }

    /// The submenu row's action: a one-use invite (or the vanity code), then the link in their DM.
    fn invite_to_guild(&mut self, user: Id, guild: Id, cx: &mut Context<Self>) {
        let me = self.stores.me();
        let Some(known) = self.stores.guilds.iter().find(|known| known.id == guild) else {
            return;
        };
        let can = me.is_some_and(|me| permissions::can_in_guild(known, me, CREATE_INSTANT_INVITE));
        let vanity = known.vanity_url_code.clone();
        let Some(channel) = self.invite_channel(guild) else {
            return;
        };
        if let Some(menu) = self.profile_menu.as_mut() {
            menu.sent.insert(guild);
        }
        match (can, vanity) {
            (false, Some(code)) => self.send_link_to_dm(user, format!("https://discord.gg/{code}")),
            _ => {
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{INVITE_KEY}{user}")),
                    method: reqwest::Method::POST,
                    route: format!("/channels/{channel}/invites"),
                    body: Some(json!({"max_uses": 1, "unique": true})),
                });
            }
        }
        cx.notify();
    }

    /// The invite made for `user` came back: its link goes to them.
    pub(super) fn profile_invite_created(&mut self, key: &str, report: &serde_json::Value) {
        let Some(user) = key
            .strip_prefix(INVITE_KEY)
            .and_then(|user| user.parse::<Id>().ok())
        else {
            return;
        };
        if let Some(code) = report["body"]["code"].as_str() {
            self.send_link_to_dm(user, format!("https://discord.gg/{code}"));
        }
    }

    pub(super) fn open_profile_menu(&mut self, user: Id, which: Which, cx: &mut Context<Self>) {
        let anchor = match which {
            Which::Overflow => "dm-profile-more",
            Which::Friend => "dm-profile-friend",
        };
        self.open_profile_menu_at(user, which, anchor, false, cx);
    }

    pub(super) fn open_profile_menu_at(
        &mut self,
        user: Id,
        which: Which,
        anchor: &'static str,
        right: bool,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .profile_menu
            .as_ref()
            .map(|menu| (menu.user, self.profile_menu_which, menu.anchor));
        if open == Some((user, which, anchor)) {
            self.profile_menu = None;
        } else {
            self.profile_menu = Some(ProfileMenu {
                user,
                anchor,
                right,
                ..ProfileMenu::default()
            });
            self.profile_menu_which = which;
        }
        cx.notify();
    }

    fn profile_row(
        &self,
        id: &'static str,
        label: SharedString,
        danger: bool,
        caret: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .group(SharedString::from(format!("profile-row-{id}")))
            .relative()
            .child(self.anchor_probe(format!("profile-menu-{id}")))
            .h(px(if caret { 36. } else { 34. }))
            .px(px(8.))
            .rounded(px(2.))
            .cursor_pointer()
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside
                    && let Some(menu) = this.profile_menu.as_mut()
                    && menu.invite != caret
                {
                    menu.invite = caret;
                    cx.notify();
                }
            }))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(if danger {
                color("--text-feedback-critical")
            } else {
                color("--text-strong")
            })
            .hover(move |style| {
                style
                    .bg(if danger {
                        color("--background-feedback-mod-critical")
                    } else {
                        color("--interactive-background-hover")
                    })
                    .rounded(px(4.))
            })
            .child(div().flex_1().child(label))
            .when(caret, |this| {
                this.child(
                    // `.focused .caret_M3qk4j` turns white.
                    div().ml(px(8.)).size(px(20.)).child(
                        svg()
                            .path("icons/menu_caret.svg")
                            .size(px(20.))
                            .text_color(color("--text-muted"))
                            .group_hover(
                                SharedString::from(format!("profile-row-{id}")),
                                |style| style.text_color(color("--white")),
                            ),
                    ),
                )
            })
    }

    /// The overflow menu, or the friend button's menu, beside the button that opened it.
    pub(super) fn profile_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.profile_menu.as_ref()?;
        let user = menu.user;
        let me = self.stores.current_user.as_ref().map(|known| known.id);
        let blocked = self.is_blocked(user);
        let ignored = self.stores.is_ignored(user);
        let bot = self.stores.users.get(&user).is_some_and(|known| known.bot);
        let relationship = self.stores.relationships.get(&user).copied();
        let (probe, rows): (&str, Vec<AnyElement>) = match self.profile_menu_which {
            Which::Friend => {
                // `x` in module 508882: Remove Friend for a friend, Accept Friend Request for an incoming one.
                let row = match relationship {
                    Some(RELATIONSHIP_FRIEND) => Some(
                        self.profile_row("remove-friend", "Remove Friend".into(), false, false, cx)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.profile_menu = None;
                                this.confirm_remove_friend =
                                    Some(super::friends::RemoveFriend::Menu(user));
                                cx.notify();
                            }))
                            .into_any_element(),
                    ),
                    Some(RELATIONSHIP_INCOMING) => Some(
                        self.profile_row(
                            "add-friend",
                            "Accept Friend Request".into(),
                            false,
                            false,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile_menu = None;
                            this.set_relationship(user, Some(RELATIONSHIP_FRIEND), cx);
                            cx.notify();
                        }))
                        .into_any_element(),
                    ),
                    _ => None,
                };
                (menu.anchor, row.into_iter().collect())
            }
            Which::Overflow => {
                let mut first: Vec<AnyElement> = vec![
                    self.profile_row("share-profile", "Share Profile".into(), false, false, cx)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.profile_menu = None;
                            if let Some(known) = this.stores.users.get(&user).cloned() {
                                this.open_share_profile(known, window, cx);
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                ];
                let guilds = self.invite_guilds();
                if Some(user) != me && !bot && !guilds.is_empty() && !blocked && !ignored {
                    first.push(
                        self.profile_row(
                            "invite-to-server",
                            "Invite to Server".into(),
                            false,
                            true,
                            cx,
                        )
                        .into_any_element(),
                    );
                }
                let mut second: Vec<AnyElement> = Vec::new();
                if Some(user) != me && !blocked {
                    second.push(
                        self.profile_row(
                            "ignore",
                            super::remediation::ignore_label(ignored),
                            false,
                            false,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile_menu = None;
                            if ignored {
                                this.unignore_user(user, cx);
                            } else {
                                this.remediation = Some((user, Remedy::Ignore));
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                    );
                }
                if Some(user) != me {
                    second.push(
                        self.profile_row(
                            "block",
                            if blocked { "Unblock" } else { "Block" }.into(),
                            !blocked,
                            false,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile_menu = None;
                            if blocked {
                                this.unblock_user(user, cx);
                            } else {
                                this.remediation = Some((user, Remedy::Block));
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                    );
                    if !bot {
                        second.push(
                            self.profile_row(
                                "report-user",
                                "Report User Profile".into(),
                                true,
                                false,
                                cx,
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.profile_menu = None;
                                this.open_report_user(user, window, cx);
                            }))
                            .into_any_element(),
                        );
                    }
                }
                // Any other row closes the submenu.
                let mut rows: Vec<AnyElement> = first;
                if !second.is_empty() {
                    rows.push(
                        div()
                            .my(px(8.))
                            .h(px(1.))
                            .bg(color("--border-subtle"))
                            .into_any_element(),
                    );
                    rows.extend(second);
                }
                // Not in the pinned client: CancelFriendRequest's row at the end (`plugin_hooks.rs`).
                if self.stores.relationships.get(&user) == Some(&RELATIONSHIP_OUTGOING)
                    && super::plugin_hooks::cancel_friend_request()
                {
                    rows.push(
                        self.profile_row(
                            "cancel-friend-request",
                            super::plugin_hooks::CANCEL_FRIEND_REQUEST.into(),
                            false,
                            false,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.profile_menu = None;
                            this.set_relationship(user, None, cx);
                        }))
                        .into_any_element(),
                    );
                }
                (menu.anchor, rows)
            }
        };
        if rows.is_empty() {
            return None;
        }
        let anchor = self.tip_anchors.borrow().get(probe).copied()?;
        let menu_layer = deferred(
            anchored()
                .anchor(if menu.right {
                    Anchor::TopLeft
                } else {
                    Anchor::TopRight
                })
                .position(if menu.right {
                    point(anchor.right() + px(8.), anchor.top())
                } else {
                    point(anchor.left() - px(8.), anchor.top())
                })
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .parity_with_border("profile-menu", 1.)
                        .id("profile-menu")
                        .relative()
                        .child(self.anchor_probe("profile-menu"))
                        .occlude()
                        .p(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                let anchors = this.tip_anchors.borrow();
                                let inside = |key: &str| {
                                    anchors
                                        .get(key)
                                        .is_some_and(|bounds| bounds.contains(&event.position))
                                };
                                // The submenu and the button that toggles the menu are not outside it.
                                let keep =
                                    (this.profile_menu.as_ref().is_some_and(|menu| menu.invite)
                                        && inside("submenu"))
                                        || (event.button == gpui::MouseButton::Left
                                            && this
                                                .profile_menu
                                                .as_ref()
                                                .is_some_and(|menu| inside(menu.anchor)));
                                drop(anchors);
                                if !keep
                                    && this.press_outside(super::keyboard::Layer::ProfileMenu, cx)
                                    && event.button != gpui::MouseButton::Right
                                {
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .on_hover(cx.listener(|_, _: &bool, _, _| {}))
                        .flex()
                        .flex_col()
                        .children(rows),
                ),
        )
        .with_priority(2)
        .into_any_element();
        Some(menu_layer)
    }

    /// Invite to Server's submenu, a root layer like every submenu (absolute boxes sit in their parent).
    pub(super) fn profile_submenu_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.profile_menu.as_ref()?;
        if !menu.invite || self.profile_menu_which != Which::Overflow {
            return None;
        }
        let user = menu.user;
        let row = self
            .tip_anchors
            .borrow()
            .get("profile-menu-invite-to-server")
            .copied()?;
        let rows = self
            .invite_guilds()
            .into_iter()
            .map(|(guild, name)| {
                let sent = menu.sent.contains(&guild);
                let item = if sent {
                    // `disabled_M3qk4j`: half opacity and no pointer events, so no hover either.
                    div()
                        .h(px(34.))
                        .p(px(8.))
                        .opacity(0.5)
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child("Invite Sent")
                        .into_any_element()
                } else {
                    self.submenu_item(
                        SharedString::from(format!("profile-invite-{guild}")),
                        None,
                        name,
                        34.,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.invite_to_guild(user, guild, cx);
                    }))
                    .into_any_element()
                };
                (item, 34.)
            })
            .collect();
        Some(self.submenu_frame(rows, row, window))
    }

    /// The banner's friend button (module 508882): Add Friend sends a request; otherwise its icon says the
    /// relationship and it opens Friend Actions when there is anything to do.
    pub(super) fn profile_friend_button(&self, user: Id, cx: &mut Context<Self>) -> AnyElement {
        let relationship = self.stores.relationships.get(&user).copied();
        let (glyph, tip) = match relationship {
            Some(RELATIONSHIP_FRIEND) => ("user_check", "Friend"),
            Some(RELATIONSHIP_OUTGOING) => ("user_clock", "Outgoing Friend Request"),
            Some(RELATIONSHIP_INCOMING) => ("user_clock", "Incoming Friend Request"),
            _ => ("user_plus", "Add Friend"),
        };
        // An outgoing request has no actions: the button is disabled.
        // Not in the pinned client: with CancelFriendRequest it cancels the request (`plugin_hooks.rs`).
        let cancel = relationship == Some(RELATIONSHIP_OUTGOING) && super::plugin_hooks::cancel_friend_request();
        let tip = if cancel { super::plugin_hooks::CANCEL_FRIEND_REQUEST } else { tip };
        let disabled = !cancel
            && matches!(
                relationship,
                Some(RELATIONSHIP_OUTGOING | super::blocked::RELATIONSHIP_BLOCKED)
            );
        // The probe sits beside the button, not inside its border.
        let button = div()
            .id("dm-profile-friend-button")
            .relative()
            .child(self.anchor_probe("dm-profile-friend"))
            .child(if cancel {
                super::plugin_hooks::cancel_request_round(glyph, false, false)
            } else {
                super::dm_profile::round_button(glyph)
            })
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| this.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| match relationship {
                Some(RELATIONSHIP_OUTGOING) if cancel => this.set_relationship(user, None, cx),
                None => this.set_relationship(user, Some(0), cx),
                Some(RELATIONSHIP_OUTGOING | super::blocked::RELATIONSHIP_BLOCKED) => {}
                Some(_) => this.open_profile_menu(user, Which::Friend, cx),
            }));
        self.with_tip(
            "dm-profile-friend-tip",
            super::tooltip::Tip::text(tip, super::tooltip::Side::Top),
            button,
            cx,
        )
        .into_any_element()
    }

    /// The banner's More button: Profile Actions.
    pub(super) fn profile_more_button(&self, user: Id, cx: &mut Context<Self>) -> AnyElement {
        let button =
            div()
                .id("dm-profile-more-button")
                .relative()
                .child(self.anchor_probe("dm-profile-more"))
                .child(super::dm_profile::round_button("more_horizontal"))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_profile_menu(user, Which::Overflow, cx)
                }));
        self.with_tip(
            "dm-profile-more-tip",
            super::tooltip::Tip::text("More", super::tooltip::Side::Top),
            button,
            cx,
        )
        .into_any_element()
    }
}
