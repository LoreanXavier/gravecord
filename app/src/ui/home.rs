//! Home: the DM sidebar, the friends panel and direct messages. Geometry is from `design/dom/home/friends-darker-cozy.json`.

use super::tooltip::{Body, Tip};
use super::{Root, avatar_source, color, icon, motion, status_dot};
use crate::parity::Parity;
use dc_state::{Id, PrivateChannel};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, svg,
};

/// The in-app icon on the Home button: `nd` of module 396655 draws it 48px wide (`SIZE_48`) in the 40px
/// `childWrapper_JJ-hgR`, centred, so the tile's rounded mask shows its middle 40 of 48.
#[derive(Clone, Hash, PartialEq, Eq)]
struct RailIconSource(std::sync::Arc<gpui::Image>);

struct RailIcon;

/// The part of a `width` x `height` picture drawn 48px wide that a centred 40px tile shows: (x, y, width, height).
fn rail_icon_crop(width: u32, height: u32) -> (u32, u32, u32, u32) {
    let keep = |side: u32| ((u64::from(side) * 40 + 24) / 48).max(1) as u32;
    let (kept_width, kept_height) = (keep(width), keep(height));
    (
        (width - kept_width) / 2,
        (height - kept_height) / 2,
        kept_width,
        kept_height,
    )
}

impl gpui::Asset for RailIcon {
    type Source = RailIconSource;
    type Output = Option<std::sync::Arc<gpui::RenderImage>>;

    fn load(
        source: Self::Source,
        cx: &mut gpui::App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move {
            let decoded = source.0.to_image_data(renderer).ok()?;
            let size = decoded.size(0);
            let (width, height) = (u32::from(size.width), u32::from(size.height));
            if width == 0 || height == 0 {
                return None;
            }
            let pixels = image::RgbaImage::from_raw(width, height, decoded.as_bytes(0)?.to_vec())?;
            let (x, y, width, height) = rail_icon_crop(width, height);
            let cropped = image::imageops::crop_imm(&pixels, x, y, width, height).to_image();
            Some(std::sync::Arc::new(gpui::RenderImage::new(vec![
                image::Frame::new(cropped),
            ])))
        }
    }
}

