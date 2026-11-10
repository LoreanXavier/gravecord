//! Profile effects: which effect a profile carries, and the layer config that draws it. The effect of a user comes
//! from the profile payload's `collectibles` list (module 851767, function `O`); its config is the item of the
//! collectibles product of that sku (`getProduct` of `CollectiblesCategoryStore`, module 590180, filled by `er` of
//! module 110676 from `GET /collectibles-products/<sku>`), read into the `ProfileEffect` class of module 203632.
//! Layer timing follows the layer component of module 971217.
//!
//! Wire shapes (recorded in `design/snapshot/TRAFFIC/nitro-store.json`, trimmed in
//! `tests/fixtures/profile_effect_product.json`):
//!
//! * profile: `user_profile.collectibles` and `guild_member_profile.collectibles`, each
//!   `[{ "sku_id": "<id>", "type": 1, "expires_at": "<rfc3339>" | null }]`; type 1 is a profile effect and the last
//!   one in the list wins.
//! * product: `{ "sku_id", "name", "type": 1, "items": [ <effect> ] }` with `<effect>` =
//!   `{ "type": 1, "sku_id", "title", "description", "accessibilityLabel", "animationType": 1|2,
//!   "thumbnailPreviewSrc", "reducedMotionSrc", "staticFrameSrc"?, "effects": [ <layer> ] }`.
//! * layer: `{ "src", "loop", "duration", "start", "loopDelay", "zIndex", "randomizedSources": [{ "src" }],
//!   "position"?: { "x", "y" }, "height"?, "width"? }`, times in milliseconds. Every `src` is a PNG (an APNG for the
//!   animated layers) under `cdn.discordapp.com/media/v1/collectibles-shop/<hash>`.
use crate::Id;
use chrono::DateTime;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `R.PROFILE_EFFECT` of module 575593.
pub const COLLECTIBLE_PROFILE_EFFECT: i64 = 1;

/// How long a failed product read is not repeated (`isProductFetchBackedOff` in module 590180 backs off per sku).
const FETCH_BACKOFF_MS: f64 = 60_000.;

/// `GET /collectibles-products/<sku>`: the locale is the only query the profile reads send (`er` of module 110676;
/// the payment gateway and bundle flags are left out when unset).
pub fn product_route(sku: Id) -> String {
    format!("/collectibles-products/{sku}?locale=en-US")
}

/// The effect a profile carries: its sku and when it lapses (seconds since the epoch, as module 851767 floors it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Equipped {
    pub sku_id: Id,
    pub expires_at: Option<i64>,
}

impl Equipped {
    /// The effect of one `collectibles` list: the last type-1 entry, unless it has lapsed.
    fn from_collectibles(list: &Value, now_ms: f64) -> Option<Self> {
        let mut found = None;
        for entry in list.as_array()? {
            if entry["type"].as_i64() == Some(COLLECTIBLE_PROFILE_EFFECT) {
                found = Some(Self {
                    sku_id: crate::id(&entry["sku_id"]).or_else(|| entry["sku_id"].as_u64())?,
                    expires_at: entry["expires_at"]
                        .as_str()
                        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                        .map(|time| time.timestamp()),
                });
            }
        }
        found.filter(|effect| {
            effect
                .expires_at
                .is_none_or(|end| (end as f64) * 1000. > now_ms)
        })
    }

    /// The effect a profile payload shows: the server profile's own effect when the viewer looks at that server
    /// (`t?.profileEffect ?? e.profileEffect` in the display profile of module 527212), else the account's.
    pub fn from_profile(body: &Value, guild: Option<Id>, now_ms: f64) -> Option<Self> {
        let member = &body["guild_member_profile"];
        let member_guild = member["guild_id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok());
        let scoped = guild
            .filter(|guild| Some(*guild) == member_guild)
            .and_then(|_| Self::from_collectibles(&member["collectibles"], now_ms));
        scoped.or_else(|| Self::from_collectibles(&body["user_profile"]["collectibles"], now_ms))
    }
}

/// `ANIMATION_TYPE_*` of module 631903.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationType {
    #[default]
    Unspecified,
    Persistent,
    Intermittent,
}

impl AnimationType {
    fn from_value(value: &Value) -> Self {
        match value.as_i64() {
            Some(1) => Self::Persistent,
            Some(2) => Self::Intermittent,
            _ => Self::Unspecified,
        }
    }
}

