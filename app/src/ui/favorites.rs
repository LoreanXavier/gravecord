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

/// `FavoriteChannelType.REFERENCE` (1) and `FavoriteChannelType.CATEGORY` (2).
use dc_proto::settings::{FAVORITE_CATEGORY, FAVORITE_REFERENCE};

/// Module 349828 (`A`): literal category name auto-created when enabling Auto-Add Threads.
pub(crate) const AUTO_ADDED_THREADS_CATEGORY: &str = "Auto-Added Threads";
/// Module 349828 (`Rq`): allowed inactive-thread prune day thresholds (`0` = when archived, `7`, `14`).
pub(crate) const FAVORITES_PRUNE_DAYS: [u64; 3] = [0, 7, 14];

/// Module 167846: Favorites Thread Settings modal state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FavoritesThreadSettingsModal {
    pub auto_add: bool,
    pub prune: bool,
    pub days: u64,
    pub prune_all: bool,
    pub store_auto_add: bool,
    pub store_prune: bool,
    pub store_days: u64,
    pub store_prune_all: bool,
    pub loading: bool,
    pub error: bool,
}

impl FavoritesThreadSettingsModal {
    pub(crate) fn from_favorites(fav: &dc_proto::settings::Favorites) -> Self {
        let store_auto_add = fav.auto_add_joined_threads;
        let store_prune = fav
            .prune_inactive_threads
            .unwrap_or_else(|| fav.guild_enabled());
        let store_days = if FAVORITES_PRUNE_DAYS.contains(&fav.prune_inactive_threads_days) {
            fav.prune_inactive_threads_days
        } else {
            0
        };
        let store_prune_all = fav.prune_all_favorited_threads;
        Self {
            auto_add: store_auto_add,
            prune: store_prune,
            days: store_days,
            prune_all: store_prune_all,
            store_auto_add,
            store_prune,
            store_days,
            store_prune_all,
            loading: false,
            error: false,
        }
    }

    /// Official `canSave` dirty check from module 167846.
    pub(crate) fn can_save(&self) -> bool {
        self.auto_add != self.store_auto_add
            || self.prune != self.store_prune
            || (self.prune
                && (self.days != self.store_days || self.prune_all != self.store_prune_all))
    }
}

/// Applies module 167846 / 146092 (`Ub` + `u$`) updates onto `Favorites`.
/// Returns `false` if turning on `auto_add` requires creating `"Auto-Added Threads"` when already at the 200-item cap.
pub(crate) fn apply_favorites_thread_settings(
    fav: &mut dc_proto::settings::Favorites,
    modal: &FavoritesThreadSettingsModal,
) -> bool {
    if modal.auto_add {
        let has_cat = fav
            .channels
            .iter()
            .any(|(_, c)| c.kind == FAVORITE_CATEGORY && c.nickname == AUTO_ADDED_THREADS_CATEGORY);
        if !has_cat {
            if fav.channels.len() >= 200 {
                return false;
            }
            let next_pos = fav
                .channels
                .iter()
                .map(|(_, c)| c.position)
                .max()
                .map_or(1, |m| m + 1);
            let next_id = fav
                .channels
                .iter()
                .map(|(id, _)| *id)
                .max()
                .map_or(10_000, |m| m.saturating_add(1));
            fav.channels.push((
                next_id,
                dc_proto::settings::FavoriteChannel {
                    nickname: AUTO_ADDED_THREADS_CATEGORY.to_owned(),
                    kind: FAVORITE_CATEGORY,
                    position: next_pos,
                    ..dc_proto::settings::FavoriteChannel::default()
                },
            ));
        }
    }
    fav.auto_add_joined_threads = modal.auto_add;
    fav.prune_inactive_threads = Some(modal.prune);
    if modal.prune {
        fav.prune_inactive_threads_days = modal.days;
        fav.prune_all_favorited_threads = modal.prune_all;
    }
    true
}

