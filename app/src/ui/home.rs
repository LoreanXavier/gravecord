//! Home: the DM sidebar, the friends panel and direct messages. Geometry is from `design/dom/home/friends-darker-cozy.json`.

use super::tooltip::{Body, Tip};
use super::{Root, avatar_source, color, icon, motion};
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
        // `CHANNEL_SELECT` of module 578818: opening the system DM clears the urgent message flag.
        self.urgent_message_channel_selected(channel);
        super::last_channel::remember_home();
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel,
                favorites: false,
                nitro: false,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: false,
                message_requests: false,
                library: false,
            },
        );
        self.favorites_view = false;
        self.discover.open = false;
        self.shop.open = false;
        self.quests.open = false;
        self.message_requests.open = false;
        self.library.open = false;
        self.nitro_open = false;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        self.guild = None;
        self.channel = channel;
        // A message menu and the delete question act on the channel that was open when they were asked.
        self.menu = None;
        self.submenu = None;
        self.confirm_delete = None;
        self.selection.borrow_mut().clear_all();
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
            .map(|user| (user.id, user.bot));
        // A bot's DM reads the authorized apps for its Mute and Deauthorize buttons (module 891522 `s9`).
        if recipient.is_some_and(|(_, bot)| bot) {
            self.request_authorized_apps();
        }
        if let Some((user, _)) = recipient {
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: format!("profile:{user}"),
                route: self.dm_profile_route(user),
            });
            self.prefetch_known_wishlist(user);
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
                    .w(px(self.sidebar_row_width()))
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
                    // NoPendingCount hides the badge, not the row (`getRealMessageRequestCount` keeps it listed).
                    .when(
                        id == "message-requests"
                            && self.stores.message_requests_count() > 0
                            && !super::plugin_hooks::hide_message_request_count(),
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
        // 181254 `eY`: `t.isSystemDM()` (the system account's DM).
        let system = self.stores.is_system_dm(channel);
        let timer = self
            .call
            .as_ref()
            .filter(|call| call.guild == 0 && call.channel == id)
            .and_then(|_| self.call_timer());
        let selected = self.guild.is_none()
            && self.channel == Some(id)
            && !self.nitro_open
            && !self.shop_active()
            && !self.quests_active()
            && !self.message_requests_active()
            && !self.library_active();
        let hovered = self.hovered_dm == Some(id);
        let context_menu_open = self
            .context
            .open
            .as_ref()
            .is_some_and(|open| open.target == super::context_menu::Target::Dm(id));
        let row_selected = selected || context_menu_open;
        let avatar = self.images.get(&avatar_source(&recipient).0).cloned();
        let status = self.stores.status(recipient.id).to_owned();
        let look = row_look_with_menu(
            selected,
            hovered,
            context_menu_open,
            self.stores.read_states.mentions(id) > 0,
            marked
                || self
                    .stores
                    .guild_settings
                    .channel_muted(0, id, chrono::Utc::now()),
        );
        // Text takes its hover colour at layout time, which gpui's `hover` cannot reach, so the row tracks it.
        let tint = look.row;
        let typing = self
            .stores
            .typing_users(id, std::time::Instant::now())
            .contains(&recipient.id);
        let group = SharedString::from(format!("dm-row-{id}"));
        // `Pinned` (flag 2048 of the "@me" channel override, `xl` of module 816662).
        let pinned = self.stores.guild_settings.channel(0, id).flags & 2048 != 0;
        Some(
            div()
                .id(SharedString::from(format!("dm-{id}")))
                .on_mouse_up(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
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
                .relative()
                .h(px(44.))
                .pt(px(1.))
                .pl(px(9.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_home(Some(id));
                    cx.notify();
                }))
                .children(look.pill.then(|| unread_pill(look.dimmed)))
                .child(
                    div()
                        .id(SharedString::from(format!("dm-inner-{id}")))
                        // Not in the pinned client: MessagePeek's age of the last message
                        // (`plugin_hooks/message_peek.rs`).
                        .relative()
                        .children(self.message_peek_time(id, hovered))
                        .h(px(42.))
                        .w(px(self.sidebar_row_width()))
                        .pl(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .text_color(color(tint))
                        // `.interactive_zln_tS:hover`, `:active` and `.interactiveSelected_zln_tS` (834050; `s || eo` in
                        // 181254): a pressed or menu-open row looks like the selected one.
                        .when(row_selected, |this| {
                            this.bg(color("--interactive-background-selected"))
                        })
                        .when(!row_selected, |this| {
                            this.hover(|style| style.bg(color("--interactive-background-hover")))
                                .active(|style| {
                                    style.bg(color("--interactive-background-selected"))
                                })
                        })
                        .child(
                            div()
                                .relative()
                                .size(px(32.))
                                // `.muted_TBNqol .avatar_TBNqol`.
                                .when(look.dimmed, |avatar| avatar.opacity(0.3))
                                .children(self.live_avatar(&recipient, 32., hovered, cx).or_else(
                                    || {
                                        if avatar.is_none() {
                                            let (k, u) = super::avatar_source(&recipient);
                                            self.want_image(k, u, cx);
                                        }
                                        avatar.map(|image| {
                                            img(image)
                                                .size(px(32.))
                                                .rounded_full()
                                                .into_any_element()
                                        })
                                    },
                                ))
                                // `r.isSystemUser() || (n = ...)`: the system account's avatar has no status.
                                .when(!system || typing, |this| {
                                    this.child(super::typing_avatar::status_corner(
                                        &status,
                                        typing,
                                        "--background-base-lowest",
                                        32.,
                                        motion::key(&("dm-row-avatar", id)),
                                        super::typing_avatar::css_dots(&self.stores.experiments),
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .text_color(color(look.text))
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
                                                .font_weight(look.weight())
                                                .child(
                                                    super::name_styles::name(
                                                        format!("dm-name-{id}"),
                                                        self.private_channel_name(channel),
                                                        // 181254 `eY`: `PLAIN` at rest (`.9` for bold faces), `ANIMATED`
                                                        // once when selected or its menu is open, looping on hover.
                                                        if channel.kind == 1 {
                                                            self.name_styles(&recipient, None)
                                                        } else {
                                                            None
                                                        }
                                                        .as_ref(),
                                                        dm_name_display(
                                                            selected,
                                                            hovered,
                                                            context_menu_open,
                                                        ),
                                                    )
                                                    .bold_opacity(DM_BOLD_FONT_OPACITY),
                                                ),
                                        )
                                        // `clanTag_zln_tS` 4px on; `clanTagMuted_zln_tS` at .3.
                                        .children(recipient.tag.as_ref().map(|tag| {
                                            self.server_tag_chip(tag)
                                                .when(look.dimmed, |chip| chip.opacity(0.3))
                                        }))
                                        // `decorators`: the `SYSTEM_DM` tag (module 709066, `verified`) in
                                        // `decorator_zln_tS` (4px on).
                                        .when(system, |this| {
                                            this.child(
                                                self.app_tag_px(
                                                    super::message_header::AppTag::Official,
                                                    format!("dm-row-official-{id}"),
                                                    cx,
                                                )
                                                .ml(px(4.)),
                                            )
                                        }),
                                )
                                // `activityStatusText_zln_tS` is `color: inherit`: the line takes the row's text colour.
                                // Not in the pinned client: MessagePeek's last message in its place
                                // (`plugin_hooks/message_peek.rs`).
                                // `subText` of a system DM: `body-xs/medium` (12px / 16px, 500) in the row's colour,
                                // `subtext_zln_tS` cut with an ellipsis: "Discord Product Updates" (`FL5T01`) for the DM
                                // of the product-updates account (module 343328, `tL` of module 559868), else "Official
                                // Discord Message" (`NnY5lc`).
                                .when(system, |this| {
                                    this.child(
                                        div()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .child(if recipient.id == PRODUCT_UPDATES_USER {
                                                dc_intl::t("FL5T01")
                                            } else {
                                                dc_intl::t("NnY5lc")
                                            }),
                                    )
                                })
                                .children(
                                    (!system)
                                        .then(|| {
                                            let presence = self.presence_line_with(
                                                &recipient, None, true, 16., look.text,
                                            );
                                            match self.message_peek(channel, presence.is_some()) {
                                                super::plugin_hooks::Peek::Keep => presence,
                                                super::plugin_hooks::Peek::Empty => None,
                                                super::plugin_hooks::Peek::Line(line) => Some(line),
                                            }
                                        })
                                        .flatten(),
                                ),
                        )
                        // `callDuration_zln_tS` in `iconsContainer_zln_tS` (4px before, 8px after): the running call's
                        // time, `body-xs/normal` 12px / 16px in the code font, `--text-feedback-positive`.
                        .children(timer.clone().filter(|_| !hovered).map(|text| {
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
                        .when(marked && !hovered, |this| {
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
                        .when(hovered || (!marked && timer.is_none()), |this| {
                            // `closeButton_zln_tS` replaces the call timer / favorite mark on row hover.
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
            && !requests_selected
            && !self.library_active();
        let nitro_selected = self.guild.is_none() && self.channel.is_none() && self.nitro_open;
        // 645959: a message request or spam channel is in neither sorted list; pinned conversations (`FAVORITE`, the
        // "@me" override flag) come first, each group newest first.
        let listed = dc_state::private_channels::sorted_channels(&self.stores);
        let no_private_channels = listed.is_empty();
        let rows: Vec<AnyElement> = listed
            .into_iter()
            .filter_map(|channel| self.dm_row(channel, cx))
            .collect();
        div()
            .parity("sidebar")
            .relative()
            .flex_none()
            .w(px(self.sidebar_width()))
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
                                .parity("shop-new-badge")
                                .absolute()
                                // The inset row's 16px end padding, plus its 8px outer gap.
                                .right(px(24.))
                                .top(px(12.))
                                .h(px(16.))
                                .px(px(8.))
                                .rounded(px(8.))
                                .bg(color("--badge-expressive-background"))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--badge-expressive-text"))
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
                    .w(px(self.sidebar_row_width() - 1.))
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
            .when(no_private_channels, |list| {
                list.child(no_private_channels_art())
            })
    }

    /// `FriendsStore.isInitialized()` (module 595623): true from the session's own `CONNECTION_OPEN`. A READY replayed
    /// from the start cache is not it: the friends' presences come with the live session, so before it every friend
    /// would be listed offline.
    pub(super) fn friends_initialized(&self) -> bool {
        self.stores.current_user.is_some() && !self.hydrated_from_cache
    }

    pub(super) fn friends_panel(
        &mut self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 596339 `AQ`: until the store is initialized the whole page is `Aq`, the default spinner centred on
        // `loading_zrUwGA` (`design/dom/loading/connecting.json`: no tab bar, no "no friends online" note).
        if !self.friends_initialized() {
            return div()
                .parity("friends-loading")
                .flex_1()
                .size_full()
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .child(super::login::wandering_cubes())
                .into_any_element();
        }
        let people = self.friends_people(window, cx);
        let wide = f32::from(window.viewport_size().width) > 1200.;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(self.friends_header(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .w_full()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .bg(color("--background-base-lower"))
                    // `peopleColumn_zrUwGA` is `flex: 1 1 auto`, `nowPlayingColumn_zrUwGA` `flex: 0 1 30%` between 360 and
                    // 420 wide and `display: none` at a window of 1200 or less (599618's stylesheet).
                    .child(
                        div()
                            .parity("people-column")
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .child(people),
                    )
                    .when(wide, |row| row.child(self.active_now_column(cx))),
            )
            .into_any_element()
    }
}

/// The DM list with no conversations (module 830507 `Q`, the `no-private-channels` row): ten rows of a 32px circle and
/// a 144 x 20 pill, 44px apart, fading from 1 to .1, in a 184 x 428 picture with 16px of padding
/// (`empty_Bce46W`: `fill: var(--background-base-low)`). The list draws it before READY too.
fn no_private_channels_art() -> impl IntoElement {
    let fill = color("--background-base-low");
    div()
        .parity("no-private-channels")
        .flex_none()
        .relative()
        // The list's 1px border, then the picture's padding.
        .ml(px(17.))
        .mt(px(16.))
        .w(px(184.))
        .h(px(428.))
        .children((0..10).map(move |row| {
            let top = 44. * row as f32;
            let opacity = 1. - 0.1 * row as f32;
            div()
                .absolute()
                .left_0()
                .top(px(top))
                .w(px(184.))
                .h(px(32.))
                .opacity(opacity)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .size(px(32.))
                        .rounded_full()
                        .bg(fill),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(40.))
                        .top(px(6.))
                        .w(px(144.))
                        .h(px(20.))
                        .rounded(px(10.))
                        .bg(fill),
                )
        }))
}

/// `tL` of module 559868: the account whose DM is "Discord Product Updates" (module 343328).
const PRODUCT_UPDATES_USER: Id = 1_232_523_165_893_132_288;

/// How a row of the DM list looks (module 181254 `eY` and its stylesheets 834050, 884848 and 121046).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowLook {
    /// The row's text colour: `.interactive_zln_tS` (`--text-muted`), `.interactive_nl_k3a:hover`
    /// (`--interactive-text-hover`, which outranks the selected rule) and `.interactiveSelected_zln_tS`
    /// (`--text-default`; the DOM of a selected row computes to it).
    pub row: &'static str,
    /// The name's and the activity line's colour (`layout_TBNqol`): `--text-strong` when highlighted, `--text-muted`
    /// when muted, else the row's.
    pub text: &'static str,
    /// `unreadPill_zln_tS`: the account has mentions waiting here (for a DM every message counts).
    pub pill: bool,
    /// `eZ`: muted, ignored or blocked, and neither selected nor hovered: the pill, the avatar, the clan tag and
    /// the activity icon at .3.
    pub dimmed: bool,
    /// `highlighted: e$ && !eZ`.
    pub highlighted: bool,
}

impl RowLook {
    /// `.highlighted_TBNqol .name_TBNqol` is `var(--font-weight-semibold-1x-light-theme, 500)`: 600 in the light
    /// theme, 500 in the others (the name is `body-md/medium`, 500, at rest).
    pub(super) fn weight(&self) -> FontWeight {
        if self.highlighted && super::theme() == dc_tokens::Theme::Light {
            FontWeight(600.)
        } else {
            FontWeight(500.)
        }
    }
}

/// 181254 `eY`: `boldFontOpacity: .9` on a DM row's `UserNameWithEffects`.
const DM_BOLD_FONT_OPACITY: f32 = 0.9;

/// 181254 `eY`: `effectDisplayType: b || s || eo ? ANIMATED : PLAIN, loop: b`.
fn dm_name_display(
    selected: bool,
    hovered: bool,
    context_menu_open: bool,
) -> super::name_styles::Display {
    if hovered {
        super::name_styles::Display::Animated
    } else if selected || context_menu_open {
        super::name_styles::Display::AnimatedOnce
    } else {
        super::name_styles::Display::Font
    }
}

/// Module 181254: `eZ = (muted || ignored || blocked) && !(selected || hovered)`, `e$ = mentionCount > 0`.
#[cfg(test)]
pub(super) fn row_look(selected: bool, hovered: bool, mentions: bool, quiet: bool) -> RowLook {
    row_look_with_menu(selected, hovered, false, mentions, quiet)
}

/// Module 181254 `eY`: `ew.Q2` (`interactiveSelected_zln_tS`) applies for `s || eo` (`selected || context_menu_open`),
/// while `eZ` (`dimmed`) stays `(muted || ignored || blocked) && !(selected || hovered)`.
pub(super) fn row_look_with_menu(
    selected: bool,
    hovered: bool,
    context_menu_open: bool,
    mentions: bool,
    quiet: bool,
) -> RowLook {
    let dimmed = quiet && !(selected || hovered);
    let highlighted = mentions && !dimmed;
    let row = if hovered {
        "--interactive-text-hover"
    } else if selected || context_menu_open {
        "--text-default"
    } else {
        "--text-muted"
    };
    let text = if highlighted {
        "--text-strong"
    } else if dimmed {
        "--text-muted"
    } else {
        row
    };
    RowLook {
        row,
        text,
        pill: mentions,
        dimmed,
        highlighted,
    }
}

/// `unreadPill_zln_tS`: 4 x 8 in `--interactive-text-active`, `--radius-xs` on its right corners, 8px left of the
/// row (the list's edge), centred on the 44px item; at .3 for a muted row.
pub(super) fn unread_pill(dimmed: bool) -> AnyElement {
    div()
        .absolute()
        .left(px(1.))
        .top(px(18.))
        .w(px(4.))
        .h(px(8.))
        .rounded_r(px(4.))
        .bg(color("--interactive-text-active"))
        .when(dimmed, |pill| pill.opacity(0.3))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::super::name_styles::Display;
    use super::{
        DM_BOLD_FONT_OPACITY, dm_name_display, rail_icon_crop, row_look, row_look_with_menu,
    };

    #[test]
    fn dm_row_name_display_matches_official_states() {
        assert!((DM_BOLD_FONT_OPACITY - 0.9).abs() < f32::EPSILON);
        // Idle and unread at rest: `PLAIN` (`Display::Font`), no colour/stroke/glow effect.
        assert_eq!(dm_name_display(false, false, false), Display::Font);
        // Selected or DM context menu open (not hovered): `ANIMATED` with `loop: false` (`Display::AnimatedOnce`).
        assert_eq!(dm_name_display(true, false, false), Display::AnimatedOnce);
        assert_eq!(dm_name_display(false, false, true), Display::AnimatedOnce);
        assert_eq!(dm_name_display(true, false, true), Display::AnimatedOnce);
        // Hovered (alone or while selected / menu open): `ANIMATED` with `loop: true` (`Display::Animated`).
        assert_eq!(dm_name_display(false, true, false), Display::Animated);
        assert_eq!(dm_name_display(true, true, false), Display::Animated);
        assert_eq!(dm_name_display(false, true, true), Display::Animated);

        // `ew.Q2` on context-menu-open DM row (`s || eo`), while `dimmed` (`eZ`) only clears on `s || b`.
        let menu_look = row_look_with_menu(false, false, true, false, false);
        assert_eq!(
            (menu_look.row, menu_look.text),
            ("--text-default", "--text-default")
        );
        let muted_menu_look = row_look_with_menu(false, false, true, false, true);
        assert!(muted_menu_look.dimmed);
        assert_eq!(
            (muted_menu_look.row, muted_menu_look.text),
            ("--text-default", "--text-muted")
        );
    }

    #[test]
    fn an_unread_dm_row_is_highlighted_unless_muted_and_at_rest() {
        // Unread at rest: the pill and the strong name (the owner's report: the row only moved up).
        let look = row_look(false, false, true, false);
        assert!(look.pill && look.highlighted && !look.dimmed);
        assert_eq!((look.row, look.text), ("--text-muted", "--text-strong"));
        // Read at rest: muted text, no pill.
        let look = row_look(false, false, false, false);
        assert!(!look.pill && !look.highlighted);
        assert_eq!(look.text, "--text-muted");
        // Selected: `--text-default`; hovered: `--interactive-text-hover`, even when selected.
        assert_eq!(row_look(true, false, false, false).text, "--text-default");
        assert_eq!(
            row_look(true, true, false, false).text,
            "--interactive-text-hover"
        );
        assert_eq!(
            row_look(false, true, false, false).text,
            "--interactive-text-hover"
        );
        // Muted with mentions at rest: dimmed pill, no highlight; hovered it is a plain highlighted row again.
        let look = row_look(false, false, true, true);
        assert!(look.pill && look.dimmed && !look.highlighted);
        assert_eq!(look.text, "--text-muted");
        let look = row_look(false, true, true, true);
        assert!(!look.dimmed && look.highlighted);
        assert_eq!(look.text, "--text-strong");
    }

    #[test]
    fn the_home_button_shows_the_middle_of_a_48px_icon() {
        // A 96px asset drawn 48px wide: the 40px tile shows its middle 80px.
        assert_eq!(rail_icon_crop(96, 96), (8, 8, 80, 80));
        assert_eq!(rail_icon_crop(48, 48), (4, 4, 40, 40));
        assert_eq!(rail_icon_crop(1, 1), (0, 0, 1, 1));
    }

    #[test]
    fn active_now_derives_voice_and_presence_cards_or_empty_when_quiet() {
        let mut stores = dc_state::Stores::default();
        stores.dispatch(
            "READY",
            &serde_json::json!({
                "user": {"id": "1", "username": "me"},
                "users": [
                    {"id": "2", "username": "alice", "global_name": "Alice"},
                    {"id": "3", "username": "bob", "global_name": "Bob"},
                    {"id": "4", "username": "carol", "global_name": "Carol"}
                ],
                "relationships": [
                    {"id": "2", "type": 1},
                    {"id": "3", "type": 1},
                    {"id": "4", "type": 1}
                ],
                "guilds": [{
                    "id": "10",
                    "name": "Raid Squad",
                    "owner_id": "1",
                    "channels": [{"id": "100", "type": 2, "name": "Lounge"}],
                    "voice_states": []
                }]
            }),
        );
        // Online friends with no activity or voice fall back to empty Active Now state.
        assert!(
            dc_state::now_playing::feed(&stores, &[], dc_state::now_playing::Sort::Default, |_| {
                None
            })
            .parties
            .is_empty()
        );

        stores.dispatch(
            "PRESENCE_UPDATE",
            &serde_json::json!({
                "user": {"id": "2"},
                "status": "online",
                "activities": [{
                    "type": 2,
                    "name": "Spotify",
                    "details": "Clair de Lune",
                    "party": {"id": "spotify:session"},
                    "state": "Debussy; Jean"
                }]
            }),
        );
        stores.dispatch(
            "VOICE_STATE_UPDATE",
            &serde_json::json!({
                "guild_id": "10",
                "channel_id": "100",
                "user_id": "3",
                "session_id": "s3"
            }),
        );
        let cards =
            dc_state::now_playing::feed(&stores, &[], dc_state::now_playing::Sort::Default, |_| {
                None
            })
            .parties;
        assert_eq!(cards.len(), 2);
        // Voice friend first, then activity friend.
        assert_eq!(cards[0].priority, vec![3]);
        assert_eq!(cards[0].voice[0].guild, 10);
        assert_eq!(cards[0].voice[0].channel, 100);
        assert_eq!(cards[1].priority, vec![2]);
        let act = &cards[1].activities[0];
        assert_eq!(act.application.name, "Spotify");
        assert_eq!(act.value["details"], "Clair de Lune");
        assert_eq!(act.value["state"], "Debussy; Jean");
    }
}
