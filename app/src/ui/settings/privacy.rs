//! Messaging Permissions and Activity Privacy: the switches that are lists of servers or bits of one setting.
//!
//! All of it lives in the `privacy` message of the settings proto (module 253932): `restricted_guild_ids` (3, direct
//! messages off per server), `message_request_restricted_guild_ids` (16), `activity_restricted_guild_ids` (13) are
//! repeated fixed64, `default_guilds_restricted_v2` (27) and `default_message_request_restricted` (17) are `BoolValue`s
//! and `friend_source_flags` (11) a `UInt32Value`. Each change is the `PATCH /users/@me/settings-proto/1` the official
//! client sends, with the whole `privacy` message in it (`dc_proto::settings`).
//!
//! The pure decisions (what a switch shows, what a click writes) are plain functions with tests; the `Root` methods
//! only read the stores and send.

use crate::net::Request;
use crate::ui::Root;
use dc_proto::settings::{
    ACTIVITY_RESTRICTED_GUILDS, DM_DEFAULT_RESTRICTED, DM_RESTRICTED_GUILDS, FRIEND_EVERYONE,
    FRIEND_MUTUAL_FRIENDS, FRIEND_MUTUAL_GUILDS, FRIEND_NO_RELATION, FRIEND_SOURCE_FLAGS, IdList,
    MESSAGE_REQUEST_RESTRICTED_GUILDS, ProtoPref, REQUESTS_DEFAULT_RESTRICTED,
};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::json;

pub(super) const GUILD_SELECTOR: &str = "permissions_guild_selector";
pub(super) const GUILD_SELECTOR_OPEN: &str = "permissions_guild_selector/open";
pub(super) const DMS: &str = "permissions_dms_setting";
pub(super) const REQUESTS: &str = "permissions_message_requests_setting";
pub(super) const FRIENDS_EVERYONE: &str = "friend_requests_everyone_setting";
pub(super) const FRIENDS_OF_FRIENDS: &str = "friend_requests_mutual_friends_setting";
pub(super) const FRIENDS_SERVERS: &str = "friend_requests_mutual_guilds_setting";
pub(super) const ACTIVITY_GUILDS: &str = "activity_sharing_per_guild_setting";
const ACTIVITY_SORT_OPEN: &str = "activity_sharing_sort/open";

/// A switch's value and whether it can be pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct View {
    pub on: bool,
    pub disabled: bool,
}

/// Direct Messages (`vI`, module 158935): "All servers" shows the default for new servers, a server its own entry.
pub(super) fn dms_view(selected: u64, default_restricted: bool, restricted: &[u64]) -> View {
    View {
        on: if selected == 0 {
            !default_restricted
        } else {
            !restricted.contains(&selected)
        },
        disabled: false,
    }
}

/// What the Message requests switch reads (`vy`).
pub(super) struct RequestFacts<'a> {
    pub selected: u64,
    pub default_restricted: bool,
    pub restricted: &'a [u64],
    pub default_requests_restricted: bool,
    pub requests_restricted: &'a [u64],
}

/// Message requests: on unless filtering is off for the default or the server, and disabled while direct messages
/// are off there (a server that takes no DMs takes no requests).
pub(super) fn requests_view(facts: &RequestFacts) -> View {
    if facts.selected == 0 {
        View {
            on: !facts.default_restricted && !facts.default_requests_restricted,
            disabled: facts.default_restricted,
        }
    } else {
        let dms_off = facts.restricted.contains(&facts.selected);
        View {
            on: !dms_off && !facts.requests_restricted.contains(&facts.selected),
            disabled: dms_off,
        }
    }
}

/// `list` with `guild` put in (restricted) or taken out.
pub(super) fn with_guild(list: &[u64], guild: u64, restricted: bool) -> Vec<u64> {
    let mut out: Vec<u64> = list.iter().copied().filter(|id| *id != guild).collect();
    if restricted {
        out.push(guild);
    }
    out
}

