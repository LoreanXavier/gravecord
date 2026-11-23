//! Friends in Server modal (`GuildFriendsModal`, module 151203, CSS `707185`).
//!
//! Lists all of the current user's friends who belong to the active guild, sorted case-insensitively
//! by guild nickname (fallback username), with profile popout on click and `REQUEST_GUILD_MEMBERS`
//! (Gateway Op 8) loading state (`EtQnZi` while fetching, `OgMdNS` once done).

use super::{Root, avatar_source, color, icon};
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img, prelude::*, px,
    rgba,
};
use std::borrow::Cow;

/// Per-guild friends member-fetch state (`542986`: 0 idle, 1 fetching, 2 done).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GuildFriendsFetchState {
    #[default]
    Idle,
    Fetching,
    Done,
}

/// One resolved friend row in `GuildFriendsModal` (module 151203).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GuildFriendRow {
    pub user_id: Id,
    pub display_name: String,
    pub username: String,
    pub is_owner: bool,
}

/// Open state for `GuildFriendsModal` (module 151203).
#[derive(Clone, Debug)]
pub(crate) struct GuildFriendsModalState {
    #[allow(dead_code)]
    pub guild_id: Id,
    pub guild_name: String,
    pub fetch_state: GuildFriendsFetchState,
    pub rows: Vec<GuildFriendRow>,
}

/// Module 151203 (`EtQnZi` while fetching vs `OgMdNS` once done).
pub(crate) fn guild_friends_subtitle(
    fetching: bool,
    num_friends: usize,
    guild_name: &str,
) -> String {
    let key = if fetching { "EtQnZi" } else { "OgMdNS" };
    if dc_intl::has_key(key) {
        dc_intl::format(
            key,
            &[
                ("numFriends", Arg::Num(num_friends as f64)),
                ("guildName", Arg::Str(Cow::Borrowed(guild_name))),
            ],
        )
    } else if fetching {
        if num_friends == 0 {
            format!("Friends in {guild_name}")
        } else {
            format!("{num_friends}+ Friends in {guild_name}")
        }
    } else if num_friends == 1 {
        format!("1 Friend in {guild_name}")
    } else {
        format!("{num_friends} Friends in {guild_name}")
    }
}

/// Module 151203: sorts guild friend rows case-insensitively by `(nick ?? username)`.
pub(crate) fn sort_guild_friends(rows: &mut [GuildFriendRow]) {
    rows.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
}

