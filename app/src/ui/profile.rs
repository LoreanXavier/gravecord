//! The user profile popout: `outer_P1YEjQ`, header `header_NOlkhE`, body `body_NOlkhE`, footer `footer_NOlkhE`.
//! Measured in `design/dom/channel/profile-popout-darker-cozy.json` (this client's own popout). The banner falls back
//! to the avatar palette (`crate::palette`), as module 515718 does.

use super::{Root, avatar_source, color, icon, status_dot_large};
use crate::net::Request;
use crate::palette::avatar_palette;
use dc_state::{Id, User};
use gpui::{
    AnyElement, BoxShadow, Context, Focusable, FontWeight, IntoElement, Pixels, Point, Rgba,
    SharedString, anchored, canvas, deferred, div, hsla, img, point, prelude::*, px, rgba,
};

pub(crate) struct Popout {
    pub user: Id,
    /// The clicked avatar or name, in window coordinates.
    pub anchor: gpui::Bounds<Pixels>,
    /// Members in the member list open their popout to the left; messages open it to the right.
    pub left: bool,
    /// "Message @name" on someone else's popout.
    pub message: Option<super::profile_other::PopoutMessage>,
}

fn profile_key(user: Id) -> String {
    format!("profile:{user}")
}

impl Root {
    /// A clickable avatar or name that opens the popout beside itself.
    pub(super) fn profile_trigger(
        &self,
        key: String,
        user: Id,
        target: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.profile_trigger_side(key, user, target, false, cx)
    }

