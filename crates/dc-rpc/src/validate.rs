//! The `SET_ACTIVITY` request schema (module 596339, `Rt[SET_ACTIVITY].request`), checked the way Joi checks it
//! with `convert: false` and the default `abortEarly`: the first failure wins, and its message is Joi's.
//! Objects of the schema accept unknown keys (`O3` is `required().unknown(true)`).

use serde_json::{Map, Value};
use url::Url;

/// `ActivityGamePlatforms` (module 570138).
pub const PLATFORMS: &[&str] = &[
    "xbox",
    "desktop",
    "samsung",
    "ios",
    "android",
    "ps4",
    "ps5",
    "embedded",
    "meta_quest",
];
/// `ActivityType` values the schema accepts: PLAYING, LISTENING, WATCHING, COMPETING (module 350835).
pub const TYPES: &[i64] = &[0, 2, 3, 5];
/// `StatusDisplayType`: NAME, STATE, DETAILS (module 599026).
pub const STATUS_DISPLAY_TYPES: &[i64] = &[0, 1, 2];

type Check = Result<(), String>;

fn join(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}

/// Joi counts string length in UTF-16 code units.
fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("\"{label}\" must be of type object"))
}

struct Text {
    min: usize,
    max: usize,
    uri: bool,
}

fn text(value: &Value, label: &str, rules: Text) -> Check {
    let Some(string) = value.as_str() else {
        return Err(format!("\"{label}\" must be a string"));
    };
    if string.is_empty() {
        return Err(format!("\"{label}\" is not allowed to be empty"));
    }
    // Rules run in the order the schema declares them: `uri`, then `min`, then `max`.
    if rules.uri && !is_uri(string) {
        return Err(format!("\"{label}\" must be a valid uri"));
    }
    if length(string) < rules.min {
        return Err(format!(
            "\"{label}\" length must be at least {} characters long",
            rules.min
        ));
    }
    if length(string) > rules.max {
        return Err(format!(
            "\"{label}\" length must be less than or equal to {} characters long",
            rules.max
        ));
    }
    Ok(())
}

/// Joi's `uri()` wants an absolute URI with a scheme (RFC 3986).
fn is_uri(value: &str) -> bool {
    let scheme_ok = value.find(':').is_some_and(|end| {
        let scheme = &value[..end];
        scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    });
    scheme_ok
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && (Url::parse(value).is_ok() || !value.contains("//"))
}

fn number(value: &Value, label: &str, min: Option<f64>) -> Result<f64, String> {
    let Some(found) = value.as_f64().filter(|_| value.is_number()) else {
        return Err(format!("\"{label}\" must be a number"));
    };
    if let Some(min) = min.filter(|min| found < *min) {
        return Err(format!(
            "\"{label}\" must be greater than or equal to {min}"
        ));
    }
    Ok(found)
}

fn one_of_numbers(value: &Value, label: &str, allowed: &[i64]) -> Check {
    let found = number(value, label, None)?;
    if allowed.iter().any(|allowed| *allowed as f64 == found) {
        return Ok(());
    }
    let list: Vec<String> = allowed.iter().map(i64::to_string).collect();
    Err(format!("\"{label}\" must be one of [{}]", list.join(", ")))
}

fn array<'a>(
    value: &'a Value,
    label: &str,
    min: usize,
    max: usize,
) -> Result<&'a Vec<Value>, String> {
    let Some(items) = value.as_array() else {
        return Err(format!("\"{label}\" must be an array"));
    };
    if items.len() < min {
        return Err(format!("\"{label}\" must contain at least {min} items"));
    }
    if items.len() > max {
        return Err(format!(
            "\"{label}\" must contain less than or equal to {max} items"
        ));
    }
    Ok(items)
}

/// Validates the `args` of a `SET_ACTIVITY` request. `None` is an absent `args`.
pub fn set_activity(args: Option<&Value>) -> Check {
    let Some(args) = args else {
        return Err("\"value\" is required".into());
    };
    let args = object(args, "value")?;
    if let Some(pid) = args.get("pid") {
        number(pid, "pid", Some(0.0))?;
    }
    match args.get("activity") {
        None | Some(Value::Null) => Ok(()),
        Some(activity) => activity_object(activity),
    }
}

fn optional<T>(
    map: &Map<String, Value>,
    key: &str,
    check: impl FnOnce(&Value) -> Result<T, String>,
) -> Check {
    match map.get(key) {
        Some(value) => check(value).map(|_| ()),
        None => Ok(()),
    }
}

