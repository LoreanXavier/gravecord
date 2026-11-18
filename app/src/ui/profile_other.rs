//! Someone else's profile popout (`user-profile-popout`): the profile theme around a 300px card, Add Friend and More
//! on the banner, the 80px avatar (it opens the full profile), names with pronouns and badges, mutual friends and
//! servers, the bio, and "Message @name" (module 784886: Enter sends to their DM, opens it and closes the popout).
//! Measured in `design/dom/popouts/user-popout-other-darker-cozy.json`.

use super::motion::{PopKind, PopSide, Popout as _};
use super::profile::Popout;
use super::profile_menu::Which;
use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, BoxShadow, Context, Entity, FontWeight, Hsla, IntoElement, Pixels, Point,
    Subscription, anchored, deferred, div, hsla, img, linear_color_stop, linear_gradient, point,
    prelude::*, px, rgb,
};

/// The popout's message field and its subscription.
pub(crate) struct PopoutMessage {
    pub editor: Entity<Editor>,
    _events: Subscription,
}

fn ordered_roles(guild: &dc_state::Guild, member: &dc_state::members::Member) -> Vec<Id> {
    let mut roles: Vec<_> = member
        .roles
        .iter()
        .copied()
        .filter(|id| *id != guild.id && guild.roles.contains_key(id))
        .collect();
    roles.sort_by_key(|id| {
        let role = guild.role_info.get(id).copied().unwrap_or_default();
        (role.guild_connection, std::cmp::Reverse(role.position), *id)
    });
    roles
}

impl Root {
    pub(super) fn profile_role_chips(
        &self,
        user: Id,
        strong: Hsla,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.profile_role_chip_list(self.guild?, user, strong, Some(cx))
    }

    pub(super) fn profile_role_chips_for(
        &self,
        guild: Id,
        user: Id,
        strong: Hsla,
    ) -> Option<AnyElement> {
        self.profile_role_chip_list(guild, user, strong, None)
    }

