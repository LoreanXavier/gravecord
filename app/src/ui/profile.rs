//! The user profile popout: `outer_P1YEjQ`, header `header_NOlkhE`, body `body_NOlkhE`, footer `footer_NOlkhE`.
//! Measured in `design/dom/channel/profile-popout-darker-cozy.json` (this client's own popout). The banner falls back
//! to the avatar palette (`crate::palette`), as module 515718 does.

use super::motion::{PopKind, PopSide, Popout as _};
use super::{Root, avatar_source, color, icon, status_dot_large};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, BoxShadow, Context, Focusable, FontWeight, IntoElement, Pixels, Point, Rgba,
    SharedString, anchored, canvas, deferred, div, hsla, img, point, prelude::*, px,
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
        let trigger = div()
            .id(SharedString::from(format!("profile-trigger-{key}")))
            .relative()
            .cursor_pointer();
        // The member list is one tab stop whose rows the arrows, Home and End move between (`useListNavigator`,
        // 36544): its first row takes the tab stop.
        let trigger = if left {
            let handle = self.stop(&format!("member-row-{key}"), cx);
            let first = self.join("members", super::keyboard::Nav::List, &handle);
            let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
            // Where Shift+F10 puts the member's menu (`keyboard/context_key.rs`).
            let trigger = trigger.child(self.anchor_probe(format!("stop-{key}")));
            let trigger =
                super::keyboard::tab_stop(trigger, &handle, first && self.modal_open() == 0);
            if ringed {
                trigger.child(super::keyboard::hollow_ring(4.))
            } else {
                trigger
            }
        } else {
            trigger
        };
        trigger
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

    pub(super) fn open_profile(
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

    /// The banner's fill (module 915614), as `dm_profile::profile_banner_fill` reads it.
    pub(super) fn banner_color(&self, user: &User) -> Rgba {
        self.profile_banner_fill(user, self.guild)
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

    /// One line per fetched profile badge: whether its picture was asked for, arrived, or failed (harness `badge state`).
    #[cfg(feature = "capture")]
    pub(crate) fn badge_state_line(&self) -> String {
        let errors = crate::net::IMAGE_ERRORS.lock().ok();
        let mut out = Vec::new();
        for (key, body) in &self.fetched {
            if !key.starts_with("profile:") {
                continue;
            }
            for badge in body["badges"].as_array().into_iter().flatten() {
                let Some(icon) = badge["icon"].as_str() else {
                    continue;
                };
                let image = format!("badge:{icon}");
                out.push(format!(
                    "{key} {} {icon} requested={} bytes={} retries={} error={:?}",
                    badge["id"].as_str().unwrap_or("?"),
                    self.requested.contains(&image),
                    self.images
                        .get(&image)
                        .map_or(0, |image| image.bytes().len()),
                    self.image_failures(&image),
                    errors.as_ref().and_then(|errors| errors.get(&image)),
                ));
            }
        }
        let failed = errors.as_ref().map_or(0, |errors| errors.len());
        let sample: Vec<String> = errors
            .iter()
            .flat_map(|errors| errors.iter())
            .take(6)
            .map(|(key, error)| format!("{key}: {error}"))
            .collect();
        out.push(format!(
            "failed_total={failed} fetch_permits_free={} sample={sample:?}",
            crate::net::image_permits_free()
        ));
        out.join(" | ")
    }

    /// Badge icons a fetched profile names.
    pub(super) fn request_profile_images(&mut self, user: Id) {
        let Some(body) = self.fetched.get(&profile_key(user)) else {
            return;
        };
        let has_badges = body["badges"]
            .as_array()
            .is_some_and(|badges| !badges.is_empty());
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
        if has_badges {
            self.request_badge_catalog(user);
        }
        self.request_profile_cards(user);
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
        let badges = self.profile_badges("popout", user.id, cx);
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
        let body = div()
            .pl(px(16.))
            .pt(px(4.))
            .pr(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(names)
            .when(own, |this| {
                this.child(upsell(self.premium().is_subscriber(), cx))
            })
            .children(own.then(|| self.popout_voice_card(user.id, cx)).flatten())
            .children(
                own.then_some(self.guild)
                    .flatten()
                    .and_then(|guild| self.popout_roles(guild, user.id, cx)),
            );
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
            // The 300px card ends 8px before the member list's edge, 16px before the row (menus2/self-popout: row
            // at 1024, card 708 to 1008; popouts/pt-member-game the same).
            point(popout.anchor.left() - px(316.), popout.anchor.top())
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
                            // 12px kept below the card (menus2/self-popout: its border ends at y=708 of 720).
                            .snap_to_window_with_margin(gpui::Edges {
                                top: px(8.),
                                right: px(8.),
                                bottom: px(12.),
                                left: px(8.),
                            })
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
}

/// `upsellContainer_vJa-rc`: "Amp up your profile" with Get Nitro and Shop, inside a two-color gradient edge.
/// The card for a subscriber differs: module 560296 `$` swaps the Get Nitro button for "View Perks" (string `0Q61kF`,
/// `NitroWheelIcon`) when `isPremiumUser`.
pub(super) fn upsell_card_for(premium: bool, cx: &mut Context<Root>) -> impl IntoElement {
    upsell(premium, cx)
}

/// The first button's icon and label: Get Nitro, or View Perks for a subscriber (module 560296 `$`).
fn upsell_first_button(premium: bool) -> (&'static str, &'static str) {
    if premium {
        ("nitro_wheel", "View Perks")
    } else {
        ("nitro_mark", "Get Nitro")
    }
}

/// What the card's buttons do (module 560296 `$`): View Perks opens the Shop (`APPLICATION_STORE`), Get Nitro opens the
/// premium purchase modal for Nitro (`GET_PREMIUM`, `TIER_2`), Shop opens the Shop; each closes the popout
/// (`onInteraction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpsellAction {
    OpenShop,
    GetNitro,
}

fn upsell_action(premium: bool, shop_button: bool) -> UpsellAction {
    if shop_button || premium {
        UpsellAction::OpenShop
    } else {
        UpsellAction::GetNitro
    }
}

fn upsell(premium: bool, cx: &mut Context<Root>) -> impl IntoElement {
    let button =
        |id: &'static str, icon_name: &'static str, label: &'static str, action: UpsellAction| {
            div()
                .id(id)
                .cursor_pointer()
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.status_popout = None;
                    this.profile = None;
                    match action {
                        UpsellAction::OpenShop => this.open_shop(cx),
                        UpsellAction::GetNitro => this.open_purchase(false, cx),
                    }
                    cx.notify();
                }))
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
                .child({
                    let (icon_name, label) = upsell_first_button(premium);
                    button(
                        "upsell-first",
                        icon_name,
                        label,
                        upsell_action(premium, false),
                    )
                })
                .child(button(
                    "upsell-shop",
                    "shop",
                    "Shop",
                    upsell_action(premium, true),
                )),
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
            hover: None,
            hover_underline: false,
            gradient: None,
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

#[cfg(test)]
mod upsell_tests {
    use super::{UpsellAction, upsell_action, upsell_first_button};

    #[test]
    fn the_upsell_buttons_go_by_tier() {
        // Non-subscriber: Get Nitro buys, Shop opens the Shop. Subscriber: View Perks opens the Shop too.
        assert_eq!(upsell_action(false, false), UpsellAction::GetNitro);
        assert_eq!(upsell_action(false, true), UpsellAction::OpenShop);
        assert_eq!(upsell_action(true, false), UpsellAction::OpenShop);
        assert_eq!(upsell_action(true, true), UpsellAction::OpenShop);
    }

    #[test]
    fn a_subscriber_reads_view_perks_instead_of_get_nitro() {
        assert_eq!(upsell_first_button(false), ("nitro_mark", "Get Nitro"));
        assert_eq!(upsell_first_button(true), ("nitro_wheel", "View Perks"));
    }
}
