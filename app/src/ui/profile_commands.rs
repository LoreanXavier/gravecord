//! The Commands section of a bot's profiles (module 612770): the DM profile (`aG` of module 289287 in both sidebar
//! layouts), the bot popout (module 476897) and the About tab of the bot profile modal (`ep` of module 148540). It
//! lists the application's popular commands (`application.popular_application_command_ids` of the profile answer) as
//! 842209 `ZV` finds them: in the application's section of the user-installed index (`/users/@me/
//! application-command-index`) or else of the channel's index, through the permission filter `zl` of module 46477,
//! then without NSFW commands and commands with sub-commands (612770's own filter). Each is a `sm` secondary "/name"
//! button with its description as a tooltip (990474 `Oh`) in `list_QBk4TQ` (wrapping, 6px apart) under the
//! `body-xs/semibold` heading "Commands" (900179). A press is a command mention's press (990474 `C`): "Clear
//! message?" over a draft, then the command's chip in the composer.

use super::Root;
use super::dm_profile::Palette;
use super::tooltip::{Body, Side, Tip};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use serde_json::Value;

/// `ApplicationCommandOptionType` `SUB_COMMAND` and `SUB_COMMAND_GROUP`.
const SUB_COMMAND: u64 = 1;
const SUB_COMMAND_GROUP: u64 = 2;
/// `ApplicationCommandType.CHAT`, the only type 842209 asks for (`p = [CHAT]`).
const CHAT: u64 = 1;
/// `InteractionContextType`: GUILD, BOT_DM, PRIVATE_CHANNEL (module 155718 `OL`).
const CONTEXT_GUILD: u64 = 0;
const CONTEXT_BOT_DM: u64 = 1;
const CONTEXT_PRIVATE_CHANNEL: u64 = 2;
/// `ApplicationIntegrationType` GUILD_INSTALL and USER_INSTALL (module 485845).
const GUILD_INSTALL: u64 = 0;
const USER_INSTALL: u64 = 1;
/// `ApplicationCommandPermissionType` (module 392054 `RA`).
const PERMISSION_ROLE: u64 = 1;
const PERMISSION_USER: u64 = 2;
const PERMISSION_CHANNEL: u64 = 3;
/// The key of the user-installed command index.
pub(super) const USER_INDEX_KEY: &str = "command-index:user";

/// What `zl` of module 46477 reads about where the command would run (`Bh`/`MW` of module 240591).
#[derive(Clone, Debug, Default)]
pub(super) struct CommandPlace {
    /// The guild of the channel; `None` for a private channel.
    pub guild: Option<Id>,
    /// The channel (a thread's parent).
    pub channel: Option<Id>,
    /// The private channel is a DM whose recipient is this bot.
    pub dm_recipient: Option<Id>,
    /// A private channel: no permissions apply (`m` of 240591 gives 0 with both base checks true).
    pub private: bool,
    pub permissions: u64,
    /// VIEW_CHANNEL and USE_APPLICATION_COMMANDS (ADMINISTRATOR passes both).
    pub base_access: bool,
    /// SEND_MESSAGES, or SEND_MESSAGES_IN_THREADS in a thread.
    pub send_messages: bool,
    pub me: Id,
    pub roles: Vec<Id>,
    pub nsfw_allowed: bool,
}

/// The id of a value that may be a string or a number.
fn id_of(value: &Value) -> Option<Id> {
    value
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| value.as_u64())
}

/// A permission list's entry for `id` of `kind` (the keyed map of module 200662 `UZ`, read from the raw list).
fn entry(permissions: &Value, id: Id, kind: u64) -> Option<bool> {
    permissions.as_array()?.iter().find_map(|item| {
        (id_of(&item["id"]) == Some(id) && item["type"].as_u64() == Some(kind))
            .then(|| item["permission"].as_bool())
            .flatten()
    })
}

/// `_W` of module 46477: the channel's entry, else the "all channels" entry (the guild id minus one).
fn allowed_for_channel(permissions: &Value, channel: Id, guild: Id) -> Option<bool> {
    entry(permissions, channel, PERMISSION_CHANNEL)
        .or_else(|| entry(permissions, guild.wrapping_sub(1), PERMISSION_CHANNEL))
}

