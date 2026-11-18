//! RelationshipNotifier (Vencord `src/plugins/relationshipNotifier`, GPL-3.0-or-later): the servers you are a member
//! of, your group DMs and your friends and incoming requests are kept per account (`relationship-notifier-*-<id>` in the
//! plugins' storage); when one goes without you having left it, a notification ("Relationship Notifier") says so: "X
//! removed you as a friend.", "A friend request from X has been removed.", "You were removed from the server X.",
//! "You were removed from the group X." (a click on a person's opens their profile). At sign-in (`CONNECTION_OPEN`,
//! and 5 s after the plugin is turned on) what went while you were away is told with `offlineRemovals`: "You are no
//! longer in the group/server X.", "You are no longer friends with X.", "Friend request from X has been revoked.".
//! Leaving a server, a group or removing a friend yourself says nothing. `notices` also shows the notice bar with
//! "OK". Each kind has its switch. Not part of the pinned client.

use super::super::Root;
use super::plugin_notifications::{Action, Notification};
use crate::plugins::State;
use dc_state::{Id, User};
use gpui::Context;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::time::Duration;

const ID: &str = "RelationshipNotifier";
const FRIEND: u64 = 1;
const INCOMING: u64 = 3;
const TITLE: &str = "Relationship Notifier";
/// The answers of the person lookups: `<KEY><user>:<why>`.
pub(in crate::ui) const KEY: &str = "plugin-relationship-notifier:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Settings {
    notices: bool,
    offline: bool,
    friends: bool,
    requests: bool,
    servers: bool,
    groups: bool,
}

fn settings_of(state: &State) -> Option<Settings> {
    if !state.is_on(ID) {
        return None;
    }
    let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
    Some(Settings {
        notices: flag("notices"),
        offline: flag("offlineRemovals"),
        friends: flag("friends"),
        requests: flag("friendRequestCancels"),
        servers: flag("servers"),
        groups: flag("groups"),
    })
}

fn current() -> Option<Settings> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(settings_of)
}

/// A server or a group as kept: its name and its icon's URL.
#[derive(Clone, Debug, PartialEq)]
struct Kept {
    id: Id,
    name: String,
    icon: Option<String>,
}

#[derive(Default)]
struct Memory {
    guilds: Vec<Kept>,
    groups: Vec<Kept>,
    friends: Vec<Id>,
    requests: Vec<Id>,
    /// `manuallyRemovedFriend`, `manuallyRemovedGuild`, `manuallyRemovedGroup`.
    removed_friend: Option<Id>,
    removed_guild: Option<Id>,
    removed_group: Option<Id>,
    /// The plugin was seen on (its `start`).
    started: bool,
    /// The lists changed since they were last written; a write is on its way.
    dirty: bool,
    flush_pending: bool,
}

thread_local! {
    static MEMORY: RefCell<Memory> = RefCell::new(Memory::default());
}

fn key(kind: &str, me: Id) -> String {
    format!("plugin-relationship-notifier-{kind}-{me}")
}

fn kept_to_value(list: &[Kept]) -> Value {
    Value::Array(
        list.iter()
            .map(|kept| json!({"id": kept.id.to_string(), "name": kept.name, "iconURL": kept.icon}))
            .collect(),
    )
}

fn kept_from_value(value: &Value) -> Vec<Kept> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Kept {
                id: item["id"].as_str()?.parse().ok()?,
                name: item["name"].as_str().unwrap_or_default().to_owned(),
                icon: item["iconURL"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

fn ids_from_value(value: &Value) -> Vec<Id> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str()?.parse().ok())
        .collect()
}

/// `getUniqueUsername`.
fn unique_username(username: &str, discriminator: &str) -> String {
    if discriminator == "0" || discriminator.is_empty() {
        username.to_owned()
    } else {
        format!("{username}#{discriminator}")
    }
}

/// What to say about a person once known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum About {
    RemovedFriend,
    RemovedRequest,
    NoLongerFriends,
    RequestRevoked,
}

impl About {
    fn tag(self) -> &'static str {
        match self {
            About::RemovedFriend => "removed-friend",
            About::RemovedRequest => "removed-request",
            About::NoLongerFriends => "no-longer-friends",
            About::RequestRevoked => "request-revoked",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "removed-friend" => About::RemovedFriend,
            "removed-request" => About::RemovedRequest,
            "no-longer-friends" => About::NoLongerFriends,
            "request-revoked" => About::RequestRevoked,
            _ => return None,
        })
    }

    fn text(self, name: &str) -> String {
        match self {
            About::RemovedFriend => format!("{name} removed you as a friend."),
            About::RemovedRequest => format!("A friend request from {name} has been removed."),
            About::NoLongerFriends => format!("You are no longer friends with {name}."),
            About::RequestRevoked => format!("Friend request from {name} has been revoked."),
        }
    }
}