/// One layer of an effect (`u` of module 431955): an image that plays from `start` for `duration` and, when it
/// loops, again after each `loop_delay`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectLayer {
    pub src: String,
    pub randomized_sources: Vec<String>,
    pub looping: bool,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub loop_delay_ms: Option<f64>,
    pub z_index: i64,
    /// Offset of the image's top-left corner in CSS pixels (`position` of the layer; unset is 0, 0).
    pub position: (f64, f64),
    pub width: Option<f64>,
    pub height: Option<f64>,
}

impl EffectLayer {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            src: value["src"].as_str()?.to_owned(),
            randomized_sources: value["randomizedSources"]
                .as_array()
                .map(|sources| {
                    sources
                        .iter()
                        .filter_map(|source| source["src"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            looping: value["loop"].as_bool().unwrap_or(false),
            start_ms: value["start"].as_f64().unwrap_or(0.),
            duration_ms: value["duration"].as_f64().unwrap_or(0.),
            loop_delay_ms: value["loopDelay"].as_f64(),
            z_index: value["zIndex"].as_i64().unwrap_or(0),
            position: (
                value["position"]["x"].as_f64().unwrap_or(0.),
                value["position"]["y"].as_f64().unwrap_or(0.),
            ),
            width: value["width"].as_f64(),
            height: value["height"].as_f64(),
        })
    }

    /// Where the layer is in its own run at `time_ms` on the effect clock, or `None` while it is hidden (the layer
    /// component of module 971217): nothing before `start`, nothing after `start + duration` unless it loops, and a
    /// looping layer with a loop delay is hidden for that delay after each `duration`. The image restarts from its
    /// first frame at every run, which is what the component's blank-image swap does.
    pub fn local_time(&self, time_ms: f64) -> Option<f64> {
        if time_ms < self.start_ms {
            return None;
        }
        if !self.looping && time_ms > self.duration_ms + self.start_ms {
            return None;
        }
        let mut local = time_ms - self.start_ms;
        if self.looping
            && let Some(delay) = self.loop_delay_ms.filter(|delay| *delay > 0.)
        {
            let cycle = self.duration_ms + delay;
            local -= (local / cycle).floor() * cycle;
            if local > self.duration_ms {
                return None;
            }
        }
        Some(local)
    }

    /// The source this layer draws for the random pick of the effect (`b` of module 971217).
    pub fn source(&self, pick: usize) -> &str {
        self.randomized_sources
            .get(pick)
            .map_or(self.src.as_str(), String::as_str)
    }
}

/// A profile effect as the product carries it (`ProfileEffect`, module 203632).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileEffect {
    pub sku_id: Id,
    pub title: String,
    pub description: String,
    pub accessibility_label: String,
    pub animation_type: AnimationType,
    pub reduced_motion_src: String,
    pub thumbnail_preview_src: String,
    pub static_frame_src: Option<String>,
    /// The layers in paint order (ascending `zIndex`, as the component sorts them).
    pub layers: Vec<EffectLayer>,
}

impl ProfileEffect {
    /// One `items[]` entry of a collectibles product (`T7` of module 474012); `None` for any other item type.
    pub fn from_item(item: &Value) -> Option<Self> {
        if item["type"].as_i64() != Some(COLLECTIBLE_PROFILE_EFFECT) {
            return None;
        }
        let text = |key: &str| item[key].as_str().unwrap_or_default().to_owned();
        let mut layers: Vec<EffectLayer> = item["effects"]
            .as_array()
            .map(|layers| layers.iter().filter_map(EffectLayer::from_value).collect())
            .unwrap_or_default();
        layers.sort_by_key(|layer| layer.z_index);
        Some(Self {
            sku_id: crate::id(&item["sku_id"]).or_else(|| item["sku_id"].as_u64())?,
            title: text("title"),
            description: text("description"),
            accessibility_label: text("accessibilityLabel"),
            animation_type: AnimationType::from_value(&item["animationType"]),
            reduced_motion_src: text("reducedMotionSrc"),
            thumbnail_preview_src: text("thumbnailPreviewSrc"),
            static_frame_src: item["staticFrameSrc"].as_str().map(str::to_owned),
            layers,
        })
    }

    /// The effect of a `GET /collectibles-products/<sku>` body: its first item, which must be a profile effect
    /// (`getProduct(sku).items[0]` in module 942426).
    pub fn from_product(body: &Value) -> Option<Self> {
        Self::from_item(body["items"].as_array()?.first()?)
    }

    /// How many sources the random pick can choose between: the shortest non-empty `randomizedSources` list among the
    /// layers (`b` of module 971217); 0 when no layer randomises.
    pub fn pick_range(&self) -> usize {
        self.layers
            .iter()
            .map(|layer| layer.randomized_sources.len())
            .filter(|len| *len > 0)
            .min()
            .unwrap_or(0)
    }

