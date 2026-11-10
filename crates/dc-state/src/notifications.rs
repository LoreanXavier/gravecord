//! Message notification eligibility: 411238 K/$/X, 534765 MESSAGE_CREATE and 119622 showNotification.
use crate::{
    ChannelKind, Id, Stores,
    guild_settings::{self, ALL_MESSAGES, INHERIT, NO_MESSAGES},
};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub struct DeviceSettings {
    pub desktop: bool,
    pub message_sound: bool,
    pub selected_channel_sound: bool,
    pub taskbar_flash: bool,
    pub streamer_notifications: bool,
    pub streamer_sounds: bool,
    pub quiet_mode: bool,
    pub restricted_hours: bool,
    pub required_action: bool,
    pub overlay_handles_notifications: bool,
    pub voice_channel: Option<Id>,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            desktop: true,
            message_sound: true,
            selected_channel_sound: false,
            taskbar_flash: true,
            streamer_notifications: false,
            streamer_sounds: false,
            quiet_mode: false,
            restricted_hours: false,
            required_action: false,
            overlay_handles_notifications: false,
            voice_channel: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decision {
    pub desktop: bool,
    pub sound: Option<&'static str>,
    pub flash: bool,
}

pub fn mentions_user(
    message: &Value,
    user: Id,
    roles: &[Id],
    suppress_everyone: bool,
    suppress_roles: bool,
) -> bool {
    message["mentions"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|value| crate::id(&value["id"]) == Some(user))
        || (!suppress_everyone && message["mention_everyone"] == true)
        || (!suppress_roles
            && message["mention_roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(crate::id)
                .any(|role| roles.contains(&role)))
}

/// None means this channel is muted, opted out, or has notifications set to Nothing.
fn level(
    stores: &Stores,
    guild: Id,
    channel: Id,
    parent: Option<Id>,
    default: u64,
    now: DateTime<Utc>,
) -> Option<u64> {
    let settings = stores.guild_settings.guilds.get(&guild);
    let own = settings.and_then(|settings| settings.channels.get(&channel));
    let category =
        parent.and_then(|parent| settings.and_then(|settings| settings.channels.get(&parent)));
    if settings
        .is_some_and(|settings| guild_settings::mute_active(settings.muted, settings.mute_end, now))
        || own.is_some_and(|own| guild_settings::mute_active(own.muted, own.mute_end, now))
        || category.is_some_and(|category| {
            guild_settings::mute_active(category.muted, category.mute_end, now)
        })
    {
        return None;
    }
    // OPT_IN_CHANNELS_ON and OPT_IN_ENABLED, module 465856.
    if guild != 0
        && settings.is_some_and(|settings| settings.flags & (1 << 14) != 0)
        && !own.is_some_and(|own| own.flags & (1 << 12) != 0)
        && !category.is_some_and(|category| category.flags & (1 << 12) != 0)
    {
        return None;
    }
    let resolved = own
        .map(|own| own.message_notifications)
        .filter(|level| *level != INHERIT)
        .or_else(|| {
            category
                .map(|category| category.message_notifications)
                .filter(|level| *level != INHERIT)
        })
        .or_else(|| {
            settings
                .map(|settings| settings.message_notifications)
                .filter(|level| *level != INHERIT)
        })
        .unwrap_or(default);
    (resolved != NO_MESSAGES).then_some(resolved)
}

pub fn message_decision(
    stores: &Stores,
    message: &Value,
    selected: Option<Id>,
    focused: bool,
    device: DeviceSettings,
    now: DateTime<Utc>,
) -> Decision {
    let silent = Decision::default();
    let Some(me) = stores.current_user.as_ref() else {
        return silent;
    };
    let (Some(channel), Some(author)) = (
        crate::id(&message["channel_id"]),
        crate::id(&message["author"]["id"]),
    ) else {
        return silent;
    };
    if stores.status == "dnd"
        || device.quiet_mode
        || device.restricted_hours
        || device.required_action
        || stores.relationships.get(&author) == Some(&2)
        || stores.ignored.contains(&author)
        || message["author"]["flags"].as_u64().unwrap_or(0) & 1048576 != 0
    {
        return silent;
    }
    let message_type = message["type"].as_u64().unwrap_or(0);
    let guild_channel = stores.guilds.iter().find_map(|guild| {
        guild
            .channels
            .iter()
            .find(|known| known.id == channel)
            .map(|known| (guild, known))
    });
    let private = stores
        .private_channels
        .iter()
        .find(|known| known.id == channel);
    let (notification_level, mentioned, normal_allowed, all_allowed) =
        if let Some((guild, target)) = guild_channel {
            let Some(level) = level(
                stores,
                guild.id,
                channel,
                target.parent_id,
                guild.default_message_notifications,
                now,
            ) else {
                return silent;
            };
            let settings = stores.guild_settings.guilds.get(&guild.id);
            let mentioned = mentions_user(
                message,
                me.id,
                &guild.my_roles,
                settings.is_some_and(|settings| settings.suppress_everyone),
                settings.is_some_and(|settings| settings.suppress_roles),
            );
            let voice = target.kind == ChannelKind::Voice || target.kind == ChannelKind::Other(13);
            (
                level,
                mentioned,
                true,
                !voice || device.voice_channel == Some(channel),
            )
        } else if let Some(private) = private {
            if private.kind == 3 && private.application_id.is_some() {
                return silent;
            }
            let Some(level) = level(stores, 0, channel, None, ALL_MESSAGES, now) else {
                return silent;
            };
            (
                level,
                mentions_user(message, me.id, &[], false, false),
                !private.is_request() && !(private.kind == 3 && message_type == 2),
                true,
            )
        } else {
            return silent;
        };
    let is_selected = selected == Some(channel);
    let selected_sound = device.selected_channel_sound && is_selected && author != me.id;
    let normal = normal_allowed
        && (author != me.id || message_type == 46)
        && message["flags"].as_u64().unwrap_or(0) & 4096 == 0
        && !(is_selected && focused)
        && !(message.get("activity_instance").is_some()
            && crate::id(&message["interaction"]["user"]["id"]) == Some(me.id))
        && ((notification_level == ALL_MESSAGES && all_allowed) || mentioned);
    if !selected_sound && !normal {
        return silent;
    }
    let sound_enabled = device.message_sound && !device.streamer_sounds;
    let mut decision = Decision {
        sound: (selected_sound && sound_enabled).then_some("message3"),
        ..Decision::default()
    };
    if (selected_sound && !focused) || !normal || device.overlay_handles_notifications {
        return decision;
    }
    decision.sound = sound_enabled.then_some("message1");
    decision.desktop = device.desktop && !device.streamer_notifications;
    decision.flash = decision.desktop && device.taskbar_flash;
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn setup() -> (Stores, Value) {
        let mut stores = Stores::default();
        stores.dispatch("READY", &json!({
            "user":{"id":"1","username":"me"}, "users":[{"id":"2","username":"sender"}],
            "guilds":[{"id":"10","properties":{"name":"Fixture","default_message_notifications":1},
                "channels":[{"id":"20","name":"chat","type":0,"parent_id":"30"}],"roles":[]}],
            "private_channels":[{"id":"40","type":1,"recipient_ids":["2"]}]
        }));
        stores.guilds[0].my_roles = vec![50];
        (
            stores,
            json!({"id":"100","channel_id":"20","author":{"id":"2","username":"sender"},"content":"test","mentions":[{"id":"1"}]}),
        )
    }
    fn decide(stores: &Stores, message: &Value, device: DeviceSettings) -> Decision {
        message_decision(stores, message, None, true, device, Utc::now())
    }
    #[test]
    fn desktop_and_sound_switches_are_independent() {
        let (stores, message) = setup();
        let settings = DeviceSettings::default();
        assert_eq!(
            decide(&stores, &message, settings),
            Decision {
                desktop: true,
                sound: Some("message1"),
                flash: true
            }
        );
        assert_eq!(
            decide(
                &stores,
                &message,
                DeviceSettings {
                    desktop: false,
                    ..settings
                }
            ),
            Decision {
                sound: Some("message1"),
                ..Decision::default()
            }
        );
        assert_eq!(
            decide(
                &stores,
                &message,
                DeviceSettings {
                    streamer_notifications: true,
                    ..settings
                }
            ),
            Decision {
                sound: Some("message1"),
                ..Decision::default()
            }
        );
        assert_eq!(
            decide(
                &stores,
                &message,
                DeviceSettings {
                    streamer_sounds: true,
                    ..settings
                }
            )
            .sound,
            None
        );
    }
    #[test]
    fn dnd_blocks_and_selected_channel_follow_separate_paths() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        assert_eq!(
            message_decision(&stores, &message, Some(20), true, device, Utc::now()),
            Decision::default()
        );
        let selected = DeviceSettings {
            selected_channel_sound: true,
            ..device
        };
        assert_eq!(
            message_decision(&stores, &message, Some(20), true, selected, Utc::now()).sound,
            Some("message3")
        );
        assert!(
            !message_decision(&stores, &message, Some(20), false, selected, Utc::now()).desktop
        );
        message["flags"] = json!(4096);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        assert_eq!(
            message_decision(&stores, &message, Some(20), true, selected, Utc::now()).sound,
            Some("message3")
        );
        stores.status = "dnd".to_owned();
        assert_eq!(
            message_decision(&stores, &message, Some(20), true, selected, Utc::now()),
            Decision::default()
        );
        stores.status = "online".to_owned();
        stores.relationships.insert(2, 2);
        assert_eq!(decide(&stores, &message, selected), Decision::default());
    }
    #[test]
    fn category_overrides_muting_and_role_suppression_are_resolved() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        message["mentions"] = json!([]);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["mention_roles"] = json!(["50"]);
        assert!(decide(&stores, &message, device).desktop);
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","message_notifications":1,"suppress_roles":true}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        stores.guild_settings.update(&json!({"guild_id":"10","message_notifications":1,"channel_overrides":[{"channel_id":"30","message_notifications":0}]}));
        assert!(decide(&stores, &message, device).desktop);
        stores
            .guild_settings
            .guilds
            .get_mut(&10)
            .unwrap()
            .channels
            .get_mut(&30)
            .unwrap()
            .muted = true;
        message["mentions"] = json!([{"id":"1"}]);
        assert_eq!(decide(&stores, &message, device), Decision::default());
    }
    #[test]
    fn dm_null_guild_overrides_and_opt_in_are_honored() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        message["channel_id"] = json!("40");
        assert!(decide(&stores, &message, device).desktop);
        stores.guild_settings.update(
            &json!({"guild_id":null,"channel_overrides":[{"channel_id":"40","muted":true}]}),
        );
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["channel_id"] = json!("20");
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","flags":16384}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        stores.guild_settings.update(&json!({"guild_id":"10","flags":16384,"channel_overrides":[{"channel_id":"30","flags":4096}]}));
        assert!(decide(&stores, &message, device).desktop);
    }

    #[test]
    fn voice_channels_notify_mentions_without_requiring_a_join() {
        let (mut stores, mut message) = setup();
        stores.guilds[0].channels[0].kind = ChannelKind::Voice;
        stores.guilds[0].default_message_notifications = ALL_MESSAGES;
        let device = DeviceSettings::default();
        assert!(decide(&stores, &message, device).desktop);
        message["mentions"] = json!([]);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        assert!(
            decide(
                &stores,
                &message,
                DeviceSettings {
                    voice_channel: Some(20),
                    ..device
                }
            )
            .desktop
        );
    }
}