impl Root {
    /// `notify`: the notice with `notices`, and the notification.
    fn relationship_notify(
        &mut self,
        settings: Settings,
        text: String,
        icon: Option<String>,
        action: Action,
        cx: &mut Context<Self>,
    ) {
        if settings.notices {
            self.plugin_notice(text.clone(), "OK".to_owned(), cx);
        }
        let focused = self.notification_focused;
        self.plugin_notify(
            Notification {
                title: TITLE.to_owned(),
                body: text,
                icon,
                action,
            },
            focused,
            cx,
        );
    }

    fn relationship_guilds(&self) -> Vec<Kept> {
        self.stores
            .guilds
            .iter()
            .filter(|guild| !guild.joined_at.is_empty())
            .map(|guild| Kept {
                id: guild.id,
                name: guild.name.clone(),
                icon: guild.icon.as_ref().map(|hash| {
                    format!("https://cdn.discordapp.com/icons/{}/{hash}.png", guild.id)
                }),
            })
            .collect()
    }

    fn relationship_groups(&self) -> Vec<Kept> {
        dc_state::private_channels::sorted_channels(&self.stores)
            .into_iter()
            .filter(|channel| channel.kind == 3)
            .map(|channel| Kept {
                id: channel.id,
                name: channel
                    .name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| {
                        channel
                            .recipients
                            .iter()
                            .filter_map(|id| self.stores.users.get(id))
                            .map(|user| user.username.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }),
                icon: channel.icon.as_ref().map(|hash| {
                    format!(
                        "https://cdn.discordapp.com/channel-icons/{}/{hash}.png",
                        channel.id
                    )
                }),
            })
            .collect()
    }

    fn relationship_people(&self) -> (Vec<Id>, Vec<Id>) {
        let mut friends = Vec::new();
        let mut requests = Vec::new();
        for (id, kind) in &self.stores.relationships {
            match *kind {
                FRIEND => friends.push(*id),
                INCOMING => requests.push(*id),
                _ => {}
            }
        }
        (friends, requests)
    }

    fn me(&self) -> Option<Id> {
        self.stores.current_user.as_ref().map(|me| me.id)
    }