    /// The end of the last layer run that does not loop, and the start of the first loop: the component's `loopEnd`
    /// and `firstLoop`.
    pub fn loop_bounds(&self) -> (f64, f64) {
        let mut end = 0.;
        let mut first_loop = f64::INFINITY;
        for layer in &self.layers {
            end = f64::max(end, layer.start_ms + layer.duration_ms);
            if layer.looping {
                first_loop = first_loop.min(layer.start_ms);
            }
        }
        (end, first_loop)
    }
}

/// Effect configs by sku (the product cache of `CollectiblesCategoryStore`, kept for effects only) and the reads
/// in flight.
#[derive(Default)]
pub struct ProfileEffectStore {
    effects: HashMap<Id, ProfileEffect>,
    fetching: HashSet<Id>,
    failed: HashMap<Id, f64>,
}

impl ProfileEffectStore {
    pub fn get(&self, sku: Id) -> Option<&ProfileEffect> {
        self.effects.get(&sku)
    }

    /// Whether the caller should send the product read for `sku` now: not known, not in flight, not backed off.
    pub fn begin_fetch(&mut self, sku: Id, now_ms: f64) -> bool {
        if self.effects.contains_key(&sku) || self.fetching.contains(&sku) {
            return false;
        }
        if self
            .failed
            .get(&sku)
            .is_some_and(|at| now_ms - at < FETCH_BACKOFF_MS)
        {
            return false;
        }
        self.fetching.insert(sku)
    }

    /// Takes the body of a product read. Returns whether it held a profile effect.
    pub fn fetched(&mut self, sku: Id, body: &Value, now_ms: f64) -> bool {
        self.fetching.remove(&sku);
        match ProfileEffect::from_product(body) {
            Some(effect) => {
                self.failed.remove(&sku);
                self.effects.insert(sku, effect);
                true
            }
            None => {
                self.failed.insert(sku, now_ms);
                false
            }
        }
    }

