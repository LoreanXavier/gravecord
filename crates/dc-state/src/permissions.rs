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
pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
/// `SEND_MESSAGES_IN_THREADS` (module 818348).
pub const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
/// `EMBED_LINKS` (module 818348, bit 14).
pub const EMBED_LINKS: u64 = 1 << 14;
/// `ATTACH_FILES` (module constants `xBc`, 32768).
pub const ATTACH_FILES: u64 = 1 << 15;
pub const MANAGE_MESSAGES: u64 = 1 << 13;
/// `USE_EXTERNAL_STICKERS` (module 818348, bit 37).
pub const USE_EXTERNAL_STICKERS: u64 = 1 << 37;
/// `ADD_REACTIONS` (module constants `xBc`, 64).
pub const ADD_REACTIONS: u64 = 1 << 6;
pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
pub const CHANGE_NICKNAME: u64 = 1 << 26;
pub const MANAGE_NICKNAMES: u64 = 1 << 27;
/// `SEND_VOICE_MESSAGES` (module 375389, bit 46).
pub const SEND_VOICE_MESSAGES: u64 = 1 << 46;
pub const MANAGE_ROLES: u64 = 1 << 28;
pub const MANAGE_GUILD_EXPRESSIONS: u64 = 1 << 30;
pub const MANAGE_THREADS: u64 = 1 << 34;
pub const MODERATE_MEMBERS: u64 = 1 << 40;
pub const CREATE_GUILD_EXPRESSIONS: u64 = 1 << 43;
pub const USE_EXTERNAL_SOUNDS: u64 = 1 << 45;
pub const BYPASS_SLOWMODE: u64 = 1 << 52;
/// `SET_VOICE_CHANNEL_STATUS` (modules 818348 and 375389, bit 48).
pub const SET_VOICE_CHANNEL_STATUS: u64 = 1 << 48;
/// `REQUEST_TO_SPEAK` (module 818348, bit 32).
pub const REQUEST_TO_SPEAK: u64 = 1 << 32;

/// Every bit: what the owner and administrators hold (`R`).
/// `xBc.CONNECT`, `xBc.MOVE_MEMBERS` (module 375389).
pub const CONNECT: u64 = 1 << 20;
/// `STREAM` (module 818348, bit 9).
pub const STREAM: u64 = 1 << 9;
pub const MOVE_MEMBERS: u64 = 1 << 24;
/// `MUTE_MEMBERS` and `DEAFEN_MEMBERS` (module 375389, bits 22 and 23).
pub const MUTE_MEMBERS: u64 = 1 << 22;
pub const DEAFEN_MEMBERS: u64 = 1 << 23;
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

/// `w` over a set of overwrites (none for the guild itself). While View Server As Role is on (`isViewingRoles`),
/// module 488926 `G` skips the owner shortcut and the member store (696451 `getMember`) hands back the account with
/// only the viewed roles; its pending flag, timeout and member overwrite stay the real ones.
fn compute(guild: &Guild, overwrites: &[Overwrite], me: Me, now_ms: i64) -> u64 {
    match guild.viewing_roles.as_deref() {
        Some(roles) => compute_with(guild, overwrites, me, now_ms, roles, false),
        None => compute_with(guild, overwrites, me, now_ms, &guild.my_roles, true),
    }
}