    fn profile_role_chip_list(
        &self,
        guild: Id,
        user: Id,
        strong: Hsla,
        cx: Option<&mut Context<Self>>,
    ) -> Option<AnyElement> {
        let guild = self.stores.guilds.iter().find(|known| known.id == guild)?;
        let member = self.stores.members.get(guild.id, user)?;
        let roles = ordered_roles(guild, member);
        let none = roles.is_empty();
        // 403369 passes allowEditing for the profile popout, including a member with no roles.
        let can_add =
            cx.is_some() && self.my_permission(guild.id, None, dc_state::permissions::MANAGE_ROLES);
        if none && !can_add {
            return None;
        }
        let palette = self.profile_palette(user, Some(guild.id), false);
        let border = palette.token(if palette.themed {
            "--user-profile-border"
        } else {
            "--border-subtle"
        });
        let dot_halo = palette.token("--background-base-lowest");
        let role_dot_style = self.pref_index("role_style", 0) == 1;
        let guild_shows_icons = guild.features.iter().any(|feature| feature == "ROLE_ICONS");
        Some(
            div()
                .parity("profile-roles")
                .mt(px(2.))
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .children(roles.into_iter().map(|id| {
                    let role = guild.role_info.get(&id).copied().unwrap_or_default();
                    let tint = if role.color == 0 {
                        color("--primary-300")
                    } else {
                        rgb(role.color)
                    };
                    let indicator = match (role_dot_style, guild.role_colors_of(id)) {
                        // `roleDot` (`m.W` of 545442): 12px halo with a 10px inner dot.
                        (true, Some(colors)) => div()
                            .mx(px(4.))
                            .size(px(12.))
                            .flex_none()
                            .rounded_full()
                            .bg(dot_halo)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::img(super::role_gradient::circle(&colors, 10, 1.))
                                    .size(px(10.)),
                            ),
                        (true, None) => div()
                            .mx(px(4.))
                            .size(px(12.))
                            .flex_none()
                            .rounded_full()
                            .bg(dot_halo)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(10.)).rounded_full().bg(tint)),
                        // `roleCircle` (`m.R` of 545442): the gradient or solid tint on the 12px circle.
                        (false, Some(colors)) => div().mx(px(4.)).size(px(12.)).flex_none().child(
                            gpui::img(super::role_gradient::circle(&colors, 12, 1.)).size(px(12.)),
                        ),
                        (false, None) => div()
                            .mx(px(4.))
                            .size(px(12.))
                            .flex_none()
                            .rounded_full()
                            .bg(tint),
                    };
                    let role_icon = self.role_icon_art(guild, id, 12.).or_else(|| {
                        let picture = guild.role_icons.get(&id)?;
                        if !guild_shows_icons && !picture.subscription {
                            return None;
                        }
                        let emoji = picture.unicode_emoji.as_deref()?;
                        Some(
                            div()
                                .text_size(px(12.))
                                .line_height(px(12.))
                                .child(emoji.to_owned())
                                .into_any_element(),
                        )
                    });
                    div()
                        .min_h(px(24.))
                        .max_w_full()
                        .pl(px(2.))
                        .pr(px(8.))
                        .border_1()
                        .border_color(border)
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .child(indicator)
                        .when(role.guild_connection, |this| {
                            this.child(icon("share_link", 12., "--text-default"))
                        })
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_ellipsis()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(400.))
                                .text_color(strong)
                                .child(guild.roles[&id].clone()),
                        )
                        // `.roleIcon_xL50-d`: 12px, 4px after the name.
                        .children(role_icon.map(|art| div().flex_none().ml(px(4.)).child(art)))
                }))
                .when_some(cx.filter(|_| can_add), |row, cx| {
                    row.child(self.profile_role_add_button(guild.id, user, none, cx))
                })
                .into_any_element(),
        )
    }

    /// The "Message @name" field for `user`'s popout.
    pub(super) fn popout_message(&mut self, user: Id, cx: &mut Context<Self>) -> PopoutMessage {
        let name = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        let palette = self.profile_palette(user, self.guild, false);
        let editor = cx.new(|cx| {
            // `editor_q6l9qz`: 14px on 22px lines on the profile's own tokens.
            let style = crate::editor::EditorStyle {
                size: 14.,
                line_height: 22.,
                text: palette.token("--text-default").into(),
                placeholder: palette.token("--text-muted").into(),
                caret: palette.token("--text-default").into(),
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
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let user = self.stores.users.get(&popout.user)?.clone();
        let id = user.id;
        let palette = self.profile_palette(id, self.guild, false);
        let light = palette
            .theme
            .as_ref()
            .is_some_and(super::profile_theme::ProfileTheme::light);
        let strong: Hsla = palette.text;
        let body = self.fetched.get(&format!("profile:{id}"));
        let private = body.is_some_and(|body| body["private"] == true);
        let display_name = self.guild_display_name(&user);
        let presence = self.stores.status(id).to_owned();
        let banner = if palette.themed {
            palette.top
        } else {
            self.banner_color(&user)
        };
        let for_guild = self.guild.is_some_and(|guild| {
            body.is_some_and(|body| {
                body["guild_member_profile"].is_object()
                    && body["guild_member_profile"]["guild_id"]
                        .as_str()
                        .is_none_or(|id| id == guild.to_string().as_str())
            })
        });
        let pronouns = body
            .and_then(|body| {
                for_guild
                    .then(|| {
                        body["guild_member_profile"]["pronouns"]
                            .as_str()
                            .filter(|pronouns| !pronouns.is_empty())
                    })
                    .flatten()
                    .or_else(|| body["user_profile"]["pronouns"].as_str())
            })
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let bio = body
            .and_then(|body| {
                for_guild
                    .then(|| {
                        body["guild_member_profile"]["bio"]
                            .as_str()
                            .filter(|bio| !bio.is_empty())
                    })
                    .flatten()
                    .or_else(|| body["user_profile"]["bio"].as_str())
            })
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let friends = body
            .and_then(|body| {
                body["mutual_friends"]
                    .as_array()
                    .map(Vec::len)
                    .or_else(|| body["mutual_friends_count"].as_u64().map(|n| n as usize))
            })
            .unwrap_or(0);
        let servers = body
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        let subtle = palette.subtle;
        let plural = |count: usize, one: &str, many: &str| {
            if count == 1 {
                format!("1 {one}")
            } else {
                format!("{count} {many}")
            }
        };
        let header: AnyElement = div()
            .relative()
            .h(px(138.))
            .flex_none()
            .child(
                div()
                    .parity("profile-banner")
                    .h(px(105.))
                    .rounded_t(px(6.))
                    .overflow_hidden()
                    .relative()
                    .bg(banner)
                    .children(
                        self.profile_banner_image(id)
                            .map(|image| img(image).size_full().object_fit(gpui::ObjectFit::Cover)),
                    )
                    .children(self.live_banner(id, 300., 105., cx)),
            )
            .child(
                // `avatar_WRLIGC clickable`: "View Full Profile".
                div()
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
                                self.with_tip(
                                    "popout-avatar-tip",
                                    super::tooltip::Tip::text(
                                        dc_intl::t!("+Xp3hq"),
                                        super::tooltip::Side::Top,
                                    ),
                                    div()
                                        .id("popout-avatar")
                                        .group("other-popout-avatar")
                                        .relative()
                                        .size(px(80.))
                                        .rounded_full()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.profile = None;
                                            this.open_full_profile(id, window, cx);
                                        }))
                                        .child(self.profile_avatar(&user, &presence, cx))
                                        .child(super::profile::popout_avatar_hover_overlay(
                                            "other-popout-avatar",
                                        )),
                                    cx,
                                ),
                            ),
                    ),
            )
            .into_any_element();
        // The buttons on the banner sit on the top layer, above the profile effect (z-index 3 against its 2), so they
        // are drawn after it as children of the card body: 32px down when the private heading precedes the header.
        let actions: AnyElement = div()
            .parity("profile-actions")
            .absolute()
            .top(px(if private { 46. } else { 14. }))
            .right(px(10.))
            .flex()
            .gap(px(8.))
            .child(self.profile_friend_button_at(id, "popout-friend", cx))
            .child(self.profile_more_button_at(id, "popout-more", cx))
            .into_any_element();
        let effect = self.profile_effect_sku(id).and_then(|sku| {
            let hovering = self.profile_effect_hovered("popout");
            self.profile_effect_layer(
                "popout",
                sku,
                super::profile_effects::Surface::profile(300., hovering),
                window,
                cx,
            )
        });
        let names: AnyElement = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("popout-display-name")
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.profile = None;
                        this.open_full_profile(id, window, cx);
                    }))
                    .h(px(24.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(strong)
                    .child(super::name_styles::probed(
                        super::name_styles::name(
                            format!("popout-name-{id}"),
                            display_name.clone(),
                            self.name_styles(&user, self.guild).as_ref(),
                            super::name_styles::Display::Animated,
                        ),
                        "profile-name-styled",
                    )),
            )
            .child(
                div()
                    .min_h(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(6.))
                    .gap_y(px(2.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(strong)
                    .child({
                        let (shown, concealed) = self.profile_username(&user);
                        let line = div()
                            .id("popout-username")
                            .cursor_pointer()
                            .hover(|style| style.underline())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.profile = None;
                                this.open_full_profile(id, window, cx);
                            }))
                            .child(shown);
                        // Module 297413: a concealed name carries "Streamer Mode Enabled".
                        if concealed {
                            self.with_tip(
                                "popout-username-streamer",
                                super::tooltip::Tip::text(
                                    dc_intl::t("Br1ls3"),
                                    super::tooltip::Side::Top,
                                ),
                                line,
                                cx,
                            )
                            .into_any_element()
                        } else {
                            line.into_any_element()
                        }
                    })
                    .when_some(pronouns, |this, pronouns| {
                        this.child(div().size(px(4.)).rounded_full().bg(strong))
                            .child(pronouns)
                    })
                    .children(self.badge_pill("popout", id, &palette, cx)),
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
                        .id("popout-mutual-friends")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.profile = None;
                            this.open_full_profile_tab(
                                id,
                                super::full_profile::Tab::Friends,
                                window,
                                cx,
                            )
                        }))
                        .child(self.mutual_avatars(body))
                        .child(plural(friends, "Mutual Friend", "Mutual Friends")),
                )
            })
            .when(friends > 0 && servers > 0, |this| {
                this.child(div().size(px(4.)).rounded_full().bg(subtle))
            })
            .when(servers > 0, |this| {
                this.child(
                    div()
                        .id("popout-mutual-servers")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.profile = None;
                            this.open_full_profile_tab(
                                id,
                                super::full_profile::Tab::Servers,
                                window,
                                cx,
                            )
                        }))
                        .child(plural(servers, "Mutual Server", "Mutual Servers")),
                )
            })
            .into_any_element();
        let content: AnyElement = div()
            .pl(px(16.))
            .pt(px(4.))
            // The reference reserves an additional 8px scrollbar gutter.
            .pr(px(16.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(names)
            .when(friends > 0 || servers > 0, |this| this.child(mutuals))
            // Module 348434: the private notice (501193 in the profile's `Overlay`) sits after the mutuals and
            // before the bio.
            .when(private, |this| {
                this.child(div().parity("private-profile-notice").child(
                    super::profile_board::private_notice(&display_name, &palette),
                ))
            })
            .when_some(bio, |this, bio| this.child(self.bio(&bio, strong, cx)))
            .children(self.live_activity_cards(
                id,
                super::live_activity::Surface::Popout,
                strong,
                subtle,
                cx,
            ))
            .children(self.popout_voice_card(id, cx))
            .children(self.profile_role_chips(id, strong, cx))
            // Not in the pinned client: ShowConnections (`plugin_hooks/show_connections.rs`).
            .children(self.connections_row(id, false, cx))
            .into_any_element();
        let message: Option<AnyElement> = popout.message.as_ref().map(|message| {
            div()
                .px(px(16.))
                .pb(px(12.))
                .child(
                    div()
                        .h(px(44.))
                        .rounded(px(8.))
                        .bg(palette.token("--background-mod-subtle"))
                        .pl(px(15.))
                        .flex()
                        .items_center()
                        .child(div().flex_1().min_w(px(0.)).child(message.editor.clone()))
                        .child({
                            let field = message.editor.clone();
                            div()
                                .id("popout-emoji")
                                .size(px(32.))
                                .mr(px(4.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_click(cx.listener(
                                    move |this, event: &gpui::ClickEvent, window, cx| {
                                        let at = event.position();
                                        this.picker_field = Some(field.clone());
                                        this.open_picker(
                                            super::picker::Target::Field,
                                            gpui::point(at.x, at.y - px(24.)),
                                            window,
                                            cx,
                                        );
                                    },
                                ))
                                .children(
                                    self.images
                                        .get("emoji_button")
                                        .cloned()
                                        .map(|picture| img(picture).size(px(18.)).grayscale(true)),
                                )
                        }),
                )
                .into_any_element()
        });
        // The frame of module 744808 over the popout (`user-profile-popout`), when the window is taller than 480px.
        let (frame_back, frame_front) = self.profile_frame_layers(
            id,
            304.,
            None,
            super::profile_frames::FrameSurface::Popout,
            f32::from(window.viewport_size().height),
        );
        let card = div()
            .parity("user-popout")
            .id("profile-popout")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("popout", *inside, cx);
            }))
            .occlude()
            .relative()
            .w(px(304.))
            .max_h(window.viewport_size().height - px(20.))
            .p(px(2.))
            .children(frame_back)
            .rounded(px(8.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.),
                linear_color_stop(palette.bottom, 1.),
            ))
            .shadow(vec![
                BoxShadow {
                    color: hsla(0., 0., if light { 0. } else { 1. }, 0.08),
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
                    .relative()
                    .rounded(px(6.))
                    .max_h(window.viewport_size().height - px(24.))
                    .bg(palette.overlay)
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    // `banner_B_p0HC` of module 312381 in the popout: 8px around the lock and Private Profile in
                    // `--text-default`, 8px pulled under the header; `--background-surface-higher`, or on a themed
                    // profile the top of the theme's gradient (primary to secondary over 600px).
                    .when(private, |this| {
                        let text = palette.token("--text-default");
                        this.child(
                            div()
                                .parity("private-profile-heading")
                                .h(px(32.))
                                .flex_none()
                                .mb(px(-8.))
                                .rounded_t(px(6.))
                                .map(|band| match palette.theme.as_ref() {
                                    Some(theme) => {
                                        let (top, bottom) =
                                            super::profile_theme::popout_private_band(theme, 32.);
                                        band.bg(gpui::linear_gradient(
                                            180.,
                                            gpui::linear_color_stop(top, 0.),
                                            gpui::linear_color_stop(bottom, 1.),
                                        ))
                                    }
                                    None => band.bg(color("--background-surface-higher")),
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(4.))
                                .child(
                                    gpui::svg()
                                        .path("icons/access_lock.svg")
                                        .size(px(16.))
                                        .text_color(text),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(text)
                                        .child(dc_intl::t!("KPnd2O")),
                                ),
                        )
                    })
                    .child(header)
                    .child(
                        div()
                            .id("profile-body-scroll")
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .child(content),
                    )
                    .children(message.map(|message| div().flex_none().child(message)))
                    .children(effect)
                    .child(actions),
            )
            .children(frame_front);
        let at: Point<Pixels> = if popout.left {
            point(popout.anchor.left() - px(320.), popout.anchor.top())
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
                            .snap_to_window_with_margin(px(12.))
                            // `animatorLeft` for a member, `animatorRight` for a message author, both translate
                            // (design/dom/popouts/pt-decorated-member, channel/profile-popout).
                            .child(card.popout(
                                "user-popout",
                                PopKind::Translate,
                                if popout.left {
                                    PopSide::Left
                                } else {
                                    PopSide::Right
                                },
                            )),
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
        // A blocked person has no friend actions either: the button is drawn disabled.
        // Not in the pinned client: with CancelFriendRequest an outgoing request is cancelled here (`plugin_hooks.rs`).
        let cancel = relationship == Some(super::friends::RELATIONSHIP_OUTGOING)
            && super::plugin_hooks::cancel_friend_request();
        let outgoing = !cancel
            && matches!(
                relationship,
                Some(super::friends::RELATIONSHIP_OUTGOING | super::blocked::RELATIONSHIP_BLOCKED)
            );
        let sidebar = probe.starts_with("dm-profile");
        let palette = self.profile_palette(user, if sidebar { None } else { self.guild }, sidebar);
        let light = palette
            .theme
            .as_ref()
            .is_some_and(super::profile_theme::ProfileTheme::light);
        div()
            .id(probe)
            .relative()
            .child(self.anchor_probe(probe))
            .child(if cancel {
                super::plugin_hooks::cancel_request_round(glyph, palette.themed, light)
            } else {
                super::dm_profile::round_button_themed(glyph, palette.themed, light)
            })
            .when(outgoing, |this| this.opacity(0.5))
            .when(!outgoing, |this| this.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| match relationship {
                Some(super::friends::RELATIONSHIP_OUTGOING) if cancel => {
                    this.set_relationship(user, None, cx)
                }
                None | Some(0) => this.set_relationship(user, Some(0), cx),
                Some(
                    super::friends::RELATIONSHIP_OUTGOING | super::blocked::RELATIONSHIP_BLOCKED,
                ) => {}
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
        let sidebar = probe.starts_with("dm-profile");
        let palette = self.profile_palette(user, if sidebar { None } else { self.guild }, sidebar);
        let light = palette
            .theme
            .as_ref()
            .is_some_and(super::profile_theme::ProfileTheme::light);
        div()
            .id(probe)
            .relative()
            .cursor_pointer()
            .child(self.anchor_probe(probe))
            .child(super::dm_profile::round_button_themed(
                "more_horizontal",
                palette.themed,
                light,
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_profile_menu_at(user, Which::Overflow, probe, false, cx)
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profile_roles_follow_hierarchy_and_put_linked_roles_last() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch("GUILD_CREATE", &json!({
            "id":"100", "name":"fixture", "channels":[],
            "roles":[
                {"id":"100","name":"everyone","position":0},
                {"id":"101","name":"lower","position":1},
                {"id":"102","name":"older","position":5},
                {"id":"103","name":"newer","position":5},
                {"id":"104","name":"linked","position":10,"tags":{"guild_connections":null}}
            ],
            "members":[{"user":{"id":"200","username":"member"},"roles":["104","103","999","101","100","102"]}]
        }));
        assert_eq!(
            ordered_roles(&stores.guilds[0], stores.members.get(100, 200).unwrap()),
            [102, 103, 101, 104]
        );
    }
}
