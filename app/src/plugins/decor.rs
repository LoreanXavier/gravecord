//! Decor (Vencord `src/plugins/decor`, GPL-3.0-or-later): the service's addresses and answers, and the cache of who
//! wears which decoration. The drawing and the dialogs are in `ui/plugin_hooks/decor.rs`. Not part of the pinned
//! client.

use dc_state::Id;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

/// `settings.baseUrl`'s default.
pub(crate) const DEFAULT_BASE: &str = "https://decor.fieryflames.dev";
/// `CDN_URL` and `CLIENT_ID` until the service's `/api/config` says otherwise.
pub(crate) const DEFAULT_CDN: &str = "https://ugc.decor.fieryflames.dev";
pub(crate) const DEFAULT_CLIENT_ID: &str = "1096966363416899624";
/// The service's Discord server and its invite.
pub(crate) const GUILD_ID: Id = 1096357702931841148;
pub(crate) const INVITE_KEY: &str = "dXp2SdxDcP";
pub(crate) const GUIDELINES_URL: &str =
    "https://github.com/decor-discord/.github/blob/main/GUIDELINES.md";
/// `DECORATION_FETCH_COOLDOWN`: a person's decoration is asked for again after four hours at the soonest.
pub(crate) const FETCH_COOLDOWN: Duration = Duration::from_secs(4 * 60 * 60);
/// At most this many people in one `/users` call (the address carries their ids).
pub(crate) const BATCH: usize = 100;

/// The service's addresses (`setBaseUrl`): the API under the base, the CDN and the OAuth2 application from its config.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Config {
    pub base: String,
    pub cdn: String,
    pub client_id: String,
}

impl Config {
    pub(crate) fn new(base: &str) -> Self {
        let base = base.trim().trim_end_matches('/');
        Self {
            base: if base.is_empty() { DEFAULT_BASE } else { base }.to_owned(),
            cdn: DEFAULT_CDN.to_owned(),
            client_id: DEFAULT_CLIENT_ID.to_owned(),
        }
    }

    /// `/api/config`'s `CDN_URL` and `CLIENT_ID`; anything missing keeps what was there.
    pub(crate) fn apply(&mut self, answer: &Value) {
        if let Some(cdn) = answer["CDN_URL"].as_str().filter(|cdn| !cdn.is_empty()) {
            cdn.trim_end_matches('/').clone_into(&mut self.cdn);
        }
        match &answer["CLIENT_ID"] {
            Value::String(id) if !id.is_empty() => id.clone_into(&mut self.client_id),
            Value::Number(id) => self.client_id = id.to_string(),
            _ => {}
        }
    }

    pub(crate) fn api(&self) -> String {
        format!("{}/api", self.base)
    }

    pub(crate) fn config_url(&self) -> String {
        format!("{}/api/config", self.base)
    }

    /// `AUTHORIZE_URL`: the redirect of the OAuth2 grant, which also trades its code for the service's token.
    pub(crate) fn authorize_url(&self) -> String {
        format!("{}/api/authorize", self.base)
    }

    /// `getUsersDecorations(ids)`: `/users?ids=["…",…]`.
    pub(crate) fn users_url(&self, ids: &[Id]) -> String {
        let list = Value::from(ids.iter().map(Id::to_string).collect::<Vec<_>>()).to_string();
        let mut url = url::Url::parse(&format!("{}/users", self.api()))
            .unwrap_or_else(|_| url::Url::parse("https://invalid.invalid/").expect("static"));
        url.query_pairs_mut().append_pair("ids", &list);
        url.to_string()
    }

    /// The token trade after the grant: the redirect with the code and `client=vencord`, answered with the token as
    /// text.
    pub(crate) fn token_url(&self, code: &str) -> String {
        let mut url = url::Url::parse(&self.authorize_url())
            .unwrap_or_else(|_| url::Url::parse("https://invalid.invalid/").expect("static"));
        url.query_pairs_mut()
            .append_pair("code", code)
            .append_pair("client", "vencord");
        url.to_string()
    }