fn secret(inner: &Map<String, Value>, label: &str, key: &str) -> Check {
    optional(inner, key, |v| {
        text(
            v,
            &join(label, key),
            Text {
                min: 2,
                max: 128,
                uri: false,
            },
        )
    })
}

fn activity_object(activity: &Value) -> Check {
    let p = "activity";
    let map = object(activity, p)?;
    let string = |key: &str, min, max, uri| {
        optional(map, key, |value| {
            text(value, &join(p, key), Text { min, max, uri })
        })
    };
    string("name", 1, 128, false)?;
    string("state", 2, 128, false)?;
    string("state_url", 1, 256, true)?;
    string("details", 2, 128, false)?;
    string("details_url", 1, 256, true)?;
    optional(map, "timestamps", |value| {
        let label = join(p, "timestamps");
        let inner = object(value, &label)?;
        for key in ["start", "end"] {
            optional(inner, key, |v| number(v, &join(&label, key), Some(1.0)))?;
        }
        Ok(())
    })?;
    optional(map, "assets", |value| {
        let label = join(p, "assets");
        let inner = object(value, &label)?;
        let rules = [
            ("large_image", 1, 300, false),
            ("large_text", 2, 128, false),
            ("large_url", 1, 256, true),
            ("small_image", 1, 300, false),
            ("small_text", 2, 128, false),
            ("small_url", 1, 256, true),
            ("invite_cover_image", 1, 300, false),
        ];
        for (key, min, max, uri) in rules {
            optional(inner, key, |v| {
                text(v, &join(&label, key), Text { min, max, uri })
            })?;
        }
        Ok(())
    })?;
    optional(map, "party", |value| {
        let label = join(p, "party");
        let inner = object(value, &label)?;
        secret(inner, &label, "id")?;
        optional(inner, "size", |v| {
            let size_label = join(&label, "size");
            let items = v
                .as_array()
                .ok_or_else(|| format!("\"{size_label}\" must be an array"))?;
            for (index, item) in items.iter().enumerate() {
                number(item, &format!("{size_label}[{index}]"), Some(0.0))?;
            }
            if items.len() != 2 {
                return Err(format!("\"{size_label}\" must contain 2 items"));
            }
            Ok(())
        })?;
        optional(inner, "privacy", |v| {
            one_of_numbers(v, &join(&label, "privacy"), &[0, 1])
        })
    })?;
    optional(map, "secrets", |value| {
        let label = join(p, "secrets");
        let inner = object(value, &label)?;
        for key in ["match", "join", "spectate"] {
            secret(inner, &label, key)?;
        }
        Ok(())
    })?;
    optional(map, "buttons", |value| {
        let label = join(p, "buttons");
        let items = array(value, &label, 1, 2)?;
        for (index, item) in items.iter().enumerate() {
            let item_label = format!("{label}[{index}]");
            let inner = object(item, &item_label)?;
            for (key, max, uri) in [("label", 32, false), ("url", 512, true)] {
                let Some(found) = inner.get(key) else {
                    return Err(format!("\"{item_label}.{key}\" is required"));
                };
                text(found, &join(&item_label, key), Text { min: 1, max, uri })?;
            }
        }
        Ok(())
    })?;
    optional(map, "instance", |value| {
        if value.is_boolean() {
            Ok(())
        } else {
            Err(format!("\"{}\" must be a boolean", join(p, "instance")))
        }
    })?;
    optional(map, "supported_platforms", |value| {
        let label = join(p, "supported_platforms");
        let items = array(value, &label, 1, 10)?;
        for (index, item) in items.iter().enumerate() {
            let item_label = format!("{label}[{index}]");
            if !item.is_string() {
                return Err(format!("\"{item_label}\" must be a string"));
            }
            if !item.as_str().is_some_and(|name| PLATFORMS.contains(&name)) {
                return Err(format!(
                    "\"{item_label}\" must be one of [{}]",
                    PLATFORMS.join(", ")
                ));
            }
        }
        Ok(())
    })?;
    optional(map, "type", |value| {
        one_of_numbers(value, &join(p, "type"), TYPES)
    })?;
    optional(map, "status_display_type", |value| {
        one_of_numbers(value, &join(p, "status_display_type"), STATUS_DISPLAY_TYPES)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn check(activity: Value) -> Check {
        set_activity(Some(&json!({"pid": 4, "activity": activity})))
    }

    #[test]
    fn the_example_activity_of_a_real_game_is_valid() {
        assert_eq!(
            check(json!({
                "details": "In the menus",
                "state": "New Game+ 4/4",
                "timestamps": {"start": 1794312327},
                "assets": {"large_image": "pt", "large_text": "P.T. PC Port"},
            })),
            Ok(())
        );
    }

    #[test]
    fn a_missing_object_a_wrong_type_and_a_negative_pid_are_refused() {
        assert_eq!(set_activity(None), Err("\"value\" is required".into()));
        assert_eq!(
            set_activity(Some(&json!(3))),
            Err("\"value\" must be of type object".into())
        );
        assert_eq!(
            set_activity(Some(&json!({"pid": "4"}))),
            Err("\"pid\" must be a number".into())
        );
        assert_eq!(
            set_activity(Some(&json!({"pid": -1}))),
            Err("\"pid\" must be greater than or equal to 0".into())
        );
        assert_eq!(set_activity(Some(&json!({"activity": null}))), Ok(()));
        assert_eq!(set_activity(Some(&json!({"pid": 0}))), Ok(()));
    }

    #[test]
    fn string_limits_are_the_official_ones() {
        assert_eq!(
            check(json!({"state": "a"})),
            Err("\"activity.state\" length must be at least 2 characters long".into())
        );
        assert_eq!(
            check(json!({"details": "x".repeat(129)})),
            Err(
                "\"activity.details\" length must be less than or equal to 128 characters long"
                    .into()
            )
        );
        assert_eq!(
            check(json!({"name": ""})),
            Err("\"activity.name\" is not allowed to be empty".into())
        );
        assert_eq!(check(json!({"name": "x".repeat(128)})), Ok(()));
        assert_eq!(
            check(json!({"assets": {"large_image": "x".repeat(301)}})),
            Err("\"activity.assets.large_image\" length must be less than or equal to 300 characters long".into())
        );
        assert_eq!(
            check(json!({"state": 5})),
            Err("\"activity.state\" must be a string".into())
        );
    }

    #[test]
    fn two_utf16_units_satisfy_the_two_character_minimum() {
        assert_eq!(check(json!({"state": "😀"})), Ok(()));
    }

    #[test]
    fn urls_need_a_scheme() {
        assert_eq!(
            check(json!({"state_url": "not a url"})),
            Err("\"activity.state_url\" must be a valid uri".into())
        );
        assert_eq!(
            check(json!({"details_url": "https://example.com/x"})),
            Ok(())
        );
    }

    #[test]
    fn timestamps_party_and_buttons_follow_their_limits() {
        assert_eq!(
            check(json!({"timestamps": {"start": 0}})),
            Err("\"activity.timestamps.start\" must be greater than or equal to 1".into())
        );
        assert_eq!(
            check(json!({"party": {"id": "ab", "size": [1]}})),
            Err("\"activity.party.size\" must contain 2 items".into())
        );
        assert_eq!(
            check(json!({"party": {"id": "ab", "size": [1, 4], "privacy": 1}})),
            Ok(())
        );
        assert_eq!(
            check(json!({"party": {"privacy": 2}})),
            Err("\"activity.party.privacy\" must be one of [0, 1]".into())
        );
        assert_eq!(
            check(json!({"buttons": []})),
            Err("\"activity.buttons\" must contain at least 1 items".into())
        );
        let button = json!({"label": "a", "url": "https://a.b"});
        assert_eq!(
            check(json!({"buttons": [button, button, button]})),
            Err("\"activity.buttons\" must contain less than or equal to 2 items".into())
        );
        assert_eq!(
            check(json!({"buttons": [{"label": "a"}]})),
            Err("\"activity.buttons[0].url\" is required".into())
        );
        assert_eq!(check(json!({"buttons": [button]})), Ok(()));
    }

    #[test]
    fn type_and_platforms_take_only_listed_values() {
        assert_eq!(
            check(json!({"type": 1})),
            Err("\"activity.type\" must be one of [0, 2, 3, 5]".into())
        );
        assert_eq!(check(json!({"type": 2})), Ok(()));
        assert_eq!(
            check(json!({"supported_platforms": ["toaster"]})),
            Err("\"activity.supported_platforms[0]\" must be one of [xbox, desktop, samsung, ios, android, ps4, ps5, embedded, meta_quest]".into())
        );
        assert_eq!(
            check(json!({"status_display_type": 3})),
            Err("\"activity.status_display_type\" must be one of [0, 1, 2]".into())
        );
    }

    #[test]
    fn unknown_keys_are_allowed_inside_objects() {
        assert_eq!(
            set_activity(Some(&json!({"extra": 1, "activity": {"x": 1}}))),
            Ok(())
        );
    }
}
