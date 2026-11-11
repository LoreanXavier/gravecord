//! The Favorites server: the star in the server rail once the account has the server on (`favorites.guild_visible` of
//! the settings proto, `favoriteGuildEnabled` of module 181079), its channel list (module 132598 `et`: the header
//! "Favorites" with the add button, then the favourites as ordinary channel rows) and its menus. The route is
//! `/channels/@favorites/<channel>`: here the real server and channel are selected as usual and `favorites_view` only
//! swaps the rail highlight and the sidebar. Frames: menus2/favorites-added, favorites-rail-hover, favorites-view,
//! favorites-rail-menu, favorites-channel-menu, dm-pinned (the star in the rail).

use super::tooltip::{Body, Tip};
use super::{Root, channel_icon, color, context_menu, icon, motion};
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, SharedString, div,
    prelude::*, px, svg,
};

/// `FavoriteChannelType.REFERENCE`: a channel the list points at (categories made by the account are not built).
use dc_proto::settings::FAVORITE_REFERENCE;

impl Root {
    /// Whether the rail shows the star (`favoriteGuildEnabled`).
    pub(super) fn favorites_guild_enabled(&self) -> bool {
        self.stores.favorites().guild_enabled()
    }

    /// The Favorites server is the open place: its view was opened and the channel on screen is one of its own.
    pub(super) fn in_favorites(&self) -> bool {
        self.favorites_view
            && self.favorites_guild_enabled()
            && self
                .channel
                .is_none_or(|channel| self.stores.favorites().is_favorite(channel))
    }

    /// The favourite channels in list order (`position`), references only.
    fn favorite_ids(&self) -> Vec<(Id, String)> {
        let mut list: Vec<_> = self
            .stores
            .favorites()
            .channels
            .into_iter()
            .filter(|(_, known)| known.kind == FAVORITE_REFERENCE)
            .collect();
        list.sort_by_key(|(_, known)| known.position);
        list.into_iter()
            .map(|(id, known)| (id, known.nickname))
            .collect()
    }

    fn favorite_channel(&self, id: Id) -> Option<(Id, &Channel)> {
        self.stores.guilds.iter().find_map(|guild| {
            guild
                .channels
                .iter()
                .find(|channel| channel.id == id)
                .map(|channel| (guild.id, channel))
        })
    }

    /// The star's click: the favourite last open, else the first one (`/channels/@favorites/<channel>`).
    pub(super) fn open_favorites(&mut self, cx: &mut Context<Self>) {
        let ids = self.favorite_ids();
        let target = self
            .channel_history
            .iter()
            .copied()
            .find(|channel| ids.iter().any(|(id, _)| id == channel))
            .or_else(|| ids.first().map(|(id, _)| *id));
        match target {
            Some(channel) => self.open_favorite_channel(channel, cx),
            None => {
                self.select_home(None);
                self.favorites_view = true;
            }
        }
        self.request_images();
        cx.notify();
    }

    /// A favourite's row: its own server and channel, inside the Favorites view.
    pub(super) fn open_favorite_channel(&mut self, channel: Id, cx: &mut Context<Self>) {
        match self.favorite_channel(channel).map(|(guild, _)| guild) {
            Some(guild) => self.select(guild, Some(channel)),
            None => self.select_home(Some(channel)),
        }
        self.favorites_view = true;
        self.request_images();
        cx.notify();
    }

    /// "Hide Favorites" (module 146092 `a_`): the server goes from the rail, the favourites stay saved.
    pub(super) fn set_favorites_guild(&mut self, visible: bool, cx: &mut Context<Self>) {
        let mut favorites = self.stores.favorites();
        if favorites.guild_visible == Some(visible) {
            return;
        }
        favorites.guild_visible = Some(visible);
        self.send_favorites(&favorites, "Hide Favorites");
        if !visible && self.favorites_view {
            self.favorites_view = false;
            self.select_home(None);
        }
        cx.notify();
    }

    /// "Thread Settings" of the Favorites server: its modal is not built yet (no capture of it).
    pub(super) fn open_favorites_settings(&mut self, _cx: &mut Context<Self>) {}

    /// "Change Nickname" of a favourite: its modal is not built yet (no capture of it).
    pub(super) fn open_favorite_nickname(&mut self, _channel: Id, _cx: &mut Context<Self>) {}

    /// The Appearance switch "Show Favorites server".
    pub(super) fn favorites_toggled(&mut self, on: bool) {
        let mut favorites = self.stores.favorites();
        favorites.guild_visible = Some(on);
        self.send_favorites(&favorites, "Show Favorites server");
        if !on && self.favorites_view {
            self.favorites_view = false;
        }
    }

    /// Stores the favourites in the settings and tells the service (`PATCH /users/@me/settings-proto/1`).
    pub(super) fn send_favorites(&mut self, favorites: &dc_proto::settings::Favorites, what: &str) {
        let patch = self.stores.write_favorites(favorites);
        super::actions::log_effect(what, "PATCH /users/@me/settings-proto/1", patch.len());
        if let Some(request) = super::rail_folders::guild_folders_request(&patch) {
            let _ = self.requests.send(request);
        }
    }

