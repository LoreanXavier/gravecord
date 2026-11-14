//! The Favorites server: the star in the server rail once the account has the server on (`favorites.guild_visible` of
//! the settings proto, `favoriteGuildEnabled` of module 181079), its channel list (module 132598 `et`: the header
//! "Favorites" with the add button, then the favourites as ordinary channel rows) and its menus. The route is
//! `/channels/@favorites/<channel>`: here the real server and channel are selected as usual and `favorites_view` only
//! swaps the rail highlight and the sidebar. Frames: menus2/favorites-added, favorites-rail-hover, favorites-view,
//! favorites-rail-menu, favorites-channel-menu, dm-pinned (the star in the rail).

use super::tooltip::{Body, Tip};
use super::{Root, avatar_source, channel_icon, color, context_menu, icon, motion, status_dot};
use crate::parity::Parity;
use dc_state::{Channel, ChannelKind, Id, PrivateChannel};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, SharedString, div,
    img, prelude::*, px, svg,
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
                let from = self.route();
                let replaying = self.history.replaying;
                self.history.replaying = true;
                self.select_home(None);
                self.favorites_view = true;
                self.history.replaying = replaying;
                self.history.visit(from, self.route());
                self.request_images();
                cx.notify();
            }
        }
    }

    /// A favourite's row: its own server and channel, inside the Favorites view.
    pub(super) fn open_favorite_channel(&mut self, channel: Id, cx: &mut Context<Self>) {
        let from = self.route();
        let replaying = self.history.replaying;
        self.history.replaying = true;
        match self.favorite_channel(channel).map(|(guild, _)| guild) {
            Some(guild) => self.select(guild, Some(channel)),
            None => self.select_home(Some(channel)),
        }
        self.favorites_view = true;
        self.history.replaying = replaying;
        self.history.visit(from, self.route());
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

    /// Capture only: `favorites on <channel ids...>` hands the root the settings update another session would send
    /// (the favourites, the server on); `favorites off` turns the server off; `favorites open` opens its view.
    #[cfg(feature = "capture")]
    pub(crate) fn script_favorites(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let note = match words {
            ["on", ids @ ..] => {
                let mut favorites = self.stores.favorites();
                for id in ids.iter().filter_map(|id| id.parse::<u64>().ok()) {
                    let kind = self
                        .favorite_channel(id)
                        .map(|(_, channel)| context_menu::channel_type_number(channel.kind))
                        .or_else(|| {
                            self.stores
                                .private_channels
                                .iter()
                                .find(|dm| dm.id == id)
                                .map(|dm| dm.kind)
                        });
                    favorites.add_channel(id, kind);
                }
                favorites.guild_visible = Some(true);
                let (patch, _) = dc_proto::settings::set_favorites(&[], &favorites);
                self.stores.dispatch(
                    "USER_SETTINGS_PROTO_UPDATE",
                    &serde_json::json!({"settings": {"type": 1, "proto": patch}, "partial": true}),
                );
                format!("favorites: on, {} channels", favorites.channels.len())
            }
            ["off"] => {
                let mut favorites = self.stores.favorites();
                favorites.guild_visible = Some(false);
                let (patch, _) = dc_proto::settings::set_favorites(&[], &favorites);
                self.stores.dispatch(
                    "USER_SETTINGS_PROTO_UPDATE",
                    &serde_json::json!({"settings": {"type": 1, "proto": patch}, "partial": true}),
                );
                "favorites: off".to_owned()
            }
            ["open"] => {
                self.open_favorites(cx);
                "favorites: opened".to_owned()
            }
            ["state"] => {
                let favorites = self.stores.favorites();
                format!(
                    "favorites: visible {:?}, channels {:?}, view {}, in_favorites {}",
                    favorites.guild_visible,
                    favorites
                        .channels
                        .iter()
                        .map(|(id, _)| *id)
                        .collect::<Vec<_>>(),
                    self.favorites_view,
                    self.in_favorites()
                )
            }
            _ => "usage: favorites on <ids>|off|open|state".to_owned(),
        };
        cx.notify();
        note
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
                                // `favoriteIcon_850pDF`: 20px.
                                .size(px(20.))
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
                if dm.kind == 1 {
                    rows.extend(self.favorite_dm_row(dm, nickname, cx));
                } else {
                    rows.extend(self.dm_row(dm, cx));
                }
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
                                    .child(icon("dm_plus", 20., "--text-strong")),
                            ),
                    ),
            )
            .children(self.favorites_suggestions(cx))
            .child(div().flex_none().h(px(12.)))
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

    /// The server breadcrumb of the channel header in the Favorites view (`guildBreadcrumbContainer_z_gUBg`, frame
    /// menus2/favorites-view): the server's 24px icon (its acronym without one) and a 10 by 24 caret, 45px wide
    /// so the channel's icon lands 436px from the window's left edge.
    pub(super) fn favorites_breadcrumb(&self) -> Option<AnyElement> {
        if !self.in_favorites() {
            return None;
        }
        let guild = self.current_guild()?;
        let image = self.images.get(&format!("guild:{}", guild.id)).cloned();
        Some(
            div()
                .id("favorites-breadcrumb")
                .ml(px(1.))
                .flex_none()
                .w(px(45.))
                .h(px(24.))
                .flex()
                .items_center()
                .cursor_pointer()
                .child(match image {
                    Some(image) => img(image).size(px(24.)).rounded(px(8.)).into_any_element(),
                    None => div()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.))
                        .line_height(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(guild.acronym())
                        .into_any_element(),
                })
                .child(
                    div()
                        .ml(px(4.))
                        .w(px(10.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("menu_caret_right", 12., "--text-muted")),
                )
                .into_any_element(),
        )
    }

    /// The "Jump" button after the title (`followButton_z_gUBg`: a 60 by 32 secondary button inside 8px of padding):
    /// the channel in its own server, as "Go to Server" does.
    pub(super) fn favorites_jump_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.in_favorites() {
            return None;
        }
        let channel = self.channel?;
        Some(
            div()
                .flex_none()
                .ml(px(8.))
                .px(px(8.))
                .py(px(4.))
                .child(
                    div()
                        .id("favorites-jump")
                        .min_w(px(60.))
                        .h(px(32.))
                        .px(px(11.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((guild, _)) = this.favorite_channel(channel) {
                                this.select(guild, Some(channel));
                                this.request_images();
                                cx.notify();
                            }
                        }))
                        .child("Jump"),
                )
                .into_any_element(),
        )
    }

    /// The suggestions (module 132598 `I`): up to four, channels and people alternating, none already a favourite.
    /// The official list ranks by the account's affinity data, which is not kept here: the order is the lists'.
    fn favorite_suggestion_ids(&self) -> Vec<Id> {
        let favorites = self.stores.favorites();
        let mut channels = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| guild.channels.iter())
            .filter(|channel| channel.kind == ChannelKind::Text)
            .map(|channel| channel.id)
            .filter(|id| !favorites.is_favorite(*id));
        let mut people = self
            .stores
            .private_channels
            .iter()
            .filter(|dm| dm.kind == 1 && self.stores.dm_recipient(dm).is_some())
            .map(|dm| dm.id)
            .filter(|id| !favorites.is_favorite(*id));
        let mut out = Vec::new();
        while out.len() < 4 {
            let next = if out.len() % 2 == 0 {
                channels.next().or_else(|| people.next())
            } else {
                people.next().or_else(|| channels.next())
            };
            match next {
                Some(id) => out.push(id),
                None => break,
            }
        }
        out
    }

    /// "Add" of a suggestion, "Add to Favorites" of a channel menu (`nR` of module 146092, source "suggestions").
    pub(super) fn add_favorite(&mut self, channel: Id, cx: &mut Context<Self>) {
        let kind = match self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel)
        {
            Some(dm) => dm.kind,
            None => self.favorite_channel(channel).map_or(0, |(_, known)| {
                context_menu::channel_type_number(known.kind)
            }),
        };
        let mut favorites = self.stores.favorites();
        if favorites.add_channel(channel, Some(kind)) {
            self.send_favorites(&favorites, "Add to Favorites");
        }
        cx.notify();
    }

    /// The "Suggested Channels" block above the list (`aboveScroller_EXeLI-`, frame favorites-view): the heading with
    /// the dismiss button, four 48px rows, and the divider.
    fn favorites_suggestions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.favorites_suggest_dismissed {
            return None;
        }
        let ids = self.favorite_suggestion_ids();
        if ids.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        for id in ids {
            let (icon_part, name, sub) = if let Some(dm) = self
                .stores
                .private_channels
                .iter()
                .find(|known| known.id == id)
            {
                let user = self.stores.dm_recipient(dm).cloned();
                let Some(user) = user else { continue };
                let avatar = self.images.get(&avatar_source(&user).0).cloned();
                if avatar.is_none() {
                    let (k, u) = avatar_source(&user);
                    self.want_image(k, u, cx);
                }
                let status = self.stores.status(user.id).to_owned();
                (
                    div()
                        .relative()
                        .size(px(32.))
                        .children(avatar.map(|image| {
                            img(image).size(px(32.)).rounded_full().into_any_element()
                        }))
                        .child(status_dot(&status, "--background-base-lowest"))
                        .into_any_element(),
                    self.private_channel_name(dm),
                    user.username.clone(),
                )
            } else {
                let Some((guild, channel)) = self.favorite_channel(id) else {
                    continue;
                };
                let (guild_name, image) = self
                    .stores
                    .guild(guild)
                    .map(|known| {
                        (
                            known.name.clone(),
                            self.images.get(&format!("guild:{guild}")).cloned(),
                        )
                    })
                    .unwrap_or_default();
                let facts = channel_icon::Facts {
                    kind: channel.kind,
                    rules: false,
                    nsfw: channel.nsfw,
                    private: false,
                    pending: false,
                    locked: false,
                    active_threads: false,
                };
                (
                    // `iconWithGuildIcon_NKPsbl`: the server's icon (24px) with the channel's 20px badge at (12, 12).
                    div()
                        .relative()
                        .size(px(32.))
                        .child(
                            div()
                                .size(px(24.))
                                .rounded_full()
                                .bg(color("--background-mod-strong"))
                                .children(
                                    image.map(|image| img(image).size(px(24.)).rounded_full()),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(12.))
                                .top(px(12.))
                                .size(px(20.))
                                .rounded_full()
                                .bg(color("--background-mod-strong"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(channel_icon::name(&facts), 10., "--icon-default")),
                        )
                        .into_any_element(),
                    channel.name.clone(),
                    guild_name,
                )
            };
            rows.push(
                div()
                    .flex_none()
                    .h(px(48.))
                    .w(px(self.sidebar_row_width()))
                    .ml(px(8.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(icon_part)
                            .child(
                                // `labels_dhbHaH`: 150px wide, the line below the name ellipsised.
                                div()
                                    .w(px(150.))
                                    .flex_none()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-muted"))
                                            .child(sub),
                                    ),
                            ),
                    )
                    .child(
                        // `button_F7qvhY sm secondary`: 60 by 32, "Add" at 14px / 18px medium.
                        div()
                            .id(SharedString::from(format!("favorites-suggest-{id}")))
                            .flex_none()
                            .w(px(60.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|style| style.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .on_click(cx.listener(move |this, _, _, cx| this.add_favorite(id, cx)))
                            .child("Add"),
                    )
                    .into_any_element(),
            );
        }
        Some(
            div()
                .flex_none()
                .w_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_none()
                        .h(px(24.))
                        .mt(px(8.))
                        .mb(px(8.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child("Suggested Channels"),
                        )
                        .child(
                            div()
                                .id("favorites-suggest-dismiss")
                                .size(px(16.))
                                .mr(px(1.))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.favorites_suggest_dismissed = true;
                                    cx.notify();
                                }))
                                .child(icon("sb_close", 16., "--interactive-icon-default")),
                        ),
                )
                .children(rows)
                .child(div().flex_none().h(px(19.)))
                .child(
                    div()
                        .flex_none()
                        .mx(px(16.))
                        .h(px(1.))
                        .bg(color("--background-mod-subtle")),
                )
                .into_any_element(),
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
                    .w(px(self.sidebar_row_width()))
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

    /// A favourited 1:1 private channel (`ChannelItem` `B` + `J` of module 166444, stylesheet `f4b2f93f8f92b799.css`):
    /// 34px row with a 20px avatar (`SIZE_20`) and `UserNameWithEffects` at `effectDisplayType: PLAIN` and
    /// `boldFontOpacity: .9`.
    fn favorite_dm_row(
        &self,
        dm: &PrivateChannel,
        nickname: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let recipient = self.stores.dm_recipient(dm)?.clone();
        let id = dm.id;
        let selected = self.channel == Some(id);
        let hovered = self.hovered_dm == Some(id);
        let muted = self.is_blocked(recipient.id)
            || self.stores.is_ignored(recipient.id)
            || self
                .stores
                .guild_settings
                .channel_muted(0, id, chrono::Utc::now());
        let mentions = self.stores.read_states.mentions(id);
        let unread_important = !muted && !selected && mentions > 0;
        let muted_rest = muted && !selected && !hovered;
        let name_tint = if selected || unread_important {
            "--text-strong"
        } else if hovered {
            "--interactive-text-hover"
        } else if muted {
            "--text-muted"
        } else {
            "--channels-default"
        };
        let weight = if (selected || unread_important) && super::theme() == dc_tokens::Theme::Light
        {
            FontWeight(600.)
        } else {
            FontWeight(500.)
        };
        let avatar = self.images.get(&avatar_source(&recipient).0).cloned();
        let status = self.stores.status(recipient.id).to_owned();
        let typing = self
            .stores
            .typing_users(id, std::time::Instant::now())
            .contains(&recipient.id);
        let label = if nickname.is_empty() {
            self.private_channel_name(dm)
        } else {
            nickname.to_owned()
        };
        let group = SharedString::from(format!("favorite-{id}"));
        Some(
            div()
                .flex_none()
                .relative()
                .h(px(34.))
                .pt(px(1.))
                .pl(px(8.))
                .children(unread_important.then(|| {
                    div()
                        .absolute()
                        .left(px(1.))
                        .top(px(13.))
                        .w(px(4.))
                        .h(px(8.))
                        .rounded_r(px(4.))
                        .bg(color("--interactive-text-active"))
                }))
                .child(
                    div()
                        .id(SharedString::from(format!("favorite-row-{id}")))
                        .group(group)
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                            if *inside {
                                this.hovered_dm = Some(id);
                            } else if this.hovered_dm == Some(id) {
                                this.hovered_dm = None;
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                this.open_context_menu(
                                    context_menu::Target::Dm(id),
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
                        .w(px(self.sidebar_row_width()))
                        .pl(px(8.))
                        .pr(px(8.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(selected, |this| {
                            this.bg(color("--interactive-background-selected"))
                        })
                        .when(!selected, |this| {
                            this.hover(|style| style.bg(color("--interactive-background-hover")))
                        })
                        .child(
                            div()
                                .relative()
                                .flex_none()
                                .size(px(20.))
                                .when(muted_rest, |icon_box| icon_box.opacity(0.5))
                                .children(self.live_avatar(&recipient, 20., hovered, cx).or_else(
                                    || {
                                        if avatar.is_none() {
                                            let (k, u) = avatar_source(&recipient);
                                            self.want_image(k, u, cx);
                                        }
                                        avatar.map(|image| {
                                            img(image)
                                                .size(px(20.))
                                                .rounded_full()
                                                .into_any_element()
                                        })
                                    },
                                ))
                                .child(super::typing_avatar::status_corner(
                                    &status,
                                    typing,
                                    "--background-base-lowest",
                                    20.,
                                    motion::key(&("fav-dm-avatar", id)),
                                    super::typing_avatar::css_dots(&self.stores.experiments),
                                )),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .font_weight(weight)
                                .text_color(color(name_tint))
                                .when(muted_rest, |name| name.opacity(0.5))
                                .child(
                                    super::name_styles::name(
                                        format!("favorite-dm-name-{id}"),
                                        label,
                                        self.name_styles(&recipient, None).as_ref(),
                                        super::name_styles::Display::Font,
                                    )
                                    .bold_opacity(0.9),
                                ),
                        )
                        .when(mentions > 0, |row| {
                            row.child(super::unread_badges::number(
                                mentions,
                                "--badge-notification-background",
                                None,
                            ))
                        }),
                )
                .into_any_element(),
        )
    }
}
