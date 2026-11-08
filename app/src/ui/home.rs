//! Home: the DM sidebar, the friends panel and direct messages. Geometry is from `design/dom/home/friends-darker-cozy.json`.

use super::tooltip::{Body, Tip};
use super::{Root, avatar_source, color, icon, status_dot, status_label};
use crate::parity::Parity;
use dc_state::{Id, PrivateChannel, User};
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
            },
        );
        self.guild = None;
        self.channel = channel;
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
                        this.parity("friends-link")
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
        let id = channel.id;
        let selected = self.channel == Some(id);
        let avatar = self.images.get(&avatar_source(&recipient).0).cloned();
        let status = self.stores.status(recipient.id).to_owned();
        let tint = if selected {
            "--text-strong"
        } else {
            "--text-muted"
        };
        Some(
            div()
                .id(SharedString::from(format!("dm-{id}")))
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
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(tint))
                                .child(recipient.display_name().to_owned()),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn home_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let friends_selected = self.guild.is_none() && self.channel.is_none();
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
            .child(self.home_link("nitro", "home_nitro", "Nitro", false))
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
                    .child(icon("dm_plus", 16., "--text-muted")),
            )
            .children(rows)
    }

    fn friend_row(&self, user: &User) -> impl IntoElement {
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let status = self.stores.status(user.id).to_owned();
        let action = |name: &str| {
            div()
                .size(px(36.))
                .rounded_full()
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--interactive-text-default"))
        };
        let group = SharedString::from(format!("friend-{}", user.id));
        div()
            .relative()
            .pl(px(14.))
            .pr(px(26.))
            .child(
                div()
                    .absolute()
                    .left(px(30.))
                    .top(px(0.))
                    .w(px(479.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .child(
                div()
                    .group(group.clone())
                    .h(px(61.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .child(
                        div()
                            .relative()
                            .size(px(32.))
                            .children(avatar.map(|image| img(image).size(px(32.)).rounded_full()))
                            .child(status_dot(&status, "--background-base-lower")),
                    )
                    .child(
                        div()
                            .ml(px(12.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(user.display_name().to_owned()),
                                    )
                                    .child(
                                        div()
                                            .ml(px(5.))
                                            .opacity(0.)
                                            .group_hover(group, |style| style.opacity(1.))
                                            .text_size(px(14.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(user.username.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(17.5))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(status_label(&status)),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(action("chat_bubble"))
                            .child(action("more_dots")),
                    ),
            )
    }

    fn friends_header(&self) -> impl IntoElement {
        let tab = |label: &'static str, active: bool| {
            div()
                .h(px(32.))
                .px(px(12.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .when(active, |this| {
                    this.bg(color("--background-mod-strong"))
                        .text_color(color("--text-strong"))
                })
                .when(!active, |this| {
                    this.text_color(color("--interactive-text-default"))
                })
                .child(label)
        };
        div()
            .parity_below("chat-header", 1.)
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .pl(px(22.))
            .pr(px(14.))
            .flex()
            .items_center()
            .bg(color("--background-base-lower"))
            .child(icon("friends_header", 20., "--channels-default"))
            .child(
                div()
                    .ml(px(6.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Friends"),
            )
            .child(
                div()
                    .ml(px(12.))
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--background-mod-strong")),
            )
            .child(div().ml(px(12.)).child(tab("Online", true)))
            .child(div().ml(px(16.)).child(tab("All", false)))
            .child(
                div()
                    .ml(px(16.))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--background-brand"))
                    .flex()
                    .items_center()
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .child("Add Friend"),
            )
            .child(div().flex_1())
            .child(icon("new_group", 20., "--text-muted"))
    }

    pub(super) fn friends_panel(&self) -> impl IntoElement {
        let online = self.stores.online_friends();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .child(self.friends_header())
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
                            .child(
                                div()
                                    .parity_with_border("people-search", 1.)
                                    .mt(px(12.))
                                    .ml(px(24.))
                                    .w(px(505.))
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .bg(color("--input-background-default"))
                                    .border_1()
                                    .border_color(color("--background-mod-subtle"))
                                    .flex()
                                    .items_center()
                                    .pl(px(11.))
                                    .child(icon("search_people", 16., "--interactive-text-default"))
                                    .child(
                                        div()
                                            .ml(px(11.))
                                            .text_color(color("--input-placeholder-text-default"))
                                            .child("Search"),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(24.))
                                    .ml(px(24.))
                                    .h(px(34.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(format!("Online — {}", online.len())),
                            )
                            .children(online.iter().map(|user| self.friend_row(user).into_any_element()).collect::<Vec<_>>()),
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
