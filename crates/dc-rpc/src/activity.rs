//! `SET_ACTIVITY`: what the official handler (module 596339, `L0[SET_ACTIVITY]`) makes of a validated request:
//! the activity as `LOCAL_ACTIVITY_UPDATE` stores it and as the command replies with it.

use crate::directory::{Cache, Directory};
use crate::{RpcError, codes};
use serde_json::{Map, Value, json};

/// `ActivityFlags` (module 652215).
pub mod flags {
    pub const INSTANCE: u64 = 1;
    pub const JOIN: u64 = 2;
    pub const PARTY_PRIVACY_FRIENDS: u64 = 64;
    pub const PARTY_PRIVACY_VOICE_CHANNEL: u64 = 128;
}

/// `PARTY_PRIVACY` (`KI` in module 818348): `PRIVATE` is 0, `PUBLIC` is 1.
const PARTY_PUBLIC: i64 = 1;

/// Applications whose secrets may equal the party id (`LZ`).
const SECRETS_MAY_MATCH_PARTY: &[&str] = &["1402418171662569542"];

/// What a `SET_ACTIVITY` asks the store to do.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// `activity: null`: the socket's activity is removed.
    Cleared { pid: i64 },
    /// The activity is stored for the socket; `data` is the reply.
    Set {
        pid: i64,
        activity: Value,
        party_privacy: Option<i64>,
    },
}

/// The activity flags (module 182892 `E`, for a desktop transport): `INSTANCE`, `JOIN` with a join secret, and for a
/// public party the user's two party privacy settings.
pub fn activity_flags(
    has_join_secret: bool,
    instance: bool,
    party_privacy: Option<i64>,
    settings: (bool, bool),
) -> u64 {
    let mut out = 0;
    if instance {
        out |= flags::INSTANCE;
    }
    if has_join_secret {
        out |= flags::JOIN;
    }
    // `ACTIVITIES_FORCE_PUBLIC` is an experiment that is off by default.
    if party_privacy == Some(PARTY_PUBLIC) {
        if settings.0 {
            out |= flags::PARTY_PRIVACY_FRIENDS;
        } else {
            out &= !flags::PARTY_PRIVACY_FRIENDS;
        }
        if settings.1 {
            out |= flags::PARTY_PRIVACY_VOICE_CHANNEL;
        } else {
            out &= !flags::PARTY_PRIVACY_VOICE_CHANNEL;
        }
    }
    out
}

/// JavaScript's `Number.prototype.toString().length` for a JSON number.
fn digits(number: &Value) -> usize {
    match number.as_i64() {
        Some(whole) => whole.to_string().len(),
        None => number.as_f64().unwrap_or(0.0).to_string().len(),
    }
}

fn invalid_secret(message: &str) -> RpcError {
    RpcError::new(codes::INVALID_ACTIVITY_SECRET, message)
}

