//! Someone else's profile popout (`user-profile-popout`): the profile theme around a 300px card, Add Friend and More
//! on the banner, the 80px avatar (it opens the full profile), names with pronouns and badges, mutual friends and
//! servers, the bio, and "Message @name" (module 784886: Enter sends to their DM, opens it and closes the popout).
//! Measured in `design/dom/popouts/user-popout-other-darker-cozy.json`.

use super::profile::Popout;
use super::profile_menu::Which;
use super::{Root, avatar_source, status_dot_large};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, BoxShadow, Context, Entity, FontWeight, Hsla, IntoElement, Pixels, Point,
    Subscription, anchored, deferred, div, hsla, img, linear_color_stop, linear_gradient, point,
    prelude::*, px,
};

/// The popout's message field and its subscription.
pub(crate) struct PopoutMessage {
    pub editor: Entity<Editor>,
    _events: Subscription,
}

impl Root {
    /// The "Message @name" field for `user`'s popout.
    pub(super) fn popout_message(&mut self, user: Id, cx: &mut Context<Self>) -> PopoutMessage {
        let name = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        let editor = cx.new(|cx| {
            // `editor_q6l9qz`: 14px on 22px lines, the placeholder at 76% white.
            let style = crate::editor::EditorStyle {
                size: 14.,
                line_height: 22.,
                text: hsla(0., 0., 1., 0.96),
                placeholder: hsla(0., 0., 1., 0.76),
                caret: hsla(0., 0., 1., 0.96),
                ..super::editor_style()
            };
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder(format!("Message @{name}"));
            editor
        });
        let events = cx.subscribe(&editor, move |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let content = text.trim().to_owned();
                    if !content.is_empty() {
                        this.message_from_popout(user, content, cx);
                    }
                }
                EditorEvent::Cancel => this.profile = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        PopoutMessage {
            editor,
            _events: events,
        }
    }

    /// `p.p({userId, content, openChannel: true})`: the message in their DM (made first when there is none), the DM
    /// opened, the popout closed.
    fn message_from_popout(&mut self, user: Id, content: String, cx: &mut Context<Self>) {
        let existing = self
            .stores
            .private_channels
            .iter()
            .find(|channel| channel.recipients == [user])
            .map(|channel| channel.id);
        self.send_link_to_dm(user, content);
        match existing {
            Some(channel) => self.select_home(Some(channel)),
            None => self.open_dm_after_send = true,
        }
        self.profile = None;
        cx.notify();
    }

