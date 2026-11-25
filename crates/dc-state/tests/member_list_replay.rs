//! A long member list replayed through the stores and the subscription manager: the first hundred, a scroll that
//! asks for the next, a channel with its own list, an INVALIDATE and the re-sync, a resume and a new session. The
//! rows in view are all filled at each step, and the op 37 sends are the official ones (modules 661670, 36124,
//! 361610, 963307).

use dc_state::Stores;
use dc_state::guild_subscriptions::{DEFAULT_RANGES, GuildSubscriptions, ranges_for};
use dc_state::member_list::{self, Row};
use serde_json::{Value, json};

const GUILD: u64 = 100;
const ONLINE: u64 = 250;

fn member(id: u64) -> Value {
    json!({"member": {"user": {"id": id.to_string(), "username": format!("user{id}")}, "roles": [], "presence": {"status": "online"}}})
}

/// The server's answer for `range` of a list of one "online" group of `ONLINE` members.
fn sync(range: [u64; 2]) -> Value {
    json!({"op": "SYNC", "range": range, "items": items(range)})
}

fn items(range: [u64; 2]) -> Vec<Value> {
    (range[0]..=range[1].min(ONLINE))
        .map(|row| {
            if row == 0 {
                json!({"group": {"id": "online", "count": ONLINE}})
            } else {
                member(1000 + row)
            }
        })
        .collect()
}

fn update(list: &str, ops: Vec<Value>) -> Value {
    json!({"guild_id": GUILD.to_string(), "id": list, "member_count": ONLINE, "online_count": ONLINE,
        "groups": [{"id": "online", "count": ONLINE}], "ops": ops})
}

fn guild_create() -> Value {
    json!({"id": GUILD.to_string(), "name": "Test", "roles": [{"id": GUILD.to_string(), "name": "@everyone", "permissions": "1024", "position": 0}],
    "channels": [
        {"id": "1", "type": 0, "name": "general", "position": 0},
        {"id": "2", "type": 0, "name": "staff", "position": 1, "permission_overwrites": [
            {"id": GUILD.to_string(), "type": 0, "allow": "0", "deny": "1024"},
            {"id": "300", "type": 0, "allow": "1024", "deny": "0"}]}
    ]})
}

/// The rows a 900px list scrolled `y` down shows (42px rows), each filled.
fn assert_view_filled(stores: &Stores, list: &str, y: f32) {
    let list = stores.member_lists.list(GUILD, list).expect("the list");
    assert_eq!(list.rows.len() as u64, ONLINE + 1);
    let first = (y / 42.) as usize;
    let last = (((y + 900.) / 42.).ceil() as usize).min(list.rows.len());
    for index in first..last {
        assert!(
            list.group_at(index).is_some() || matches!(list.rows[index], Row::Member { .. }),
            "row {index} is not filled"
        );
    }
}

fn list_id_of(stores: &Stores, channel: u64) -> String {
    let guild = stores.guild(GUILD).unwrap();
    let channel = guild
        .channels
        .iter()
        .find(|known| known.id == channel)
        .unwrap();
    member_list::list_id(guild, channel, None)
}

fn sent(subscriptions: &mut GuildSubscriptions) -> Value {
    Value::Object(subscriptions.take_pending().expect("a send"))
}

#[test]
fn a_long_list_fills_as_it_scrolls_and_survives_other_lists_and_reconnects() {
    let mut stores = Stores::default();
    stores.dispatch("GUILD_CREATE", &guild_create());
    let mut subscriptions = GuildSubscriptions::default();
    assert_eq!(list_id_of(&stores, 1), "everyone");
    let staff = list_id_of(&stores, 2);
    assert_ne!(staff, "everyone");

    // Opening #general: the guild's features and the first hundred rows.
    subscriptions.subscribe_guild(GUILD);
    subscriptions.subscribe_channel(GUILD, 1, &DEFAULT_RANGES, true);
    assert_eq!(
        sent(&mut subscriptions),
        json!({"100": {"typing": true, "activities": true, "threads": true, "channels": {"1": [[0, 99]]}}})
    );
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &update("everyone", vec![sync([0, 99])]),
    );
    assert_view_filled(&stores, "everyone", 0.);
    // The list is as long as its group; the rows past the first hundred wait for their range.
    let everyone = stores.member_lists.list(GUILD, "everyone").unwrap();
    assert_eq!(everyone.rows[150], Row::Unknown);

    // Scrolled 4000px down, the view wants the second hundred too.
    let ranges = ranges_for(4000., 900., 42.);
    assert_eq!(ranges, vec![[0, 99], [100, 199]]);
    assert!(subscriptions.subscribe_channel(GUILD, 1, &ranges, false));
    assert_eq!(
        sent(&mut subscriptions),
        json!({"100": {"channels": {"1": [[0, 99], [100, 199]]}}})
    );
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &update("everyone", vec![sync([100, 199])]),
    );
    assert_view_filled(&stores, "everyone", 4000.);

    // #staff reads another list; #general keeps its subscription beside it.
    subscriptions.subscribe_channel(GUILD, 2, &DEFAULT_RANGES, true);
    assert_eq!(
        sent(&mut subscriptions),
        json!({"100": {"channels": {"2": [[0, 99]], "1": [[0, 99], [100, 199]]}}})
    );
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &json!({"guild_id": "100", "id": staff, "groups": [{"id": "online", "count": 2}],
            "ops": [{"op": "SYNC", "range": [0, 99], "items": [{"group": {"id": "online", "count": 2}}, member(1), member(2)]}]}),
    );
    stores.member_lists.show(GUILD, &staff);
    assert_eq!(stores.member_lists.get(&GUILD).unwrap().rows.len(), 3);
    // The @everyone list's second hundred is invalidated and synced again; #staff's list never changes.
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &update(
            "everyone",
            vec![json!({"op": "INVALIDATE", "range": [100, 199]})],
        ),
    );
    assert_eq!(
        stores.member_lists.list(GUILD, "everyone").unwrap().rows[150],
        Row::Unknown
    );
    assert!(
        stores
            .member_lists
            .get(&GUILD)
            .unwrap()
            .rows
            .iter()
            .skip(1)
            .all(|row| matches!(row, Row::Member { .. }))
    );
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &update("everyone", vec![sync([100, 199])]),
    );
    stores.member_lists.show(GUILD, "everyone");
    assert_view_filled(&stores, "everyone", 4000.);

    // A resume sends the guild whole again, scrolled ranges included.
    let resumed = subscriptions.connection_resumed(|_| true).unwrap();
    assert_eq!(
        resumed["100"]["channels"],
        json!({"2": [[0, 99]], "1": [[0, 99], [100, 199]]})
    );

    // A new session drops the lists; the whole subscription goes again and its answer fills them.
    stores.dispatch("READY", &json!({"user": {"id": "9", "username": "me"}}));
    assert!(stores.member_lists.list(GUILD, "everyone").is_none());
    stores.dispatch("GUILD_CREATE", &guild_create());
    let whole = subscriptions
        .connection_open(&[GUILD], |guild| stores.guild(guild).is_some())
        .unwrap();
    assert_eq!(whole["100"]["channels"]["1"], json!([[0, 99], [100, 199]]));
    stores.dispatch(
        "GUILD_MEMBER_LIST_UPDATE",
        &update("everyone", vec![sync([0, 99]), sync([100, 199])]),
    );
    assert_view_filled(&stores, "everyone", 0.);
    assert_view_filled(&stores, "everyone", 4000.);
}