/// Runs a validated `SET_ACTIVITY` (`args` passed `validate::set_activity`).
pub async fn set_activity(
    dir: &dyn Directory,
    cache: &Cache,
    application: &Value,
    args: &Value,
    now_ms: u64,
) -> Result<Outcome, RpcError> {
    let pid = args
        .get("pid")
        .and_then(Value::as_f64)
        .map(|pid| pid as i64)
        .ok_or_else(|| RpcError::new(codes::INVALID_COMMAND, "nonzero pid required"))?;
    let Some(sent) = args.get("activity").and_then(Value::as_object) else {
        return Ok(Outcome::Cleared { pid });
    };
    let application_id = application["id"].as_str().map(str::to_owned);

    // `{buttons, ...rest}`, then `name` and `type` filled in when the game left them out.
    let mut activity: Map<String, Value> = sent.clone();
    let buttons = activity.shift_remove("buttons");
    if activity.get("name").is_none_or(Value::is_null) {
        activity.insert("name".into(), application["name"].clone());
    }
    if activity.get("type").is_none_or(Value::is_null) {
        activity.insert("type".into(), json!(0));
    }
    activity.insert(
        "application_id".into(),
        application_id.clone().map_or(Value::Null, Value::String),
    );
    activity.insert("platform".into(), json!("desktop"));

    let instance = activity
        .get("instance")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let party_privacy = activity
        .get("party")
        .and_then(|party| party.get("privacy"))
        .and_then(Value::as_f64)
        .map(|privacy| privacy as i64);
    activity.shift_remove("instance");
    if let Some(party) = activity.get_mut("party").and_then(Value::as_object_mut) {
        party.shift_remove("privacy");
    }
    let has_join = activity
        .get("secrets")
        .is_some_and(|secrets| !secrets["join"].is_null());
    let computed = activity_flags(has_join, instance, party_privacy, dir.party_privacy());
    if computed > 0 {
        activity.insert("flags".into(), json!(computed));
    }

    if let Some(secrets) = activity.get("secrets").and_then(Value::as_object) {
        let values: Vec<&str> = secrets
            .values()
            .filter_map(Value::as_str)
            .filter(|value| !value.is_empty())
            .collect();
        let party_id = activity.get("party").and_then(|party| party["id"].as_str());
        let exempt = application_id
            .as_deref()
            .is_some_and(|id| SECRETS_MAY_MATCH_PARTY.contains(&id));
        if party_id.is_some_and(|id| values.contains(&id)) && !exempt {
            return Err(invalid_secret("secrets cannot match the party id"));
        }
        let mut unique = values.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() < values.len() {
            return Err(invalid_secret("secrets must be unique"));
        }
        if buttons.is_some() {
            return Err(invalid_secret(
                "secrets cannot currently be sent with buttons",
            ));
        }
    }

    let mut metadata = Map::new();
    if let Some(buttons) = &buttons {
        let list = buttons.as_array().cloned().unwrap_or_default();
        metadata.insert(
            "button_urls".into(),
            Value::Array(list.iter().map(|button| button["url"].clone()).collect()),
        );
        activity.insert(
            "buttons".into(),
            Value::Array(list.iter().map(|button| button["label"].clone()).collect()),
        );
    }
    activity.insert("metadata".into(), Value::Object(metadata));

    // Seconds become milliseconds when the number is more than two digits shorter than the clock.
    let clock_digits = now_ms.to_string().len();
    if let Some(stamps) = activity
        .get_mut("timestamps")
        .and_then(Value::as_object_mut)
    {
        for key in ["start", "end"] {
            let Some(stamp) = stamps.get(key).filter(|stamp| stamp.is_number()) else {
                continue;
            };
            if clock_digits as i64 - digits(stamp) as i64 > 2 {
                let millis = (stamp.as_f64().unwrap_or(0.0) * 1000.0).floor();
                stamps.insert(key.into(), json!(millis as u64));
            }
        }
    }

    if let Some(assets) = activity.get("assets").and_then(Value::as_object) {
        let Some(id) = application_id.as_deref() else {
            return Err(RpcError::new(codes::INVALID_COMMAND, "No application."));
        };
        let keys = ["large_image", "small_image", "invite_cover_image"];
        let sent_keys: Vec<Option<String>> = keys
            .iter()
            .map(|key| assets.get(*key).and_then(Value::as_str).map(str::to_owned))
            .collect();
        let resolved = cache
            .resolve_assets(
                dir,
                id,
                [
                    sent_keys[0].as_deref(),
                    sent_keys[1].as_deref(),
                    sent_keys[2].as_deref(),
                ],
            )
            .await;
        if let Some(assets) = activity.get_mut("assets").and_then(Value::as_object_mut) {
            for (key, found) in keys.iter().zip(resolved) {
                match found {
                    Some(found) => {
                        assets.insert((*key).into(), Value::String(found));
                    }
                    None => {
                        assets.shift_remove(*key);
                    }
                }
            }
        }
    }

    Ok(Outcome::Set {
        pid,
        activity: Value::Object(activity),
        party_privacy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::BoxFuture;

    struct Fake;
    impl Directory for Fake {
        fn application(&self, _: &str) -> BoxFuture<'_, Option<Value>> {
            Box::pin(async { None })
        }
        fn assets(&self, _: &str) -> BoxFuture<'_, Option<Vec<Value>>> {
            Box::pin(async {
                Some(vec![
                    json!({"id": "1559539669316800582", "name": "pt", "type": 1}),
                ])
            })
        }
        fn external_assets(&self, _: &str, _: Vec<String>) -> BoxFuture<'_, Option<Vec<Value>>> {
            Box::pin(async { None })
        }
        fn current_user(&self) -> Option<Value> {
            None
        }
    }

    fn app() -> Value {
        json!({"id": "1559538474065993750", "name": "P.T. Silent Hills"})
    }

    async fn run(args: Value) -> Result<Outcome, RpcError> {
        set_activity(&Fake, &Cache::default(), &app(), &args, 1_794_312_400_000).await
    }

    fn set(outcome: Outcome) -> Value {
        match outcome {
            Outcome::Set { activity, .. } => activity,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn the_real_games_activity_gets_the_shape_the_official_client_stored() {
        let activity = set(run(json!({
            "pid": 3456,
            "activity": {
                "details": "In the menus",
                "state": "New Game+ 4/4",
                "timestamps": {"start": 1794312327},
                "assets": {"large_image": "pt", "large_text": "P.T. PC Port"},
            }
        }))
        .await
        .unwrap());
        let expected = json!({
            "details": "In the menus",
            "state": "New Game+ 4/4",
            "timestamps": {"start": 1794312327000u64},
            "assets": {"large_image": "1559539669316800582", "large_text": "P.T. PC Port"},
            "name": "P.T. Silent Hills",
            "type": 0,
            "application_id": "1559538474065993750",
            "platform": "desktop",
            "metadata": {}
        });
        assert_eq!(activity, expected);
        // The key order is the official one too.
        let keys: Vec<&String> = activity.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "details",
                "state",
                "timestamps",
                "assets",
                "name",
                "type",
                "application_id",
                "platform",
                "metadata"
            ]
        );
    }

    #[tokio::test]
    async fn a_pid_is_required_and_null_clears() {
        assert_eq!(
            run(json!({"activity": {"state": "ab"}})).await,
            Err(RpcError::new(
                codes::INVALID_COMMAND,
                "nonzero pid required"
            ))
        );
        assert_eq!(
            run(json!({"pid": 9, "activity": null})).await,
            Ok(Outcome::Cleared { pid: 9 })
        );
        assert_eq!(
            run(json!({"pid": 9})).await,
            Ok(Outcome::Cleared { pid: 9 })
        );
    }

    #[tokio::test]
    async fn the_game_may_name_itself_and_choose_a_type_and_milliseconds_stay() {
        let activity = set(run(json!({
            "pid": 1,
            "activity": {"name": "Mine", "type": 2, "timestamps": {"start": 1794312327000u64, "end": 1794312399}}
        }))
        .await
        .unwrap());
        assert_eq!(activity["name"], "Mine");
        assert_eq!(activity["type"], 2);
        assert_eq!(activity["timestamps"]["start"], 1794312327000u64);
        assert_eq!(activity["timestamps"]["end"], 1794312399000u64);
    }

    #[tokio::test]
    async fn buttons_become_labels_and_urls_move_to_metadata() {
        let activity = set(run(json!({
            "pid": 1,
            "activity": {"buttons": [{"label": "Play", "url": "https://a.b"}]}
        }))
        .await
        .unwrap());
        assert_eq!(activity["buttons"], json!(["Play"]));
        assert_eq!(activity["metadata"]["button_urls"], json!(["https://a.b"]));
    }

    #[tokio::test]
    async fn secrets_follow_the_official_rules() {
        let with = |secrets: Value, extra: Value| {
            let mut activity =
                json!({"party": {"id": "party1", "size": [1, 2]}, "secrets": secrets});
            activity
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            json!({"pid": 1, "activity": activity})
        };
        assert_eq!(
            run(with(json!({"join": "party1"}), json!({}))).await,
            Err(invalid_secret("secrets cannot match the party id"))
        );
        assert_eq!(
            run(with(
                json!({"join": "secretA", "match": "secretA"}),
                json!({})
            ))
            .await,
            Err(invalid_secret("secrets must be unique"))
        );
        assert_eq!(
            run(with(
                json!({"join": "secretA"}),
                json!({"buttons": [{"label": "a", "url": "https://a.b"}]})
            ))
            .await,
            Err(invalid_secret(
                "secrets cannot currently be sent with buttons"
            ))
        );
        let ok = set(run(with(json!({"join": "secretA"}), json!({})))
            .await
            .unwrap());
        assert_eq!(ok["flags"], flags::JOIN);
    }

    #[tokio::test]
    async fn flags_follow_instance_join_and_the_party_privacy_settings() {
        assert_eq!(activity_flags(false, true, None, (true, true)), 1);
        assert_eq!(activity_flags(true, false, None, (true, true)), 2);
        assert_eq!(activity_flags(false, false, Some(1), (true, false)), 64);
        assert_eq!(
            activity_flags(true, true, Some(1), (true, true)),
            1 | 2 | 64 | 128
        );
        let activity = set(run(json!({
            "pid": 1,
            "activity": {"instance": true, "party": {"id": "party1", "size": [1, 2], "privacy": 1}}
        }))
        .await
        .unwrap());
        assert_eq!(activity["flags"], 1 | 64 | 128);
        assert!(activity.get("instance").is_none());
        assert!(activity["party"].get("privacy").is_none());
    }
}
