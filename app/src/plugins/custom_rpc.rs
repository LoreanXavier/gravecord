//! CustomRPC's activity and form checks (Vencord `src/plugins/customRPC/index.tsx` and `RpcSettings.tsx`,
//! GPL-3.0-or-later): `createActivity` from the plugin's values (the pictures' keys already turned into asset ids), and
//! the form's validators with their messages.

use serde_json::{Map, Value, json};

/// `ActivityType`.
pub(crate) const PLAYING: u64 = 0;
pub(crate) const STREAMING: u64 = 1;

/// `TimestampMode`.
pub(crate) const NOW: u64 = 1;
pub(crate) const TIME: u64 = 2;
pub(crate) const CUSTOM: u64 = 3;

/// What `createActivity` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Config {
    pub app_id: String,
    pub app_name: String,
    pub details: String,
    pub details_url: String,
    pub state: String,
    pub state_url: String,
    pub kind: u64,
    pub stream_link: String,
    pub timestamp_mode: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub image_big: String,
    pub image_big_url: String,
    pub image_big_tooltip: String,
    pub image_small: String,
    pub image_small_url: String,
    pub image_small_tooltip: String,
    pub button_one_text: String,
    pub button_one_url: String,
    pub button_two_text: String,
    pub button_two_url: String,
    pub party_size: u64,
    pub party_max_size: u64,
}

/// `createActivity`: none without a name. It reads no clock of its own, so the same form makes the same activity
/// at every frame: `opened_ms` is when the activity was first made (`NOW`: `Date.now()` there), `local_midnight_ms`
/// the start of today (`TIME`), `loop_anchor` the custom loop's start; `big` and `small` the asset ids of the two
/// pictures (an empty id: none could be fetched).
pub(crate) fn activity(
    config: &Config,
    opened_ms: u64,
    local_midnight_ms: u64,
    loop_anchor: u64,
    big: Option<&str>,
    small: Option<&str>,
) -> Option<Value> {
    if config.app_name.is_empty() {
        return None;
    }
    let mut activity = Map::new();
    activity.insert(
        "application_id".into(),
        Value::from(if config.app_id.is_empty() {
            "0"
        } else {
            config.app_id.as_str()
        }),
    );
    activity.insert("name".into(), Value::from(config.app_name.clone()));
    activity.insert("state".into(), Value::from(config.state.clone()));
    activity.insert("details".into(), Value::from(config.details.clone()));
    activity.insert("type".into(), Value::from(config.kind));
    activity.insert("flags".into(), Value::from(1u64));
    if config.kind == STREAMING {
        activity.insert("url".into(), Value::from(config.stream_link.clone()));
    }
    match config.timestamp_mode {
        NOW => {
            activity.insert("timestamps".into(), json!({"start": opened_ms}));
        }
        // "Same as your current time": the start of today, so the elapsed time reads as the clock.
        TIME => {
            activity.insert("timestamps".into(), json!({"start": local_midnight_ms}));
        }
        CUSTOM if config.start_time != 0 || config.end_time != 0 => {
            let mut timestamps = Map::new();
            if config.start_time != 0 && config.end_time != 0 && config.end_time > config.start_time
            {
                timestamps.insert("start".into(), Value::from(loop_anchor));
                timestamps.insert(
                    "end".into(),
                    Value::from(loop_anchor + (config.end_time - config.start_time)),
                );
            } else {
                if config.start_time != 0 {
                    timestamps.insert("start".into(), Value::from(config.start_time));
                }
                if config.end_time != 0 {
                    timestamps.insert("end".into(), Value::from(config.end_time));
                }
            }
            activity.insert("timestamps".into(), Value::Object(timestamps));
        }
        _ => {}
    }
    if !config.details_url.is_empty() {
        activity.insert(
            "details_url".into(),
            Value::from(config.details_url.clone()),
        );
    }
    if !config.state_url.is_empty() {
        activity.insert("state_url".into(), Value::from(config.state_url.clone()));
    }
    if !config.button_one_text.is_empty() {
        let texts: Vec<&str> = [
            config.button_one_text.as_str(),
            config.button_two_text.as_str(),
        ]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect();
        let urls: Vec<&str> = [
            config.button_one_url.as_str(),
            config.button_two_url.as_str(),
        ]
        .into_iter()
        .filter(|url| !url.is_empty())
        .collect();
        activity.insert("buttons".into(), json!(texts));
        activity.insert("metadata".into(), json!({"button_urls": urls}));
    }
    let mut assets = Map::new();
    if !config.image_big.is_empty() {
        // A picture whose id could not be fetched (`getApplicationAsset` answered undefined) has no key at all.
        if big != Some("") {
            assets.insert(
                "large_image".into(),
                json!(big.unwrap_or(&config.image_big)),
            );
        }
        if !config.image_big_tooltip.is_empty() {
            assets.insert("large_text".into(), json!(config.image_big_tooltip));
        }
        if !config.image_big_url.is_empty() {
            assets.insert("large_url".into(), json!(config.image_big_url));
        }
    }
    if !config.image_small.is_empty() {
        if small != Some("") {
            assets.insert(
                "small_image".into(),
                json!(small.unwrap_or(&config.image_small)),
            );
        }
        if !config.image_small_tooltip.is_empty() {
            assets.insert("small_text".into(), json!(config.image_small_tooltip));
        }
        if !config.image_small_url.is_empty() {
            assets.insert("small_url".into(), json!(config.image_small_url));
        }
    }
    if !assets.is_empty() {
        activity.insert("assets".into(), Value::Object(assets));
    }
    if config.party_max_size != 0 && config.party_size != 0 {
        activity.insert(
            "party".into(),
            json!({"size": [config.party_size, config.party_max_size]}),
        );
    }
    // `if (!v || v.length === 0) delete activity[k]`, the type kept.
    activity.retain(|key, value| {
        key == "type"
            || !matches!(value, Value::Null)
                && value != &Value::from("")
                && value != &Value::from(0u64)
                && value.as_array().is_none_or(|list| !list.is_empty())
    });
    Some(Value::Object(activity))
}