/// `we` of module 46477: the member's own entry, then any role's (one allowing wins, else one denying), then the
/// everyone role's (the guild id).
fn allowed_for_user(permissions: &Value, guild: Id, me: Id, roles: &[Id]) -> Option<bool> {
    if let Some(found) = entry(permissions, me, PERMISSION_USER) {
        return Some(found);
    }
    let mut denied = false;
    for role in roles {
        match entry(permissions, *role, PERMISSION_ROLE) {
            Some(true) => return Some(true),
            Some(false) => denied = true,
            None => {}
        }
    }
    if denied {
        return Some(false);
    }
    entry(permissions, guild, PERMISSION_ROLE)
}

fn includes(list: &Value, value: u64) -> bool {
    list.as_array()
        .is_some_and(|items| items.iter().any(|item| item.as_u64() == Some(value)))
}

/// `zl` of module 46477 for a command of the profile's application: `ALLOWED` (true) or one of its refusals.
/// `application` is the command's section (its `permissions` and `bot_id`); the installs say which index has it.
pub(super) fn command_allowed(
    command: &Value,
    application: &Value,
    place: &CommandPlace,
    guild_installed: bool,
    user_installed: bool,
) -> bool {
    if command["type"].as_u64().unwrap_or(CHAT) != CHAT {
        return false;
    }
    if command["nsfw"] == true && !place.nsfw_allowed {
        return false;
    }
    let context = if place.private {
        if place.dm_recipient.is_some() && place.dm_recipient == id_of(&application["bot_id"]) {
            CONTEXT_BOT_DM
        } else {
            CONTEXT_PRIVATE_CHANNEL
        }
    } else {
        CONTEXT_GUILD
    };
    if command["contexts"].is_array() {
        if !includes(&command["contexts"], context) {
            return false;
        }
    } else if (command["dm_permission"] == false && context == CONTEXT_BOT_DM)
        || context == CONTEXT_PRIVATE_CHANNEL
    {
        // `inputType === BOT`: every command of an application's section.
        return false;
    }
    let Some(guild) = place.guild.filter(|_| !place.private) else {
        return true;
    };
    let types = &command["integration_types"];
    if place.permissions & dc_state::permissions::ADMINISTRATOR != 0
        || (user_installed && includes(types, USER_INSTALL))
    {
        return true;
    }
    if (!place.base_access || !place.send_messages)
        && guild_installed
        && (!types.is_array() || includes(types, GUILD_INSTALL))
    {
        return false;
    }
    let app_permissions = &application["permissions"];
    if let Some(channel) = place.channel {
        let for_channel = allowed_for_channel(app_permissions, channel, guild);
        match allowed_for_channel(&command["permissions"], channel, guild) {
            Some(false) => return false,
            Some(true) => {}
            None if for_channel == Some(false) => return false,
            None => {}
        }
    }
    match allowed_for_user(&command["permissions"], guild, place.me, &place.roles) {
        Some(true) => return true,
        Some(false) => return false,
        None => {}
    }
    if allowed_for_user(app_permissions, guild, place.me, &place.roles) == Some(false) {
        return false;
    }
    if let Some(needed) = command["default_member_permissions"]
        .as_str()
        .and_then(|text| text.parse::<u64>().ok())
        .or_else(|| command["default_member_permissions"].as_u64())
        && (needed == 0 || place.permissions & needed != needed)
    {
        return false;
    }
    true
}