    /// The star under Home: 40px like Home, the same pill and brand tile (frames favorites-added, favorites-rail-hover).
    pub(super) fn favorites_tile(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.favorites_guild_enabled() {
            return None;
        }
        let selected = self.in_favorites();
        let row_key = "favorites";
        Some(
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
                        "favorites",
                        Tip::rail(Body::Heading("Favorites".into())),
                        self.group_stop(
                            div().id("favorites"),
                            "rail",
                            super::keyboard::Nav::List,
                            "favorites",
                            selected,
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
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.open_context_menu(
                                    context_menu::Target::Favorites,
                                    event.position,
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.open_favorites(cx)))
                        .child(
                            svg()
                                .path("icons/clips_star.svg")
                                .size(px(24.))
                                .flex_none()
                                .text_color(motion::rail::tile_foreground(row_key, selected)),
                        ),
                        cx,
                    ),
                )
                .into_any_element(),
        )
    }

    /// The channel list of the Favorites server: the header (`header_XH4OpX`: star, name, chevron, the add button) and
    /// the favourites in list order. The first-run suggestions (affinity data of the account) are not drawn.
    pub(super) fn favorites_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ids = self.favorite_ids();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (id, nickname) in &ids {
            if let Some(dm) = self
                .stores
                .private_channels
                .iter()
                .find(|known| known.id == *id)
            {
                rows.extend(self.dm_row(dm, cx));
                continue;
            }
            let Some((guild, channel)) = self.favorite_channel(*id) else {
                continue;
            };
            if self.guild == Some(guild) && nickname.is_empty() {
                rows.push(self.channel_row(channel, cx));
                if channel.kind == ChannelKind::Text || channel.is_forum() {
                    rows.extend(self.thread_rows(channel.id, cx));
                }
            } else {
                rows.push(self.favorite_row(guild, channel, nickname, cx));
            }
        }
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
                    .parity_below("guild-header", 1.)
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .px(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .child(
                                // `.guildDropdown_XH4OpX` with the `favoritesIcon_XH4OpX` star (20px) and the name.
                                div()
                                    .id("favorites-name")
                                    .flex()
                                    .items_center()
                                    .h(px(32.))
                                    .px(px(8.))
                                    .rounded(px(8.))
                                    .hover(|style| style.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                            this.open_context_menu(
                                                context_menu::Target::Favorites,
                                                event.position,
                                                cx,
                                            );
                                            cx.stop_propagation();
                                        }),
                                    )
                                    .child(
                                        svg()
                                            .path("icons/clips_star.svg")
                                            .size(px(20.))
                                            .flex_none()
                                            .text_color(color("--text-strong")),
                                    )
                                    .child(
                                        div()
                                            .ml(px(16.))
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child("Favorites"),
                                    )
                                    .child(
                                        div().ml(px(2.)).child(
                                            svg()
                                                .path("icons/chevron_down.svg")
                                                .size(px(16.))
                                                .text_color(color("--text-strong")),
                                        ),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                // `addActionButton_XH4OpX` "Add to Favorites".
                                div()
                                    .id("favorites-add")
                                    .size(px(32.))
                                    .rounded(px(8.))
                                    .hover(|style| style.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("dm_plus", 16., "--text-strong")),
                            ),
                    ),
            )
            .child(div().flex_none().h(px(8.)))
            .child(
                div()
                    .id("favorites-rows")
                    .flex_1()
                    .min_h(px(0.))
                    .mb(px(4.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(rows)
                    .child(div().flex_none().h(px(16.))),
            )
            .child(
                div()
                    .flex_none()
                    .h(self.user_panel_height.unwrap_or(px(58.)) - px(8.)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right(px(1.))
                    .bottom_0()
                    .h(px(8.))
                    .bg(color("--background-base-lowest")),
            )
    }

    /// A favourite of another server (or one with a nickname): the channel row's look with the channel's own server.
    fn favorite_row(
        &self,
        guild: Id,
        channel: &Channel,
        nickname: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let selected = self.channel == Some(id);
        let facts = channel_icon::Facts {
            kind: channel.kind,
            rules: false,
            nsfw: channel.nsfw,
            private: false,
            pending: false,
            locked: false,
            active_threads: false,
        };
        let tint = if selected {
            "--text-strong"
        } else {
            "--channels-default"
        };
        let label = if nickname.is_empty() {
            channel.name.clone()
        } else {
            nickname.to_owned()
        };
        let group = SharedString::from(format!("favorite-{id}"));
        let _ = guild;
        div()
            .flex_none()
            .h(px(34.))
            .pt(px(1.))
            .pl(px(8.))
            .child(
                div()
                    .id(SharedString::from(format!("favorite-row-{id}")))
                    .group(group)
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.open_context_menu(
                                context_menu::Target::Channel(id),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_favorite_channel(id, cx);
                    }))
                    .h(px(32.))
                    .w(px(286.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .when(selected, |this| {
                        this.bg(color("--interactive-background-selected"))
                    })
                    .when(!selected, |this| {
                        this.hover(|style| style.bg(color("--interactive-background-hover")))
                    })
                    .child(icon(channel_icon::name(&facts), 20., tint))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(16.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color(tint))
                            .child(label),
                    ),
            )
            .into_any_element()
    }
}