    pub(super) fn profile_trigger_side(
        &self,
        key: String,
        user: Id,
        target: impl IntoElement,
        left: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let anchors = self.tip_anchors.clone();
        let stored = SharedString::from(key.clone());
        let lookup = stored.clone();
        div()
            .id(SharedString::from(format!("profile-trigger-{key}")))
            .relative()
            .cursor_pointer()
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    let guild = this.guild;
                    this.open_context_menu(
                        super::context_menu::Target::User { user, guild },
                        event.position,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .child(target)
            .child(
                canvas(
                    move |bounds, _, _| {
                        anchors.borrow_mut().insert(stored.clone(), bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                let anchor = this.tip_anchors.borrow().get(&lookup).copied();
                if let Some(anchor) = anchor {
                    this.open_profile(user, anchor, left, cx);
                    // `disableAutoFocus: false`: someone else's popout puts the caret in "Message @name".
                    if let Some(message) = this
                        .profile
                        .as_ref()
                        .and_then(|popout| popout.message.as_ref())
                    {
                        let handle = message.editor.read(cx).focus_handle(cx);
                        window.focus(&handle, cx);
                    }
                }
            }))
            .into_any_element()
    }

    fn open_profile(
        &mut self,
        user: Id,
        anchor: gpui::Bounds<Pixels>,
        left: bool,
        cx: &mut Context<Self>,
    ) {
        let me = self.stores.current_user.as_ref().map(|current| current.id);
        let other = Some(user) != me;
        let mut route = format!(
            "/users/{user}/profile?type=popout&with_mutual_guilds={other}&with_mutual_friends={other}&with_mutual_friends_count=false"
        );
        if let Some(guild) = self.guild {
            route.push_str(&format!("&guild_id={guild}"));
        }
        let _ = self.requests.send(Request::Fetch {
            key: profile_key(user),
            route,
        });
        let message = other.then(|| self.popout_message(user, cx));
        self.profile = Some(Popout {
            user,
            anchor,
            left,
            message,
        });
        cx.notify();
    }

    fn profile_user(&self, user: Id) -> User {
        self.stores
            .users
            .get(&user)
            .cloned()
            .or_else(|| {
                self.stores
                    .current_user
                    .clone()
                    .filter(|current| current.id == user)
            })
            .unwrap_or_default()
    }

    /// `banner_color` or `accent_color` from the profile, else the first avatar palette color.
    pub(super) fn banner_color(&self, user: &User) -> Rgba {
        let body = self.fetched.get(&profile_key(user.id));
        let explicit = body.and_then(|body| {
            body["user_profile"]["accent_color"]
                .as_u64()
                .or(body["user"]["accent_color"].as_u64())
        });
        if let Some(value) = explicit {
            return rgba(((value as u32) << 8) | 0xff);
        }
        let picture = self.images.get(&avatar_source(user).0);
        let rgb = picture
            .and_then(|picture| image::load_from_memory(picture.bytes()).ok())
            .map(|decoded| avatar_palette(decoded.to_rgba8().as_raw())[0])
            .unwrap_or([0, 0, 0]);
        rgba(
            (u32::from(rgb[0]) << 24) | (u32::from(rgb[1]) << 16) | (u32::from(rgb[2]) << 8) | 0xff,
        )
    }

    pub(super) fn badges(&self, user: Id) -> Vec<AnyElement> {
        let Some(body) = self.fetched.get(&profile_key(user)) else {
            return Vec::new();
        };
        body["badges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|badge| badge["icon"].as_str())
            .map(|icon_hash| {
                let key = format!("badge:{icon_hash}");
                let picture = self.images.get(&key).cloned();
                div()
                    .size(px(20.))
                    .children(picture.map(|picture| img(picture).size(px(20.))))
                    .into_any_element()
            })
            .collect()
    }

    /// Badge icons the open popout needs.
    /// Badge icons a fetched profile names.
    pub(super) fn request_profile_images(&mut self, user: Id) {
        let Some(body) = self.fetched.get(&profile_key(user)) else {
            return;
        };
        let wanted: Vec<(String, String)> = body["badges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|badge| badge["icon"].as_str())
            .map(|icon_hash| {
                (
                    format!("badge:{icon_hash}"),
                    format!("https://cdn.discordapp.com/badge-icons/{icon_hash}.png"),
                )
            })
            .collect();
        let mutuals: Vec<(String, String)> = body["mutual_friends"]
            .as_array()
            .into_iter()
            .flatten()
            .take(3)
            .filter_map(super::dm_profile::mutual_user)
            .map(|user| super::avatar_source(&user))
            .collect();
        let banner = super::profile_art::banner_source(user, self.guild, body);
        for (key, url) in wanted.into_iter().chain(mutuals).chain(banner) {
            self.request_image(key, url);
        }
        if let Some(user) = self.stores.users.get(&user).cloned() {
            let (key, url) = self.guild_avatar_source(&user);
            self.request_image(key, url);
        }
    }

    pub(super) fn profile_layer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let popout = self.profile.as_ref()?;
        let user = self.profile_user(popout.user);
        let own = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|current| current.id == user.id);
        if !own {
            return self.other_popout(popout, window, cx);
        }
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let banner = self.banner_color(&user);
        let badges = self.badges(user.id);
        let header = div()
            .relative()
            .h(px(152.5))
            .child(div().h(px(105.)).rounded_t(px(8.)).bg(banner))
            // `avatar_WRLIGC`: 80px, ringed by 6px of the card, its status dot at the lower right.
            .child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(px(55.))
                    .size(px(92.))
                    .rounded_full()
                    .bg(color("--background-surface-high"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .relative()
                            .size(px(80.))
                            .rounded_full()
                            .children(
                                avatar.map(|picture| img(picture).size(px(80.)).rounded_full()),
                            )
                            .child(status_dot_large(
                                &self.stores.status,
                                "--background-surface-high",
                            )),
                    ),
            )
            .when(own, |this| this.child(status_bubble()));
        // The button on the banner is on the top layer, above the profile effect, so it is drawn after it.
        let more = div()
            .absolute()
            .top(px(8.))
            .right(px(12.))
            .size(px(32.))
            .rounded_full()
            .border_1()
            .border_color(hsla(0., 0., 1., 0.08))
            .bg(hsla(0., 0., 0., 0.56))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("more_horizontal", 16., "--white"));
        let effect = self.profile_effect_sku(user.id).and_then(|sku| {
            let hovering = self.profile_effect_hovered("popout");
            self.profile_effect_layer(
                "popout",
                sku,
                super::profile_effects::Surface::profile(300., hovering),
                window,
                cx,
            )
        });
        let names = div()
            .flex()
            .flex_col()
            .child(
                div().h(px(24.)).flex().items_center().gap(px(4.)).child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-default"))
                        .child(super::name_styles::probed(
                            super::name_styles::name(
                                format!("popout-name-{}", user.id),
                                user.display_name().to_owned(),
                                self.name_styles(&user, self.guild).as_ref(),
                                super::name_styles::Display::Animated,
                            ),
                            "profile-name-styled",
                        )),
                ),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(9.33))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child(user.username.clone()),
                    )
                    .when(!badges.is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(22.))
                                .px(px(3.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(badges),
                        )
                    }),
            );
        let owner = self
            .current_guild()
            .is_some_and(|guild| guild.owner_id == Some(user.id));
        let body = div()
            .pl(px(16.))
            .pt(px(4.))
            .pr(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(names)
            .when(own, |this| this.child(upsell()))
            .when(own && owner, |this| {
                // `roleListContainer_xL50-d`: the guild owner can add roles to their own profile here.
                this.child(
                    div()
                        .mt(px(2.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--interactive-text-default"))
                        .child(icon("add_role", 18., "--interactive-text-default"))
                        .child("Add role"),
                )
            });
        let footer = own.then(|| {
            div().px(px(16.)).pb(px(12.)).child(
                div()
                    .h(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.08))
                    .bg(color("--control-primary-background-default"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(gpui::white())
                    .child(icon("msg_edit", 16., "--white"))
                    .child("Edit Profile"),
            )
        });
        let card = div()
            .id("profile-popout")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                this.profile_effect_hover("popout", *inside, cx);
            }))
            .occlude()
            .relative()
            .w(px(300.))
            .rounded(px(8.))
            .bg(color("--background-surface-high"))
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
            .pb(px(4.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(header)
            .child(body)
            .children(footer)
            .children(effect)
            .child(more);
        let at: Point<Pixels> = if popout.left {
            point(popout.anchor.left() - px(308.), popout.anchor.top())
        } else {
            point(popout.anchor.right() + px(8.), popout.anchor.top() - px(1.))
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
                            this.profile = None;
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
}

/// `upsellContainer_vJa-rc`: "Amp up your profile" with Get Nitro and Shop, inside a two-color gradient edge.
pub(super) fn upsell_card() -> impl IntoElement {
    upsell()
}

fn upsell() -> impl IntoElement {
    let button = |icon_name: &'static str, label: &'static str| {
        div()
            .flex_1()
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(icon(icon_name, 20., "--control-secondary-text-default"))
            .child(label)
    };
    // The edge is two 2px gradient borders (#b377f3 to #3e70dd); gpui draws one color, so it takes the first.
    div()
        .rounded(px(8.))
        .border_2()
        .border_color(gpui::rgb(0xb377f3))
        .bg(color("--background-surface-highest"))
        .p(px(10.))
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(12.))
                .h(px(18.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-strong"))
                        .child("Amp up your profile"),
                )
                .child(icon("upsell_close", 16., "--interactive-text-default")),
        )
        .child(
            div()
                .flex()
                .gap(px(8.))
                .child(button("nitro_mark", "Get Nitro"))
                .child(button("shop", "Shop")),
        )
}

/// The bubble's text: an inline add icon, then the placeholder in italic, wrapping at 155px.
fn status_text() -> impl IntoElement {
    let muted: Rgba = color("--text-muted");
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (muted.r * 255.).round() as u8,
        (muted.g * 255.).round() as u8,
        (muted.b * 255.).round() as u8
    );
    let source = include_str!("../../assets/icons/status_add.svg");
    let inner = source
        .split_once('>')
        .map(|(_, rest)| rest.trim_end().trim_end_matches("</svg>"))
        .unwrap_or_default()
        .replace("#000", &hex);
    // A 20px cell: the 16px icon centred on the line, then the 4px margin.
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20"><svg x="0" y="3" width="16" height="16" viewBox="0 0 24 24">{inner}</svg></svg>"#
    );
    let picture = std::sync::Arc::new(gpui::Image::from_bytes(
        gpui::ImageFormat::Svg,
        svg.into_bytes(),
    ));
    let mut italic = gpui::font(super::SANS);
    italic.style = gpui::FontStyle::Italic;
    italic.features = super::kerning();
    let piece =
        |text: &str, image: Option<(std::sync::Arc<gpui::Image>, Pixels)>| crate::inline::Piece {
            text: text.to_owned(),
            font: italic.clone(),
            size: px(14.),
            color: muted.into(),
            underline: false,
            strike: false,
            boxed: None,
            image,
        };
    div().w(px(155.)).child(crate::inline::inline(
        vec![
            piece("\u{fffc}", Some((picture, px(20.)))),
            piece("What are you currently obsessed with?", None),
        ],
        px(18.),
    ))
}

/// `outer_rmCN9_`: this client's empty custom status, a speech bubble with two thought dots trailing to the avatar.
fn status_bubble() -> impl IntoElement {
    let surface = color("--background-surface-highest");
    let dot = |size: f32, left: f32, top: f32| {
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .size(px(size))
            .rounded_full()
            .bg(surface)
            .border_1()
            .border_color(color("--border-muted"))
    };
    div()
        .absolute()
        .left(px(105.))
        .top(px(98.))
        .w(px(181.))
        .child(
            div()
                .relative()
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(surface)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.14),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(4.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .child(dot(20., 10., -8.))
                .child(dot(10., -3., -15.))
                // `content_rmCN9_`: the add icon is inline (16px, 4px after it), so the text wraps under it.
                .child(div().px(px(12.)).py(px(8.)).child(status_text())),
        )
}
