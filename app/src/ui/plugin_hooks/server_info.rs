//! ServerInfo (Vencord `src/plugins/serverInfo`, GPL-3.0-or-later): "Server Info" at the end of the server menu's
//! privacy group (`guild-context`) opens a large dialog (`GuildInfoModal`): the server's icon (or acronym), name and
//! description as its title; the banner above the tabs on Server Info; and the tabs Server Info, Friends, Mutual Users,
//! Blocked Users and Ignored Users, each with its count once shown. Server Info is a three-column grid: Server Owner
//! (20px avatar and mention), Created At, Joined At, Vanity Link, Preferred Locale, Verification Level, Server Boosts
//! (`n (Level t)`), Channels and Roles (without @everyone). Friends, Blocked and Ignored list the people of that kind
//! who are members (missing ones asked for once with `GUILD_MEMBERS_REQUEST`), sorted by `sorting`; Mutual Users the
//! members who share more than this server with you, most shared first, with up to three server icons and `+n`. A
//! row opens the person's profile; the icon and banner open the picture viewer. The server header's dropdown
//! (`guild-header-popout`) ends its privacy group with the row too, with the InfoIcon (`guild_menu.rs`). Not part of
//! the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use super::super::{avatar_source, color};
use crate::plugins::State;
use dc_state::{Id, User};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, StyledImage as _, div, img, px,
};
use std::cell::RefCell;
use std::collections::HashSet;

const ID: &str = "ServerInfo";
const CDN: &str = "https://cdn.discordapp.com";

/// The dialog's tabs (`Tabs`), in the bar's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Tab {
    ServerInfo,
    Friends,
    MutualMembers,
    BlockedUsers,
    IgnoredUsers,
}

const TABS: [(Tab, &str); 5] = [
    (Tab::ServerInfo, "Server Info"),
    (Tab::Friends, "Friends"),
    (Tab::MutualMembers, "Mutual Users"),
    (Tab::BlockedUsers, "Blocked Users"),
    (Tab::IgnoredUsers, "Ignored Users"),
];

/// `sorting`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sorting {
    Username,
    DisplayName,
    None,
}

fn sorting(state: &State) -> Option<Sorting> {
    state
        .is_on(ID)
        .then(|| match state.value(ID, "sorting").as_str() {
            Some("username") => Sorting::Username,
            Some("none") => Sorting::None,
            _ => Sorting::DisplayName,
        })
}

fn current() -> Option<Sorting> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(sorting)
}

/// Whether the plugin is on (the server header dropdown's row).
pub(in crate::ui) fn server_info_on() -> bool {
    current().is_some()
}

/// The open dialog.
#[derive(Clone, Debug)]
struct Open {
    guild: Id,
    tab: Tab,
    /// The tabs shown so far (their counts show from then on), and the relationship kinds already asked for.
    seen: HashSet<u8>,
    asked: HashSet<u8>,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// `getGuildAcronym`: the first letter of every word, punctuation kept.
pub(super) fn acronym(name: &str) -> String {
    name.replace("'s ", " ")
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .collect()
}

/// The sort of a list (`localeCompare` on the username, or on the global name falling back to the username).
fn sort_people(people: &mut [User], by: Sorting) {
    match by {
        Sorting::Username => people.sort_by_key(|user| user.username.to_lowercase()),
        Sorting::DisplayName => people.sort_by_key(|user| {
            user.global_name
                .as_deref()
                .unwrap_or(&user.username)
                .to_lowercase()
        }),
        Sorting::None => {}
    }
}

/// `["None", "Low", "Medium", "High", "Highest"][verificationLevel] || "?"`.
pub(super) fn verification(level: Option<u64>) -> &'static str {
    match level {
        Some(0) => "None",
        Some(1) => "Low",
        Some(2) => "Medium",
        Some(3) => "High",
        Some(4) => "Highest",
        _ => "?",
    }
}

/// The time a snowflake was made, in Unix seconds.
fn snowflake_seconds(id: Id) -> i64 {
    (((id >> 22) + 1_420_070_400_000) / 1000) as i64
}