    /// `getDecorAvatarDecorationURL`: the picture of an asset. A still one drops the `a_` of an animated asset.
    pub(crate) fn asset_url(&self, asset: &str, animate: bool) -> String {
        let shown = if animate {
            asset
        } else {
            asset.strip_prefix("a_").unwrap_or(asset)
        };
        format!("{}/{shown}.png", self.cdn)
    }
}

/// An id as the API writes it: a string, or a number.
pub(crate) fn id_of(value: &Value) -> Option<Id> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

/// `Decoration` of the API.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Decoration {
    pub hash: String,
    pub animated: bool,
    pub alt: Option<String>,
    pub author_id: Option<Id>,
    /// `false` while it waits for review.
    pub reviewed: Option<bool>,
    pub preset_id: Option<String>,
}

impl Decoration {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            hash: value["hash"].as_str()?.to_owned(),
            animated: value["animated"].as_bool().unwrap_or(false),
            alt: value["alt"].as_str().map(str::to_owned),
            author_id: id_of(&value["authorId"]),
            reviewed: value["reviewed"].as_bool(),
            preset_id: value["presetId"].as_str().map(str::to_owned),
        })
    }

    /// `decorationToAsset`.
    pub(crate) fn asset(&self) -> String {
        format!("{}{}", if self.animated { "a_" } else { "" }, self.hash)
    }

    pub(crate) fn pending(&self) -> bool {
        self.reviewed == Some(false)
    }
}

/// `Preset` of the API.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preset {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub decorations: Vec<Decoration>,
    pub author_ids: Vec<Id>,
}

pub(crate) fn decorations(answer: &Value) -> Vec<Decoration> {
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Decoration::from_value)
        .collect()
}