    pub(super) fn other_popout(
        &self,
        popout: &Popout,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let user = self.stores.users.get(&popout.user)?.clone();
        let id = user.id;
        let palette = self.dm_palette(id);
        let strong: Hsla = palette.text;
        let body = self.fetched.get(&format!("profile:{id}"));
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let presence = self
            .stores
            .presences
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "offline".to_owned());
        let banner = if palette.themed {
            palette.top
        } else {
            self.banner_color(&user)
        };
        let pronouns = body
            .and_then(|body| body["user_profile"]["pronouns"].as_str())
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let bio = body
            .and_then(|body| body["user_profile"]["bio"].as_str())
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let friends = body
            .and_then(|body| body["mutual_friends"].as_array().map(Vec::len))
            .unwrap_or(0);
        let servers = body
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        let badges = self.badges(id);
        let subtle = hsla(0., 0., 0.78, 1.);
        let plural = |count: usize, one: &str, many: &str| {
            if count == 1 {
                format!("1 {one}")
            } else {
                format!("{count} {many}")
            }
        };
        let header: AnyElement =
            div()
                .relative()
                .h(px(138.))
                .child(div().h(px(105.)).rounded_t(px(6.)).bg(banner))
                .child(
                    // `avatar_WRLIGC clickable`: "View Full Profile".
                    div()
                        .id("popout-avatar")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.profile = None;
                            this.open_full_profile(id, window, cx);
                        }))
                        .absolute()
                        .left(px(8.))
                        .top(px(53.))
                        .size(px(92.))
                        .rounded_full()
                        .bg(palette.top)
                        .child(
                            div()
                                .size(px(92.))
                                .rounded_full()
                                .bg(palette.overlay)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .relative()
                                        .size(px(80.))
                                        .rounded_full()
                                        .children(avatar.map(|picture| {
                                            img(picture).size(px(80.)).rounded_full()
                                        }))
                                        .child(status_dot_large(&presence, "--black")),
                                ),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(6.))
                        .right(px(10.))
                        .flex()
                        .gap(px(8.))
                        .child(self.profile_friend_button_at(id, "popout-friend", cx))
                        .child(self.profile_more_button_at(id, "popout-more", cx)),
                )
                .into_any_element();
        let names: AnyElement = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(strong)
                    .child(user.display_name().to_owned()),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(strong)
                    .child(user.username.clone())
                    .when_some(pronouns, |this, pronouns| {
                        this.child(div().size(px(4.)).rounded_full().bg(strong))
                            .child(pronouns)
                    })
                    .when(!badges.is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(22.))
                                .px(px(3.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .bg(hsla(240. / 360., 0.03, 0.61, 0.12))
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(badges),
                        )
                    }),
            )
            .into_any_element();
        let mutuals: AnyElement = div()
            .h(px(24.))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(subtle)
            .when(friends > 0, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(self.mutual_avatars(body))
                        .child(plural(friends, "Mutual Friend", "Mutual Friends")),
                )
            })
            .when(friends > 0 && servers > 0, |this| {
                this.child(div().size(px(4.)).rounded_full().bg(subtle))
            })
            .when(servers > 0, |this| {
                this.child(plural(servers, "Mutual Server", "Mutual Servers"))
            })
            .into_any_element();
        let content: AnyElement = div()
            .pl(px(16.))
            .pt(px(4.))
            .pr(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(names)
            .when(friends > 0 || servers > 0, |this| this.child(mutuals))
            .when_some(bio, |this, bio| this.child(self.bio(&bio, strong, cx)))
            .into_any_element();
        let message: Option<AnyElement> = popout.message.as_ref().map(|message| {
            div()
                .px(px(16.))
                .pb(px(12.))
                .child(
                    div()
                        .h(px(44.))
                        .rounded(px(8.))
                        .bg(hsla(0., 0., 1., 0.12))
                        .pl(px(15.))
                        .flex()
                        .items_center()
                        .child(div().flex_1().min_w(px(0.)).child(message.editor.clone()))
                        .child(
                            div()
                                .size(px(32.))
                                .mr(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(
                                    self.images
                                        .get("emoji_button")
                                        .cloned()
                                        .map(|picture| img(picture).size(px(18.)).grayscale(true)),
                                ),
                        ),
                )
                .into_any_element()
        });
        let card = div()
            .parity("user-popout")
            .id("profile-popout")
            .occlude()
            .w(px(304.))
            .p(px(2.))
            .rounded(px(8.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.),
                linear_color_stop(palette.bottom, 1.),
            ))
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., 1., 0.08),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: hsla(0., 0., 0., 0.24),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .child(
                div()
                    .rounded(px(6.))
                    .bg(palette.overlay)
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(header)
                    .child(content)
                    .children(message),
            );
        let at: Point<Pixels> = if popout.left {
            point(popout.anchor.left() - px(312.), popout.anchor.top())
        } else {
            point(popout.anchor.right() + px(8.), popout.anchor.top())
        };
        Some(
            deferred(
                div()
                    .id("profile-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.profile_menu.is_none() {
                                this.profile = None;
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        anchored()
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(card),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The banner friend button under any probe key, its menu opening to the left of it.
    pub(super) fn profile_friend_button_at(
        &self,
        user: Id,
        probe: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let relationship = self.stores.relationships.get(&user).copied();
        let glyph = match relationship {
            Some(super::friends::RELATIONSHIP_FRIEND) => "user_check",
            Some(super::friends::RELATIONSHIP_OUTGOING | super::friends::RELATIONSHIP_INCOMING) => {
                "user_clock"
            }
            _ => "user_plus",
        };
        let outgoing = relationship == Some(super::friends::RELATIONSHIP_OUTGOING);
        div()
            .id(probe)
            .relative()
            .child(self.anchor_probe(probe))
            .child(super::dm_profile::round_button(glyph))
            .when(outgoing, |this| this.opacity(0.5))
            .when(!outgoing, |this| this.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| match relationship {
                None | Some(0) => this.set_relationship(user, Some(0), cx),
                Some(super::friends::RELATIONSHIP_OUTGOING) => {}
                Some(_) => this.open_profile_menu_at(user, Which::Friend, probe, false, cx),
            }))
            .into_any_element()
    }

    pub(super) fn profile_more_button_at(
        &self,
        user: Id,
        probe: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(probe)
            .relative()
            .cursor_pointer()
            .child(self.anchor_probe(probe))
            .child(super::dm_profile::round_button("more_horizontal"))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_profile_menu_at(user, Which::Overflow, probe, false, cx)
            }))
            .into_any_element()
    }
}