impl Root {
    pub(super) fn select_home(&mut self, channel: Option<Id>) {
        super::last_channel::remember_home();
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: false,
                message_requests: false,
            },
        );
        self.discover.open = false;
        self.shop.open = false;
        self.quests.open = false;
        self.message_requests.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.guild = None;
        self.channel = channel;
        // A message menu and the delete question act on the channel that was open when they were asked.
        self.menu = None;
        self.submenu = None;
        self.confirm_delete = None;
        self.note_activity();
        self.unread_from = None;
        if let Some(channel) = channel {
            self.remember_channel(channel);
        }
        if let Some(channel) = channel {
            self.open_channel_messages(channel);
            // `CHANNEL_SELECT` asks the gateway for the channel's call (module 470710 `A`).
            let private = self
                .stores
                .private_channels
                .iter()
                .any(|dm| dm.id == channel);
            self.stores.calls.connect_channel(channel, private, false);
            self.calls_after_dispatch_effects();
        }
        // The DM welcome shows mutual servers from the other person's profile.
        let recipient = self
            .current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .map(|user| user.id);
        if let Some(user) = recipient {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: format!("profile:{user}"),
                route: format!(
                    "/users/{user}/profile?type=popout&with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=false"
                ),
            });
        }
        self.stop_lurking_elsewhere();
        self.rebuild_rows();
        self.request_images();
    }

    pub(super) fn home_tile(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.guild.is_none() && !self.discover.open && !self.in_favorites();
        let requests = self.friend_request_count();
        // Hovered, the tile takes the selected colors and a 20px pill, as the rail's guild tiles do.
        let row_key = "home";
        // `nd` (module 396655): the chosen in-app icon instead of Clyde, once it is not the default one.
        let glyph: AnyElement = match self.rail_app_icon() {
            Some(picture) => {
                let source = RailIconSource(picture);
                img(move |window: &mut gpui::Window, cx: &mut gpui::App| {
                    window.use_asset::<RailIcon>(&source, cx).flatten().map(Ok)
                })
                .size(px(40.))
                .rounded(px(12.))
                .flex_none()
                .into_any_element()
            }
            None => svg()
                .path(crate::branding::home_path())
                .size(px(24.))
                .flex_none()
                .text_color(motion::rail::tile_foreground(row_key, selected))
                .into_any_element(),
        };
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex_none()
            .flex()
            .justify_center()
            .child(motion::rail::pill(row_key, selected, false))
            .child(motion::hover_probe(motion::rail::hover_key(row_key)))
            .child(
                self.with_tip(
                    "home",
                    Tip::rail(Body::Heading(dc_intl::tr("Direct Messages").into())),
                    self.group_stop(
                        div().id("home"),
                        "rail",
                        super::keyboard::Nav::List,
                        "home",
                        self.guild.is_none(),
                        cx,
                    )
                    .size(px(40.))
                    .rounded(px(12.))
                    .bg(motion::rail::tile_background(
                        row_key,
                        selected,
                        "--background-mod-subtle",
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.select_home(None);
                        cx.notify();
                    }))
                    .child(glyph),
                    cx,
                ),
            )
            .when(requests > 0, |tile| {
                tile.child(super::unread_badges::rail_number(
                    requests,
                    false,
                    Some("home-request-badge"),
                ))
            })
    }

    /// Friends, Nitro, Shop and Quests rows.
    fn home_link(
        &self,
        id: &'static str,
        name: &str,
        label: &str,
        selected: bool,
    ) -> gpui::Stateful<gpui::Div> {
        let tint = if selected {
            "--text-strong"
        } else {
            "--text-muted"
        };
        div()
            .id(id)
            .flex_none()
            .h(px(40.))
            .pt(px(1.))
            .pl(px(9.))
            .child(
                div()
                    .id(SharedString::from(format!("home-link-inner-{id}")))
                    .h(px(38.))
                    .w(px(286.))
                    .pl(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(selected, |this| {
                        this.parity(match id {
                            "nitro" => "nitro-link",
                            "shop" => "shop-link",
                            "quests" => "quests-link",
                            "message-requests" => "message-requests-link",
                            _ => "friends-link",
                        })
                        .bg(color("--interactive-background-selected"))
                    })
                    // `.interactive_zln_tS:hover` and `:active` (834050).
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--interactive-background-hover")))
                            .active(|style| style.bg(color("--interactive-background-selected")))
                    })
                    .child(icon(name, 20., tint))
                    .child(
                        div()
                            .when(id == "friends" || id == "message-requests", |label| {
                                label.flex_1()
                            })
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(tint))
                            .child(label.to_owned()),
                    )
                    .when(id == "friends" && self.friend_request_count() > 0, |row| {
                        row.child(
                            super::unread_badges::number(
                                self.friend_request_count(),
                                "--badge-notification-background",
                                Some("friends-request-badge"),
                            )
                            .mr(px(16.)),
                        )
                    })
                    // `IL`: the badge counts message requests, not spam, and only above zero.
                    .when(
                        id == "message-requests" && self.stores.message_requests_count() > 0,
                        |row| {
                            row.child(
                                super::unread_badges::number(
                                    self.stores.message_requests_count() as u64,
                                    "--badge-notification-background",
                                    Some("message-requests-badge"),
                                )
                                .mr(px(16.)),
                            )
                        },
                    ),
            )
    }

    pub(super) fn dm_row(
        &self,
        channel: &PrivateChannel,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if channel.kind == 3 {
            return Some(self.group_list_row(channel, cx));
        }
        let recipient = self.stores.dm_recipient(channel)?.clone();
        // Blocked or ignored people carry their mark where the close button would be.
        let marked = self.is_blocked(recipient.id) || self.stores.is_ignored(recipient.id);
        let id = channel.id;
        let timer = self
            .call
            .as_ref()
            .filter(|call| call.guild == 0 && call.channel == id)
            .and_then(|_| self.call_timer());
        let selected = self.channel == Some(id);
        let avatar = self.images.get(&avatar_source(&recipient).0).cloned();
        let status = self.stores.status(recipient.id).to_owned();
        // Text takes its hover colour at layout time, which gpui's `hover` cannot reach, so the row tracks it.
        let tint = if selected || self.hovered_dm == Some(id) {
            "--text-strong"
        } else {
            "--text-muted"
        };
        let group = SharedString::from(format!("dm-row-{id}"));
        // `Pinned` (flag 2048 of the "@me" channel override, `xl` of module 816662).
        let pinned = self.stores.guild_settings.channel(0, id).flags & 2048 != 0;
        Some(
            div()
                .id(SharedString::from(format!("dm-{id}")))
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                        this.open_context_menu(
                            super::context_menu::Target::Dm(id),
                            event.position,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .group(group.clone())
                .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                    if *inside {
                        this.hovered_dm = Some(id);
                    } else if this.hovered_dm == Some(id) {
                        this.hovered_dm = None;
                    }
                    cx.notify();
                }))
                .flex_none()
                .h(px(44.))
                .pt(px(1.))
                .pl(px(9.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_home(Some(id));
                    cx.notify();
                }))
                .child(
                    div()
                        .id(SharedString::from(format!("dm-inner-{id}")))
                        .h(px(42.))
                        .w(px(286.))
                        .pl(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .text_color(color(tint))
                        // `.interactive_zln_tS:hover`, `:active` and `.interactiveSelected_zln_tS` (834050): a pressed
                        // row looks like the selected one.
                        .when(selected, |this| {
                            this.bg(color("--interactive-background-selected"))
                        })
                        .when(!selected, |this| {
                            this.hover(|style| style.bg(color("--interactive-background-hover")))
                                .active(|style| {
                                    style.bg(color("--interactive-background-selected"))
                                })
                        })
                        .child(
                            div()
                                .relative()
                                .size(px(32.))
                                .children(
                                    self.live_avatar(
                                        &recipient,
                                        32.,
                                        self.hovered_dm == Some(id),
                                        cx,
                                    )
                                    .or_else(|| {
                                        avatar.map(|image| {
                                            img(image)
                                                .size(px(32.))
                                                .rounded_full()
                                                .into_any_element()
                                        })
                                    }),
                                )
                                .child(status_dot(&status, "--background-base-lowest")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .min_w(px(0.))
                                        .child(
                                            div()
                                                .min_w(px(0.))
                                                .text_ellipsis()
                                                .text_size(px(16.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(500.))
                                                .child(super::name_styles::name(
                                                    format!("dm-name-{id}"),
                                                    self.private_channel_name(channel),
                                                    // 110574: a 1:1 DM's row shows the effect, animated under the pointer.
                                                    if channel.kind == 1 {
                                                        self.name_styles(&recipient, None)
                                                    } else {
                                                        None
                                                    }
                                                    .as_ref(),
                                                    if self.hovered_dm == Some(id) {
                                                        super::name_styles::Display::Animated
                                                    } else {
                                                        super::name_styles::Display::Static
                                                    },
                                                )),
                                        )
                                        .children(
                                            recipient
                                                .tag
                                                .as_ref()
                                                .map(|tag| self.server_tag_chip(tag)),
                                        ),
                                )
                                .children(self.presence_line_with(
                                    &recipient,
                                    None,
                                    true,
                                    16.,
                                    // A selected or hovered row lifts its activity line to the row's text colour.
                                    if selected || self.hovered_dm == Some(id) {
                                        "--text-strong"
                                    } else {
                                        "--channels-default"
                                    },
                                )),
                        )
                        // `callDuration_zln_tS` in `iconsContainer_zln_tS` (4px before, 8px after): the running call's
                        // time, `body-xs/normal` 12px / 16px in the code font, `--text-feedback-positive`.
                        .children(timer.clone().map(|text| {
                            div().flex_none().pl(px(4.)).pr(px(8.)).child(
                                div()
                                    .parity("dm-call-timer")
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_family(super::MONO)
                                    .text_color(color("--text-feedback-positive"))
                                    .child(text),
                            )
                        }))
                        .when(marked, |this| {
                            // `favoriteIcon_zln_tS`: the blocked (or ignored) mark, 16px with 8px after it.
                            this.child(div().mr(px(8.)).child(icon(
                                if self.is_blocked(recipient.id) {
                                    "blocked"
                                } else {
                                    "eye_slash"
                                },
                                16.,
                                "--text-default",
                            )))
                        })
                        .when(!marked && timer.is_none(), |this| {
                            // The close button is not laid out while the call timer shows.
                            this.child(
                                // `closeButton_zln_tS` "Close DM": 18px, 8px from the right, at 0.7 while the row is
                                // hovered. `closePrivateChannel`: DELETE /channels/{id}?silent=false, then Friends if it
                                // was open.
                                div()
                                    .id(SharedString::from(format!("dm-close-{id}")))
                                    .mr(px(8.))
                                    .size(px(18.))
                                    .flex_none()
                                    .rounded_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    // A pinned DM keeps the button at rest (frame menus2/dm-pinned: the row's own
                                    // text colour at 0.7); the others show it under the pointer.
                                    .opacity(if pinned { 0.7 } else { 0. })
                                    .group_hover(group, |style| style.opacity(0.7))
                                    .hover(|style| style.opacity(1.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.close_dm(id, cx);
                                    }))
                                    .child(icon("friend_ignore", 16., tint)),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// `chipletContainerInner_k8jyN7`: the server tag after a name, 4px on, a 14px badge and 12/16 semibold text.
    pub(super) fn server_tag_chip(&self, tag: &dc_state::ServerTag) -> gpui::Div {
        let badge = tag
            .badge_url()
            .and_then(|url| self.images.get(&url).cloned());
        div()
            .ml(px(7.5))
            .h(px(16.))
            .px(px(4.))
            .rounded(px(4.))
            .bg(color("--background-mod-strong"))
            .flex()
            .items_center()
            .children(badge.map(|image| img(image).size(px(14.)).mr(px(2.))))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(tag.tag.clone()),
            )
    }

    pub(super) fn close_dm(&mut self, channel: Id, cx: &mut Context<Self>) {
        self.close_dm_with(channel, false, cx);
    }

    /// `closePrivateChannel(id, _, silent)`: the channel goes at once, then the service is told (`silent` leaves a group
    /// without a system message).
    pub(super) fn close_dm_with(&mut self, channel: Id, silent: bool, cx: &mut Context<Self>) {
        self.stores
            .private_channels
            .retain(|known| known.id != channel);
        if self.channel == Some(channel) && self.guild.is_none() {
            self.select_home(None);
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/channels/{channel}?silent={silent}"),
            body: None,
        });
        cx.notify();
    }

    pub(super) fn home_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shop_selected = self.shop_active();
        let quests_selected = self.quests_active();
        let requests_selected = self.message_requests_active();
        let friends_selected = self.guild.is_none()
            && self.channel.is_none()
            && !self.nitro_open
            && !shop_selected
            && !quests_selected
            && !requests_selected;
        let nitro_selected = self.guild.is_none() && self.channel.is_none() && self.nitro_open;
        // 645959: a message request or spam channel is in neither sorted list; pinned conversations (`FAVORITE`, the
        // "@me" override flag) come first, each group newest first.
        let mut listed: Vec<_> = self
            .stores
            .private_channels
            .iter()
            .filter(|channel| !channel.is_request())
            .collect();
        listed.sort_by_key(|channel| {
            self.stores.guild_settings.channel(0, channel.id).flags & 2048 == 0
        });
        let rows: Vec<AnyElement> = listed
            .into_iter()
            .filter_map(|channel| self.dm_row(channel, cx))
            .collect();
        div()
            .parity("sidebar")
            .relative()
            .flex_none()
            .w(px(303.))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_l_1()
                    .border_t_1()
                    .rounded_tl(px(12.))
                    .border_color(color("--background-mod-subtle")),
            )
            .child(div().flex_none().h(px(1.)))
            .child(
                div()
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .pl(px(9.))
                    .pr(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .parity("find-button")
                            .id("find-button")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_quick_switcher(window, cx);
                            }))
                            .h(px(32.))
                            .rounded(px(8.))
                            .bg(color("--background-mod-subtle"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::tr("Find or start a conversation")),
                    ),
            )
            .child(div().flex_none().h(px(8.)))
            .child(
                self.home_link(
                    "friends",
                    "home_friends",
                    dc_intl::tr("Friends"),
                    friends_selected,
                )
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.select_home(None);
                    cx.notify();
                })),
            )
            // 596339 `Iy` sits after Friends; 309199 `SM`: it shows while the requests or the spam list is not empty.
            .children(self.stores.message_requests_entry_visible().then(|| {
                self.home_link(
                    "message-requests",
                    "message_requests_envelope",
                    dc_intl::tr("Message Requests"),
                    requests_selected,
                )
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.open_message_requests(cx)))
            }))
            .child(
                self.home_link("nitro", "home_nitro", dc_intl::tr("Nitro"), nitro_selected)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
            )
            .child(
                self.home_link("shop", "home_shop", dc_intl::tr("Shop"), shop_selected)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.open_shop(cx)))
                    .relative()
                    // The NEW pill is gone once the page is open (the Shop capture has none).
                    .when(!shop_selected && !quests_selected, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left(px(236.))
                                .top(px(12.))
                                .h(px(16.))
                                .px(px(8.))
                                .rounded(px(8.))
                                .bg(color("--white"))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--background-surface-highest"))
                                .child(dc_intl::tr("NEW")),
                        )
                    }),
            )
            .child(
                self.home_link(
                    "quests",
                    "home_quests",
                    dc_intl::tr("Quests"),
                    quests_selected,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_quests(cx))),
            )
            .child(
                div()
                    .flex_none()
                    .mt(px(12.))
                    .ml(px(9.))
                    .w(px(285.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .child(
                div()
                    .flex_none()
                    .mt(px(12.))
                    .h(px(24.))
                    .pl(px(17.))
                    .pr(px(16.))
                    .flex()
                    .items_start()
                    .pt(px(1.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    // `.privateChannelsHeaderContainer_Bce46W:hover` (814796).
                    .hover(|style| style.text_color(color("--interactive-text-hover")))
                    .child(dc_intl::tr("Direct Messages"))
                    .child(div().flex_1())
                    .child(
                        // "Create DM": the same New Message picker as the Friends page's New Group DM.
                        div()
                            .id("dm-plus")
                            .mt(px(1.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_group_dm_picker(window, cx)
                            }))
                            .child(icon("dm_plus", 16., "--text-muted")),
                    ),
            )
            .children(rows)
    }

    pub(super) fn friends_panel(
        &mut self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let people = self.friends_people(window, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(self.friends_header(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(color("--background-base-lower"))
                    .child(
                        div()
                            .parity("people-column")
                            .w(px(545.))
                            .flex()
                            .flex_col()
                            .child(people),
                    )
                    .child(
                        div()
                            .parity_left("active-now", 1.)
                            .w(px(360.))
                            .h_full()
                            .border_l_1()
                            .border_color(color("--background-mod-subtle"))
                            .pl(px(16.))
                            .pr(px(16.))
                            .pt(px(14.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Active Now"),
                            )
                            .child(
                                div()
                                    .parity("active-card")
                                    .mt(px(9.))
                                    .h(px(92.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-low"))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .p(px(16.))
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child("It's quiet for now..."),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.))
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_center()
                                            .text_color(color("--interactive-text-default"))
                                            .child("When a friend starts an activity—like playing a game or hanging out on voice—we'll show it here!"),
                                    ),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::rail_icon_crop;

    #[test]
    fn the_home_button_shows_the_middle_of_a_48px_icon() {
        // A 96px asset drawn 48px wide: the 40px tile shows its middle 80px.
        assert_eq!(rail_icon_crop(96, 96), (8, 8, 80, 80));
        assert_eq!(rail_icon_crop(48, 48), (4, 4, 40, 40));
        assert_eq!(rail_icon_crop(1, 1), (0, 0, 1, 1));
    }
}
