//! Message notification eligibility: 411238 K/$/X, 534765 MESSAGE_CREATE and 119622 showNotification.
use crate::{
    ChannelKind, Guild, Id, Stores, THREAD_ALL_MESSAGES, THREAD_NO_MESSAGES, THREAD_ONLY_MENTIONS,
    Thread, USER_FLAG_SPAMMER,
    guild_settings::{self, ALL_MESSAGES, INHERIT, NO_MESSAGES, ONLY_MENTIONS},
    unread,
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

/// What only the app knows when a message arrives (the sidebar and embedded-activity inputs of 411238).
#[derive(Clone, Copy, Debug, Default)]
pub struct DecisionContext {
    /// `getCurrentSidebarChannelId`: a channel (or thread) open in the sidebar counts as selected (`_`).
    pub sidebar_channel: Option<Id>,
    /// `getCurrentEmbeddedActivity`: the running activity's application id and the channel it was launched in. A
    /// message from that application in that channel does not notify.
    pub embedded_activity: Option<(Id, Id)>,
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

fn blocked_or_ignored(stores: &Stores, user: Id) -> bool {
    stores.relationships.get(&user) == Some(&2) || stores.is_ignored(user)
}

/// 994500 `isBlockedOrIgnoredForMessage`: the author, the user whose command or component produced the message
/// (`interaction_metadata.user`), and the two users of a guild space leaderboard.
pub fn blocked_or_ignored_for_message(stores: &Stores, message: &Value) -> bool {
    let leaderboard = &message["guild_space_data"]["leaderboard"];
    [
        &message["author"]["id"],
        &message["interaction_metadata"]["user"]["id"],
        &leaderboard["user_id"],
        &leaderboard["secondary_user_id"],
    ]
    .into_iter()
    .filter_map(crate::id)
    .any(|user| blocked_or_ignored(stores, user))
}

/// 411238 `K`: `getUser(author).hasFlag(SPAMMER)`. The stored user holds `flags` and `public_flags` as 619019 does;
/// the message's own author object is the same user as the gateway just sent it, so it counts too.
fn spammer(stores: &Stores, message: &Value, author: Id) -> bool {
    stores
        .users
        .get(&author)
        .is_some_and(|user| user.has_flag(USER_FLAG_SPAMMER))
        || (message["author"]["flags"].as_u64().unwrap_or(0)
            | message["author"]["public_flags"].as_u64().unwrap_or(0))
            & USER_FLAG_SPAMMER
            != 0
}

/// 607508 `l`: the thread's notification level as a thread-member flag (`THREAD_*`). No member flags means nothing;
/// when none of the three is set the parent channel's resolved level answers.
pub fn thread_level(stores: &Stores, guild: &Guild, thread: &Thread, now: DateTime<Utc>) -> u64 {
    let Some(flags) = thread.member_flags else {
        return THREAD_NO_MESSAGES;
    };
    for flag in [
        THREAD_ALL_MESSAGES,
        THREAD_ONLY_MENTIONS,
        THREAD_NO_MESSAGES,
    ] {
        if flags & flag == flag {
            return flag;
        }
    }
    let Some(parent) = guild
        .channels
        .iter()
        .find(|channel| channel.id == thread.parent_id)
    else {
        return THREAD_NO_MESSAGES;
    };
    if unread::muted(stores, guild.id, parent.id, parent.parent_id, now) {
        return THREAD_NO_MESSAGES;
    }
    match unread::message_level(stores, guild, parent) {
        NO_MESSAGES => THREAD_NO_MESSAGES,
        ONLY_MENTIONS => THREAD_ONLY_MENTIONS,
        _ => THREAD_ALL_MESSAGES,
    }
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
    message_decision_with(
        stores,
        message,
        selected,
        focused,
        device,
        now,
        DecisionContext::default(),
    )
}

pub fn message_decision_with(
    stores: &Stores,
    message: &Value,
    selected: Option<Id>,
    focused: bool,
    device: DeviceSettings,
    now: DateTime<Utc>,
    context: DecisionContext,
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
        || blocked_or_ignored_for_message(stores, message)
        || spammer(stores, message, author)
    {
        return silent;
    }
    let message_type = message["type"].as_u64().unwrap_or(0);
    let holder = stores.guild_of_channel(channel);
    let thread = holder.and_then(|guild| {
        guild
            .threads
            .iter()
            .find(|known| known.id == channel)
            .map(|known| (guild, known))
    });
    // 411238 `$` and `X`: a thread's starter message is judged as the parent channel's message.
    let starter = message_type == 21 && thread.is_some();
    let effective = match thread {
        Some((_, thread)) if starter => thread.parent_id,
        _ => channel,
    };
    let thread = thread.filter(|_| !starter);
    let guild_channel = holder.and_then(|guild| {
        guild
            .channels
            .iter()
            .find(|known| known.id == effective)
            .map(|known| (guild, known))
    });
    if holder.is_some_and(|guild| stores.lurking_guilds.contains(&guild.id)) {
        return silent;
    }
    if let Some(application) = crate::id(&message["application_id"])
        && context.embedded_activity == Some((application, channel))
    {
        return silent;
    }
    let private = stores
        .private_channels
        .iter()
        .find(|known| known.id == channel);
    let (notification_level, mentioned, normal_allowed, all_allowed) =
        if let Some((guild, thread)) = thread {
            // 411238 `$`, thread branch: a muted thread is silent; otherwise its member level decides, and a
            // mention only counts with nothing suppressed.
            if thread.is_muted(now) {
                return silent;
            }
            let thread_level = thread_level(stores, guild, thread, now);
            if thread_level == THREAD_NO_MESSAGES {
                return silent;
            }
            let notify = thread_level == THREAD_ALL_MESSAGES
                || mentions_user(message, me.id, &guild.my_roles, false, false);
            (
                if notify { ALL_MESSAGES } else { ONLY_MENTIONS },
                false,
                true,
                true,
            )
        } else if let Some((guild, target)) = guild_channel {
            let Some(level) = level(
                stores,
                guild.id,
                effective,
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
                !voice || device.voice_channel == Some(effective),
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
    let is_selected = selected == Some(effective) || context.sidebar_channel == Some(effective);
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

    fn parse(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(time)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// The fixture plus a thread (60) under channel 20, joined with the given member fields.
    fn with_thread(member: Value) -> (Stores, Value) {
        let (mut stores, _) = setup();
        stores.dispatch(
            "THREAD_CREATE",
            &json!({"id":"60","guild_id":"10","parent_id":"20","type":11,"name":"talk","owner_id":"2",
                "last_message_id":"90","message_count":3,"member_count":2,
                "thread_metadata":{"archived":false,"locked":false,"auto_archive_duration":1440},
                "member": member}),
        );
        (
            stores,
            json!({"id":"101","channel_id":"60","type":0,"author":{"id":"2","username":"sender"},
                "content":"hello thread","mentions":[],"mention_roles":[],"mention_everyone":false}),
        )
    }

    fn member(flags: u64) -> Value {
        json!({"id":"60","user_id":"1","join_timestamp":"2026-11-09T10:00:00.000000+00:00",
            "flags":flags,"muted":false,"mute_config":null})
    }

    #[test]
    fn thread_messages_follow_the_thread_member_flags() {
        let device = DeviceSettings::default();
        let (mut stores, mut message) = with_thread(member(2));
        // ALL_MESSAGES notifies without a mention although the parent only notifies on mentions.
        assert!(decide(&stores, &message, device).desktop);
        stores.dispatch("THREAD_MEMBER_UPDATE", &json!({"id":"60","guild_id":"10","user_id":"1",
            "join_timestamp":"2026-11-09T10:00:00+00:00","flags":4,"muted":false,"mute_config":null}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["mentions"] = json!([{"id":"1","username":"me"}]);
        assert!(decide(&stores, &message, device).desktop);
        stores.dispatch("THREAD_MEMBER_UPDATE", &json!({"id":"60","guild_id":"10","user_id":"1",
            "join_timestamp":"2026-11-09T10:00:00+00:00","flags":8,"muted":false,"mute_config":null}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
    }

    #[test]
    fn thread_without_flags_falls_back_to_the_parents_level() {
        let device = DeviceSettings::default();
        // HAS_INTERACTED (1) alone sets no notification flag.
        let (mut stores, mut message) = with_thread(member(1));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["mentions"] = json!([{"id":"1","username":"me"}]);
        assert!(decide(&stores, &message, device).desktop);
        message["mentions"] = json!([]);
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","message_notifications":1,
            "channel_overrides":[{"channel_id":"20","message_notifications":0}]}));
        assert!(decide(&stores, &message, device).desktop);
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","message_notifications":1,
            "channel_overrides":[{"channel_id":"20","message_notifications":0,"muted":true}]}));
        message["mentions"] = json!([{"id":"1","username":"me"}]);
        assert_eq!(decide(&stores, &message, device), Decision::default());
    }

    #[test]
    fn threads_the_account_is_not_in_stay_silent() {
        let (mut stores, message) = with_thread(member(2));
        stores.dispatch(
            "THREAD_MEMBERS_UPDATE",
            &json!({"id":"60","guild_id":"10","member_count":1,
            "removed_member_ids":["1"]}),
        );
        assert_eq!(
            decide(&stores, &message, DeviceSettings::default()),
            Decision::default()
        );
        // Somebody else leaving or joining changes nothing for the account.
        stores.dispatch("THREAD_MEMBERS_UPDATE", &json!({"id":"60","guild_id":"10","member_count":2,
            "added_members":[{"id":"60","user_id":"5","flags":2,"join_timestamp":"2026-11-09T10:00:00+00:00"}]}));
        assert_eq!(
            decide(&stores, &message, DeviceSettings::default()),
            Decision::default()
        );
        stores.dispatch(
            "THREAD_MEMBER_UPDATE",
            &json!({"id":"60","guild_id":"10","user_id":"5","flags":2}),
        );
        assert_eq!(
            decide(&stores, &message, DeviceSettings::default()),
            Decision::default()
        );
        stores.dispatch("THREAD_MEMBERS_UPDATE", &json!({"id":"60","guild_id":"10","member_count":3,
            "added_members":[{"id":"60","user_id":"1","flags":2,"join_timestamp":"2026-11-09T10:00:00+00:00"}]}));
        assert!(decide(&stores, &message, DeviceSettings::default()).desktop);
        // A thread listed without a member (and not newly created) is not one the account is in.
        stores.dispatch("THREAD_LIST_SYNC", &json!({"guild_id":"10","threads":[
            {"id":"61","guild_id":"10","parent_id":"20","type":11,"name":"other","thread_metadata":{"archived":false}}],
            "members":[]}));
        let mut other = message.clone();
        other["channel_id"] = json!("61");
        assert_eq!(
            decide(&stores, &other, DeviceSettings::default()),
            Decision::default()
        );
        stores.dispatch("THREAD_LIST_SYNC", &json!({"guild_id":"10","threads":[
            {"id":"61","guild_id":"10","parent_id":"20","type":11,"name":"other","thread_metadata":{"archived":false}}],
            "members":[{"id":"61","user_id":"1","flags":2,"join_timestamp":"2026-11-09T10:00:00+00:00"}]}));
        assert!(decide(&stores, &other, DeviceSettings::default()).desktop);
    }

    #[test]
    fn a_muted_thread_is_silent_until_its_mute_ends() {
        let device = DeviceSettings::default();
        let end = "2026-11-09T12:00:00.000000+00:00";
        let (stores, message) = with_thread(json!({"id":"60","user_id":"1","flags":2,"muted":true,
            "mute_config":{"selected_time_window":3600,"end_time":end}}));
        let at = |now| message_decision(&stores, &message, None, true, device, now);
        assert_eq!(at(parse("2026-11-09T11:00:00Z")), Decision::default());
        // 544743 `setTimer`: a thread's mute is over once the end is not ahead any more.
        assert!(at(parse(end)).desktop);
        assert!(at(parse("2026-11-09T12:00:01Z")).desktop);
        let (stores, message) =
            with_thread(json!({"id":"60","user_id":"1","flags":2,"muted":true,"mute_config":null}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
    }

    #[test]
    fn a_thread_starter_message_is_judged_as_the_parent_channels() {
        let device = DeviceSettings::default();
        let (stores, mut message) = with_thread(member(2));
        message["type"] = json!(21);
        // The parent only notifies on mentions, whatever the thread's own level is.
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["mentions"] = json!([{"id":"1","username":"me"}]);
        assert!(decide(&stores, &message, device).desktop);
        // Viewing the parent counts as viewing the starter message.
        let selected = DeviceSettings {
            selected_channel_sound: true,
            ..device
        };
        assert_eq!(
            message_decision(&stores, &message, Some(20), true, selected, Utc::now()).sound,
            Some("message3")
        );
    }

    #[test]
    fn own_messages_do_not_notify_except_the_self_mentionable_types() {
        let (stores, mut message) = setup();
        let device = DeviceSettings::default();
        message["author"] = json!({"id":"1","username":"me"});
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["type"] = json!(46);
        assert!(decide(&stores, &message, device).desktop);
    }

    #[test]
    fn everyone_mentions_notify_until_suppressed() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        message["mentions"] = json!([]);
        message["mention_everyone"] = json!(true);
        assert!(decide(&stores, &message, device).desktop);
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","message_notifications":1,"suppress_everyone":true}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        // A thread member level of ALL_MESSAGES ignores suppression as 411238's thread branch passes false.
        let (mut stores, mut thread_message) = with_thread(member(4));
        stores
            .guild_settings
            .update(&json!({"guild_id":"10","message_notifications":1,"suppress_everyone":true}));
        thread_message["mention_everyone"] = json!(true);
        assert!(decide(&stores, &thread_message, device).desktop);
    }

    #[test]
    fn a_channel_mute_with_an_end_time_lapses_at_its_end() {
        let (mut stores, message) = setup();
        let device = DeviceSettings::default();
        let end = "2026-11-09T12:00:00.000000+00:00";
        stores.guild_settings.update(
            &json!({"guild_id":"10","message_notifications":1,"channel_overrides":[
            {"channel_id":"20","message_notifications":3,"muted":true,
             "mute_config":{"selected_time_window":900,"end_time":end}}]}),
        );
        let at = |now| message_decision(&stores, &message, None, true, device, now);
        assert_eq!(at(parse("2026-11-09T11:59:59Z")), Decision::default());
        // 544743 `tG`: the end itself still counts as muted.
        assert_eq!(at(parse(end)), Decision::default());
        assert!(at(parse("2026-11-09T12:00:00.001Z")).desktop);
    }

    #[test]
    fn group_dms_notify_by_their_own_settings_and_skip_recipient_removals() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        stores.add_private_channel(
            &json!({"id":"41","type":3,"name":"trio","recipient_ids":["2","3"],
            "last_message_id":"5"}),
        );
        message["channel_id"] = json!("41");
        message["mentions"] = json!([]);
        assert!(decide(&stores, &message, device).desktop);
        // RECIPIENT_REMOVE (2) in a group DM is a system line, not a notification.
        message["type"] = json!(2);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["type"] = json!(0);
        stores
            .guild_settings
            .update(&json!({"guild_id":null,"channel_overrides":[
            {"channel_id":"41","message_notifications":1}]}));
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["mentions"] = json!([{"id":"1","username":"me"}]);
        assert!(decide(&stores, &message, device).desktop);
    }

    #[test]
    fn a_blocked_or_ignored_interaction_user_silences_the_bots_reply() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        message["author"] = json!({"id":"7","username":"helper","bot":true});
        message["type"] = json!(20);
        message["interaction_metadata"] =
            json!({"id":"9","type":2,"user":{"id":"2","username":"sender"}});
        assert!(decide(&stores, &message, device).desktop);
        stores.relationships.insert(2, 2);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        assert!(blocked_or_ignored_for_message(&stores, &message));
        stores.relationships.remove(&2);
        stores.set_ignored(2, true);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        stores.set_ignored(2, false);
        message["guild_space_data"] =
            json!({"leaderboard":{"user_id":"8","secondary_user_id":"2"}});
        stores.set_ignored(2, true);
        assert!(blocked_or_ignored_for_message(&stores, &message));
    }

    #[test]
    fn spammers_are_read_from_the_stored_user_and_the_gateway_author() {
        let (mut stores, mut message) = setup();
        let device = DeviceSettings::default();
        assert!(decide(&stores, &message, device).desktop);
        message["author"]["public_flags"] = json!(1 << 20);
        assert_eq!(decide(&stores, &message, device), Decision::default());
        message["author"]["public_flags"] = json!(0);
        stores.dispatch(
            "GUILD_MEMBERS_CHUNK",
            &json!({"guild_id":"10","members":[
            {"user":{"id":"2","username":"sender","flags":1048576,"public_flags":0},"roles":[]}]}),
        );
        assert!(stores.users[&2].has_flag(crate::USER_FLAG_SPAMMER));
        assert_eq!(decide(&stores, &message, device), Decision::default());
    }

    #[test]
    fn lurked_guilds_the_sidebar_and_a_running_activity_follow_411238() {
        let (mut stores, message) = setup();
        let device = DeviceSettings::default();
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id":"11","joined_at":null,"properties":{"name":"lurk"},
            "channels":[{"id":"21","name":"x","type":0}],"roles":[]}),
        );
        assert!(stores.lurking_guilds.contains(&11));
        let mut lurked = message.clone();
        lurked["channel_id"] = json!("21");
        assert_eq!(decide(&stores, &lurked, device), Decision::default());
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id":"11","joined_at":"2026-11-01T00:00:00+00:00",
            "properties":{"name":"lurk"},"channels":[{"id":"21","name":"x","type":0}],"roles":[]}),
        );
        assert!(!stores.lurking_guilds.contains(&11));
        let selected = DeviceSettings {
            selected_channel_sound: true,
            ..device
        };
        let sidebar = DecisionContext {
            sidebar_channel: Some(20),
            ..DecisionContext::default()
        };
        let at = |context| {
            message_decision_with(&stores, &message, None, true, selected, Utc::now(), context)
        };
        assert!(at(DecisionContext::default()).desktop);
        // Open in the sidebar and focused: only the selected-channel sound plays.
        assert!(!at(sidebar).desktop);
        assert_eq!(at(sidebar).sound, Some("message3"));
        let mut activity = message.clone();
        activity["application_id"] = json!("777");
        let running = |channel| DecisionContext {
            embedded_activity: Some((777, channel)),
            ..DecisionContext::default()
        };
        let in_activity = |context| {
            message_decision_with(&stores, &activity, None, true, device, Utc::now(), context)
        };
        assert_eq!(in_activity(running(20)), Decision::default());
        assert!(in_activity(running(99)).desktop);
    }

    #[test]
    fn notification_flags_follow_the_settings_update() {
        let (mut stores, _) = setup();
        assert_eq!(stores.notification_flags, 0);
        stores.dispatch(
            "NOTIFICATION_SETTINGS_UPDATE",
            &json!({"flags":48,"declarative_settings_proto":null}),
        );
        assert_eq!(stores.notification_flags, 48);
        stores.dispatch(
            "NOTIFICATION_SETTINGS_UPDATE",
            &json!({"declarative_settings_proto":null}),
        );
        assert_eq!(stores.notification_flags, 48);
    }
}
