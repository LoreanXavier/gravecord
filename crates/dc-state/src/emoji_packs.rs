//! Nitro emoji packs (`EmojiPackStore`, module 956396; actions in 934499).
//! Populated from `GET /emoji-packs` and toggled via `2026-09-default-emojis`.

use serde_json::Value;
use std::collections::HashSet;

/// Experiment gating Nitro emoji packs (`490035.js`): variation `1` enables `enabled: true`.
pub const EXPERIMENT: &str = "2026-09-default-emojis";

/// Whether `2026-09-default-emojis` enables Nitro emoji packs for this account (`490035.js`).
pub fn experiment_enabled(variant: Option<i64>) -> bool {
    variant == Some(1)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FetchState {
    #[default]
    NotFetched,
    Fetching,
    Fetched,
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackEmoji {
    pub id: String,
    pub pack_id: String,
    pub name: String,
    pub animated: bool,
    pub require_colons: bool,
    pub available: bool,
    pub managed: bool,
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmojiPack {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub available: bool,
    pub representative_emoji_id: Option<String>,
    pub emojis: Vec<PackEmoji>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmojiPackStore {
    pub packs: Vec<EmojiPack>,
    pub fetch_state: FetchState,
    pub pinned_pack_ids: Vec<String>,
}

impl EmojiPackStore {
    pub fn start_fetch(&mut self) -> bool {
        if self.fetch_state == FetchState::Fetching {
            return false;
        }
        self.fetch_state = FetchState::Fetching;
        true
    }

    pub fn fail_fetch(&mut self) {
        self.fetch_state = FetchState::Failed;
    }

    pub fn clear(&mut self) {
        self.packs.clear();
        self.fetch_state = FetchState::NotFetched;
    }

    /// Applies the `Request::Rest` response wrapper (or direct JSON array) from `GET /emoji-packs` (`934499.js`).
    /// HTTP 404 yields an empty fetched pack list; other errors transition to `FetchState::Failed`.
    pub fn apply_rest_response(&mut self, response: &Value) {
        if response.is_array() {
            self.apply_packs(response);
            return;
        }
        if response["ok"].as_bool() == Some(true) {
            self.apply_packs(&response["body"]);
        } else if response["status"].as_u64() == Some(404) {
            self.packs.clear();
            self.fetch_state = FetchState::Fetched;
        } else {
            self.fail_fetch();
        }
    }

    /// Applies the JSON body returned by `GET /emoji-packs` (`EMOJI_PACKS_FETCH_SUCCESS`).
    pub fn apply_packs(&mut self, value: &Value) {
        let Some(items) = value.as_array() else {
            self.packs.clear();
            self.fetch_state = FetchState::Fetched;
            return;
        };
        let mut seen = HashSet::new();
        let mut packs = Vec::with_capacity(items.len());
        for item in items {
            let Some(pack_id) = item["id"].as_str().map(str::to_owned) else {
                continue;
            };
            if !seen.insert(pack_id.clone()) {
                continue;
            }
            let Some(name) = item["name"].as_str().map(str::to_owned) else {
                continue;
            };
            let emojis = item["emojis"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|emoji| {
                    let id = emoji["id"].as_str()?.to_owned();
                    let name = emoji["name"].as_str()?.to_owned();
                    Some(PackEmoji {
                        id,
                        pack_id: pack_id.clone(),
                        name,
                        animated: emoji["animated"].as_bool().unwrap_or(false),
                        require_colons: emoji["require_colons"].as_bool().unwrap_or(true),
                        available: emoji["available"].as_bool().unwrap_or(true),
                        managed: emoji["managed"].as_bool().unwrap_or(false),
                        roles: emoji["roles"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| r.as_str().map(str::to_owned))
                            .collect(),
                    })
                })
                .collect();
            packs.push(EmojiPack {
                id: pack_id,
                name,
                description: item["description"].as_str().map(str::to_owned),
                available: item["available"].as_bool().unwrap_or(true),
                representative_emoji_id: item["representative_emoji_id"]
                    .as_str()
                    .map(str::to_owned),
                emojis,
            });
        }
        self.packs = packs;
        self.fetch_state = FetchState::Fetched;
    }

    pub fn pack(&self, id: &str) -> Option<&EmojiPack> {
        self.packs.iter().find(|pack| pack.id == id)
    }

    pub fn pack_for_emoji(&self, emoji_id: &str) -> Option<&EmojiPack> {
        self.packs
            .iter()
            .find(|pack| pack.emojis.iter().any(|emoji| emoji.id == emoji_id))
    }

    pub fn emoji(&self, emoji_id: &str) -> Option<&PackEmoji> {
        self.pack_for_emoji(emoji_id)?
            .emojis
            .iter()
            .find(|emoji| emoji.id == emoji_id)
    }

    /// Available pack emojis when the `2026-09-default-emojis` experiment is enabled (`getIndexablePackEmojis`).
    pub fn indexable_pack_emojis(&self, enabled: bool) -> Vec<&PackEmoji> {
        if !enabled {
            return Vec::new();
        }
        self.packs
            .iter()
            .filter(|pack| pack.available)
            .flat_map(|pack| pack.emojis.iter().filter(|emoji| emoji.available))
            .collect()
    }

    /// Whether a pack emoji is usable under the active experiment state (`isPackEmojiUsable`).
    pub fn is_pack_emoji_usable(&self, emoji_id: &str, enabled: bool) -> bool {
        if !enabled {
            return false;
        }
        self.pack_for_emoji(emoji_id).is_some_and(|pack| {
            pack.available
                && pack
                    .emojis
                    .iter()
                    .any(|emoji| emoji.id == emoji_id && emoji.available)
        })
    }

    /// Pinned available packs in user pin order (`200589.js` / `920134.js`).
    pub fn pinned_packs(&self) -> Vec<&EmojiPack> {
        self.pinned_pack_ids
            .iter()
            .filter_map(|id| self.pack(id).filter(|pack| pack.available))
            .collect()
    }

    pub fn apply_pinned_ids(&mut self, ids: Vec<String>) {
        self.pinned_pack_ids = ids;
    }

    pub fn is_pinned(&self, pack_id: &str) -> bool {
        self.pinned_pack_ids.iter().any(|id| id == pack_id)
    }

    pub fn pin(&mut self, pack_id: &str) -> bool {
        if self.pack(pack_id).is_none_or(|pack| !pack.available) || self.is_pinned(pack_id) {
            return false;
        }
        if self.fetch_state == FetchState::Fetched {
            let packs = &self.packs;
            self.pinned_pack_ids.retain(|id| {
                packs
                    .iter()
                    .find(|pack| pack.id == *id)
                    .is_some_and(|pack| pack.available)
            });
        }
        self.pinned_pack_ids.push(pack_id.to_owned());
        true
    }

    pub fn unpin(&mut self, pack_id: &str) -> bool {
        let before = self.pinned_pack_ids.len();
        self.pinned_pack_ids.retain(|id| id != pack_id);
        self.pinned_pack_ids.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_payload() -> Value {
        json!([
            {
                "id": "pack_1",
                "name": "Wumpus Wonders",
                "description": "Classic Wumpus reactions",
                "available": true,
                "representative_emoji_id": "101",
                "emojis": [
                    {
                        "id": "101",
                        "name": "wumpus_wave",
                        "animated": false,
                        "require_colons": true,
                        "available": true,
                        "managed": false,
                        "roles": []
                    },
                    {
                        "id": "102",
                        "name": "wumpus_unavailable",
                        "animated": true,
                        "require_colons": true,
                        "available": false,
                        "managed": false,
                        "roles": []
                    }
                ]
            },
            {
                "id": "pack_1",
                "name": "Duplicate Should Skip",
                "available": true,
                "emojis": []
            },
            {
                "id": "pack_2",
                "name": "Retired Pack",
                "available": false,
                "emojis": [
                    {
                        "id": "201",
                        "name": "retired",
                        "animated": false,
                        "available": true
                    }
                ]
            }
        ])
    }

    #[test]
    fn apply_packs_dedupes_indexes_and_handles_404_vs_errors() {
        let mut store = EmojiPackStore::default();
        assert_eq!(store.fetch_state, FetchState::NotFetched);
        assert!(store.start_fetch());
        assert!(!store.start_fetch());

        store.apply_rest_response(&json!({"ok": true, "status": 200, "body": sample_payload()}));
        assert_eq!(store.fetch_state, FetchState::Fetched);
        assert_eq!(store.packs.len(), 2);
        assert_eq!(store.pack("pack_1").unwrap().name, "Wumpus Wonders");
        assert_eq!(store.pack_for_emoji("101").unwrap().id, "pack_1");
        assert_eq!(store.emoji("101").unwrap().name, "wumpus_wave");
        assert!(store.emoji("missing").is_none());

        // 404 yields an empty fetched list (module 934499).
        store.apply_rest_response(&json!({"ok": false, "status": 404, "body": {"message": "404"}}));
        assert_eq!(store.fetch_state, FetchState::Fetched);
        assert!(store.packs.is_empty());

        // 500 marks the store failed.
        store.apply_rest_response(&json!({"ok": false, "status": 500, "body": null}));
        assert_eq!(store.fetch_state, FetchState::Failed);

        store.clear();
        assert_eq!(store.fetch_state, FetchState::NotFetched);
        assert!(store.packs.is_empty());
    }

    #[test]
    fn pin_and_unpin_respect_availability_and_experiment_gate() {
        assert!(!experiment_enabled(None));
        assert!(!experiment_enabled(Some(0)));
        assert!(experiment_enabled(Some(1)));

        let mut store = EmojiPackStore::default();
        store.apply_packs(&sample_payload());
        store.apply_pinned_ids(vec!["pack_2".to_owned()]);

        assert!(!store.pin("missing"));
        assert!(!store.pin("pack_2"), "unavailable pack cannot be pinned");
        assert!(store.pin("pack_1"));
        assert!(!store.pin("pack_1"), "duplicate pin ignored");
        assert_eq!(store.pinned_pack_ids, vec!["pack_1".to_owned()]);
        assert_eq!(
            store
                .pinned_packs()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["pack_1"]
        );

        assert!(store.indexable_pack_emojis(false).is_empty());
        assert!(!store.is_pack_emoji_usable("101", false));
        let indexable = store.indexable_pack_emojis(true);
        assert_eq!(indexable.len(), 1);
        assert_eq!(indexable[0].id, "101");
        assert!(store.is_pack_emoji_usable("101", true));
        assert!(!store.is_pack_emoji_usable("102", true));
        assert!(!store.is_pack_emoji_usable("201", true));

        assert!(store.unpin("pack_1"));
        assert!(!store.unpin("pack_1"));
        assert!(store.pinned_packs().is_empty());
    }
}