/// How many of the servers differ from the default (the line under the selector, `uyBKps`).
pub(super) fn custom_count(guilds: &[u64], restricted: &[u64], default_restricted: bool) -> usize {
    guilds
        .iter()
        .filter(|id| restricted.contains(id) != default_restricted)
        .count()
}

/// Which of the three friend request switches a row is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Friends {
    Everyone,
    Mutual,
    Servers,
}

pub(super) fn friends_of(key: &str) -> Option<Friends> {
    match key {
        FRIENDS_EVERYONE => Some(Friends::Everyone),
        FRIENDS_OF_FRIENDS => Some(Friends::Mutual),
        FRIENDS_SERVERS => Some(Friends::Servers),
        _ => None,
    }
}

/// `ii.Lx`: all three bits is "everyone"; everyone also counts as each of the others.
pub(super) fn friends_on(flags: u64, which: Friends) -> bool {
    if flags & FRIEND_EVERYONE == FRIEND_EVERYONE {
        return true;
    }
    match which {
        Friends::Everyone => false,
        Friends::Mutual => flags & FRIEND_MUTUAL_FRIENDS != 0,
        Friends::Servers => flags & FRIEND_MUTUAL_GUILDS != 0,
    }
}

/// The flags after a click (`setValue` of `y5`, `y4`, `y6`): everyone sets all three or clears only "no relation";
/// the others add their bit, or clear it together with "no relation", which an "everyone" no longer is.
pub(super) fn friends_set(flags: u64, which: Friends, on: bool) -> u64 {
    match which {
        Friends::Everyone => {
            if on {
                FRIEND_EVERYONE
            } else {
                FRIEND_EVERYONE & !FRIEND_NO_RELATION
            }
        }
        Friends::Mutual | Friends::Servers => {
            let bit = if which == Friends::Mutual {
                FRIEND_MUTUAL_FRIENDS
            } else {
                FRIEND_MUTUAL_GUILDS
            };
            if on {
                flags | bit
            } else {
                flags & !bit & !FRIEND_NO_RELATION
            }
        }
    }
}

/// The member count line of a server row (`cI3aQA`).
pub(super) fn members_line(count: u64) -> String {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if count == 1 {
        format!("{grouped} member")
    } else {
        format!("{grouped} members")
    }
}

/// The order of the Activity Privacy server list (`Sq`, `SZ`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::ui) enum Sort {
    #[default]
    ServerOrder,
    RecentlyJoined,
    SharingOn,
    SharingOff,
}

pub(super) const SORTS: [(Sort, &str); 4] = [
    (Sort::ServerOrder, "Server Order"),
    (Sort::RecentlyJoined, "Recently Joined"),
    (Sort::SharingOn, "Activity Sharing On"),
    (Sort::SharingOff, "Activity Sharing Off"),
];

/// The list in the chosen order. `guilds` is (id, joined_at) in server order; `restricted` holds the servers whose
/// sharing is off. Ties keep the server order (the sort is stable).
pub(super) fn sorted(sort: Sort, guilds: &[(u64, String)], restricted: &[u64]) -> Vec<u64> {
    let mut list: Vec<&(u64, String)> = guilds.iter().collect();
    match sort {
        Sort::ServerOrder => {}
        // Newest first; a server without a join time goes first (`null == e.joinedAt ? -1`).
        Sort::RecentlyJoined => list.sort_by(|a, b| match (a.1.is_empty(), b.1.is_empty()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => b.1.cmp(&a.1),
        }),
        // Sharing on first: the servers that are not restricted.
        Sort::SharingOn => list.sort_by_key(|(id, _)| restricted.contains(id)),
        Sort::SharingOff => list.sort_by_key(|(id, _)| !restricted.contains(id)),
    }
    list.into_iter().map(|(id, _)| *id).collect()
}