/// `makeValidator(maxLength, isRequired)`.
pub(crate) fn length_error(value: &str, max: usize, required: bool) -> Option<String> {
    if required && value.is_empty() {
        return Some("This field is required.".to_owned());
    }
    (value.encode_utf16().count() > max)
        .then(|| format!("Must be not longer than {max} characters."))
}

/// `isAppIdValid`.
pub(crate) fn app_id_error(value: &str) -> Option<String> {
    let digits = value.len();
    (!(16..=21).contains(&digits) || !value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| "Must be a valid Discord ID.".to_owned())
}

/// `isStreamLinkValid`.
pub(crate) fn stream_link_error(value: &str, streaming: bool) -> Option<String> {
    let link = regex::Regex::new(r"https?://(www\.)?(twitch\.tv|youtube\.com)/\w+").ok()?;
    if streaming && !link.is_match(value) {
        return Some("Streaming link must be a valid URL.".to_owned());
    }
    (!value.is_empty() && value.encode_utf16().count() > 512)
        .then(|| "Streaming link must be not longer than 512 characters.".to_owned())
}

/// `parseNumber` with `isNumberValid`: the number, or the message.
pub(crate) fn number(value: &str) -> Result<u64, String> {
    if value.is_empty() {
        return Ok(0);
    }
    // `parseInt(value, 10)`: the leading digits, a sign allowed.
    let trimmed = value.trim_start();
    let (negative, rest) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err("Must be a number.".to_owned());
    }
    if negative && !digits.trim_start_matches('0').is_empty() {
        return Err("Must be a positive number.".to_owned());
    }
    digits
        .parse::<u64>()
        .map_err(|_| "Must be a number.".to_owned())
}

/// `isUrlValid`.
pub(crate) fn url_error(value: &str) -> Option<String> {
    let url = regex::Regex::new(r"^https?://.+").ok()?;
    (!value.is_empty() && !url.is_match(value)).then(|| "Must be a valid URL.".to_owned())
}

/// `isImageKeyValid`.
pub(crate) fn image_key_error(value: &str) -> Option<String> {
    let discord = regex::Regex::new(r"https?://(cdn|media)\.discordapp\.(com|net)/").ok()?;
    if discord.is_match(value) {
        return Some("Don't use a Discord link. Use an Imgur image link instead.".to_owned());
    }
    // `(?!i\.)?imgur\.com`: an optional lookahead matches nothing, so only `imgur.com` right after the scheme counts.
    if regex::Regex::new(r"https?://imgur\.com/")
        .ok()?
        .is_match(value)
    {
        return Some(
            "Imgur link must be a direct link to the image (e.g. https://i.imgur.com/...). Right click the image and click 'Copy image address'"
                .to_owned(),
        );
    }
    if regex::Regex::new(r"https?://tenor\.com/")
        .ok()?
        .is_match(value)
    {
        return Some(
            "Tenor link must be a direct link to the image (e.g. https://media.tenor.com/...). Right click the GIF and click 'Copy image address'"
                .to_owned(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activities_read_as_create_activity_builds_them() {
        let mut config = Config::default();
        assert!(activity(&config, 1, 0, 0, None, None).is_none());
        config.app_name = "Game".into();
        config.details = "line".into();
        config.button_one_text = "Go".into();
        config.button_one_url = "https://a".into();
        config.timestamp_mode = NOW;
        let made = activity(&config, 50, 0, 0, None, None).unwrap();
        assert_eq!(made["application_id"], "0");
        assert_eq!(made["type"], 0);
        assert!(made.get("state").is_none());
        assert_eq!(made["timestamps"]["start"], 50);
        assert_eq!(made["buttons"], json!(["Go"]));
        config.timestamp_mode = CUSTOM;
        config.start_time = 10;
        config.end_time = 30;
        let looped = activity(&config, 50, 0, 1000, None, None).unwrap();
        assert_eq!(looped["timestamps"], json!({"start": 1000, "end": 1020}));
        config.image_big = "key".into();
        let pictured = activity(&config, 50, 0, 1000, Some("123"), None).unwrap();
        assert_eq!(pictured["assets"]["large_image"], "123");
        config.image_big_tooltip = "tip".into();
        let missing = activity(&config, 50, 0, 1000, Some(""), None).unwrap();
        assert!(missing["assets"].get("large_image").is_none());
        assert_eq!(missing["assets"]["large_text"], "tip");
    }

    #[test]
    fn checks_read_as_the_form_writes_them() {
        assert_eq!(
            length_error("", 128, true).as_deref(),
            Some("This field is required.")
        );
        assert!(app_id_error("1234567890123456").is_none());
        assert!(app_id_error("12a").is_some());
        assert!(stream_link_error("https://twitch.tv/me", true).is_none());
        assert!(stream_link_error("nope", true).is_some());
        assert!(stream_link_error("nope", false).is_none());
        assert_eq!(number("42"), Ok(42));
        assert!(number("-1").is_err());
        assert!(number("x").is_err());
        assert!(url_error("ftp://a").is_some());
        assert!(url_error("").is_none());
        assert!(image_key_error("https://cdn.discordapp.com/x").is_some());
        assert!(image_key_error("https://imgur.com/x").is_some());
        assert!(image_key_error("https://i.imgur.com/x.png").is_none());
        assert!(image_key_error("https://tenor.com/view/x").is_some());
        assert!(image_key_error("key").is_none());
    }
}