    /// `syncGuilds`, `syncGroups`, `syncFriends`: what is now, kept. The storage is written only when a list changed,
    /// at most once a second (startup's burst of `GUILD_CREATE`s is one write, not one per server).
    fn relationship_sync(
        &mut self,
        guilds: bool,
        groups: bool,
        friends: bool,
        cx: &mut Context<Self>,
    ) {
        if self.me().is_none() {
            return;
        }
        let new_guilds = guilds.then(|| self.relationship_guilds());
        let new_groups = groups.then(|| self.relationship_groups());
        let new_people = friends.then(|| self.relationship_people());
        let schedule = MEMORY.with(|memory| {
            let mut memory = memory.borrow_mut();
            let mut changed = false;
            if let Some(list) = new_guilds {
                changed |= memory.guilds != list;
                memory.guilds = list;
            }
            if let Some(list) = new_groups {
                changed |= memory.groups != list;
                memory.groups = list;
            }
            if let Some((mut friends, mut requests)) = new_people {
                friends.sort_unstable();
                requests.sort_unstable();
                changed |= memory.friends != friends || memory.requests != requests;
                memory.friends = friends;
                memory.requests = requests;
            }
            memory.dirty |= changed;
            let schedule = memory.dirty && !memory.flush_pending;
            if schedule {
                memory.flush_pending = true;
            }
            schedule
        });
        if schedule {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let _ = this.update(cx, |this, _| this.relationship_flush());
            })
            .detach();
        }
    }

    fn relationship_flush(&self) {
        let Some(me) = self.me() else {
            return;
        };
        MEMORY.with(|memory| {
            let mut memory = memory.borrow_mut();
            memory.flush_pending = false;
            if !std::mem::take(&mut memory.dirty) {
                return;
            }
            crate::storage::set(&key("guilds", me), kept_to_value(&memory.guilds));
            crate::storage::set(&key("groups", me), kept_to_value(&memory.groups));
            crate::storage::set(
                &key("friends", me),
                json!({
                    "friends": memory.friends.iter().map(Id::to_string).collect::<Vec<_>>(),
                    "requests": memory.requests.iter().map(Id::to_string).collect::<Vec<_>>(),
                }),
            );
        });
    }

    /// `syncAndRunChecks`: what was kept last time against what is now.
    fn relationship_check(&mut self, cx: &mut Context<Self>) {
        let (Some(settings), Some(me)) = (current(), self.me()) else {
            return;
        };
        let old_guilds =
            crate::storage::get(&key("guilds", me)).map(|value| kept_from_value(&value));
        let old_groups =
            crate::storage::get(&key("groups", me)).map(|value| kept_from_value(&value));
        let old_friends = crate::storage::get(&key("friends", me));
        self.relationship_sync(true, true, true, cx);
        if !settings.offline {
            return;
        }
        let (groups, guilds, friends, requests) = MEMORY.with(|memory| {
            let memory = memory.borrow();
            (
                memory.groups.iter().map(|kept| kept.id).collect::<Vec<_>>(),
                memory.guilds.iter().map(|kept| kept.id).collect::<Vec<_>>(),
                memory.friends.clone(),
                memory.requests.clone(),
            )
        });
        if settings.groups {
            for group in old_groups.unwrap_or_default() {
                if !groups.contains(&group.id) {
                    let text = format!("You are no longer in the group {}.", group.name);
                    self.relationship_notify(settings, text, group.icon, Action::None, cx);
                }
            }
        }
        if settings.servers {
            for guild in old_guilds.unwrap_or_default() {
                if !guilds.contains(&guild.id)
                    && !self.stores.unavailable_guilds.contains(&guild.id)
                {
                    let text = format!("You are no longer in the server {}.", guild.name);
                    self.relationship_notify(settings, text, guild.icon, Action::None, cx);
                }
            }
        }
        let old_friends = old_friends.unwrap_or(Value::Null);
        if settings.friends {
            for id in ids_from_value(&old_friends["friends"]) {
                if !friends.contains(&id) {
                    self.relationship_about(id, About::NoLongerFriends, cx);
                }
            }
        }
        if settings.requests {
            for id in ids_from_value(&old_friends["requests"]) {
                // FRIEND, BLOCKED or OUTGOING_REQUEST now: not revoked.
                let now = self.stores.relationships.get(&id).copied();
                if requests.contains(&id) || matches!(now, Some(1 | 2 | 4)) {
                    continue;
                }
                self.relationship_about(id, About::RequestRevoked, cx);
            }
        }
    }

    /// `UserUtils.getUser`: from the store, else fetched; then the notification.
    fn relationship_about(&mut self, id: Id, about: About, cx: &mut Context<Self>) {
        match self.stores.users.get(&id).cloned() {
            Some(user) => self.relationship_about_user(&user, about, cx),
            None => {
                let _ = self.requests.send(crate::net::Request::Fetch {
                    key: format!("{KEY}{id}:{}", about.tag()),
                    route: format!("/users/{id}"),
                });
            }
        }
    }

    fn relationship_about_user(&mut self, user: &User, about: About, cx: &mut Context<Self>) {
        let Some(settings) = current() else {
            return;
        };
        let (_, url) = super::super::avatar_source(user);
        let name = unique_username(&user.username, &user.discriminator);
        self.relationship_notify(
            settings,
            about.text(&name),
            Some(url),
            Action::OpenProfile(user.id),
            cx,
        );
    }

    /// A person looked up for a notification arrived. True when the key was ours.
    pub(in crate::ui) fn relationship_notifier_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rest) = key.strip_prefix(KEY) else {
            return false;
        };
        // A failed lookup (`.catch(() => null)`) says nothing.
        if let Some((_, tag)) = rest.split_once(':')
            && let Some(about) = About::from_tag(tag)
            && let Some(user) = User::from_value(body)
        {
            self.relationship_about_user(&user, about, cx);
        }
        true
    }

    /// The plugin's flux handlers and its `start`.
    pub(in crate::ui) fn relationship_notifier_dispatch(
        &mut self,
        name: &str,
        data: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(settings) = current() else {
            MEMORY.with(|memory| memory.borrow_mut().started = false);
            return;
        };
        let start =
            MEMORY.with(|memory| !std::mem::replace(&mut memory.borrow_mut().started, true));
        if start && name != "READY" {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let _ = this.update(cx, |this, cx| this.relationship_check(cx));
            })
            .detach();
        }
        let id = |value: &Value| value["id"].as_str().and_then(|id| id.parse::<Id>().ok());
        match name {
            "READY" => self.relationship_check(cx),
            "GUILD_CREATE" => self.relationship_sync(true, false, false, cx),
            "GUILD_DELETE" => {
                let Some(guild) = id(data) else {
                    return;
                };
                if !settings.servers
                    || data["unavailable"] == Value::Bool(true)
                    || self.stores.unavailable_guilds.contains(&guild)
                {
                    return;
                }
                let (manual, known) = MEMORY.with(|memory| {
                    let mut memory = memory.borrow_mut();
                    let manual = memory.removed_guild == Some(guild);
                    if manual {
                        memory.removed_guild = None;
                    }
                    let known = memory.guilds.iter().find(|kept| kept.id == guild).cloned();
                    memory.guilds.retain(|kept| kept.id != guild);
                    (manual, known)
                });
                self.relationship_sync(true, false, false, cx);
                if !manual && let Some(kept) = known {
                    let text = format!("You were removed from the server {}.", kept.name);
                    self.relationship_notify(settings, text, kept.icon, Action::None, cx);
                }
            }
            "CHANNEL_CREATE" => self.relationship_sync(false, true, false, cx),
            "CHANNEL_DELETE" => {
                let Some(channel) = id(data) else {
                    return;
                };
                if !settings.groups || data["type"].as_u64() != Some(3) {
                    return;
                }
                let (manual, known) = MEMORY.with(|memory| {
                    let mut memory = memory.borrow_mut();
                    let manual = memory.removed_group == Some(channel);
                    if manual {
                        memory.removed_group = None;
                    }
                    let known = memory
                        .groups
                        .iter()
                        .find(|kept| kept.id == channel)
                        .cloned();
                    memory.groups.retain(|kept| kept.id != channel);
                    (manual, known)
                });
                self.relationship_sync(false, true, false, cx);
                if !manual && let Some(kept) = known {
                    let text = format!("You were removed from the group {}.", kept.name);
                    self.relationship_notify(settings, text, kept.icon, Action::None, cx);
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
                self.relationship_sync(false, false, true, cx);
            }
            "RELATIONSHIP_REMOVE" => {
                if let Some(user) = id(data) {
                    let manual = MEMORY.with(|memory| {
                        let mut memory = memory.borrow_mut();
                        let manual = memory.removed_friend == Some(user);
                        if manual {
                            memory.removed_friend = None;
                        }
                        manual
                    });
                    let about = match data["type"].as_u64() {
                        Some(FRIEND) if settings.friends => Some(About::RemovedFriend),
                        Some(INCOMING) if settings.requests => Some(About::RemovedRequest),
                        _ => None,
                    };
                    if !manual && let Some(about) = about {
                        self.relationship_about(user, about, cx);
                    }
                }
                self.relationship_sync(false, false, true, cx);
            }
            _ => {}
        }
    }
}