impl Root {
    /// The server menu with "Server Info" after Edit Per-server Profile; as it is with the plugin off.
    pub(in crate::ui) fn with_server_info_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if current().is_none() || !matches!(target, Target::Guild(_)) {
            return items;
        }
        let at = items
            .iter()
            .position(|entry| matches!(entry, Entry::Row(row) if row.act == Act::EditProfile))
            .map_or(items.len(), |at| at + 1);
        items.insert(
            at,
            Entry::Row(Row::new(Act::PluginServerInfo, "Server Info")),
        );
        items
    }

    /// Opens the dialog on Server Info.
    pub(in crate::ui) fn server_info_open(&mut self, guild: Id, cx: &mut Context<Self>) {
        OPEN.with(|open| {
            *open.borrow_mut() = Some(Open {
                guild,
                tab: Tab::ServerInfo,
                seen: HashSet::new(),
                asked: HashSet::new(),
            });
        });
        cx.notify();
    }

    fn server_info_close(&mut self, cx: &mut Context<Self>) {
        OPEN.with(|open| open.borrow_mut().take());
        cx.notify();
    }

    fn server_info_select(&mut self, tab: Tab, cx: &mut Context<Self>) {
        OPEN.with(|open| {
            if let Some(open) = open.borrow_mut().as_mut() {
                open.tab = tab;
            }
        });
        cx.notify();
    }

    /// The people of a relationship list who are members of `guild`, and the ones still unknown.
    fn server_info_people(&self, guild: Id, tab: Tab) -> (Vec<User>, Vec<Id>) {
        let ids: Vec<Id> = match tab {
            Tab::Friends => self
                .stores
                .relationships
                .iter()
                .filter(|(_, kind)| **kind == dc_state::RELATIONSHIP_FRIEND)
                .map(|(user, _)| *user)
                .collect(),
            Tab::BlockedUsers => self
                .stores
                .relationships
                .iter()
                .filter(|(_, kind)| **kind == 2)
                .map(|(user, _)| *user)
                .collect(),
            Tab::IgnoredUsers => self
                .stores
                .ignored
                .iter()
                .copied()
                .filter(|user| self.stores.is_ignored(*user))
                .collect(),
            Tab::ServerInfo | Tab::MutualMembers => Vec::new(),
        };
        let mut members = Vec::new();
        let mut missing = Vec::new();
        for user in ids {
            match self.stores.members.get(guild, user) {
                Some(member) => members.push(member.user.clone()),
                None => missing.push(user),
            }
        }
        (members, missing)
    }

    /// Members who share another server with you: each with the servers they are in.
    fn server_info_mutuals(&self, guild: Id) -> Vec<(User, Vec<Id>)> {
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let mut out: Vec<(User, Vec<Id>)> = self
            .stores
            .members
            .of(guild)
            .iter()
            .filter(|member| Some(member.user.id) != me)
            .map(|member| {
                let shared: Vec<Id> = self
                    .stores
                    .guilds
                    .iter()
                    .filter(|known| self.stores.members.get(known.id, member.user.id).is_some())
                    .map(|known| known.id)
                    .collect();
                (member.user.clone(), shared)
            })
            .filter(|(_, shared)| shared.len() > 1)
            .collect();
        out.sort_by_key(|(_, shared)| std::cmp::Reverse(shared.len()));
        out
    }

    /// The dialog over everything, while it is open.
    pub(in crate::ui) fn server_info_layer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = OPEN.with(|open| open.borrow().clone())?;
        let Some(by) = current() else {
            OPEN.with(|open| open.borrow_mut().take());
            return None;
        };
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|known| known.id == open.guild)
            .cloned()?;
        // `useEffect`: the missing members of a list are asked for once.
        let kind = open.tab as u8;
        if matches!(
            open.tab,
            Tab::Friends | Tab::BlockedUsers | Tab::IgnoredUsers
        ) && !open.asked.contains(&kind)
        {
            let (_, missing) = self.server_info_people(guild.id, open.tab);
            if !missing.is_empty() {
                let _ = self.requests.send(crate::net::Request::GuildMembers {
                    guilds: vec![guild.id],
                    query: String::new(),
                    limit: 0,
                    user_ids: missing,
                });
            }
            OPEN.with(|cell| {
                if let Some(open) = cell.borrow_mut().as_mut() {
                    open.asked.insert(kind);
                }
            });
        }
        OPEN.with(|cell| {
            if let Some(open) = cell.borrow_mut().as_mut() {
                open.seen.insert(kind);
            }
        });
        let seen = OPEN.with(|cell| cell.borrow().as_ref().map(|open| open.seen.clone()))?;

        let icon_url = guild.icon.as_ref().map(|hash| {
            format!(
                "{CDN}/icons/{}/{hash}.webp?size=512&animated={}",
                guild.id,
                hash.starts_with("a_")
            )
        });
        let banner_url = guild
            .banner
            .as_ref()
            .map(|hash| format!("{CDN}/banners/{}/{hash}.webp?size=1024", guild.id));
        let icon = icon_url.as_ref().and_then(|url| {
            let key = format!("server-info:{url}");
            let picture = self.images.get(&key).cloned();
            if picture.is_none() {
                self.want_image(key, url.clone(), cx);
            }
            picture
        });
        let banner = banner_url.as_ref().and_then(|url| {
            let key = format!("server-info:{url}");
            let picture = self.images.get(&key).cloned();
            if picture.is_none() {
                self.want_image(key, url.clone(), cx);
            }
            picture
        });

        let count_of = |tab: Tab| -> Option<usize> {
            if !seen.contains(&(tab as u8)) {
                return None;
            }
            Some(match tab {
                Tab::MutualMembers => self.server_info_mutuals(guild.id).len(),
                Tab::ServerInfo => 0,
                _ => self.server_info_people(guild.id, tab).0.len(),
            })
        };

        let title_icon = match (icon, icon_url.clone()) {
            (Some(picture), Some(url)) => div()
                .id("server-info-icon")
                .size(px(48.))
                .flex_none()
                .cursor_pointer()
                .child(img(picture).size(px(48.)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_plain_picture(url.clone(), url.clone(), (512, 512), cx);
                }))
                .into_any_element(),
            _ => div()
                .size(px(48.))
                .flex_none()
                .rounded_full()
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(acronym(&guild.name))
                .into_any_element(),
        };
        let header = div()
            .m(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(title_icon)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(guild.name.clone()),
                    )
                    .children(guild.description.clone().map(|text| {
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(text)
                    })),
            );

        let tabs = div()
            .mt(px(20.))
            .mx(px(12.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(32.))
            .children(TABS.iter().map(|(tab, label)| {
                let tab = *tab;
                let selected = tab == open.tab;
                let text = match count_of(tab) {
                    Some(count) if tab != Tab::ServerInfo => format!("{label} ({count})"),
                    _ => (*label).to_owned(),
                };
                div()
                    .id(SharedString::from(format!("server-info-tab-{}", tab as u8)))
                    .pb(px(14.))
                    .border_b_2()
                    .border_color(if selected {
                        color("--interactive-icon-active")
                    } else {
                        gpui::rgba(0)
                    })
                    .hover(|style| style.border_color(color("--interactive-icon-active")))
                    .cursor_pointer()
                    .text_size(px(16.))
                    .line_height(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(if selected {
                        color("--interactive-text-active")
                    } else {
                        color("--interactive-icon-default")
                    })
                    .child(text)
                    .on_click(cx.listener(move |this, _, _, cx| this.server_info_select(tab, cx)))
            }));

        let content = match open.tab {
            Tab::ServerInfo => self.server_info_grid(&guild, cx),
            Tab::MutualMembers => {
                let rows: Vec<AnyElement> = self
                    .server_info_mutuals(guild.id)
                    .into_iter()
                    .map(|(user, shared)| self.server_info_row(user, Some(shared), cx))
                    .collect();
                self.server_info_scroller(rows)
            }
            tab => {
                let (mut people, _) = self.server_info_people(guild.id, tab);
                sort_people(&mut people, by);
                let rows: Vec<AnyElement> = people
                    .into_iter()
                    .map(|user| self.server_info_row(user, None, cx))
                    .collect();
                self.server_info_scroller(rows)
            }
        };

        let banner_part = (open.tab == Tab::ServerInfo)
            .then_some(())
            .and(banner.zip(banner_url))
            .map(|(picture, url)| {
                div()
                    .id("server-info-banner")
                    .px(px(8.))
                    .h(px(334.))
                    .w_full()
                    .cursor_pointer()
                    .child(img(picture).size_full().object_fit(gpui::ObjectFit::Cover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_plain_picture(url.clone(), url.clone(), (1024, 576), cx);
                    }))
            });

        let dialog = div()
            .id("server-info-dialog")
            .occlude()
            .w(px(800.))
            .max_h(px(self.viewport.1 * 0.9))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div().relative().p(px(16.)).child(header).child(
                    div()
                        .id("server-info-close")
                        .absolute()
                        .top(px(16.))
                        .right(px(16.))
                        .size(px(24.))
                        .cursor_pointer()
                        .child(super::super::icon("lb_close", 24., "--icon-default"))
                        .on_click(cx.listener(|this, _, _, cx| this.server_info_close(cx))),
                ),
            )
            .children(banner_part)
            .child(tabs)
            .child(div().m(px(16.)).child(content));

        Some(
            gpui::deferred(
                div()
                    .id("server-info-scrim")
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| this.server_info_close(cx)))
                    .child(dialog),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn server_info_scroller(&self, rows: Vec<AnyElement>) -> AnyElement {
        div()
            .id("server-info-scroller")
            .max_h(px(500.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// The Server Info grid.
    fn server_info_grid(&self, guild: &dc_state::Guild, cx: &mut Context<Self>) -> AnyElement {
        let stamp = |seconds: i64| super::super::format_timestamp(seconds, 'f');
        let joined = chrono::DateTime::parse_from_rfc3339(&guild.joined_at)
            .map(|time| stamp(time.timestamp()))
            .unwrap_or_else(|_| "-".to_owned());
        let owner: AnyElement = match guild
            .owner_id
            .and_then(|owner| self.stores.users.get(&owner).cloned())
        {
            Some(user) => {
                let (key, url) = avatar_source(&user);
                let picture = self.images.get(&key).cloned();
                if picture.is_none() {
                    self.want_image(key, url, cx);
                }
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .children(picture.map(|picture| img(picture).size(px(20.)).rounded_full()))
                    .child(
                        div()
                            .px(px(2.))
                            .rounded(px(3.))
                            .bg(color("--mention-background"))
                            .text_color(color("--mention-foreground"))
                            .font_weight(FontWeight(500.))
                            .child(format!("@{}", user.display_name())),
                    )
                    .into_any_element()
            }
            None => "Loading...".into_any_element(),
        };
        let channels = guild
            .channels
            .iter()
            .filter(|channel| channel.kind != dc_state::ChannelKind::Category)
            .count();
        let fields: Vec<(&str, AnyElement)> = vec![
            ("Server Owner", owner),
            (
                "Created At",
                stamp(snowflake_seconds(guild.id)).into_any_element(),
            ),
            ("Joined At", joined.into_any_element()),
            (
                "Vanity Link",
                guild
                    .vanity_url_code
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |code| format!("discord.gg/{code}"))
                    .into_any_element(),
            ),
            (
                "Preferred Locale",
                guild
                    .preferred_locale
                    .clone()
                    .unwrap_or_else(|| "-".to_owned())
                    .into_any_element(),
            ),
            (
                "Verification Level",
                verification(guild.verification_level).into_any_element(),
            ),
            (
                "Server Boosts",
                format!(
                    "{} (Level {})",
                    guild.premium_subscription_count, guild.premium_tier
                )
                .into_any_element(),
            ),
            ("Channels", channels.to_string().into_any_element()),
            (
                "Roles",
                guild
                    .roles
                    .len()
                    .saturating_sub(1)
                    .to_string()
                    .into_any_element(),
            ),
        ];
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_y(px(16.))
            .children(fields.into_iter().map(|(name, value)| {
                div()
                    .w(gpui::relative(1. / 3.))
                    .pr(px(16.))
                    .flex()
                    .flex_col()
                    .text_color(color("--text-default"))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .child(value)
            }))
            .into_any_element()
    }

    /// A person's row (`FriendRow`): the avatar with its status, the name and username; with `shared`, the shared
    /// servers' icons at the end. A press opens the profile.
    fn server_info_row(
        &self,
        user: User,
        shared: Option<Vec<Id>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (key, url) = avatar_source(&user);
        let picture = self.images.get(&key).cloned();
        if picture.is_none() {
            self.want_image(key, url, cx);
        }
        let status = self.stores.status(user.id).to_owned();
        let id = user.id;
        let icons = shared.map(|shared| {
            let more = shared.len().saturating_sub(3);
            div()
                .ml_auto()
                .pl(px(8.))
                .flex()
                .items_center()
                .gap(px(4.))
                .children(shared.into_iter().take(3).filter_map(|guild| {
                    let known = self.stores.guilds.iter().find(|known| known.id == guild)?;
                    let picture = known.icon.as_ref().and_then(|hash| {
                        let url = format!("{CDN}/icons/{guild}/{hash}.webp?size=20");
                        let key = format!("server-info:{url}");
                        let picture = self.images.get(&key).cloned();
                        if picture.is_none() {
                            self.want_image(key, url, cx);
                        }
                        picture
                    });
                    Some(
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(color("--background-base-low"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(8.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .map(|this| match picture {
                                Some(picture) => this.child(img(picture).size_full()),
                                None => this.child(acronym(&known.name).to_uppercase()),
                            }),
                    )
                }))
                .when(more > 0, |this| {
                    this.child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-muted"))
                            .child(format!("+{more}")),
                    )
                })
        });
        div()
            .id(SharedString::from(format!("server-info-row-{id}")))
            .h(px(62.))
            .my(px(1.))
            .px(px(10.))
            .pr(px(16.))
            .flex()
            .items_center()
            .gap(px(12.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .child(
                div()
                    .relative()
                    .size(px(32.))
                    .flex_none()
                    .children(picture.map(|picture| img(picture).size(px(32.)).rounded_full()))
                    .child(super::super::status_dot(&status, "--background-base-low")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .children(icons)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.server_info_close(cx);
                this.open_full_profile(id, window, cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn acronyms_and_levels_follow_the_original() {
        assert_eq!(acronym("gravecord fixture"), "gf");
        assert_eq!(acronym("Bob's Place Here"), "BPH");
        assert_eq!(verification(Some(2)), "Medium");
        assert_eq!(verification(Some(9)), "?");
        assert_eq!(verification(None), "?");
        // The fixture server's id is a snowflake of 2026.
        assert_eq!(
            chrono::DateTime::from_timestamp(snowflake_seconds(1568692564637524109), 0)
                .unwrap()
                .format("%Y")
                .to_string(),
            "2026"
        );
    }

    #[test]
    fn sorting_defaults_to_display_names() {
        let mut state = State::default();
        assert_eq!(sorting(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(sorting(&state), Some(Sorting::DisplayName));
        state.set_value(ID, "sorting", &json!("username"));
        assert_eq!(sorting(&state), Some(Sorting::Username));
        let mut people = vec![
            User::from_value(&json!({"id": "1", "username": "zed", "global_name": "Alpha"}))
                .unwrap(),
            User::from_value(&json!({"id": "2", "username": "amy", "global_name": "Zulu"}))
                .unwrap(),
        ];
        sort_people(&mut people, Sorting::DisplayName);
        assert_eq!(people[0].username, "zed");
        sort_people(&mut people, Sorting::Username);
        assert_eq!(people[0].username, "amy");
    }
}
