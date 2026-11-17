//! Capture only: `content demo <set>` puts messages whose content opens popouts (a role mention, a code block, a server
//! tag, a bot tag, a role icon, attachments, embeds) into the open channel, in memory only. Nothing is sent anywhere.
//! Sets: `roles`, `code`, `header`, `files`, `embeds`, `reply`, `links`, `reactions`.

use super::Root;
use dc_state::members::Member;
use dc_state::{Id, User};
use gpui::Context;
use serde_json::{Value, json};

const MODS: Id = 900_100;
const LEADS: Id = 900_101;
const ICON_ROLE: Id = 900_102;
const GRAD2: Id = 900_103;
const GRAD3: Id = 900_104;
const GRAD_PLAIN: Id = 900_105;

impl Root {
    pub(crate) fn script_content(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let [verb, set] = words else {
            return "content: usage `content demo <roles|code|header|files|embeds|reply>`"
                .to_owned();
        };
        if *verb != "demo" {
            return "content: usage `content demo <set>`".to_owned();
        }
        let Some(channel) = self.channel else {
            return "content: no channel is open".to_owned();
        };
        let guild = self.guild;
        let now = chrono::Utc::now();
        let base = ((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22;
        let mut made: Vec<Value> = Vec::new();
        let mut push = |author: Value, content: &str, extra: Value| {
            let n = made.len() as u64;
            let mut value = json!({
                "id": (base + n + 1).to_string(),
                "channel_id": channel.to_string(),
                "type": 0,
                "content": content,
                "timestamp": (now - chrono::Duration::seconds(300 - n as i64 * 20))
                    .to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                "author": author,
            });
            for (key, field) in extra.as_object().into_iter().flatten() {
                value[key] = field.clone();
            }
            made.push(value);
        };
        let person = |id: u64, name: &str| json!({"id": id.to_string(), "username": name});
        match *set {
            "roles" => {
                let Some(guild) = guild else {
                    return "content: open a server first".to_owned();
                };
                self.demo_roles(guild);
                push(
                    person(900, "ada"),
                    &format!("hello <@&{MODS}> and <@&{LEADS}> and @everyone and @here"),
                    json!({}),
                );
            }
            "code" => {
                push(
                    person(900, "ada"),
                    "a line of `inline code` first\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\nand after it",
                    json!({}),
                );
            }
            "reactions" => {
                push(
                    person(900, "ada"),
                    "a message with reactions",
                    json!({"reactions": [
                        {"emoji": {"id": null, "name": "\u{1f44d}"}, "count": 3, "me": false},
                        {"emoji": {"id": null, "name": "\u{2764}\u{fe0f}"}, "count": 1, "me": false}
                    ]}),
                );
            }
            // Unicode emoji, one with a skin tone, to right-press for the favourite row.
            "emoji" => {
                push(
                    person(900, "ada"),
                    "nice \u{1f44d} and \u{1f44d}\u{1f3fd} and \u{2764}\u{fe0f}",
                    json!({}),
                );
            }
            "links" => {
                push(
                    person(900, "ada"),
                    "trusted: [Discord docs](https://discord.com/developers/docs) and untrusted: [Example](https://example.com/page)",
                    json!({}),
                );
            }
            // Links the app keeps: a thread of this server, a settings page, a profile, an invite (not built).
            "deeplinks" => {
                let Some(guild) = guild else {
                    return "content: open a server first".to_owned();
                };
                let thread = self.stores.guild(guild).and_then(|known| {
                    known
                        .threads
                        .first()
                        .map(|thread| (thread.parent_id, thread.id))
                });
                let Some((parent, thread)) = thread else {
                    return "content: this server has no thread".to_owned();
                };
                push(
                    person(900, "ada"),
                    &format!(
                        "thread: [open the thread](https://discord.com/channels/{guild}/{parent}/threads/{thread})"
                    ),
                    json!({}),
                );
                push(
                    person(900, "ada"),
                    "settings: [voice settings](discord://-/settings/voice) and https://discord.com/settings/appearance",
                    json!({}),
                );
                push(
                    person(900, "ada"),
                    "invite: https://discord.com/invite/fixture and a web page [docs](https://discord.com/developers/docs)",
                    json!({}),
                );
            }
            "reply" => {
                let target = base + 1;
                let original = json!({
                    "id": target.to_string(), "channel_id": channel.to_string(), "type": 0,
                    "content": "the message being answered",
                    "timestamp": (now - chrono::Duration::seconds(300))
                        .to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
                    "author": person(901, "grace"),
                });
                push(
                    person(901, "grace"),
                    "the message being answered",
                    json!({}),
                );
                push(
                    person(900, "ada"),
                    "an answer",
                    json!({"type": 19, "message_reference": {"message_id": target.to_string(), "channel_id": channel.to_string()},
                           "referenced_message": original}),
                );
            }
            "header" => {
                let tagged = json!({"id": "902", "username": "tagged", "primary_guild": {
                    "identity_enabled": true, "identity_guild_id": "1092820800203141130",
                    "badge": "cfae2f34c559dda6b48e57558217662b", "tag": "R6TB"}});
                push(tagged, "a person with a server tag", json!({}));
                push(
                    json!({"id": "903", "username": "Helper", "bot": true, "public_flags": 65536}),
                    "a verified app speaking",
                    json!({}),
                );
                push(
                    json!({"id": "904", "username": "Plain Bot", "bot": true}),
                    "an app that is not verified",
                    json!({}),
                );
                if let Some(guild) = guild {
                    self.demo_role_icon(guild);
                    push(person(905, "Booster"), "a role with an icon", json!({}));
                }
            }
            // The enhanced role styles: a two-colour role, a holographic one, and the signed-in account on the
            // first so the member list shows it too.
            "gradient" => {
                let Some(guild) = guild else {
                    return "content: open a server first".to_owned();
                };
                self.demo_gradient_roles(guild);
                push(person(906, "Fiery"), "two colours", json!({}));
                push(person(907, "Magical"), "three colours", json!({}));
                push(person(908, "Plain"), "no gradient role", json!({}));
                push(
                    person(908, "Plain"),
                    "role mentions <@&900103> <@&900104> <@&900105>",
                    json!({}),
                );
            }
            "files" => {
                push(
                    person(900, "ada"),
                    "attachments",
                    json!({"attachments": [
                        {"id": "9001", "filename": "notes.txt", "size": 2048, "url": "https://cdn.discordapp.com/attachments/1/2/notes.txt", "content_type": "text/plain"},
                        {"id": "9002", "filename": "song.mp3", "size": 4_000_000, "url": "https://cdn.discordapp.com/attachments/1/2/song.mp3", "content_type": "audio/mpeg"},
                        {"id": "9003", "filename": "clip.mp4", "size": 9_000_000, "url": "https://cdn.discordapp.com/attachments/1/2/clip.mp4", "content_type": "video/mp4", "width": 640, "height": 360},
                    ]}),
                );
            }
            "embeds" => {
                push(
                    person(900, "ada"),
                    "an embed with links",
                    json!({"embeds": [{
                        "type": "rich", "title": "A linked title", "url": "https://example.com/title",
                        "description": "Some [inline link](https://example.com/inline) text.",
                        "author": {"name": "Author Name", "url": "https://example.com/author"},
                        "color": 5793266,
                        "fields": [{"name": "Field one", "value": "[a field link](https://example.com/field)", "inline": true}],
                        "footer": {"text": "Footer"}
                    }]}),
                );
            }
            other => return format!("content: no demo set {other}"),
        }
        let list = self.stores.messages.entry(channel).or_default();
        list.extend(made.iter().filter_map(dc_state::Message::from_value));
        let added = made.len();
        self.request_images();
        self.rebuild_rows();
        cx.notify();
        format!("content: added {added} demo messages to {channel}")
    }

    /// A role with a unicode icon and a member of it, in a server that has role icons.
    fn demo_role_icon(&mut self, guild: Id) {
        if let Some(one) = self.stores.guilds.iter_mut().find(|one| one.id == guild) {
            if !one.features.iter().any(|feature| feature == "ROLE_ICONS") {
                one.features.push("ROLE_ICONS".to_owned());
            }
            one.roles.insert(ICON_ROLE, "Server Booster".to_owned());
            one.role_info.insert(
                ICON_ROLE,
                dc_state::RoleInfo {
                    position: 9,
                    ..dc_state::RoleInfo::default()
                },
            );
            one.role_icons.insert(
                ICON_ROLE,
                dc_state::RoleIcon {
                    icon: None,
                    unicode_emoji: char::from_u32(0x1f525).map(String::from),
                    subscription: false,
                },
            );
        }
        self.stores.members.upsert(
            guild,
            Member {
                user: User {
                    id: 905,
                    username: "Booster".to_owned(),
                    ..User::default()
                },
                nick: None,
                avatar: None,
                avatar_decoration: None,
                roles: vec![ICON_ROLE],
                joined_at: None,
                pending: false,
                display_name_styles: None,
                timeout_until: None,
                flags: 0,
            },
        );
    }

    /// Module 178758's `fire` pair and module 989433's holographic triple (`a9c9ff`, `ffbbec`, `ffc3a0`) as roles of
    /// a server with `ENHANCED_ROLE_COLORS`.
    fn demo_gradient_roles(&mut self, guild: Id) {
        let me = self.stores.me().map(|me| me.id);
        if let Some(one) = self.stores.guilds.iter_mut().find(|one| one.id == guild) {
            for feature in ["ENHANCED_ROLE_COLORS", "ROLE_ICONS"] {
                if !one.features.iter().any(|f| f == feature) {
                    one.features.push(feature.to_owned());
                }
            }
            one.role_icons.insert(
                GRAD3,
                dc_state::RoleIcon {
                    icon: None,
                    unicode_emoji: char::from_u32(0x1f525).map(String::from),
                    subscription: false,
                },
            );
            for (role, name, color, second, third) in [
                (GRAD2, "Fiery", 0xaa3b3b, Some(0xff4848), None),
                (GRAD3, "Magical", 0xa9c9ff, Some(0xffbbec), Some(0xffc3a0)),
                (GRAD_PLAIN, "Plain", 0x1abc9c, None, None),
            ] {
                one.roles.insert(role, name.to_owned());
                one.role_info.insert(
                    role,
                    dc_state::RoleInfo {
                        position: 7,
                        color,
                        secondary_color: second,
                        tertiary_color: third,
                        ..dc_state::RoleInfo::default()
                    },
                );
            }
        }
        for (id, name, role) in [
            (906, "Fiery", GRAD2),
            (907, "Magical", GRAD3),
            (908, "Plain", GRAD_PLAIN),
        ] {
            self.stores.members.upsert(
                guild,
                Member {
                    user: User {
                        id,
                        username: name.to_owned(),
                        ..User::default()
                    },
                    nick: None,
                    avatar: None,
                    avatar_decoration: None,
                    roles: vec![role],
                    joined_at: None,
                    pending: false,
                    display_name_styles: None,
                    timeout_until: None,
                    flags: 0,
                },
            );
        }
        if let Some(me) = me
            && let Some(mut member) = self.stores.members.get(guild, me).cloned()
        {
            member.roles.push(GRAD3);
            self.stores.members.upsert(guild, member);
        }
        // A hoisted role group named after the role with the icon, as the server lists it (the group header shows
        // the icon before the name), above the online members.
        if let Some(me) = self.stores.current_user.clone() {
            let listed = |id: Id, name: &str| {
                json!({"member": {"nick": null, "user": {"id": id.to_string(), "username": name, "discriminator": "0"},
                    "presence": {"status": "online"}, "roles": [GRAD3.to_string()]}})
            };
            self.stores.dispatch(
                "GUILD_MEMBER_LIST_UPDATE",
                &json!({"guild_id": guild.to_string(),
                    "groups": [{"id": GRAD3.to_string(), "count": 2}, {"id": "online", "count": 2}],
                    "ops": [{"op": "SYNC", "range": [0, 99], "items": [
                        {"group": {"id": GRAD3.to_string()}},
                        listed(907, "Magical"),
                        listed(me.id, &me.username),
                        {"group": {"id": "online"}},
                        {"member": {"nick": null, "user": {"id": "906", "username": "Fiery", "discriminator": "0"}, "presence": {"status": "online"}, "roles": [GRAD2.to_string()]}},
                        {"member": {"nick": null, "user": {"id": "908", "username": "Plain", "discriminator": "0"}, "presence": {"status": "idle"}, "roles": [GRAD_PLAIN.to_string()]}},
                    ]}]}),
            );
        }
    }

    /// Two roles with a colour and a few members each, as the server would have answered the preloads.
    fn demo_roles(&mut self, guild: Id) {
        if let Some(one) = self.stores.guilds.iter_mut().find(|one| one.id == guild) {
            one.roles.insert(MODS, "Moderators".to_owned());
            one.roles.insert(LEADS, "Team Leads".to_owned());
            one.role_info.insert(
                MODS,
                dc_state::RoleInfo {
                    position: 5,
                    color: 0x1abc9c,
                    mentionable: true,
                    guild_connection: false,
                    managed: false,
                    ..dc_state::RoleInfo::default()
                },
            );
        }
        let names = [
            "Zoe", "ada", "grace", "linus", "mallory", "Ben", "carol", "dave", "erin", "frank",
            "gus", "hana",
        ];
        for (index, name) in names.iter().enumerate() {
            let id = 950 + index as Id;
            let roles = if index % 3 == 0 {
                vec![MODS, LEADS]
            } else {
                vec![MODS]
            };
            self.stores.members.upsert(
                guild,
                Member {
                    user: User {
                        id,
                        username: (*name).to_owned(),
                        ..User::default()
                    },
                    nick: None,
                    avatar: None,
                    avatar_decoration: None,
                    roles,
                    joined_at: None,
                    pending: false,
                    display_name_styles: None,
                    timeout_until: None,
                    flags: 0,
                },
            );
        }
        // The lookups the popout would make are answered: 31 members have the role, 12 of them are known.
        self.requested.insert(format!("rolepop-ids:{guild}:{MODS}"));
        self.requested
            .insert(format!("rolepop-ids:{guild}:{LEADS}"));
        self.requested.insert(format!("rolepop-counts:{guild}"));
        self.fetched.insert(
            format!("rolepop-counts:{guild}"),
            json!({ MODS.to_string(): 31, LEADS.to_string(): 4 }),
        );
    }
}