pub(crate) fn presets(answer: &Value) -> Vec<Preset> {
    answer
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|preset| {
            Some(Preset {
                id: preset["id"].as_str()?.to_owned(),
                name: preset["name"].as_str().unwrap_or_default().to_owned(),
                description: preset["description"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
                decorations: decorations(&preset["decorations"]),
                author_ids: preset["authorIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(id_of)
                    .collect(),
            })
        })
        .collect()
}

/// `UsersDecorationsStore`: each person's asset (`None` for none) and when it was asked for, and the people waiting
/// for the next call.
#[derive(Debug, Default)]
pub(crate) struct Users {
    known: HashMap<Id, (Option<String>, Instant)>,
    queue: BTreeSet<Id>,
    /// Asked for and not answered yet: not queued again meanwhile.
    asked: BTreeSet<Id>,
}

impl Users {
    /// `fetch(userId, force)`: queues the person unless their answer is younger than the cooldown or already on its
    /// way. True when queued.
    pub(crate) fn want(&mut self, user: Id, now: Instant, force: bool) -> bool {
        if self.asked.contains(&user) || self.queue.contains(&user) {
            return false;
        }
        if !force
            && self
                .known
                .get(&user)
                .is_some_and(|(_, at)| now.saturating_duration_since(*at) < FETCH_COOLDOWN)
        {
            return false;
        }
        self.queue.insert(user)
    }

    /// The next call's people (`bulkFetch`), at most [`BATCH`].
    pub(crate) fn take_batch(&mut self) -> Vec<Id> {
        let batch: Vec<Id> = self.queue.iter().copied().take(BATCH).collect();
        for user in &batch {
            self.queue.remove(user);
            self.asked.insert(*user);
        }
        batch
    }

    /// The answer of a call for `ids` (`{id: asset | null}`); a person it leaves out has none. A failed call
    /// (`None`) leaves what was known and counts as none for the rest, so a dead service is asked again only after
    /// the cooldown.
    pub(crate) fn answered(&mut self, ids: &[Id], answer: Option<&Value>, now: Instant) {
        // Everyone the answer names counts too (an answer may come in after its call was given up on).
        let named = answer
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(id, _)| id.parse::<Id>().ok());
        let ids: BTreeSet<Id> = ids.iter().copied().chain(named).collect();
        for user in &ids {
            self.asked.remove(user);
            // A failed call changes nothing it knew (the original's store keeps its state when the call throws).
            if answer.is_none() && self.known.contains_key(user) {
                continue;
            }
            let asset = answer
                .and_then(|answer| answer[user.to_string()].as_str())
                .filter(|asset| !asset.is_empty())
                .map(str::to_owned);
            self.known.insert(*user, (asset, now));
        }
    }

    /// `set(userId, decoration)`: the account's own choice, at once.
    pub(crate) fn set(&mut self, user: Id, asset: Option<String>, now: Instant) {
        self.known.insert(user, (asset, now));
    }

    pub(crate) fn asset(&self, user: Id) -> Option<&str> {
        self.known.get(&user)?.0.as_deref()
    }

    pub(crate) fn has_queue(&self) -> bool {
        !self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decor_addresses_and_assets() {
        let mut config = Config::new("https://decor.example/");
        assert_eq!(config.api(), "https://decor.example/api");
        assert_eq!(
            config.users_url(&[1, 22]),
            "https://decor.example/api/users?ids=%5B%221%22%2C%2222%22%5D"
        );
        assert_eq!(
            config.token_url("a b"),
            "https://decor.example/api/authorize?code=a+b&client=vencord"
        );
        config.apply(&json!({"CDN_URL": "https://cdn.example/", "CLIENT_ID": "9"}));
        assert_eq!(config.client_id, "9");
        assert_eq!(
            config.asset_url("a_abc", false),
            "https://cdn.example/abc.png"
        );
        assert_eq!(
            config.asset_url("a_abc", true),
            "https://cdn.example/a_abc.png"
        );
        assert_eq!(Config::new("").base, DEFAULT_BASE);
        let decoration = Decoration::from_value(
            &json!({"hash": "h", "animated": true, "alt": "Cube", "authorId": "5", "reviewed": false, "presetId": null}),
        )
        .unwrap();
        assert_eq!(decoration.asset(), "a_h");
        assert!(decoration.pending());
        assert_eq!(decoration.author_id, Some(5));
        let presets = presets(&json!([{"id": "p", "name": "Cats", "description": "",
            "decorations": [{"hash": "x", "animated": false}], "authorIds": ["7"]}]));
        assert_eq!(presets[0].description, None);
        assert_eq!(presets[0].decorations[0].asset(), "x");
        assert_eq!(presets[0].author_ids, [7]);
    }

    #[test]
    fn decor_users_are_asked_once_per_cooldown() {
        let mut users = Users::default();
        let now = Instant::now();
        assert!(users.want(1, now, false));
        assert!(!users.want(1, now, false));
        assert!(users.want(2, now, false));
        assert_eq!(users.take_batch(), [1, 2]);
        // On its way: not queued again.
        assert!(!users.want(1, now, true));
        users.answered(&[1, 2], Some(&json!({"1": "a_h", "2": null})), now);
        assert_eq!(users.asset(1), Some("a_h"));
        assert_eq!(users.asset(2), None);
        assert!(!users.want(1, now + Duration::from_secs(60), false));
        assert!(users.want(1, now + FETCH_COOLDOWN, false));
        assert_eq!(users.take_batch(), [1]);
        users.answered(&[1], None, now);
        assert_eq!(users.asset(1), Some("a_h"));
        users.answered(&[3], None, now);
        assert_eq!(users.asset(3), None);
        assert!(!users.want(3, now, false));
        assert!(users.want(1, now, true));
        let mut many = Users::default();
        for user in 0..(BATCH as u64 + 5) {
            many.want(user, now, false);
        }
        assert_eq!(many.take_batch().len(), BATCH);
        assert!(many.has_queue());
    }
}