fn compute_with(
    guild: &Guild,
    overwrites: &[Overwrite],
    me: Me,
    now_ms: i64,
    roles: &[Id],
    owner_holds_all: bool,
) -> u64 {
    if owner_holds_all && guild.owner_id == Some(me.id) {
        return elevated(guild, me, ALL);
    }
    let mut total = guild.role_permissions.get(&guild.id).copied().unwrap_or(0);
    for role in roles {
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
            if entry.kind == 0 && roles.contains(&entry.id) {
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

/// Module 279250 `vz`: guild streaming requires Stream, a non-AFK channel and Stage video capacity.
/// The channel chooser also requires basic Connect/View access; an existing call checks Stream only.
pub fn can_stream_in_channel(
    guild: &Guild,
    channel: &Channel,
    me: Me,
    afk_channel: Option<Id>,
    require_connect: bool,
) -> bool {
    matches!(
        channel.kind,
        crate::ChannelKind::Voice | crate::ChannelKind::Other(13)
    ) && (channel.kind != crate::ChannelKind::Other(13) || guild.max_stage_video_channel_users > 0)
        && afk_channel != Some(channel.id)
        && can_in_channel(guild, channel, me, STREAM)
        && (!require_connect || can_in_channel(guild, channel, me, CONNECT | VIEW_CHANNEL))
}

/// `can(permission, guild)`.
pub fn can_in_guild(guild: &Guild, me: Me, permission: u64) -> bool {
    guild_permissions(guild, me) & permission == permission
}

/// The account's real permissions in the guild, ignoring a View Server As Role preview (25742 `V`: `aH` with
/// `forceRoles` set to the true member's roles, plus the owner).
pub fn true_guild_permissions(guild: &Guild, me: Me) -> u64 {
    compute_with(guild, &[], me, now_ms(), &guild.my_roles, true)
}

/// The roles permissions are computed with: the viewed ones during View Server As Role, else the account's.
pub fn effective_roles(guild: &Guild) -> &[Id] {
    guild.viewing_roles.as_deref().unwrap_or(&guild.my_roles)
}

/// 488926 `HJ`: the highest of `roles` (position, ties to the lower id; @everyone last).
pub fn highest_role(guild: &Guild, roles: &[Id]) -> Option<Id> {
    roles.iter().copied().max_by(|a, b| {
        let position = |id: Id| {
            if id == guild.id {
                i64::MIN
            } else {
                guild.role_info.get(&id).map_or(0, |info| info.position)
            }
        };
        position(*a).cmp(&position(*b)).then(b.cmp(a))
    })
}

/// PermissionStore `canImpersonateRole` (module 576705): Manage Server and Manage Roles, and the role ranks below
/// the account's highest (or is it). While viewing, `isRoleHigher` ranks without the owner (`r ? void 0 : i.id`).
pub fn can_impersonate_role(guild: &Guild, me: Me, role: Id) -> bool {
    if !can_in_guild(guild, me, MANAGE_GUILD | MANAGE_ROLES) {
        return false;
    }
    let roles = effective_roles(guild);
    let ranker = if guild.viewing_roles.is_some() {
        0
    } else {
        me.id
    };
    let highest = highest_role(guild, roles);
    role_is_higher(guild, ranker, roles, role) || highest == Some(role)
}

impl crate::Stores {
    /// `IMPERSONATE_UPDATE` (164956): the guild is viewed as @everyone plus `roles` (@everyone itself is dropped).
    pub fn impersonate(&mut self, guild: Id, roles: Vec<Id>) {
        if let Some(known) = self.guilds.iter_mut().find(|known| known.id == guild) {
            known.viewing_roles = Some(roles.into_iter().filter(|role| *role != guild).collect());
        }
    }

    /// `IMPERSONATE_STOP`: false when the guild was not being viewed.
    pub fn stop_impersonating(&mut self, guild: Id) -> bool {
        self.guilds
            .iter_mut()
            .find(|known| known.id == guild)
            .and_then(|known| known.viewing_roles.take())
            .is_some()
    }
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

    #[test]
    fn game_stream_destinations_require_access_and_exclude_afk_and_unavailable_stages() {
        let mut guild = guild();
        let me = Me {
            id: 5,
            mfa_enabled: false,
        };
        guild
            .role_permissions
            .insert(guild.id, VIEW_CHANNEL | CONNECT | STREAM);
        let mut channel = guild.channels[0].clone();
        channel.kind = crate::ChannelKind::Voice;
        assert!(can_stream_in_channel(&guild, &channel, me, None, true));
        assert!(!can_stream_in_channel(
            &guild,
            &channel,
            me,
            Some(channel.id),
            true
        ));
        guild.my_pending = true;
        assert!(!can_stream_in_channel(&guild, &channel, me, None, true));
        guild.my_pending = false;
        guild.role_permissions.insert(guild.id, STREAM);
        assert!(!can_stream_in_channel(&guild, &channel, me, None, true));
        assert!(can_stream_in_channel(&guild, &channel, me, None, false));
        guild
            .role_permissions
            .insert(guild.id, VIEW_CHANNEL | CONNECT);
        assert!(!can_stream_in_channel(&guild, &channel, me, None, true));
        guild
            .role_permissions
            .insert(guild.id, VIEW_CHANNEL | CONNECT | STREAM);
        channel.kind = crate::ChannelKind::Other(13);
        assert!(!can_stream_in_channel(&guild, &channel, me, None, true));
        guild.max_stage_video_channel_users = 50;
        assert!(can_stream_in_channel(&guild, &channel, me, None, true));
        channel.kind = crate::ChannelKind::Text;
        assert!(!can_stream_in_channel(&guild, &channel, me, None, true));
    }

    #[test]
    fn stage_stream_capacity_loads_and_follows_partial_guild_updates() {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "READY",
            &json!({"guilds": [{"id": "100", "channels": [], "properties": {
                "name": "fixture", "max_stage_video_channel_users": 50
            }}]}),
        );
        assert_eq!(stores.guild(100).unwrap().max_stage_video_channel_users, 50);
        stores.dispatch("GUILD_UPDATE", &json!({"id": "100", "name": "renamed"}));
        assert_eq!(stores.guild(100).unwrap().max_stage_video_channel_users, 50);
        stores.dispatch(
            "GUILD_UPDATE",
            &json!({"id": "100", "max_stage_video_channel_users": 0}),
        );
        assert_eq!(stores.guild(100).unwrap().max_stage_video_channel_users, 0);
    }

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
    fn viewing_as_roles_replaces_the_roles_and_drops_the_owner() {
        let mut guild = guild();
        let owner = Me {
            id: 9,
            mfa_enabled: true,
        };
        guild.viewing_roles = Some(Vec::new());
        // The owner seen as @everyone only: View Channel and nothing else.
        assert_eq!(guild_permissions(&guild, owner), VIEW_CHANNEL);
        assert_eq!(true_guild_permissions(&guild, owner), ALL);
        let channel = |guild: &Guild, id: Id| {
            guild
                .channels
                .iter()
                .find(|channel| channel.id == id)
                .unwrap()
                .clone()
        };
        assert!(!can_in_channel(
            &guild,
            &channel(&guild, 3),
            owner,
            VIEW_CHANNEL
        ));
        guild.viewing_roles = Some(vec![300]);
        assert_eq!(guild_permissions(&guild, owner), ALL);
        // A member viewing as @everyone loses their own role's bit, keeps their member overwrite.
        guild.viewing_roles = Some(Vec::new());
        assert!(!can_in_guild(&guild, ME, CREATE_INSTANT_INVITE));
        assert!(can_in_channel(
            &guild,
            &channel(&guild, 2),
            ME,
            VIEW_CHANNEL
        ));
        guild.viewing_roles = None;
        assert!(can_in_guild(&guild, ME, CREATE_INSTANT_INVITE));
    }

    #[test]
    fn impersonation_drops_everyone_and_follows_role_deletes_and_guild_replacement() {
        let mut stores = crate::Stores::default();
        stores.dispatch(
            "READY",
            &json!({
                "user": {"id": "5", "username": "me"},
                "guilds": [{"id": "100", "properties": {"name": "g", "owner_id": "5"},
                    "roles": [{"id": "100", "name": "@everyone", "permissions": "1024"},
                              {"id": "200", "name": "mods", "permissions": "1"}],
                    "channels": [{"id": "1", "type": 0, "name": "open", "permission_overwrites": []}]}],
                "merged_members": [[{"user_id": "5", "roles": []}]]
            }),
        );
        stores.impersonate(100, vec![100, 200]);
        assert_eq!(stores.guild(100).unwrap().viewing_roles, Some(vec![200]));
        stores.dispatch(
            "GUILD_CREATE",
            &json!({"id": "100", "properties": {"name": "g", "owner_id": "5"},
                "roles": [{"id": "100", "name": "@everyone", "permissions": "1024"},
                          {"id": "200", "name": "mods", "permissions": "1"}],
                "channels": [{"id": "1", "type": 0, "name": "open", "permission_overwrites": []}]}),
        );
        assert_eq!(stores.guild(100).unwrap().viewing_roles, Some(vec![200]));
        stores.dispatch(
            "GUILD_ROLE_DELETE",
            &json!({"guild_id": "100", "role_id": "200"}),
        );
        assert_eq!(stores.guild(100).unwrap().viewing_roles, Some(vec![]));
        assert!(stores.stop_impersonating(100));
        assert!(!stores.stop_impersonating(100));
        assert_eq!(stores.guild(100).unwrap().viewing_roles, None);
    }

    #[test]
    fn only_managers_may_view_as_roles_at_or_below_their_highest() {
        let mut guild = guild();
        for (role, position) in [(200, 1), (300, 2)] {
            guild.role_info.insert(
                role,
                crate::RoleInfo {
                    position,
                    ..crate::RoleInfo::default()
                },
            );
        }
        // Role 200 holds Create Invite only: no Manage Server / Manage Roles.
        assert!(!can_impersonate_role(&guild, ME, 200));
        guild
            .role_permissions
            .insert(200, MANAGE_GUILD | MANAGE_ROLES);
        assert!(can_impersonate_role(&guild, ME, 200));
        assert!(can_impersonate_role(&guild, ME, 100));
        assert!(!can_impersonate_role(&guild, ME, 300));
        let owner = Me {
            id: 9,
            mfa_enabled: true,
        };
        assert!(can_impersonate_role(&guild, owner, 300));
        assert_eq!(highest_role(&guild, &[200, 300]), Some(300));
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

/// Module 7864 `l` and 488926 `wO`: whether `role` ranks below the highest of `roles` held by `me`. The owner outranks
/// every role; otherwise a higher position wins, and equal positions go to the lower id. @everyone ranks last.
pub fn role_is_higher(guild: &Guild, me: Id, roles: &[Id], role: Id) -> bool {
    if guild.owner_id == Some(me) {
        return true;
    }
    let rank = |id: Id| -> (i64, Id) {
        if id == guild.id {
            return (i64::MIN, id);
        }
        (guild.role_info.get(&id).map_or(0, |info| info.position), id)
    };
    let Some(mine) = roles
        .iter()
        .copied()
        .map(rank)
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
    else {
        return false;
    };
    let other = rank(role);
    mine.0 > other.0 || (mine.0 == other.0 && mine.1 < other.1)
}

#[cfg(test)]
mod role_rank_tests {
    use super::*;
    use crate::RoleInfo;

    fn guild() -> Guild {
        let mut guild = Guild {
            id: 1,
            owner_id: Some(100),
            ..Guild::default()
        };
        for (id, position) in [(10, 5), (11, 3), (12, 3)] {
            guild.role_info.insert(
                id,
                RoleInfo {
                    position,
                    ..RoleInfo::default()
                },
            );
        }
        guild
    }

    #[test]
    fn a_higher_position_outranks_and_ties_go_to_the_lower_id() {
        let guild = guild();
        assert!(role_is_higher(&guild, 5, &[10], 11));
        assert!(!role_is_higher(&guild, 5, &[11], 10));
        assert!(role_is_higher(&guild, 5, &[11], 12));
        assert!(!role_is_higher(&guild, 5, &[12], 11));
        assert!(!role_is_higher(&guild, 5, &[11], 11));
    }

    #[test]
    fn the_owner_outranks_every_role_and_nobody_without_roles_does() {
        let guild = guild();
        assert!(role_is_higher(&guild, 100, &[], 10));
        assert!(!role_is_higher(&guild, 5, &[], 11));
        assert!(role_is_higher(&guild, 5, &[11], 1));
    }
}