/// The application's section of an index: its entry of `applications` and its commands.
fn section<'a>(index: &'a Value, application: &str) -> Option<(&'a Value, Vec<&'a Value>)> {
    let entry = index["applications"]
        .as_array()?
        .iter()
        .find(|known| known["id"].as_str() == Some(application))?;
    let commands = index["application_commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|command| command["application_id"].as_str() == Some(application))
        .collect();
    Some((entry, commands))
}

/// The popular commands of the profile's application, in the profile's order, as 842209 `ZV` and 612770 keep them:
/// from the user-installed index's section of the application, else the channel's, allowed by `zl`, without NSFW
/// commands and ones that have sub-commands. Each is (id, name, description).
pub(super) fn popular_commands(
    profile: &Value,
    user_index: Option<&Value>,
    channel_index: Option<&Value>,
    place: &CommandPlace,
) -> Vec<(String, String, String)> {
    let Some(ids) = profile["application"]["popular_application_command_ids"].as_array() else {
        return Vec::new();
    };
    let Some(application) = profile["application"]["id"].as_str() else {
        return Vec::new();
    };
    let user = user_index.and_then(|index| section(index, application));
    let channel = channel_index.and_then(|index| section(index, application));
    let (user_installed, guild_installed) = (user.is_some(), channel.is_some());
    let Some((entry, commands)) = user.or(channel) else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(Value::as_str)
        .filter_map(|id| {
            commands
                .iter()
                .copied()
                .find(|command| command["id"].as_str() == Some(id))
        })
        .filter(|command| command_allowed(command, entry, place, guild_installed, user_installed))
        .filter(|command| command["nsfw"] != true)
        .filter(|command| {
            !command["options"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|option| {
                    matches!(
                        option["type"].as_u64(),
                        Some(SUB_COMMAND | SUB_COMMAND_GROUP)
                    )
                })
        })
        .map(|command| {
            let text = |key: &str| command[key].as_str().unwrap_or_default().to_owned();
            (text("id"), text("name"), text("description"))
        })
        .collect()
}

impl Root {
    /// The profile asks for the channel's index and the user-installed one when its application has popular commands.
    pub(super) fn request_profile_commands(&mut self, user: Id) {
        let has_commands = self
            .fetched
            .get(&format!("profile:{user}"))
            .is_some_and(|body| body["application"]["popular_application_command_ids"].is_array());
        if has_commands {
            self.request_command_index();
            if self.requested.insert(USER_INDEX_KEY.to_owned()) {
                let _ = self.requests.send(crate::net::Request::Fetch {
                    key: USER_INDEX_KEY.to_owned(),
                    route: "/users/@me/application-command-index".to_owned(),
                });
            }
        }
    }

    /// Where a command of the selected channel would run, as `MW` of module 240591 reads it.
    fn command_place(&self) -> CommandPlace {
        let me = self
            .stores
            .current_user
            .as_ref()
            .map_or(0, |known| known.id);
        let nsfw_allowed = self.stores.account.nsfw_allowed;
        let Some(guild) = self.current_guild() else {
            let dm_recipient = self
                .current_dm()
                .filter(|dm| dm.kind == 1 && dm.recipients.len() == 1)
                .map(|dm| dm.recipients[0]);
            return CommandPlace {
                private: true,
                dm_recipient,
                base_access: true,
                send_messages: true,
                me,
                nsfw_allowed,
                ..CommandPlace::default()
            };
        };
        use dc_state::permissions as p;
        let thread = self
            .channel
            .and_then(|id| guild.threads.iter().find(|thread| thread.id == id));
        let channel = thread
            .map(|thread| thread.parent_id)
            .or(self.channel)
            .and_then(|id| guild.channels.iter().find(|known| known.id == id));
        let permissions = match (self.stores.me(), channel) {
            (Some(me), Some(channel)) => p::channel_permissions(guild, channel, me),
            (Some(me), None) => p::guild_permissions(guild, me),
            _ => 0,
        };
        let admin = permissions & p::ADMINISTRATOR != 0;
        let has = |bit: u64| admin || permissions & bit == bit;
        CommandPlace {
            guild: Some(guild.id),
            channel: channel.map(|known| known.id),
            dm_recipient: None,
            private: false,
            permissions,
            base_access: has(p::VIEW_CHANNEL) && has(super::slash::USE_APPLICATION_COMMANDS),
            send_messages: if thread.is_some() {
                has(p::SEND_MESSAGES_IN_THREADS)
            } else {
                has(p::SEND_MESSAGES)
            },
            me,
            roles: self
                .stores
                .members
                .get(guild.id, me)
                .map(|member| member.roles.clone())
                .unwrap_or_default(),
            nsfw_allowed,
        }
    }

    pub(super) fn profile_commands_section(
        &self,
        user: Id,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let profile = self.fetched.get(&format!("profile:{user}"))?;
        // 612770 needs the channel in view (`null != h`).
        self.channel?;
        let commands = popular_commands(
            profile,
            self.fetched.get(USER_INDEX_KEY),
            self.command_index_value(),
            &self.command_place(),
        );
        if commands.is_empty() {
            return None;
        }
        let buttons: Vec<AnyElement> = commands
            .into_iter()
            .map(|(id, name, description)| {
                let Ok(command) = id.parse::<u64>() else {
                    return div().into_any_element();
                };
                let mention = name.clone();
                let target = self
                    .profile_small_button(
                        SharedString::from(format!("profile-command-{id}")),
                        format!("/{name}"),
                        false,
                        palette,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        // `onClick` of 612770: the popout or modal it sits in closes.
                        this.profile = None;
                        this.full_profile = None;
                        this.press_command_mention(mention.clone(), command, window, cx);
                    }));
                let tip = Tip::body(Body::Text(description.into()), Side::Top);
                self.with_tip(format!("profile-command-tip-{id}"), tip, target, cx)
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .h(px(16.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(palette.token("--text-default"))
                        .child(dc_intl::t("0hKkS+")),
                )
                .child(div().flex().flex_wrap().gap(px(6.)).children(buttons))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dm() -> CommandPlace {
        CommandPlace {
            private: true,
            base_access: true,
            send_messages: true,
            ..CommandPlace::default()
        }
    }

    fn guild_place(permissions: u64) -> CommandPlace {
        CommandPlace {
            guild: Some(100),
            channel: Some(500),
            permissions,
            base_access: true,
            send_messages: true,
            me: 7,
            roles: vec![201],
            ..CommandPlace::default()
        }
    }

    #[test]
    fn popular_commands_keep_the_profile_order_and_skip_nsfw_and_grouped_ones() {
        let profile = json!({"application": {"id": "9", "popular_application_command_ids": ["3", "1", "2", "4", "8"]}});
        let index = json!({"applications": [{"id": "9", "bot_id": "9"}], "application_commands": [
            {"id": "1", "application_id": "9", "name": "play", "description": "Plays", "contexts": [0, 1, 2]},
            {"id": "2", "application_id": "9", "name": "admin", "description": "", "contexts": [0, 1, 2], "options": [{"type": 1, "name": "ban"}]},
            {"id": "3", "application_id": "9", "name": "roll", "description": "Rolls", "contexts": [0, 1, 2], "options": [{"type": 3, "name": "sides"}]},
            {"id": "4", "application_id": "9", "name": "spicy", "description": "", "contexts": [0, 1, 2], "nsfw": true}
        ]});
        let found = popular_commands(&profile, None, Some(&index), &dm());
        let names: Vec<&str> = found.iter().map(|(_, name, _)| name.as_str()).collect();
        assert_eq!(names, ["roll", "play"]);
        assert!(popular_commands(&json!({}), None, Some(&index), &dm()).is_empty());
    }

    #[test]
    fn the_user_installed_index_wins_over_the_channels() {
        let profile =
            json!({"application": {"id": "9", "popular_application_command_ids": ["1", "2"]}});
        let user = json!({"applications": [{"id": "9"}], "application_commands": [
            {"id": "2", "application_id": "9", "name": "mine", "contexts": [0, 1, 2], "integration_types": [1]}
        ]});
        let channel = json!({"applications": [{"id": "9"}], "application_commands": [
            {"id": "1", "application_id": "9", "name": "theirs", "contexts": [0, 1, 2]}
        ]});
        let place = guild_place(0);
        let found = popular_commands(&profile, Some(&user), Some(&channel), &place);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, "mine");
        let found = popular_commands(&profile, None, Some(&channel), &dm());
        assert_eq!(found[0].1, "theirs");
    }

    #[test]
    fn contexts_and_dm_permission_decide_private_channels() {
        let bot = json!({"bot_id": "9"});
        let mut place = dm();
        place.dm_recipient = Some(9);
        // Without `contexts`, a bot's command never runs in a group DM and runs in its DM unless `dm_permission` is
        // false.
        assert!(command_allowed(&json!({}), &bot, &place, true, false));
        assert!(!command_allowed(
            &json!({"dm_permission": false}),
            &bot,
            &place,
            true,
            false
        ));
        place.dm_recipient = Some(8);
        assert!(!command_allowed(&json!({}), &bot, &place, true, false));
        assert!(command_allowed(
            &json!({"contexts": [2]}),
            &bot,
            &place,
            true,
            false
        ));
        assert!(!command_allowed(
            &json!({"contexts": [0]}),
            &bot,
            &place,
            true,
            false
        ));
        assert!(!command_allowed(
            &json!({"contexts": [0, 1, 2], "type": 2}),
            &bot,
            &place,
            true,
            false
        ));
        assert!(!command_allowed(
            &json!({"contexts": [0, 1, 2], "nsfw": true}),
            &bot,
            &place,
            true,
            false
        ));
    }

    #[test]
    fn guild_permissions_follow_the_overrides_and_default_member_permissions() {
        let app = json!({});
        let base = json!({"contexts": [0]});
        assert!(command_allowed(&base, &app, &guild_place(0), true, false));
        // Missing base access refuses a guild-installed command, unless the member is an administrator.
        let mut place = guild_place(0);
        place.send_messages = false;
        assert!(!command_allowed(&base, &app, &place, true, false));
        place.permissions = dc_state::permissions::ADMINISTRATOR;
        assert!(command_allowed(&base, &app, &place, true, false));
        // A user-installed command for user installs passes before any guild check.
        let mut place = guild_place(0);
        place.send_messages = false;
        assert!(command_allowed(
            &json!({"contexts": [0], "integration_types": [1]}),
            &app,
            &place,
            false,
            true
        ));
        // Channel entries: the command's, else the application's; the "all channels" entry is the guild id - 1.
        let denied_here = json!({"contexts": [0], "permissions": [{"id": "500", "type": 3, "permission": false}]});
        assert!(!command_allowed(
            &denied_here,
            &app,
            &guild_place(0),
            true,
            false
        ));
        let app_denies_all = json!({"permissions": [{"id": "99", "type": 3, "permission": false}]});
        assert!(!command_allowed(
            &base,
            &app_denies_all,
            &guild_place(0),
            true,
            false
        ));
        // Member entries: the user's own, then a role allowing, then the everyone role.
        let role_allows = json!({"contexts": [0], "default_member_permissions": "8", "permissions": [{"id": "201", "type": 1, "permission": true}]});
        assert!(command_allowed(
            &role_allows,
            &app,
            &guild_place(0),
            true,
            false
        ));
        let user_denied = json!({"contexts": [0], "permissions": [{"id": "7", "type": 2, "permission": false}, {"id": "201", "type": 1, "permission": true}]});
        assert!(!command_allowed(
            &user_denied,
            &app,
            &guild_place(0),
            true,
            false
        ));
        let everyone_denied = json!({"contexts": [0], "permissions": [{"id": "100", "type": 1, "permission": false}]});
        assert!(!command_allowed(
            &everyone_denied,
            &app,
            &guild_place(0),
            true,
            false
        ));
        // `default_member_permissions`: "0" is for administrators only, otherwise every bit is needed.
        let zero = json!({"contexts": [0], "default_member_permissions": "0"});
        assert!(!command_allowed(&zero, &app, &guild_place(0), true, false));
        let kick = json!({"contexts": [0], "default_member_permissions": "2"});
        assert!(!command_allowed(&kick, &app, &guild_place(0), true, false));
        assert!(command_allowed(&kick, &app, &guild_place(2), true, false));
    }
}