impl Root {
    fn send_proto_patch(&mut self, patch: String) {
        if !patch.is_empty() {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PATCH,
                route: "/users/@me/settings-proto/1".to_owned(),
                body: Some(json!({"settings": patch})),
            });
        }
    }

    fn set_ids(&mut self, list: &IdList, ids: &[u64]) {
        let patch = self.stores.set_proto_ids(list, ids);
        self.send_proto_patch(patch);
    }

    /// A `BoolValue` of the `privacy` message that is not a page row.
    fn privacy_flag(&self, pref: &ProtoPref) -> bool {
        self.stores.proto_varint(pref.path, pref.field).unwrap_or(0) != 0
    }

    /// The server the Messaging Permissions selector shows: 0 is "All servers".
    fn dm_selected(&self) -> u64 {
        self.settings_extra.dm_guild
    }

    fn settings_guild_ids(&self) -> Vec<u64> {
        self.stores.guilds.iter().map(|guild| guild.id).collect()
    }

    /// The Direct Messages and Message requests switches of the selected server.
    pub(super) fn messaging_view(&self, key: &str) -> Option<View> {
        match key {
            DMS => Some(dms_view(
                self.dm_selected(),
                self.privacy_flag(&DM_DEFAULT_RESTRICTED),
                &self.stores.proto_ids(&DM_RESTRICTED_GUILDS),
            )),
            REQUESTS => Some(requests_view(&RequestFacts {
                selected: self.dm_selected(),
                default_restricted: self.privacy_flag(&DM_DEFAULT_RESTRICTED),
                restricted: &self.stores.proto_ids(&DM_RESTRICTED_GUILDS),
                default_requests_restricted: self.privacy_flag(&REQUESTS_DEFAULT_RESTRICTED),
                requests_restricted: &self.stores.proto_ids(&MESSAGE_REQUEST_RESTRICTED_GUILDS),
            })),
            _ => friends_of(key).map(|which| View {
                on: friends_on(self.friend_flags(), which),
                disabled: false,
            }),
        }
    }

    fn friend_flags(&self) -> u64 {
        // A message without the field means the account never narrowed it: everyone.
        self.stores
            .proto_varint(FRIEND_SOURCE_FLAGS.path, FRIEND_SOURCE_FLAGS.field)
            .unwrap_or(FRIEND_EVERYONE)
    }

    /// A switch of these pages was pressed. True when the key is one of them (`on` is the value it was given).
    pub(super) fn privacy_changed(&mut self, key: &'static str, on: bool, index: usize) -> bool {
        if key == GUILD_SELECTOR {
            let guilds = self.settings_guild_ids();
            self.settings_extra.dm_guild = index
                .checked_sub(1)
                .and_then(|slot| guilds.get(slot))
                .copied()
                .unwrap_or(0);
            return true;
        }
        if key == DMS || key == REQUESTS {
            // The switch shows the proto, not the click.
            self.setting_choices.remove(key);
            self.dm_switch_pressed(key, on);
            return true;
        }
        if let Some(which) = friends_of(key) {
            self.setting_choices.remove(key);
            let flags = friends_set(self.friend_flags(), which, on);
            let patch = self.stores.set_pref(&FRIEND_SOURCE_FLAGS, flags);
            self.send_proto_patch(patch);
            return true;
        }
        if key == "activity_sharing_toggle_all" {
            return true;
        }
        false
    }

    fn dm_switch_pressed(&mut self, key: &'static str, on: bool) {
        let selected = self.dm_selected();
        let restricted = !on;
        if selected == 0 {
            // "Do you want to also apply this change to all your existing servers?" (`uUr+GR`, `yAfu1p`): No keeps
            // the servers as they are, Yes writes the new default into every one of them.
            self.settings_extra.dialog = Some(super::actions::Dialog::ApplyToServers {
                requests: key == REQUESTS,
                restrict: restricted,
            });
            return;
        }
        if key == DMS {
            let list = with_guild(
                &self.stores.proto_ids(&DM_RESTRICTED_GUILDS),
                selected,
                restricted,
            );
            self.set_ids(&DM_RESTRICTED_GUILDS, &list);
        } else {
            let list = with_guild(
                &self.stores.proto_ids(&MESSAGE_REQUEST_RESTRICTED_GUILDS),
                selected,
                restricted,
            );
            self.set_ids(&MESSAGE_REQUEST_RESTRICTED_GUILDS, &list);
        }
    }

    /// The dialog's answer for the default of all servers: `apply_all` also rewrites every server's own entry.
    pub(super) fn apply_dm_default(&mut self, requests: bool, restrict: bool, apply_all: bool) {
        let all = self.settings_guild_ids();
        if requests {
            let patch = self
                .stores
                .set_pref(&REQUESTS_DEFAULT_RESTRICTED, u64::from(restrict));
            self.send_proto_patch(patch);
            if apply_all {
                let list = if restrict { all } else { Vec::new() };
                self.set_ids(&MESSAGE_REQUEST_RESTRICTED_GUILDS, &list);
            }
        } else {
            let patch = self
                .stores
                .set_pref(&DM_DEFAULT_RESTRICTED, u64::from(restrict));
            self.send_proto_patch(patch);
            if apply_all {
                let list = if restrict { all } else { Vec::new() };
                self.set_ids(&DM_RESTRICTED_GUILDS, &list);
            }
        }
    }

    /// The options of the server selector: "All servers", then the servers in the rail's order.
    pub(super) fn guild_selector(&self) -> (Vec<SharedString>, usize) {
        let mut options: Vec<SharedString> = vec!["All servers".into()];
        options.extend(
            self.stores
                .guilds
                .iter()
                .map(|guild| SharedString::from(guild.name.clone())),
        );
        let current = self
            .stores
            .guilds
            .iter()
            .position(|guild| guild.id == self.dm_selected())
            .map_or(0, |slot| slot + 1);
        (options, current)
    }

    /// The line under the selector while "All servers" is chosen: how many servers differ (`uyBKps`).
    pub(super) fn guild_selector_note(&self) -> Option<AnyElement> {
        if self.dm_selected() != 0 {
            return None;
        }
        let count = custom_count(
            &self.settings_guild_ids(),
            &self.stores.proto_ids(&DM_RESTRICTED_GUILDS),
            self.privacy_flag(&DM_DEFAULT_RESTRICTED),
        );
        if count == 0 {
            return None;
        }
        let (number, rest) = if count == 1 {
            (
                "1 server".to_owned(),
                " has custom DM permissions. Select it to see or change them.",
            )
        } else {
            (
                format!("{count} servers"),
                " have custom DM permissions. Select one to see or change them.",
            )
        };
        Some(
            div()
                .mt(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(crate::ui::color("--text-subtle"))
                .child(
                    div()
                        .flex()
                        .child(
                            div()
                                .text_color(crate::ui::color("--text-default"))
                                .child(number),
                        )
                        .child(rest),
                )
                .into_any_element(),
        )
    }

    /// The description under the Direct Messages switch for the chosen server (`vI`'s `useSubtitle`).
    pub(super) fn messaging_description(&self, key: &str) -> Option<super::model::Runs> {
        (key == DMS && self.dm_selected() != 0)
            .then_some(&[("Allow DMs from other members in this server", false)])
    }

    /// Where I Share Activity: the toggle-all row and a switch per server (`S3`).
    pub(super) fn activity_guild_list(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let restricted = self.stores.proto_ids(&ACTIVITY_RESTRICTED_GUILDS);
        let guilds: Vec<(u64, String)> = self
            .stores
            .guilds
            .iter()
            .map(|guild| (guild.id, guild.joined_at.clone()))
            .collect();
        let sort = self.settings_extra.activity_sort;
        let order = sorted(sort, &guilds, &restricted);
        let any_restricted = !restricted.is_empty();
        let sort_label = SORTS
            .iter()
            .find(|(known, _)| *known == sort)
            .map_or("", |(_, label)| *label);
        let open = self.setting_choices.get(ACTIVITY_SORT_OPEN) == Some(&super::Choice::On(true));
        let rows: Vec<AnyElement> = order
            .iter()
            .filter_map(|id| self.stores.guilds.iter().find(|guild| guild.id == *id))
            .map(|guild| {
                let id = guild.id;
                let on = !restricted.contains(&id);
                let picture = self.images.get(&format!("guild:{id}")).cloned();
                let count = self.stores.member_counts.get_member_count(id);
                div()
                    .id(SharedString::from(format!("activity-guild-{id}")))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(48.))
                            .rounded(px(16.))
                            .overflow_hidden()
                            .bg(crate::ui::color("--background-mod-muted"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(crate::ui::color("--text-subtle"))
                            .when_some(picture.filter(|_| guild.icon.is_some()), |this, picture| {
                                this.child(img(picture).size(px(48.)))
                            })
                            .when(guild.icon.is_none(), |this| {
                                this.child(dc_state::Guild::acronym_of(&guild.name))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(crate::ui::color("--text-strong"))
                                    .child(SharedString::from(guild.name.clone())),
                            )
                            .children(count.map(|count| {
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(crate::ui::color("--text-subtle"))
                                    .child(members_line(count))
                            })),
                    )
                    .child(super::render::plain_switch(
                        SharedString::from(format!("switch-activity-guild-{id}")),
                        on,
                        move |this, on| this.activity_guild_toggled(id, on),
                        cx,
                    ))
                    .into_any_element()
            })
            .collect();
        let menu = open.then(|| {
            gpui::deferred(
                crate::ui::keyboard::outside(
                    div().id("activity-sort-menu"),
                    crate::ui::keyboard::Layer::SettingsSelect,
                    cx,
                )
                .occlude()
                .absolute()
                .top(px(28.))
                .left_0()
                .w(px(220.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(crate::ui::color("--border-subtle"))
                .bg(crate::ui::color("--background-surface-higher"))
                .flex()
                .flex_col()
                .children(SORTS.iter().map(|(which, label)| {
                    let which = *which;
                    div()
                        .id(SharedString::from(format!("activity-sort-{label}")))
                        .h(px(32.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(crate::ui::color("--background-mod-normal")))
                        .text_size(px(14.))
                        .text_color(crate::ui::color("--text-default"))
                        .child(*label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings_extra.activity_sort = which;
                            this.setting_choices
                                .insert(ACTIVITY_SORT_OPEN, super::Choice::On(false));
                            cx.notify();
                        }))
                })),
            )
            .with_priority(2)
        });
        let _ = width;
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("activity-sort")
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(crate::ui::color("--text-subtle"))
                                    .child(sort_label)
                                    .child(crate::ui::icon(
                                        "settings_accordion_open",
                                        16.,
                                        "--text-subtle",
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.setting_choices
                                            .insert(ACTIVITY_SORT_OPEN, super::Choice::On(!open));
                                        cx.notify();
                                    })),
                            )
                            .children(menu),
                    )
                    .child(
                        div()
                            .id("activity-toggle-all")
                            .cursor_pointer()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(crate::ui::color("--text-link"))
                            .child(if any_restricted {
                                "Toggle All On"
                            } else {
                                "Toggle All Off"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activity_toggle_all();
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex().flex_col().gap(px(24.)).children(rows))
            .into_any_element()
    }

    fn activity_guild_toggled(&mut self, guild: u64, on: bool) {
        let list = with_guild(
            &self.stores.proto_ids(&ACTIVITY_RESTRICTED_GUILDS),
            guild,
            !on,
        );
        self.set_ids(&ACTIVITY_RESTRICTED_GUILDS, &list);
    }

    /// `onToggleAllActivityRestrictedGuilds`: any server off turns them all on, else all off.
    fn activity_toggle_all(&mut self) {
        let list = if self
            .stores
            .proto_ids(&ACTIVITY_RESTRICTED_GUILDS)
            .is_empty()
        {
            self.settings_guild_ids()
        } else {
            Vec::new()
        };
        self.set_ids(&ACTIVITY_RESTRICTED_GUILDS, &list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_messages_follow_the_default_or_the_server() {
        assert!(dms_view(0, false, &[]).on);
        assert!(!dms_view(0, true, &[]).on);
        assert!(dms_view(5, true, &[]).on);
        assert!(!dms_view(5, false, &[5]).on);
        assert!(dms_view(5, false, &[6]).on);
    }

    #[test]
    fn message_requests_need_direct_messages() {
        let facts = |selected, default_restricted, restricted: &'static [u64]| RequestFacts {
            selected,
            default_restricted,
            restricted,
            default_requests_restricted: false,
            requests_restricted: &[],
        };
        assert_eq!(
            requests_view(&facts(0, false, &[])),
            View {
                on: true,
                disabled: false
            }
        );
        assert_eq!(
            requests_view(&facts(0, true, &[])),
            View {
                on: false,
                disabled: true
            }
        );
        assert_eq!(
            requests_view(&facts(7, false, &[7])),
            View {
                on: false,
                disabled: true
            }
        );
        let filtered = RequestFacts {
            selected: 7,
            default_restricted: false,
            restricted: &[],
            default_requests_restricted: false,
            requests_restricted: &[7],
        };
        assert_eq!(
            requests_view(&filtered),
            View {
                on: false,
                disabled: false
            }
        );
    }

    #[test]
    fn a_server_is_put_in_or_taken_out_once() {
        assert_eq!(with_guild(&[1, 2], 3, true), vec![1, 2, 3]);
        assert_eq!(with_guild(&[1, 2, 3], 3, true), vec![1, 2, 3]);
        assert_eq!(with_guild(&[1, 2, 3], 2, false), vec![1, 3]);
        assert_eq!(custom_count(&[1, 2, 3], &[2], false), 1);
        assert_eq!(custom_count(&[1, 2, 3], &[2], true), 2);
    }

    #[test]
    fn friend_request_bits_follow_the_official_rules() {
        // Everyone is all three bits and counts as each switch.
        assert!(friends_on(14, Friends::Everyone));
        assert!(friends_on(14, Friends::Mutual));
        assert!(friends_on(14, Friends::Servers));
        assert!(!friends_on(6, Friends::Everyone));
        assert!(friends_on(6, Friends::Mutual));
        assert!(!friends_on(0, Friends::Servers));
        // Everyone off keeps the two narrower bits; a narrower one off also ends "everyone".
        assert_eq!(friends_set(14, Friends::Everyone, false), 6);
        assert_eq!(friends_set(0, Friends::Everyone, true), 14);
        assert_eq!(friends_set(14, Friends::Mutual, false), 4);
        assert_eq!(friends_set(4, Friends::Mutual, true), 6);
        assert_eq!(friends_set(6, Friends::Servers, false), 2);
    }

    #[test]
    fn member_counts_read_with_separators() {
        assert_eq!(members_line(1), "1 member");
        assert_eq!(members_line(2), "2 members");
        assert_eq!(members_line(2768), "2,768 members");
        assert_eq!(members_line(1_234_567), "1,234,567 members");
    }

    #[test]
    fn the_server_list_sorts_like_the_official_one() {
        let guilds = vec![
            (1, "2024-01-01".to_owned()),
            (2, "2025-06-01".to_owned()),
            (3, String::new()),
        ];
        assert_eq!(sorted(Sort::ServerOrder, &guilds, &[]), vec![1, 2, 3]);
        assert_eq!(sorted(Sort::RecentlyJoined, &guilds, &[]), vec![3, 2, 1]);
        assert_eq!(sorted(Sort::SharingOn, &guilds, &[1]), vec![2, 3, 1]);
        assert_eq!(sorted(Sort::SharingOff, &guilds, &[3]), vec![3, 1, 2]);
    }
}
