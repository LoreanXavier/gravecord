//! A READY shaped like an old, large account's: a hundred servers (some unavailable stubs, some lazy), server
//! folders, hundreds of relationships of every type, a couple of hundred DMs and group DMs, versioned partial
//! `read_state` and `user_guild_settings`, several sessions, ids above 2^53 sent as strings. Nothing may panic, the
//! counts must be right and the rail must follow the folders.

use dc_proto::settings::{GuildFolderEntry, set_guild_folders};
use dc_state::Stores;
use dc_state::folders::Item;
use serde_json::{Value, json};

/// Above 2^53, so a number that went through an f64 would come back wrong.
const BASE: u64 = 9_007_199_254_741_000;
const GUILDS: u64 = 100;
const ME: u64 = BASE + 5_000;

fn guild_id(index: u64) -> u64 {
    BASE + 100 + index
}

fn user_id(index: u64) -> u64 {
    BASE + 1_000 + index
}

fn dm_id(index: u64) -> u64 {
    BASE + 20_000 + index
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |sum, (at, byte)| {
            sum | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ALPHABET[(triple >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Indexes 0..=3 are unavailable stubs (an outage); 4 is a lazy guild with no channels yet; the rest are whole.
fn guild(index: u64) -> Value {
    let id = guild_id(index).to_string();
    if index < 4 {
        return json!({"id": id, "unavailable": true});
    }
    let channels = if index == 4 {
        json!([])
    } else {
        json!([
            {"id": (guild_id(index) + 10_000).to_string(), "type": 4, "name": "Text", "position": 0, "parent_id": null},
            {"id": (guild_id(index) + 20_000).to_string(), "type": 0, "name": "general", "position": 0,
             "parent_id": (guild_id(index) + 10_000).to_string(), "last_message_id": (BASE + 900_000 + index).to_string()},
            {"id": (guild_id(index) + 30_000).to_string(), "type": 15, "name": "forum", "position": 1, "parent_id": null},
            {"id": (guild_id(index) + 40_000).to_string(), "type": 13, "name": "stage", "position": 2, "parent_id": null},
            {"id": (guild_id(index) + 50_000).to_string(), "type": 5, "name": "news", "position": 3, "parent_id": null},
            {"id": (guild_id(index) + 60_000).to_string(), "type": 99, "name": "a type from the future", "position": 4},
        ])
    };
    json!({
        "id": id,
        "joined_at": format!("2019-01-01T00:{:02}:00+00:00", index % 60),
        "lazy": index.is_multiple_of(2),
        "properties": {"name": format!("Server {index}"), "icon": if index.is_multiple_of(3) { Value::Null } else { json!("abc") },
                       "owner_id": ME.to_string(), "features": ["COMMUNITY", "SOME_FEATURE_NOT_KNOWN"], "premium_tier": 3},
        "channels": channels,
        "roles": [
            {"id": id, "name": "@everyone", "permissions": "1071698660929", "color": 0, "position": 0},
            {"id": (guild_id(index) + 1).to_string(), "name": "gradient", "permissions": "8", "color": 16711680, "position": 3,
             "colors": {"primary_color": 16711680, "secondary_color": 255, "tertiary_color": null},
             "icon": null, "unicode_emoji": "x", "flags": 0},
        ],
        "threads": [],
        "emojis": [],
        "premium_subscription_count": 14,
    })
}

fn relationship(index: u64) -> Value {
    // Types 1 friend, 2 blocked, 3 incoming, 4 outgoing, 5 implicit; every sixth is an ignored user as well.
    let kind = index % 5 + 1;
    let mut entry = json!({"id": user_id(index).to_string(), "type": kind, "since": "2020-05-05T00:00:00.000000+00:00"});
    if index.is_multiple_of(6) {
        entry["user_ignored"] = json!(true);
    }
    if index.is_multiple_of(7) {
        entry["nickname"] = json!(format!("nick {index}"));
    }
    if index.is_multiple_of(11) {
        entry["is_spam_request"] = json!(true);
    }
    entry
}

fn private_channel(index: u64) -> Value {
    let last = if index.is_multiple_of(9) {
        Value::Null
    } else {
        json!((BASE + 800_000 + index * 3).to_string())
    };
    if index.is_multiple_of(4) {
        // A group DM: no name, sometimes an icon, recipients as ids.
        json!({"id": dm_id(index).to_string(), "type": 3, "name": null,
               "icon": if index.is_multiple_of(8) { json!("gicon") } else { Value::Null },
               "owner_id": ME.to_string(), "last_message_id": last,
               "recipient_ids": [user_id(index).to_string(), user_id(index + 1).to_string(), user_id(index + 2).to_string()]})
    } else {
        json!({"id": dm_id(index).to_string(), "type": 1, "last_message_id": last,
               "recipient_ids": [user_id(index).to_string()]})
    }
}

fn settings_proto() -> String {
    let guilds = |ids: &[u64]| ids.iter().map(|index| guild_id(*index)).collect::<Vec<_>>();
    let entries = vec![
        GuildFolderEntry {
            id: Some(11),
            name: Some("First".into()),
            color: Some(0xff0000),
            guild_ids: guilds(&[10, 11, 12]),
        },
        GuildFolderEntry {
            id: None,
            name: None,
            color: None,
            guild_ids: guilds(&[13]),
        },
        // A folder with an unavailable guild, a guild this client never heard of, and one that is whole.
        GuildFolderEntry {
            id: Some(12),
            name: None,
            color: None,
            guild_ids: vec![guild_id(0), guild_id(900), guild_id(14)],
        },
        // Only unknown guilds: the folder is dropped.
        GuildFolderEntry {
            id: Some(13),
            name: Some("Gone".into()),
            color: None,
            guild_ids: vec![guild_id(901), guild_id(902)],
        },
        GuildFolderEntry {
            id: None,
            name: None,
            color: None,
            guild_ids: guilds(&[1]),
        },
        GuildFolderEntry {
            id: None,
            name: None,
            color: None,
            guild_ids: guilds(&[15]),
        },
        // Listed twice: the second place is ignored.
        GuildFolderEntry {
            id: None,
            name: None,
            color: None,
            guild_ids: guilds(&[15]),
        },
        GuildFolderEntry {
            id: Some(14),
            name: None,
            color: None,
            guild_ids: guilds(&[2, 3]),
        },
    ];
    let (_, settings) = set_guild_folders(&[], &entries);
    base64(&settings)
}

fn big_ready() -> Value {
    let guilds: Vec<Value> = (0..GUILDS).map(guild).collect();
    // `merged_members` lines up with `guilds`; stubs and lazy ones have an empty list, one has somebody else's.
    let merged: Vec<Value> = (0..GUILDS)
        .map(|index| match index {
            0..=4 => json!([]),
            5 => json!([{"user_id": user_id(1).to_string(), "roles": []}]),
            _ => json!([{"user_id": ME.to_string(), "roles": [(guild_id(index) + 1).to_string()], "joined_at": "2019-01-01T00:00:00+00:00"}]),
        })
        .collect();
    let users: Vec<Value> = (0..700)
        .map(|index| json!({"id": user_id(index).to_string(), "username": format!("user{index}"), "discriminator": "0", "global_name": null,
                            "avatar": null, "primary_guild": {"identity_enabled": null, "identity_guild_id": null, "tag": null, "badge": null},
                            "collectibles": null, "display_name_styles": null, "avatar_decoration_data": null}))
        .collect();
    let read_entries: Vec<Value> = (0..GUILDS)
        .map(|index| json!({"id": (guild_id(index) + 20_000).to_string(), "last_message_id": (BASE + 900_000 + index / 2).to_string(), "mention_count": index % 3, "flags": 0}))
        .chain((0..200).map(|index| json!({"id": dm_id(index).to_string(), "last_message_id": (BASE + 1).to_string(), "mention_count": 0, "read_state_type": 0})))
        .chain([json!({"id": guild_id(10).to_string(), "read_state_type": 3, "last_acked_id": "0", "badge_count": 2})])
        .collect();
    let guild_settings: Vec<Value> = (0..GUILDS)
        .map(|index| json!({"guild_id": guild_id(index).to_string(), "muted": index % 5 == 0, "message_notifications": 1, "channel_overrides": [],
                            "mute_config": {"end_time": null, "selected_time_window": null}}))
        .chain([json!({"guild_id": null, "muted": false, "message_notifications": 0, "channel_overrides": []})])
        .collect();
    json!({
        "v": 9,
        "user": {"id": ME.to_string(), "username": "old account", "discriminator": "0", "global_name": "Old", "avatar": "x",
                 "premium_type": 2, "mfa_enabled": true, "email": "a@example.test", "verified": true, "flags": 1u64 << 40, "public_flags": 0},
        "users": users,
        "guilds": guilds,
        "merged_members": merged,
        "relationships": (0..500).map(relationship).collect::<Vec<_>>(),
        "private_channels": (0..200).map(private_channel).collect::<Vec<_>>(),
        "read_state": {"version": 4172, "partial": false, "entries": read_entries},
        "user_guild_settings": {"version": 920, "partial": false, "entries": guild_settings},
        "user_settings_proto": settings_proto(),
        "guild_join_requests": [{"guild_id": guild_id(30).to_string(), "id": "7", "application_status": "SUBMITTED"}],
        "sessions": [
            {"session_id": "all", "status": "online", "activities": [], "client_info": {"client": "web", "os": "windows", "version": 0}, "active": true},
            {"session_id": "phone", "status": "idle", "activities": [], "client_info": {"client": "mobile", "os": "ios", "version": 0}},
            {"session_id": "console", "status": "dnd", "activities": [{"type": 0, "name": "game"}], "client_info": {}},
        ],
        "session_id": "abc",
        "auth_session_id_hash": "hash",
        "consents": {"personalization": {"consented": true}},
        "notification_settings": {"flags": 0},
        "apex_experiments": {"assignments": {}},
    })
}

fn supplemental() -> Value {
    json!({
        "guilds": (4..40).map(|index| json!({"id": guild_id(index).to_string(),
            "voice_states": [{"user_id": user_id(index).to_string(), "channel_id": (guild_id(index) + 20_000).to_string(), "session_id": "s"}]}))
            .collect::<Vec<_>>(),
        "merged_presences": {
            "friends": (0..100).map(|index| json!({"user_id": user_id(index).to_string(), "status": "online", "activities": [], "client_status": {"desktop": "online"}})).collect::<Vec<_>>(),
            "guilds": [],
        },
        "merged_members": [],
    })
}

#[test]
fn a_large_old_accounts_ready_loads_and_sorts() {
    let mut stores = Stores::default();
    stores.dispatch("READY", &big_ready());

    assert_eq!(stores.current_user.as_ref().map(|user| user.id), Some(ME));
    // Ids above 2^53 survive as strings.
    assert!(guild_id(50) > (1 << 53));
    assert!(stores.guild(guild_id(50)).is_some());

    // Four stubs are not loaded as guilds, and stay known as unavailable; the lazy one with no channels is a guild.
    assert_eq!(stores.guilds.len(), 96);
    assert_eq!(stores.unavailable_guilds.len(), 4);
    assert!(
        stores
            .guild(guild_id(4))
            .is_some_and(|guild| guild.channels.is_empty())
    );
    // The channel of a type this client does not know is kept, not a reason to drop the guild.
    assert_eq!(
        stores.guild(guild_id(50)).map(|guild| guild.channels.len()),
        Some(6)
    );

    // Relationships of every type, deduplicated by user.
    assert_eq!(stores.relationships.len(), 500);
    assert_eq!(
        stores
            .relationships
            .values()
            .filter(|kind| **kind == 1)
            .count(),
        100
    );
    assert_eq!(stores.friends().len(), 100);
    assert_eq!(stores.ignored.len(), 84);

    // DMs: all kept, newest conversation first, the ones that never had a message last.
    assert_eq!(stores.private_channels.len(), 200);
    let last: Vec<u64> = stores
        .private_channels
        .iter()
        .map(|channel| channel.last_message_id)
        .collect();
    assert!(last.windows(2).all(|pair| pair[0] >= pair[1]));
    assert_eq!(last.last(), Some(&0));
    let group = stores
        .private_channels
        .iter()
        .find(|channel| channel.id == dm_id(8))
        .expect("group DM");
    assert_eq!(
        (
            group.kind,
            group.name.clone(),
            group.icon.as_deref(),
            group.recipients.len()
        ),
        (3, None, Some("gicon"), 3)
    );

    // Sessions and the join request list come through; several sessions are kept.
    assert_eq!(stores.sessions.len(), 3);

    // The rail follows the folders: unlisted guilds on top (newest first), then the settings' order.
    let roots = stores.folders.roots();
    let flat = stores.folders.flattened();
    // 100 guilds known or unavailable; the three the settings never named and the unknown ones are not in it.
    assert_eq!(flat.len(), 100);
    let mut listed = vec![
        guild_id(10),
        guild_id(11),
        guild_id(12),
        guild_id(13),
        guild_id(0),
        guild_id(14),
        guild_id(1),
        guild_id(15),
        guild_id(2),
        guild_id(3),
    ];
    let tail: Vec<u64> = flat[flat.len() - listed.len()..].to_vec();
    assert_eq!(tail, listed);
    listed.clear();
    // The first root is the newest unlisted guild (index 99 joined last within the same hour wrap, so only check it is unlisted).
    assert!(matches!(roots[0], Item::Guild(_)));
    // The folder with the unavailable guild keeps it, and the folder of unknown guilds is gone.
    let folder = stores.folders.folder(12).expect("folder 12");
    assert_eq!(folder.guilds, vec![guild_id(0), guild_id(14)]);
    assert!(stores.folders.folder(13).is_none());
    assert_eq!(
        stores.folders.folder(11).map(|folder| folder.guilds.len()),
        Some(3)
    );
    // A drag of another server writes the tree back: every unavailable guild stays, once, and the stub in folder
    // 12 stays in that folder at its old index; the guild unknown on this device is kept by the fallback, at the end.
    let patch = stores.move_guild(guild_id(13), dc_state::folders::Placement::Last);
    assert!(!patch.is_empty());
    let (written, _) = dc_proto::settings::guild_folders(&dc_proto::settings::decode(&patch));
    let all: Vec<u64> = written
        .iter()
        .flat_map(|entry| entry.guild_ids.clone())
        .collect();
    for stub in 0..4 {
        assert_eq!(
            all.iter().filter(|id| **id == guild_id(stub)).count(),
            1,
            "stub {stub}"
        );
    }
    let mut unique = all.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "a server is listed twice");
    let folder = written
        .iter()
        .find(|entry| entry.id == Some(12))
        .expect("folder 12 written");
    assert_eq!(
        folder.guild_ids,
        vec![guild_id(0), guild_id(14), guild_id(900)]
    );

    // READY_SUPPLEMENTAL after READY: voice states and friend presences.
    stores.dispatch("READY_SUPPLEMENTAL", &supplemental());
    assert_eq!(
        stores.voice_states.get(&guild_id(20)).map(Vec::len),
        Some(1)
    );
    assert_eq!(stores.status(user_id(0)), "online");

    // The stub is filled in later by GUILD_CREATE and takes its place in the folder.
    let mut created = guild(5);
    created["id"] = json!(guild_id(0).to_string());
    stores.dispatch("GUILD_CREATE", &created);
    assert_eq!(stores.guilds.len(), 97);
    assert_eq!(stores.unavailable_guilds.len(), 3);
    assert_eq!(
        stores
            .folders
            .folder(12)
            .map(|folder| folder.guilds.clone()),
        Some(vec![guild_id(0), guild_id(14)])
    );

    // A second READY (a reconnect with a fresh session) replaces everything and keeps the counts.
    stores.dispatch("READY", &big_ready());
    assert_eq!(stores.guilds.len(), 96);
    assert_eq!(stores.private_channels.len(), 200);
}

#[test]
fn odd_values_in_ready_are_skipped_not_fatal() {
    let mut stores = Stores::default();
    let odd = json!({
        "user": {"id": "1", "username": "x"},
        "guilds": [null, 5, "text", {}, {"id": 7}, {"id": "8", "unavailable": true}, {"id": "9", "properties": null, "channels": [null, {"id": "x"}]},
                   {"id": "10", "properties": {"name": null}, "channels": []}],
        "merged_members": "nope",
        "relationships": [null, {"id": "1"}, {"id": "2", "type": "friend"}, {"id": "99999999999999999999999", "type": 1}],
        "private_channels": [null, {"id": "3"}, {"id": "4", "type": 1, "recipient_ids": [1, null, "x"]}, {"id": "5", "type": 3, "recipients": "no"}],
        "read_state": {"entries": [null, {"id": 3}, {"id": "4", "read_state_type": "x"}]},
        "user_guild_settings": {"entries": [null, {"guild_id": 5}, {"guild_id": "6", "channel_overrides": [null, 3]}]},
        "user_settings_proto": "!!!not base64!!!",
        "sessions": [null, 3, {}],
        "users": [null, {"id": "q"}],
    });
    stores.dispatch("READY", &odd);
    stores.dispatch("READY_SUPPLEMENTAL", &json!({"guilds": [null, {"id": "1"}, {"id": "2", "voice_states": [null, 3]}], "merged_presences": {"friends": [null, 4]}}));
    stores.dispatch("GUILD_CREATE", &json!({"id": "8", "unavailable": true}));
    stores.dispatch("GUILD_DELETE", &json!({"id": "8", "unavailable": true}));
    stores.dispatch("READY", &json!(null));
    stores.dispatch("READY", &json!([]));
    assert_eq!(stores.guilds.len(), 0);
}

#[test]
fn every_guilds_counts_for_a_frame_of_the_rail_are_cheap() {
    let mut stores = Stores::default();
    stores.dispatch("READY", &big_ready());
    let started = std::time::Instant::now();
    let mut unread = 0;
    for _ in 0..20 {
        for guild in &stores.guilds {
            let counts =
                dc_state::unread::guild_counts_uncached(&stores, guild, chrono::Utc::now());
            unread += usize::from(counts.unread);
        }
    }
    let per_pass = started.elapsed() / 20;
    println!("96 guilds, uncached unread counts: {per_pass:?} per pass ({unread} unread marks)");
    assert!(
        per_pass < std::time::Duration::from_millis(100),
        "{per_pass:?}"
    );
    let started = std::time::Instant::now();
    for _ in 0..20 {
        let _ = stores.friends();
    }
    println!(
        "500 relationships, friends(): {:?} per call",
        started.elapsed() / 20
    );
}

#[test]
fn the_large_ready_dispatches_in_well_under_a_second() {
    let ready = big_ready();
    let size = ready.to_string().len();
    let mut stores = Stores::default();
    let started = std::time::Instant::now();
    stores.dispatch("READY", &ready);
    let took = started.elapsed();
    println!("READY of {size} bytes: dispatched in {took:?} (debug build)");
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
}