impl Root {
    /// Opens `GuildFriendsModal` (module 151203) for `guild_id`, collecting known friend members
    /// from `self.stores.relationships` (type 1 = friend) + `self.stores.members`.
    pub(super) fn open_guild_friends_modal(&mut self, guild_id: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.stores.guild(guild_id) else {
            return;
        };
        let guild_name = guild.name.clone();
        let owner_id = guild.owner_id;
        let friend_ids: Vec<Id> = self
            .stores
            .relationships
            .iter()
            .filter_map(|(&uid, &kind)| (kind == 1).then_some(uid))
            .collect();
        let mut rows: Vec<GuildFriendRow> = friend_ids
            .iter()
            .filter_map(|&uid| {
                let member = self.stores.members.get(guild_id, uid)?;
                let user = self.stores.users.get(&uid)?;
                let display = member
                    .nick
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&user.username)
                    .to_owned();
                Some(GuildFriendRow {
                    user_id: uid,
                    display_name: display,
                    username: user.username.clone(),
                    is_owner: owner_id == Some(uid),
                })
            })
            .collect();
        sort_guild_friends(&mut rows);
        self.guild_friends_modal = Some(GuildFriendsModalState {
            guild_id,
            guild_name,
            fetch_state: GuildFriendsFetchState::Done,
            rows,
        });
        cx.notify();
    }

    /// Renders `GuildFriendsModal` (module 151203, `kYxEcH`, `EtQnZi`/`OgMdNS`).
    pub(super) fn guild_friends_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.guild_friends_modal.as_ref()?;
        let fetching = state.fetch_state == GuildFriendsFetchState::Fetching;
        let title = if dc_intl::has_key("kYxEcH") {
            dc_intl::t("kYxEcH").to_owned()
        } else {
            "Friends in Server".to_owned()
        };
        let subtitle = guild_friends_subtitle(fetching, state.rows.len(), &state.guild_name);

        let list_body: AnyElement = if fetching {
            div()
                .h(px(120.))
                .flex()
                .items_center()
                .justify_center()
                .child(super::voice_view::spinner())
                .into_any_element()
        } else if state.rows.is_empty() {
            div()
                .py(px(28.))
                .px(px(20.))
                .text_center()
                .text_size(px(14.))
                .text_color(color("--text-muted"))
                .child(subtitle.clone())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(state.rows.iter().map(|row| {
                    let uid = row.user_id;
                    let pic = self
                        .stores
                        .users
                        .get(&uid)
                        .and_then(|u| self.images.get(&avatar_source(u).0).cloned());
                    let initial = row
                        .display_name
                        .chars()
                        .next()
                        .unwrap_or('?')
                        .to_ascii_uppercase()
                        .to_string();
                    let avatar = match pic {
                        Some(image) => img(image)
                            .size(px(32.))
                            .rounded_full()
                            .flex_none()
                            .into_any_element(),
                        None => div()
                            .flex_none()
                            .size(px(32.))
                            .rounded_full()
                            .bg(color("--background-brand"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(initial)
                            .into_any_element(),
                    };
                    div()
                        .id(SharedString::from(format!("guild-friend-row-{uid}")))
                        .w_full()
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(avatar)
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
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .truncate()
                                                .child(row.display_name.clone()),
                                        )
                                        .when(row.is_owner, |r| {
                                            r.child(icon("clips_star", 12., "--status-warning"))
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(color("--text-muted"))
                                        .truncate()
                                        .child(format!("@{}", row.username)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.guild_friends_modal = None;
                            let anchor = gpui::Bounds::new(
                                gpui::point(px(360.), px(240.)),
                                gpui::size(px(32.), px(32.)),
                            );
                            this.open_profile(uid, anchor, false, cx);
                        }))
                }))
                .into_any_element()
        };

        Some(
            deferred(
                div()
                    .id("guild-friends-modal-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.guild_friends_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("guild-friends-modal")
                            .w(px(440.))
                            .max_h(px(520.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .px(px(20.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .border_b_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-muted"))
                                                    .child(subtitle),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("guild-friends-modal-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.guild_friends_modal = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(div().p(px(12.)).child(list_body)),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_guild_friends(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.guild_friends_modal = None;
            cx.notify();
            return "guild-friends: closed".to_owned();
        }
        let guild_name = self
            .stores
            .guilds
            .first()
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "gravecord fixture".to_owned());
        let mut rows = vec![
            GuildFriendRow {
                user_id: 201,
                display_name: "plü".into(),
                username: "plu".into(),
                is_owner: true,
            },
            GuildFriendRow {
                user_id: 202,
                display_name: "lawleis".into(),
                username: "lawleis".into(),
                is_owner: false,
            },
            GuildFriendRow {
                user_id: 203,
                display_name: "Wren".into(),
                username: "wren".into(),
                is_owner: false,
            },
        ];
        sort_guild_friends(&mut rows);
        self.guild_friends_modal = Some(GuildFriendsModalState {
            guild_id: self.guild.unwrap_or(1),
            guild_name,
            fetch_state: if arg.trim() == "loading" {
                GuildFriendsFetchState::Fetching
            } else {
                GuildFriendsFetchState::Done
            },
            rows,
        });
        cx.notify();
        "guild-friends: opened".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_friends_151203_sorting_and_intl() {
        for key in ["kYxEcH", "EtQnZi", "OgMdNS"] {
            assert!(dc_intl::has_key(key), "missing 151203 key {key}");
        }
        assert_eq!(dc_intl::t("kYxEcH"), "Friends in Server");
        assert!(guild_friends_subtitle(false, 3, "Guild").contains("3 Friends in Guild"));
        assert!(guild_friends_subtitle(true, 2, "Guild").contains("2+ Friends in Guild"));
        let mut rows = vec![
            GuildFriendRow {
                user_id: 1,
                display_name: "zoe".into(),
                username: "zoe".into(),
                is_owner: false,
            },
            GuildFriendRow {
                user_id: 2,
                display_name: "Alice".into(),
                username: "alice".into(),
                is_owner: true,
            },
        ];
        sort_guild_friends(&mut rows);
        assert_eq!(rows[0].display_name, "Alice");
        assert_eq!(rows[1].display_name, "zoe");
    }
}