/// Module 167846: radio option label for `0` (`EIVQqq`) or non-zero days (`eic1d0`).
pub(crate) fn favorites_prune_days_label(days: u64) -> String {
    if days == 0 {
        dc_intl::t("EIVQqq").to_owned()
    } else {
        dc_intl::format("eic1d0", &[("days", dc_intl::Arg::Num(days as f64))])
    }
}

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
            ["category", id, rest @ ..] => {
                if let Ok(cat_id) = id.parse::<u64>() {
                    let name = if rest.is_empty() {
                        "Pinned".to_owned()
                    } else {
                        rest.join(" ")
                    };
                    let mut favorites = self.stores.favorites();
                    let pos = favorites.channels.len() as u64;
                    if let Some((_, existing)) = favorites
                        .channels
                        .iter_mut()
                        .find(|(cid, _)| *cid == cat_id)
                    {
                        existing.nickname = name.clone();
                        existing.kind = FAVORITE_CATEGORY;
                    } else {
                        favorites.channels.push((
                            cat_id,
                            dc_proto::settings::FavoriteChannel {
                                nickname: name.clone(),
                                kind: FAVORITE_CATEGORY,
                                position: pos,
                                parent_id: 0,
                                channel_type: Some(4),
                                collapsed: false,
                            },
                        ));
                    }
                    // Assign first reference channel into this category if one has no parent yet.
                    if let Some((_, first_ref)) = favorites
                        .channels
                        .iter_mut()
                        .find(|(_, known)| known.kind == FAVORITE_REFERENCE && known.parent_id == 0)
                    {
                        first_ref.parent_id = cat_id;
                    }
                    favorites.guild_visible = Some(true);
                    let (patch, _) = dc_proto::settings::set_favorites(&[], &favorites);
                    self.stores.dispatch(
                        "USER_SETTINGS_PROTO_UPDATE",
                        &serde_json::json!({"settings": {"type": 1, "proto": patch}, "partial": true}),
                    );
                    format!("favorites: category {cat_id} '{name}'")
                } else {
                    "usage: favorites category <id> <name>".to_owned()
                }
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
            ["settings"] => {
                self.open_favorites_settings(cx);
                "favorites: thread settings opened".to_owned()
            }
            _ => "usage: favorites on <ids>|off|open|state|settings".to_owned(),
        };
        cx.notify();
        note
    }

    /// Module 167846: opens the Favorites "Thread Settings" modal (`hklnjs`).
    pub(super) fn open_favorites_settings(&mut self, cx: &mut Context<Self>) {
        let fav = self.stores.favorites();
        self.favorites_thread_settings = Some(FavoritesThreadSettingsModal::from_favorites(&fav));
        cx.notify();
    }

    /// Module 167846: Favorites Thread Settings modal (`size="md"` 480px, `hklnjs` / `DIyQIF` / `bI/czq` / `THUVKS` / `KIzpqi`).
    pub(super) fn favorites_thread_settings_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.favorites_thread_settings.as_ref()?;
        let can_save = modal.can_save() && !modal.loading;
        let loading = modal.loading;

        let switch_row = |id: &'static str,
                          title: &'static str,
                          desc: &'static str,
                          checked: bool,
                          toggle: fn(&mut Root)| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .when(!loading, |d| d.cursor_pointer())
                .when(loading, |d| d.opacity(0.6))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this
                        .favorites_thread_settings
                        .as_ref()
                        .is_some_and(|m| m.loading)
                    {
                        return;
                    }
                    toggle(this);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child(desc),
                        ),
                )
                .child(
                    div()
                        .w(px(40.))
                        .h(px(24.))
                        .flex_none()
                        .rounded_full()
                        .p(px(3.))
                        .bg(color(if checked {
                            "--control-primary-background-default"
                        } else {
                            "--background-surface-higher"
                        }))
                        .border_1()
                        .border_color(color(if checked {
                            "--control-primary-border-default"
                        } else {
                            "--border-subtle"
                        }))
                        .flex()
                        .items_center()
                        .when(checked, |d| d.justify_end())
                        .child(div().size(px(16.)).rounded_full().bg(color("--white"))),
                )
        };

        let radio_row = |id: SharedString,
                         label: String,
                         selected: bool,
                         on_select: Box<dyn Fn(&mut Root) + 'static>| {
            div()
                .id(id)
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if selected {
                    "--control-primary-border-default"
                } else {
                    "--border-subtle"
                }))
                .bg(color(if selected {
                    "--background-mod-subtle"
                } else {
                    "--background-base-low"
                }))
                .when(!loading, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                })
                .when(loading, |d| d.opacity(0.6))
                .flex()
                .items_center()
                .gap(px(10.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this
                        .favorites_thread_settings
                        .as_ref()
                        .is_some_and(|m| m.loading)
                    {
                        return;
                    }
                    on_select(this);
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(18.))
                        .rounded_full()
                        .border_2()
                        .border_color(color(if selected {
                            "--control-primary-background-default"
                        } else {
                            "--interactive-icon-default"
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |d| {
                            d.child(
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(color("--control-primary-background-default")),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(if selected { 600. } else { 400. }))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
        };

        let mut stack = div()
            .px(px(24.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(switch_row(
                "fav-settings-auto-add",
                dc_intl::t("DIyQIF"),
                dc_intl::t("d15FpO"),
                modal.auto_add,
                |this| {
                    if let Some(m) = this.favorites_thread_settings.as_mut() {
                        m.auto_add = !m.auto_add;
                        m.error = false;
                    }
                },
            ))
            .child(switch_row(
                "fav-settings-prune",
                dc_intl::t("bI/czq"),
                dc_intl::t("RRsgY0"),
                modal.prune,
                |this| {
                    if let Some(m) = this.favorites_thread_settings.as_mut() {
                        m.prune = !m.prune;
                        m.error = false;
                    }
                },
            ));

        if modal.prune {
            let mut days_group = div().flex().flex_col().gap(px(8.)).child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-muted"))
                    .child(dc_intl::t("THUVKS")),
            );
            for d in FAVORITES_PRUNE_DAYS {
                days_group = days_group.child(radio_row(
                    SharedString::from(format!("fav-prune-days-{d}")),
                    favorites_prune_days_label(d),
                    modal.days == d,
                    Box::new(move |this| {
                        if let Some(m) = this.favorites_thread_settings.as_mut() {
                            m.days = d;
                            m.error = false;
                        }
                    }),
                ));
            }
            let scope_group = div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("KIzpqi")),
                )
                .child(radio_row(
                    SharedString::from("fav-prune-scope-auto-added"),
                    AUTO_ADDED_THREADS_CATEGORY.to_owned(),
                    !modal.prune_all,
                    Box::new(|this| {
                        if let Some(m) = this.favorites_thread_settings.as_mut() {
                            m.prune_all = false;
                            m.error = false;
                        }
                    }),
                ))
                .child(radio_row(
                    SharedString::from("fav-prune-scope-all"),
                    dc_intl::t("gOUr+P").to_owned(),
                    modal.prune_all,
                    Box::new(|this| {
                        if let Some(m) = this.favorites_thread_settings.as_mut() {
                            m.prune_all = true;
                            m.error = false;
                        }
                    }),
                ));
            stack = stack.child(days_group).child(scope_group);
        }

        if modal.error {
            stack = stack.child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(dc_intl::t("toOYQw")),
            );
        }

        Some(
            gpui::deferred(
                div()
                    .id("favorites-thread-settings-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if !this
                                .favorites_thread_settings
                                .as_ref()
                                .is_some_and(|m| m.loading)
                            {
                                this.favorites_thread_settings = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("favorites-thread-settings-modal")
                            .occlude()
                            .w(px(480.))
                            .max_h(px(760.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(20.))
                                    .pb(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("hklnjs")),
                                    )
                                    .child(
                                        div()
                                            .id("fav-settings-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                16.,
                                                "--interactive-text-default",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if !this
                                                    .favorites_thread_settings
                                                    .as_ref()
                                                    .is_some_and(|m| m.loading)
                                                {
                                                    this.favorites_thread_settings = None;
                                                    cx.notify();
                                                }
                                            })),
                                    ),
                            )
                            .child(stack)
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(16.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("fav-settings-cancel")
                                            .h(px(38.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-muted"))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .when(!loading, |d| d.cursor_pointer())
                                            .when(loading, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if !this
                                                    .favorites_thread_settings
                                                    .as_ref()
                                                    .is_some_and(|m| m.loading)
                                                {
                                                    this.favorites_thread_settings = None;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("fav-settings-save")
                                            .h(px(38.))
                                            .px(px(18.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .when(can_save, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(color(
                                                        "--control-primary-background-hover",
                                                    ))
                                                })
                                            })
                                            .when(!can_save, |d| d.opacity(0.5))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(dc_intl::t("R3BPH+"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let Some(snapshot) =
                                                    this.favorites_thread_settings.clone()
                                                else {
                                                    return;
                                                };
                                                if !snapshot.can_save() || snapshot.loading {
                                                    return;
                                                }
                                                let mut fav = this.stores.favorites();
                                                if !apply_favorites_thread_settings(
                                                    &mut fav, &snapshot,
                                                ) {
                                                    if let Some(m) =
                                                        this.favorites_thread_settings.as_mut()
                                                    {
                                                        m.error = true;
                                                    }
                                                    cx.notify();
                                                    return;
                                                }
                                                this.send_favorites(
                                                    &fav,
                                                    "Favorites thread settings",
                                                );
                                                this.favorites_thread_settings = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(90)
            .into_any_element(),
        )
    }

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
                        .on_mouse_up(
                            MouseButton::Right,
                            cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
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

    /// Toggles collapsed state of a `FAVORITE_CATEGORY` entry and persists it via settings-proto.
    pub(super) fn toggle_favorite_category(&mut self, category_id: Id, cx: &mut Context<Self>) {
        let mut favorites = self.stores.favorites();
        if let Some((_, entry)) = favorites
            .channels
            .iter_mut()
            .find(|(id, known)| *id == category_id && known.kind == FAVORITE_CATEGORY)
        {
            entry.collapsed = !entry.collapsed;
            self.send_favorites(&favorites, "Collapse Category");
            cx.notify();
        }
    }

    /// Deletes a `FAVORITE_CATEGORY` entry, un-parenting any child channels so they remain in Favorites.
    pub(super) fn delete_favorite_category(&mut self, category_id: Id, cx: &mut Context<Self>) {
        let mut favorites = self.stores.favorites();
        let before = favorites.channels.len();
        favorites
            .channels
            .retain(|(id, known)| !(*id == category_id && known.kind == FAVORITE_CATEGORY));
        if favorites.channels.len() == before {
            return;
        }
        for (_, known) in &mut favorites.channels {
            if known.parent_id == category_id {
                known.parent_id = 0;
            }
        }
        self.send_favorites(&favorites, "Delete Category");
        cx.notify();
    }

    fn push_favorite_ref_rows(
        &self,
        rows: &mut Vec<AnyElement>,
        id: Id,
        nickname: &str,
        cx: &mut Context<Self>,
    ) {
        if let Some(dm) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == id)
        {
            if dm.kind == 1 {
                rows.extend(self.favorite_dm_row(dm, nickname, cx));
            } else {
                rows.extend(self.dm_row(dm, cx));
            }
            return;
        }
        let Some((guild, channel)) = self.favorite_channel(id) else {
            return;
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

    /// The channel list of the Favorites server: the header (`header_XH4OpX`: star, name, chevron, the add button),
    /// uncategorized favourite channels, and collapsible `FAVORITE_CATEGORY` groups with their child channels.
    pub(super) fn favorites_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let favorites = self.stores.favorites();
        let mut categories: Vec<(Id, String, u64, bool)> = favorites
            .channels
            .iter()
            .filter(|(_, known)| known.kind == FAVORITE_CATEGORY)
            .map(|(id, known)| (*id, known.nickname.clone(), known.position, known.collapsed))
            .collect();
        categories.sort_by_key(|(_, _, pos, _)| *pos);
        let category_ids: std::collections::HashSet<Id> =
            categories.iter().map(|(id, _, _, _)| *id).collect();
        let mut refs: Vec<(Id, String, u64, Id)> = favorites
            .channels
            .iter()
            .filter(|(_, known)| known.kind == FAVORITE_REFERENCE)
            .map(|(id, known)| (*id, known.nickname.clone(), known.position, known.parent_id))
            .collect();
        refs.sort_by_key(|(_, _, pos, _)| *pos);

        let mut rows: Vec<AnyElement> = Vec::new();
        for (id, nickname, _, parent) in &refs {
            if *parent == 0 || !category_ids.contains(parent) {
                self.push_favorite_ref_rows(&mut rows, *id, nickname, cx);
            }
        }
        for (cat_id, cat_name, _, collapsed) in &categories {
            let cat_id = *cat_id;
            let collapsed = *collapsed;
            let label = if cat_name.trim().is_empty() {
                dc_intl::tr("Favorites").to_uppercase()
            } else {
                cat_name.to_uppercase()
            };
            let chevron = if collapsed {
                "overlay_chevron_right"
            } else {
                "overlay_chevron_down"
            };
            rows.push(
                div()
                    .id(("favorite-category", cat_id))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(28.))
                    .mt(px(12.))
                    .px(px(8.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--channels-default"))
                    .hover(|s| s.text_color(color("--interactive-text-hover")))
                    .child(icon(chevron, 12., "--channels-default"))
                    .child(
                        div()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(label),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_favorite_category(cat_id, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            this.open_context_menu(
                                context_menu::Target::FavoriteCategory(cat_id),
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .into_any_element(),
            );
            if !collapsed {
                for (id, nickname, _, parent) in &refs {
                    if *parent == cat_id {
                        self.push_favorite_ref_rows(&mut rows, *id, nickname, cx);
                    }
                }
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
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
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
        let status = self.display_status(recipient.id, None);
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
                        .on_mouse_up(
                            MouseButton::Right,
                            cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
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

    /// Favorites empty state (module 205375: `wh+Rz1`, `+SuGKb`, `6kk0gM` + CDN art `988572.A`).
    pub(super) fn favorites_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        const ART_URL: &str = "https://cdn.discordapp.com/assets/content/88fc1694d1f36c93bf35b9cdc7dda25f1064d8205bfa103d62fd6cbf7fe636f7.png";
        let hero = if let Some(art) = self.images.get(ART_URL).cloned() {
            img(art)
                .w(px(197.))
                .h(px(107.))
                .object_fit(gpui::ObjectFit::Contain)
                .into_any_element()
        } else {
            div()
                .w(px(197.))
                .h(px(107.))
                .rounded(px(12.))
                .bg(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("clips_star", 44., "--text-brand"))
                .into_any_element()
        };
        div()
            .id("favorites-empty-state")
            .flex_1()
            .h_full()
            .border_t_1()
            .border_color(color("--app-frame-border"))
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(440.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .text_center()
                    .child(hero)
                    .child(
                        div()
                            .max_w(px(321.))
                            .text_size(px(24.))
                            .line_height(px(30.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(dc_intl::t("wh+Rz1")),
                    )
                    .child(
                        div()
                            .max_w(px(321.))
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("+SuGKb")),
                    )
                    .child(
                        div()
                            .id("favorites-empty-add")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--control-primary-text-default"))
                            .child(icon("plus_small", 18., "--white"))
                            .child(dc_intl::t("6kk0gM"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_quick_switcher(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// No accessible text channels fallback pane (module 205375: `ai6Lbr`, `LTr+x9`).
    pub(super) fn no_text_channels_view(&self) -> AnyElement {
        div()
            .id("no-text-channels-pane")
            .flex_1()
            .h_full()
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(440.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .text_center()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("ai6Lbr")),
                    )
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("LTr+x9")),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_empty_205375_intl_keys() {
        for k in ["wh+Rz1", "+SuGKb", "6kk0gM", "ai6Lbr", "LTr+x9"] {
            assert!(dc_intl::has_key(k), "missing 205375 key {k}");
        }
    }

    #[test]
    fn favorites_thread_settings_167846_dirty_apply_and_intl() {
        for k in [
            "hklnjs", "ETE/oC", "R3BPH+", "DIyQIF", "d15FpO", "bI/czq", "RRsgY0", "THUVKS",
            "EIVQqq", "eic1d0", "KIzpqi", "gOUr+P", "toOYQw",
        ] {
            assert!(dc_intl::has_key(k), "missing 167846 key {k}");
        }
        assert_eq!(favorites_prune_days_label(0), dc_intl::t("EIVQqq"));
        assert!(favorites_prune_days_label(7).contains('7'));
        assert!(favorites_prune_days_label(14).contains("14"));

        let mut fav = dc_proto::settings::Favorites {
            guild_visible: Some(true),
            ..Default::default()
        };
        let mut modal = FavoritesThreadSettingsModal::from_favorites(&fav);
        assert!(modal.prune);
        assert!(!modal.can_save());

        modal.days = 7;
        assert!(modal.can_save());
        modal.prune = false;
        assert!(modal.can_save());
        modal.prune = true;
        modal.days = 0;
        assert!(!modal.can_save());

        modal.auto_add = true;
        modal.days = 14;
        modal.prune_all = true;
        assert!(modal.can_save());
        assert!(apply_favorites_thread_settings(&mut fav, &modal));
        assert!(fav.auto_add_joined_threads);
        assert_eq!(fav.prune_inactive_threads, Some(true));
        assert_eq!(fav.prune_inactive_threads_days, 14);
        assert!(fav.prune_all_favorited_threads);
        assert!(fav.channels.iter().any(|(_, c)| {
            c.kind == FAVORITE_CATEGORY && c.nickname == AUTO_ADDED_THREADS_CATEGORY
        }));
    }
}
