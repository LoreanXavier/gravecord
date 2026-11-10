//! Permission bits (module 818348, `vI(n)` = `1 << n`) and PermissionStore's computation (module 488926, `G`, `w`,
//! `U`, `M`): the owner holds everything; otherwise the @everyone role and the member's roles add up, an
//! administrator holds everything, and the rest go through the overwrites (@everyone, the member's roles together,
//! the member) and the timeout mask. A member still pending the rules screening keeps only View Channel and Read
//! Message History. In a guild that requires 2FA for moderation, an account without it loses the elevated bits.

use crate::{Channel, Guild, Id};

pub const CREATE_INSTANT_INVITE: u64 = 1 << 0;
pub const KICK_MEMBERS: u64 = 1 << 1;
pub const BAN_MEMBERS: u64 = 1 << 2;
pub const ADMINISTRATOR: u64 = 1 << 3;
pub const MANAGE_CHANNELS: u64 = 1 << 4;
pub const MANAGE_GUILD: u64 = 1 << 5;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const MANAGE_MESSAGES: u64 = 1 << 13;
pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
pub const CHANGE_NICKNAME: u64 = 1 << 26;
pub const MANAGE_NICKNAMES: u64 = 1 << 27;
pub const MANAGE_ROLES: u64 = 1 << 28;
pub const MANAGE_GUILD_EXPRESSIONS: u64 = 1 << 30;
pub const MANAGE_THREADS: u64 = 1 << 34;
pub const MODERATE_MEMBERS: u64 = 1 << 40;
pub const CREATE_GUILD_EXPRESSIONS: u64 = 1 << 43;
pub const USE_EXTERNAL_SOUNDS: u64 = 1 << 45;
pub const BYPASS_SLOWMODE: u64 = 1 << 52;

/// Every bit: what the owner and administrators hold (`R`).
/// `xBc.CONNECT`, `xBc.MOVE_MEMBERS` (module 375389).
pub const CONNECT: u64 = 1 << 20;
pub const MOVE_MEMBERS: u64 = 1 << 24;
pub const ALL: u64 = u64::MAX;
/// `y`: what a lurker or a pending member keeps.
const LURKER: u64 = VIEW_CHANNEL | READ_MESSAGE_HISTORY;
/// `v`: what a member in timeout keeps.
const TIMED_OUT: u64 = VIEW_CHANNEL | READ_MESSAGE_HISTORY | CHANGE_NICKNAME;
/// `eh`: the bits `mfa_level` ELEVATED (1) withholds from an account without 2FA.
const ELEVATED: u64 = KICK_MEMBERS
    | BAN_MEMBERS
    | ADMINISTRATOR
    | MANAGE_CHANNELS
    | MANAGE_GUILD
    | MANAGE_ROLES
    | MANAGE_MESSAGES
    | MANAGE_THREADS
    | MODERATE_MEMBERS;

/// Who asks: the current user and whether the account has 2FA (`mfa_enabled` on READY's user).
#[derive(Clone, Copy, Debug)]
pub struct Me {
    pub id: Id,
    pub mfa_enabled: bool,
}

/// A channel's `permission_overwrites` entry: type 0 a role, 1 a member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overwrite {
    pub id: Id,
    pub kind: u64,
    pub allow: u64,
    pub deny: u64,
}

impl Overwrite {
    pub(crate) fn list(value: &serde_json::Value) -> Vec<Self> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                Some(Self {
                    id: crate::id(&entry["id"])?,
                    kind: entry["type"].as_u64().unwrap_or(0),
                    allow: bits(&entry["allow"]),
                    deny: bits(&entry["deny"]),
                })
            })
            .collect()
    }
}