    /// Adds an effect read from somewhere else (a Shop product, a bundle's items).
    pub fn insert(&mut self, effect: ProfileEffect) {
        self.failed.remove(&effect.sku_id);
        self.effects.insert(effect.sku_id, effect);
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DEADPOOL: &str = include_str!("../../../tests/fixtures/profile_effect_product.json");
    const SKU: Id = 1564419597501989044;

    fn deadpool() -> ProfileEffect {
        ProfileEffect::from_product(&serde_json::from_str(DEADPOOL).unwrap()).unwrap()
    }

    #[test]
    fn the_recorded_product_reads_into_two_timed_layers() {
        let effect = deadpool();
        assert_eq!(effect.sku_id, SKU);
        assert_eq!(effect.title, "Deadpool");
        assert_eq!(effect.animation_type, AnimationType::Intermittent);
        assert!(effect.reduced_motion_src.ends_with("2679eb"));
        assert!(effect.thumbnail_preview_src.ends_with("b4bb6"));
        assert_eq!(effect.layers.len(), 2);
        let (first, second) = (&effect.layers[0], &effect.layers[1]);
        assert_eq!(
            (first.start_ms, first.duration_ms, first.z_index),
            (0., 5000., 100)
        );
        assert!(!first.looping);
        assert_eq!(
            (second.start_ms, second.duration_ms, second.loop_delay_ms),
            (5500., 5042., Some(3500.))
        );
        assert!(second.looping);
        assert_eq!(second.position, (0., 0.));
        assert_eq!(effect.pick_range(), 0);
        assert_eq!(effect.loop_bounds(), (10542., 5500.));
    }

    #[test]
    fn layers_are_painted_in_ascending_z_order_and_other_items_are_refused() {
        let item = json!({"type":1,"sku_id":"7","effects":[
            {"src":"b","zIndex":5},{"src":"a","zIndex":1},{"src":"c","zIndex":5}]});
        let effect = ProfileEffect::from_item(&item).unwrap();
        let order: Vec<_> = effect
            .layers
            .iter()
            .map(|layer| layer.src.as_str())
            .collect();
        assert_eq!(order, ["a", "b", "c"]);
        assert!(ProfileEffect::from_item(&json!({"type":0,"sku_id":"7"})).is_none());
        assert!(ProfileEffect::from_item(&json!({"type":1})).is_none());
        assert!(ProfileEffect::from_product(&json!({"items":[]})).is_none());
    }

    #[test]
    fn a_non_looping_layer_runs_once_and_a_looping_one_pauses_for_its_delay() {
        let effect = deadpool();
        let (once, looped) = (&effect.layers[0], &effect.layers[1]);
        assert_eq!(once.local_time(-1.), None);
        assert_eq!(once.local_time(0.), Some(0.));
        assert_eq!(once.local_time(5000.), Some(5000.));
        assert_eq!(once.local_time(5001.), None);
        assert_eq!(looped.local_time(5499.), None);
        assert_eq!(looped.local_time(5500.), Some(0.));
        assert_eq!(looped.local_time(10542.), Some(5042.));
        // The delay: 5042 ms of image, then 3500 ms of nothing, then the image again from its first frame.
        assert_eq!(looped.local_time(10543.), None);
        assert_eq!(looped.local_time(14041.), None);
        assert_eq!(looped.local_time(5500. + 8542. + 100.), Some(100.));
    }

    #[test]
    fn a_looping_layer_without_a_delay_keeps_counting_up() {
        let layer = EffectLayer {
            looping: true,
            start_ms: 100.,
            duration_ms: 1000.,
            ..Default::default()
        };
        assert_eq!(layer.local_time(50.), None);
        assert_eq!(layer.local_time(5100.), Some(5000.));
    }

    #[test]
    fn the_random_pick_uses_the_shortest_list_and_each_layer_its_own_source() {
        let item = json!({"type":1,"sku_id":"7","effects":[
            {"src":"x","randomizedSources":[{"src":"x1"},{"src":"x2"},{"src":"x3"}]},
            {"src":"y","randomizedSources":[{"src":"y1"},{"src":"y2"}]},
            {"src":"z","randomizedSources":[]}]});
        let effect = ProfileEffect::from_item(&item).unwrap();
        assert_eq!(effect.pick_range(), 2);
        let picked: Vec<_> = effect.layers.iter().map(|layer| layer.source(1)).collect();
        assert_eq!(picked, ["x2", "y2", "z"]);
    }

    #[test]
    fn the_profile_names_the_last_effect_and_a_server_profile_overrides_it_in_that_server() {
        let body = json!({
            "user_profile": {"collectibles": [
                {"sku_id":"10","type":1},
                {"sku_id":"11","type":3},
                {"sku_id":"12","type":1}]},
            "guild_member_profile": {"guild_id":"5","collectibles":[{"sku_id":"20","type":1}]}
        });
        let sku = |guild| Equipped::from_profile(&body, guild, 0.).map(|effect| effect.sku_id);
        assert_eq!(sku(None), Some(12));
        assert_eq!(sku(Some(4)), Some(12));
        assert_eq!(sku(Some(5)), Some(20));
        assert_eq!(
            Equipped::from_profile(&json!({"user_profile":{}}), None, 0.),
            None
        );
        assert_eq!(Equipped::from_profile(&json!({}), None, 0.), None);
    }

    #[test]
    fn a_lapsed_effect_is_gone_and_the_account_effect_shows_through_a_lapsed_server_one() {
        let body = json!({
            "user_profile": {"collectibles": [{"sku_id":"10","type":1,"expires_at":"2030-01-01T00:00:00.000000+00:00"}]},
            "guild_member_profile": {"guild_id":"5","collectibles":[
                {"sku_id":"20","type":1,"expires_at":"2020-01-01T00:00:00+00:00"}]}
        });
        let now = 1_800_000_000_000.;
        let global = Equipped::from_profile(&body, Some(5), now).unwrap();
        assert_eq!(
            (global.sku_id, global.expires_at),
            (10, Some(1_893_456_000))
        );
        assert_eq!(
            Equipped::from_profile(&body, None, 1_950_000_000_000.),
            None
        );
    }

    #[test]
    fn the_store_asks_once_backs_off_after_a_miss_and_keeps_what_it_read() {
        let mut store = ProfileEffectStore::default();
        assert!(store.begin_fetch(SKU, 0.));
        assert!(!store.begin_fetch(SKU, 1.));
        assert!(!store.fetched(SKU, &json!({"items":[]}), 10.));
        assert!(!store.begin_fetch(SKU, 20.));
        assert!(store.begin_fetch(SKU, 70_000.));
        assert!(store.fetched(SKU, &serde_json::from_str(DEADPOOL).unwrap(), 70_100.));
        assert!(!store.begin_fetch(SKU, 200_000.));
        assert_eq!(store.get(SKU).map(|effect| effect.layers.len()), Some(2));
        assert_eq!(
            product_route(SKU),
            "/collectibles-products/1564419597501989044?locale=en-US"
        );
    }
}
