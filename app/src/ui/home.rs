//! Home: the DM sidebar, the friends panel and direct messages. Geometry is from `design/dom/home/friends-darker-cozy.json`.

use super::tooltip::{Body, Tip};
use super::{Root, avatar_source, color, icon, status_dot};
use crate::parity::Parity;
use dc_state::{Id, PrivateChannel};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px, svg,
};

impl Root {
    pub(super) fn select_home(&mut self, channel: Option<Id>) {
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel,
                nitro: false,
            },
        );
        self.nitro_open = false;
        self.guild = None;
        self.channel = channel;
        self.unread_from = None;
        if let Some(channel) = channel {
            self.remember_channel(channel);
        }
        if let Some(channel) = channel {
            let _ = self
                .requests
                .send(crate::net::Request::LoadMessages { channel });
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
        self.rebuild_rows();
        self.request_images();
    }

    pub(super) fn home_tile(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.guild.is_none();
        // Hovered, the tile takes the selected colors and a 20px pill, as the rail's guild tiles do.
        let group = SharedString::from("home-row");
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex()
            .justify_center()
            .group(group.clone())
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(if selected { 0. } else { 10. }))
                    .w(px(4.))
                    .h(px(if selected { 40. } else { 20. }))
                    .rounded_r(px(4.))
                    .bg(color("--text-strong"))
                    .when(!selected, |this| {
                        this.opacity(0.)
                            .group_hover(group.clone(), |style| style.opacity(1.))
                    }),
            )
            .child(
                self.with_tip(
                    "home",
                    Tip::rail(Body::Heading("Direct Messages".into())),
                    div()
                        .id("home")
                        .size(px(40.))
                        .rounded(px(12.))
                        .bg(if selected {
                            color("--background-brand")
                        } else {
                            color("--background-mod-subtle")
                        })
                        .group_hover(group.clone(), |style| style.bg(color("--background-brand")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_home(None);
                            cx.notify();
                        }))
                        .child(
                            svg()
                                .path("icons/home.svg")
                                .size(px(24.))
                                .flex_none()
                                .text_color(color(if selected {
                                    "--white"
                                } else {
                                    "--text-default"
                                }))
                                .group_hover(group, |style| style.text_color(color("--white"))),
                        ),
                    cx,
                ),
            )
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
                    .h(px(38.))
                    .w(px(286.))
                    .pl(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(selected, |this| {
                        this.parity(if id == "nitro" {
                            "nitro-link"
                        } else {
                            "friends-link"
                        })
                        .bg(color("--background-mod-strong"))
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--background-mod-subtle")))
                    })
                    .child(icon(name, 20., tint))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(tint))
                            .child(label.to_owned()),
                    ),
            )
    }

    fn dm_row(&self, channel: &PrivateChannel, cx: &mut Context<Self>) -> Option<AnyElement> {
        let recipient = self.stores.dm_recipient(channel)?.clone();
        // Blocked or ignored people carry their mark where the close button would be.
        let marked = self.is_blocked(recipient.id) || self.stores.is_ignored(recipient.id);
        let id = channel.id;
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
                        .h(px(42.))
                        .w(px(286.))
                        .pl(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .text_color(color(tint))
                        .when(selected, |this| this.bg(color("--background-mod-strong")))
                        .when(!selected, |this| {
                            this.hover(|style| style.bg(color("--background-mod-subtle")))
                        })
                        .child(
                            div()
                                .relative()
                                .size(px(32.))
                                .children(
                                    avatar.map(|image| img(image).size(px(32.)).rounded_full()),
                                )
                                .child(status_dot(&status, "--background-base-lowest")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .child(recipient.display_name().to_owned()),
                                )
                                .children(
                                    recipient.tag.as_ref().map(|tag| self.server_tag_chip(tag)),
                                ),
                        )
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
                        .when(!marked, |this| {
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
                                    .opacity(0.)
                                    .group_hover(group, |style| style.opacity(0.7))
                                    .hover(|style| style.opacity(1.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.close_dm(id, cx);
                                    }))
                                    .child(icon("friend_ignore", 16., "--text-strong")),
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
        self.stores
            .private_channels
            .retain(|known| known.id != channel);
        if self.channel == Some(channel) && self.guild.is_none() {
            self.select_home(None);
        }
        let _ = self.requests.send(crate::net::Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route: format!("/channels/{channel}?silent=false"),
            body: None,
        });
        cx.notify();
    }

    pub(super) fn home_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let friends_selected = self.guild.is_none() && self.channel.is_none() && !self.nitro_open;
        let nitro_selected = self.guild.is_none() && self.channel.is_none() && self.nitro_open;
        let rows: Vec<AnyElement> = self
            .stores
            .private_channels
            .iter()
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
                            .child("Find or start a conversation"),
                    ),
            )
            .child(div().flex_none().h(px(8.)))
            .child(
                self.home_link("friends", "home_friends", "Friends", friends_selected)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.select_home(None);
                        cx.notify();
                    })),
            )
            .child(
                self.home_link("nitro", "home_nitro", "Nitro", nitro_selected)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
            )
            .child(
                self.home_link("shop", "home_shop", "Shop", false)
                    .relative()
                    .child(
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
                            .child("NEW"),
                    ),
            )
            .child(self.home_link("quests", "home_quests", "Quests", false))
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
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child("Direct Messages")
                    .child(div().flex_1())
                    .child(
                        // "Create DM": the same New Message picker as the Friends page's New Group DM.
                        div()
                            .id("dm-plus")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_group_dm_picker(window, cx)
                            }))
                            .child(icon("dm_plus", 16., "--text-muted")),
                    ),
            )
            .children(rows)
    }

    pub(super) fn friends_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let people = self.friends_people(cx);
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