/// Permissions arrive as decimal strings.
pub(crate) fn bits(value: &serde_json::Value) -> u64 {
    value
        .as_str()
        .and_then(|text| text.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .unwrap_or(0)
}

/// `M`: the 2FA requirement.
fn elevated(guild: &Guild, me: Me, permissions: u64) -> u64 {
    if guild.mfa_level == 1 && !me.mfa_enabled {
        permissions & !ELEVATED
    } else {
        permissions
    }
}

/// `w` over a set of overwrites (none for the guild itself).
fn compute(guild: &Guild, overwrites: &[Overwrite], me: Me, now_ms: i64) -> u64 {
    if guild.owner_id == Some(me.id) {
        return elevated(guild, me, ALL);
    }
    let mut total = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    for role in &guild.my_roles {
        total |= guild.role_permissions.get(role).copied().unwrap_or(0);
    }
    total = if total & ADMINISTRATOR != 0 {
        ALL
    } else {
        // `U`: @everyone, the roles together, then the member.
        if let Some(everyone) = overwrites
            .iter()
            .find(|entry| entry.kind == 0 && entry.id == guild.id)
        {
            total = (total & !everyone.deny) | everyone.allow;
        }
        let (mut allow, mut deny) = (0, 0);
        for entry in overwrites {
            if entry.kind == 0 && guild.my_roles.contains(&entry.id) {
                allow |= entry.allow;
                deny |= entry.deny;
            }
        }
        total = (total & !deny) | allow;
        if let Some(member) = overwrites
            .iter()
            .find(|entry| entry.kind == 1 && entry.id == me.id)
        {
            total = (total & !member.deny) | member.allow;
        }
        let timed_out = guild.my_timeout_until.is_some_and(|until| until > now_ms);
        if timed_out && total & ADMINISTRATOR == 0 {
            total &= TIMED_OUT;
        }
        total
    };
    if guild.my_pending {
        total &= LURKER;
    }
    elevated(guild, me, total)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The current user's permissions in the guild, before any channel.
pub fn guild_permissions(guild: &Guild, me: Me) -> u64 {
    compute(guild, &[], me, now_ms())
}

/// The current user's permissions in one channel of the guild.
pub fn channel_permissions(guild: &Guild, channel: &Channel, me: Me) -> u64 {
    compute(guild, &channel.overwrites, me, now_ms())
}

/// What another member may do in a channel: their roles' permissions with the channel's overwrites (`w` for any
/// member). Screening, timeouts and 2FA are the server's to apply, so they are left out.
pub fn member_channel_permissions(guild: &Guild, channel: &Channel, user: Id, roles: &[Id]) -> u64 {
    if guild.owner_id == Some(user) {
        return ALL;
    }
    let mut total = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    for role in roles {
        total |= guild.role_permissions.get(role).copied().unwrap_or(0);
    }
    if total & ADMINISTRATOR != 0 {
        return ALL;
    }
    let overwrites = &channel.overwrites;
    if let Some(everyone) = overwrites
        .iter()
        .find(|entry| entry.kind == 0 && entry.id == guild.id)
    {
        total = (total & !everyone.deny) | everyone.allow;
    }
    let (mut allow, mut deny) = (0, 0);
    for entry in overwrites {
        if entry.kind == 0 && roles.contains(&entry.id) {
            allow |= entry.allow;
            deny |= entry.deny;
        }
    }
    total = (total & !deny) | allow;
    if let Some(member) = overwrites
        .iter()
        .find(|entry| entry.kind == 1 && entry.id == user)
    {
        total = (total & !member.deny) | member.allow;
    }
    total
}

/// `can(permission, channel)`.
pub fn can_in_channel(guild: &Guild, channel: &Channel, me: Me, permission: u64) -> bool {
    channel_permissions(guild, channel, me) & permission == permission
}

/// `can(permission, guild)`.
pub fn can_in_guild(guild: &Guild, me: Me, permission: u64) -> bool {
    guild_permissions(guild, me) & permission == permission
}

/// 488926 `MJ`: whether @everyone holds the permission in the channel: the role's own permissions with the channel's
/// @everyone overwrite over them (deny, then allow). Administrator opens nothing here, as in the official.
pub fn everyone_can(guild: &Guild, channel: &Channel, permission: u64) -> bool {
    let mut total = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    if let Some(everyone) = channel
        .overwrites
        .iter()
        .find(|entry| entry.kind == 0 && entry.id == guild.id)
    {
        total = (total & !everyone.deny) | everyone.allow;
    }
    total & permission == permission
}

/// 148719: a private channel. A voice or stage channel @everyone cannot connect to, or any channel whose @everyone
/// overwrite denies View Channel.
pub fn is_private(guild: &Guild, channel: &Channel) -> bool {
    let vocal = matches!(
        channel.kind,
        crate::ChannelKind::Voice | crate::ChannelKind::Other(13)
    );
    (vocal && !everyone_can(guild, channel, CONNECT))
        || channel
            .overwrites
            .iter()
            .any(|entry| entry.kind == 0 && entry.id == guild.id && entry.deny & VIEW_CHANNEL != 0)
}

/// 51758 `V`: the account has not passed the membership gate of a guild that has one.
pub fn pending_gate(guild: &Guild) -> bool {
    guild.my_pending
        && guild
            .features
            .iter()
            .any(|feature| feature == "MEMBER_VERIFICATION_GATE_ENABLED")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guild() -> Guild {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "5", "username": "me"},
                "guilds": [{
                    "id": "100",
                    "properties": {"name": "g", "owner_id": "9"},
                    "roles": [
                        {"id": "100", "name": "@everyone", "permissions": "1024"},
                        {"id": "200", "name": "mods", "permissions": "1"},
                        {"id": "300", "name": "admins", "permissions": "8"}
                    ],
                    "channels": [
                        {"id": "1", "type": 0, "name": "open", "permission_overwrites": []},
                        {"id": "2", "type": 0, "name": "hidden", "permission_overwrites": [
                            {"id": "100", "type": 0, "allow": "0", "deny": "1024"},
                            {"id": "5", "type": 1, "allow": "1024", "deny": "0"}
                        ]},
                        {"id": "3", "type": 0, "name": "mods only", "permission_overwrites": [
                            {"id": "100", "type": 0, "allow": "0", "deny": "1025"}
                        ]}
                    ]
                }],
                "merged_members": [[{"user_id": "5", "roles": ["200"]}]]
            }),
        );
        stores.guilds.remove(0)
    }

    const ME: Me = Me {
        id: 5,
        mfa_enabled: false,
    };

    #[test]
    fn roles_and_overwrites_combine() {
        let guild = guild();
        assert_eq!(guild.my_roles, vec![200]);
        assert!(can_in_guild(
            &guild,
            ME,
            CREATE_INSTANT_INVITE | VIEW_CHANNEL
        ));
        assert!(!can_in_guild(&guild, ME, ADMINISTRATOR));
        let channel = |id: Id| {
            guild
                .channels
                .iter()
                .find(|channel| channel.id == id)
                .unwrap()
        };
        assert!(can_in_channel(&guild, channel(1), ME, VIEW_CHANNEL));
        // The member overwrite lets them back into a channel @everyone cannot see.
        assert!(can_in_channel(&guild, channel(2), ME, VIEW_CHANNEL));
        assert!(!can_in_channel(
            &guild,
            channel(3),
            ME,
            CREATE_INSTANT_INVITE
        ));
    }

    #[test]
    fn owners_and_administrators_hold_everything() {
        let mut guild = guild();
        assert_eq!(
            guild_permissions(
                &guild,
                Me {
                    id: 9,
                    mfa_enabled: true
                }
            ),
            ALL
        );
        guild.my_roles.push(300);
        assert_eq!(
            guild_permissions(
                &guild,
                Me {
                    id: 5,
                    mfa_enabled: true
                }
            ),
            ALL
        );
        // Without 2FA where the guild asks for it, the moderation bits go.
        guild.mfa_level = 1;
        assert!(!can_in_guild(&guild, ME, ADMINISTRATOR));
        assert!(can_in_guild(&guild, ME, CREATE_INSTANT_INVITE));
    }

    #[test]
    fn pending_and_timed_out_members_keep_little() {
        let mut guild = guild();
        guild.my_pending = true;
        assert_eq!(guild_permissions(&guild, ME), VIEW_CHANNEL);
        guild.my_pending = false;
        guild.my_timeout_until = Some(now_ms() + 60_000);
        assert!(!can_in_guild(&guild, ME, CREATE_INSTANT_INVITE));
        assert!(can_in_guild(&guild, ME, VIEW_CHANNEL));
    }
}
