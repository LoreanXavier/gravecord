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
            "links" => {
                push(
                    person(900, "ada"),
                    "trusted: [Discord docs](https://discord.com/developers/docs) and untrusted: [Example](https://example.com/page)",
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