/// `removeRelationship`, `leaveGuild` and `closePrivateChannel` of your own: what goes next says nothing.
pub(in crate::ui) fn relationship_removed_by_you(
    friend: Option<Id>,
    guild: Option<Id>,
    group: Option<Id>,
) {
    if current().is_none() {
        return;
    }
    MEMORY.with(|memory| {
        let mut memory = memory.borrow_mut();
        if friend.is_some() {
            memory.removed_friend = friend;
        }
        if guild.is_some() {
            memory.removed_guild = guild;
        }
        if group.is_some() {
            memory.removed_group = group;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_and_names_follow_the_original() {
        assert_eq!(unique_username("ann", "0"), "ann");
        assert_eq!(unique_username("ann", "1234"), "ann#1234");
        assert_eq!(
            About::RemovedFriend.text("ann"),
            "ann removed you as a friend."
        );
        assert_eq!(
            About::RequestRevoked.text("ann"),
            "Friend request from ann has been revoked."
        );
        for about in [
            About::RemovedFriend,
            About::RemovedRequest,
            About::NoLongerFriends,
            About::RequestRevoked,
        ] {
            assert_eq!(About::from_tag(about.tag()), Some(about));
        }
        let kept = vec![Kept {
            id: 5,
            name: "srv".into(),
            icon: None,
        }];
        assert_eq!(kept_from_value(&kept_to_value(&kept)), kept);
        let mut state = State::default();
        assert_eq!(settings_of(&state), None);
        state.set_enabled(ID, true);
        let settings = settings_of(&state).unwrap();
        assert!(!settings.notices && settings.offline && settings.friends && settings.requests);
        assert!(settings.servers && settings.groups);
    }
}
