//! The sticker stores: `GuildStickersStore` (module 41237), `StickersPackStore` (514983) and the `StickersStore` that
//! joins them (898578), with the picker's category list (`pD` in module 256449), the sendability rule (`W$` in module
//! 361670) and the picker's search (`queryStickers`, module 997625).
//!
//! Guild stickers come with READY's guilds and GUILD_CREATE (a full list, or the writes and deletes of a partial one)
//! and are replaced by GUILD_STICKERS_UPDATE. The standard packs come from `GET /sticker-packs?locale=`
//! (`STICKER_PACKS_FETCH_SUCCESS`), once an hour at most.

use crate::Id;
use crate::frecency::{Frecency, Kind, Usage};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// `NL`: a standard sticker belongs to a pack, a guild sticker to a guild.
pub const STANDARD: u64 = 1;
pub const GUILD: u64 = 2;

/// `USE_EXTERNAL_STICKERS` and `USE_EXTERNAL_EMOJIS` in the permission table.
pub const USE_EXTERNAL_STICKERS: u64 = 1 << 37;
pub const USE_EXTERNAL_EMOJIS: u64 = 1 << 18;

/// `ot.aG`: how many stickers a guild may hold at each Boost level (slots up to a level).
pub const STICKER_SLOTS: [usize; 4] = [5, 15, 30, 60];

/// `numFrequentlyItems` of the sticker frecency (module 931959).
pub const FREQUENT_LIMIT: usize = 20;

fn sid(value: &Value) -> Option<Id> {
    value.as_str()?.parse().ok()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// `format_type`: how the picture is stored (`u` in module 832628).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Apng,
    Lottie,
    Gif,
}

impl Format {
    pub fn from_wire(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Png),
            2 => Some(Self::Apng),
            3 => Some(Self::Lottie),
            4 => Some(Self::Gif),
            _ => None,
        }
    }
}

/// A sticker as `d` and `c` in the stores keep it: the same fields for a guild's and a pack's.
#[derive(Clone, Debug, PartialEq)]
pub struct Sticker {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    /// Guild stickers: the related emoji name or the tags the uploader typed; packs: a comma separated list.
    pub tags: Option<String>,
    /// `type`: `STANDARD` or `GUILD`.
    pub kind: u64,
    pub format_type: u64,
    pub guild_id: Option<Id>,
    pub pack_id: Option<Id>,
    /// False when the guild lost the Boost level that allowed it.
    pub available: bool,
    pub user_id: Option<Id>,
}

impl Sticker {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: sid(&value["id"])?,
            name: text(&value["name"])?,
            description: text(&value["description"]),
            tags: text(&value["tags"]),
            kind: value["type"].as_u64().unwrap_or(0),
            format_type: value["format_type"].as_u64()?,
            guild_id: sid(&value["guild_id"]),
            pack_id: sid(&value["pack_id"]),
            available: value["available"].as_bool().unwrap_or(true),
            user_id: sid(&value["user_id"]).or_else(|| sid(&value["user"]["id"])),
        })
    }

    /// `Xw`.
    pub fn is_guild(&self) -> bool {
        self.kind == GUILD
    }

    /// `FD`.
    pub fn is_standard(&self) -> bool {
        self.kind == STANDARD
    }

    pub fn format(&self) -> Option<Format> {
        Format::from_wire(self.format_type)
    }

    /// `j` in module 148355: the label a screen reader and the message tooltip read.
    pub fn alt_text(&self) -> String {
        match self.description.as_deref() {
            Some(description) => format!("Sticker, {}, {description}", self.name),
            None => format!("Sticker, {}", self.name),
        }
    }
}

/// A standard pack (`GET /sticker-packs`).
#[derive(Clone, Debug, PartialEq)]
pub struct StickerPack {
    pub id: Id,
    pub name: String,
    pub description: Option<String>,
    pub sku_id: Option<Id>,
    pub cover_sticker_id: Option<Id>,
    pub banner_asset_id: Option<Id>,
    pub stickers: Vec<Sticker>,
}

impl StickerPack {
    pub fn from_value(value: &Value) -> Option<Self> {
        let id = sid(&value["id"])?;
        Some(Self {
            id,
            name: text(&value["name"])?,
            description: text(&value["description"]),
            sku_id: sid(&value["sku_id"]),
            cover_sticker_id: sid(&value["cover_sticker_id"]),
            banner_asset_id: sid(&value["banner_asset_id"]),
            stickers: value["stickers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Sticker::from_value)
                .map(|mut sticker| {
                    // `c` keeps the pack id the sticker came in.
                    sticker.pack_id = sticker.pack_id.or(Some(id));
                    sticker.kind = if sticker.kind == 0 {
                        STANDARD
                    } else {
                        sticker.kind
                    };
                    sticker
                })
                .collect(),
        })
    }

    /// `Id` in module 378058: the cover sticker, else the first.
    pub fn cover(&self) -> Option<&Sticker> {
        self.cover_sticker_id
            .and_then(|cover| self.stickers.iter().find(|sticker| sticker.id == cover))
            .or_else(|| self.stickers.first())
    }
}

/// What the picker may do with a sticker (`Ux` in module 361670).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sendability {
    Sendable,
    /// A sticker of another server, for an account without Nitro: the click opens the upsell.
    SendableWithPremium,
    Nonsendable,
    /// The server lost the Boost level that allowed it.
    SendableWithBoostedGuild,
}

/// What `W$` reads besides the sticker: the account, the channel and the pack list.
#[derive(Clone, Copy, Debug)]
pub struct SendContext {
    /// There is a signed in user (`W$` answers `NONSENDABLE` without one).
    pub signed_in: bool,
    /// `canUseCustomStickersEverywhere`.
    pub everywhere: bool,
    /// The guild of the channel the sticker goes to; `None` in a DM.
    pub channel_guild: Option<Id>,
    /// `USE_EXTERNAL_STICKERS` in that channel.
    pub external: bool,
}

impl SendContext {
    /// `W$`. `member_of` is `getSelfMember(guild) != null`, `pack_known` is `getStickerPack(pack) != null`.
    pub fn sendability(
        &self,
        sticker: &Sticker,
        member_of: impl Fn(Id) -> bool,
        pack_known: impl Fn(Id) -> bool,
    ) -> Sendability {
        if !self.signed_in {
            return Sendability::Nonsendable;
        }
        if sticker.is_standard() {
            return match sticker.pack_id {
                Some(pack) if pack_known(pack) => Sendability::Sendable,
                _ => Sendability::Nonsendable,
            };
        }
        if !sticker.is_guild() {
            return Sendability::Nonsendable;
        }
        let Some(guild) = sticker.guild_id.filter(|guild| member_of(*guild)) else {
            return Sendability::Nonsendable;
        };
        if !sticker.available {
            return Sendability::SendableWithBoostedGuild;
        }
        if self.channel_guild == Some(guild) {
            Sendability::Sendable
        } else if self.channel_guild.is_none() || self.external {
            if self.everywhere {
                Sendability::Sendable
            } else {
                Sendability::SendableWithPremium
            }
        } else {
            Sendability::Nonsendable
        }
    }
}

/// `Z2`: the kinds of category the picker lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CategoryKind {
    Favorite,
    Recent,
    Guild,
    EmptyGuildUpsell,
    Pack,
}

/// One entry of the picker's category rail and one section of its list.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub kind: CategoryKind,
    /// Favorites and recents carry their kind's name; guilds and packs their id.
    pub id: String,
    pub name: String,
    pub stickers: Vec<Sticker>,
}

/// Everything `pD` reads about the account and the open channel besides the stores.
pub struct CategoryInputs<'a> {
    /// `favoriteStickers.stickerIds`.
    pub favorites: &'a [Id],
    /// `getFlattenedGuildIds`, in rail order, with each guild's name.
    pub guilds: &'a [(Id, String)],
    /// The guild of the channel the picker belongs to.
    pub current_guild: Option<Id>,
    /// `canManageAllExpressions` in that guild.
    pub can_manage_current: bool,
    /// `USE_EXTERNAL_EMOJIS` in the channel: without it only the channel's own guild is listed.
    pub can_use_external: bool,
    pub frequent: &'a [Id],
    pub favorites_name: &'a str,
    pub recent_name: &'a str,
}

/// The `type`s a search value can have (`cG`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaKind {
    StickerName,
    Tag,
    CorrelatedEmoji,
    GuildName,
    PackName,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub kind: MetaKind,
    pub value: String,
}

/// A search hit with its score (`queryStickers`' `{sticker, comparator, score}`).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub sticker: Sticker,
    pub score: f64,
}

#[derive(Default)]
pub struct StickersStore {
    /// `GuildStickersStore`: guild to its stickers, in the order the server sent them.
    guild: HashMap<Id, Vec<Sticker>>,
    /// `StickersPackStore`: the packs of the last `GET /sticker-packs`.
    packs: Vec<StickerPack>,
    packs_loaded: bool,
    /// Pack stickers fetched one by one (`PACK_STICKER_FETCH_SUCCESS`) are not needed here; guild ones are
    /// (`GUILD_STICKER_FETCH_SUCCESS`), and stay with their guild.
    frecency: Option<Frecency>,
    /// This session's uses, on top of the account's history (`STICKER_TRACK_USAGE`).
    used: Vec<(Id, u64)>,
    stored_usage: Vec<(String, Usage)>,
}

impl StickersStore {
    /// Applies the gateway events the two official stores react to.
    pub fn dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "READY" => {
                let mut known: HashSet<Id> = HashSet::new();
                for guild in data["guilds"].as_array().into_iter().flatten() {
                    if let Some(id) = sid(&guild["id"]) {
                        known.insert(id);
                        self.apply_guild(id, guild);
                    }
                }
                for guild in data["unavailable_guilds"].as_array().into_iter().flatten() {
                    if let Some(id) = sid(guild).or_else(|| sid(&guild["id"])) {
                        known.insert(id);
                    }
                }
                self.guild.retain(|id, _| known.contains(id));
            }
            "GUILD_CREATE" => {
                // `joined_at` null: a guild the account only looks at.
                if data.get("joined_at").is_some_and(Value::is_null) {
                    return;
                }
                if let Some(id) = sid(&data["id"]) {
                    self.apply_guild(id, data);
                }
            }
            "GUILD_DELETE" => {
                if data["unavailable"] != true
                    && let Some(id) = sid(&data["id"])
                {
                    self.guild.remove(&id);
                }
            }
            "GUILD_STICKERS_UPDATE" => {
                if let Some(id) = sid(&data["guild_id"]) {
                    self.replace(id, &data["stickers"]);
                }
            }
            _ => {}
        }
    }

    /// `_` in module 41237: a full list replaces the guild's, a partial one writes and deletes.
    fn apply_guild(&mut self, id: Id, guild: &Value) {
        if guild["data_mode"] == "partial" {
            let updates = &guild["partial_updates"];
            let list = self.guild.entry(id).or_default();
            for deleted in updates["deleted_sticker_ids"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(deleted) = sid(deleted) {
                    list.retain(|sticker| sticker.id != deleted);
                }
            }
            for written in updates["stickers"].as_array().into_iter().flatten() {
                if let Some(sticker) = Sticker::from_value(written) {
                    match list.iter_mut().find(|known| known.id == sticker.id) {
                        Some(known) => *known = sticker,
                        None => list.push(sticker),
                    }
                }
            }
        } else if guild["stickers"].is_array() {
            self.replace(id, &guild["stickers"]);
        }
    }

    /// `GUILD_STICKERS_UPDATE` and a full sync: the list replaces the guild's, and a sticker the update sends without
    /// its uploader keeps the one the store knew.
    fn replace(&mut self, guild: Id, stickers: &Value) {
        let known: HashMap<Id, Id> = self
            .guild
            .get(&guild)
            .into_iter()
            .flatten()
            .filter_map(|sticker| Some((sticker.id, sticker.user_id?)))
            .collect();
        let list = stickers
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Sticker::from_value)
            .map(|mut sticker| {
                sticker.guild_id = sticker.guild_id.or(Some(guild));
                if sticker.user_id.is_none() {
                    sticker.user_id = known.get(&sticker.id).copied();
                }
                sticker
            })
            .collect();
        self.guild.insert(guild, list);
    }

    /// `GUILD_STICKERS_FETCH_SUCCESS`: the REST list of one guild.
    pub fn set_guild_stickers(&mut self, guild: Id, stickers: &Value) {
        self.replace(guild, stickers);
    }

    /// `STICKER_PACKS_FETCH_SUCCESS`: the body of `GET /sticker-packs`.
    pub fn set_packs(&mut self, body: &Value) {
        self.packs = body["sticker_packs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(StickerPack::from_value)
            .collect();
        self.packs_loaded = true;
    }

    /// `hasLoadedStickerPacks` (the official answer also expires after an hour; a session refetches on reopening).
    pub fn has_loaded_packs(&self) -> bool {
        self.packs_loaded
    }

    pub fn packs(&self) -> &[StickerPack] {
        &self.packs
    }

    /// `getStickerPack`.
    pub fn pack(&self, id: Id) -> Option<&StickerPack> {
        self.packs.iter().find(|pack| pack.id == id)
    }

    /// `getStickersByGuildId`.
    pub fn guild_stickers(&self, guild: Id) -> &[Sticker] {
        self.guild.get(&guild).map_or(&[], Vec::as_slice)
    }

    /// `DELETED_ENTITY_IDS` (module 74362): the guild keeps only the stickers the server still lists. True when
    /// something went.
    pub fn retain_guild_stickers(
        &mut self,
        guild: Id,
        keep: &std::collections::HashSet<Id>,
    ) -> bool {
        let Some(list) = self.guild.get_mut(&guild) else {
            return false;
        };
        let before = list.len();
        list.retain(|sticker| keep.contains(&sticker.id));
        list.len() != before
    }

    /// `getAllGuildStickers`.
    pub fn all_guild_stickers(&self) -> impl Iterator<Item = (Id, &[Sticker])> {
        self.guild
            .iter()
            .map(|(guild, stickers)| (*guild, stickers.as_slice()))
    }

    /// `getStickerById`: a guild's sticker first, then a pack's.
    pub fn sticker(&self, id: Id) -> Option<&Sticker> {
        self.guild
            .values()
            .flatten()
            .find(|sticker| sticker.id == id)
            .or_else(|| {
                self.packs
                    .iter()
                    .flat_map(|pack| &pack.stickers)
                    .find(|sticker| sticker.id == id)
            })
    }

    /// The account's sticker history from its settings (`stickerFrecency.stickers`), replacing the last one.
    pub fn set_history(&mut self, history: Vec<(Id, Usage)>) {
        self.stored_usage = history
            .into_iter()
            .map(|(id, usage)| (id.to_string(), usage))
            .collect();
        self.frecency = None;
    }

    /// `STICKER_TRACK_USAGE`: one more use of each sticker.
    pub fn track(&mut self, ids: &[Id], at_ms: u64) {
        for id in ids {
            self.used.push((*id, at_ms));
        }
        self.frecency = None;
    }

    /// The frecency over the account's history and this session's uses.
    fn frecency(&self) -> Frecency {
        let mut frecency = Frecency::with_limit(Kind::Emoji, FREQUENT_LIMIT);
        frecency.overwrite(self.stored_usage.clone());
        for (id, at) in &self.used {
            frecency.track(&id.to_string(), *at);
        }
        frecency
    }

    /// `stickerFrecency.frequently`: the stickers still in a store, best first.
    pub fn frequently(&self, now_ms: u64) -> Vec<Id> {
        self.frecency()
            .frequently(now_ms, |key| {
                key.parse()
                    .ok()
                    .is_some_and(|id| self.sticker(id).is_some())
            })
            .iter()
            .filter_map(|key| key.parse().ok())
            .collect()
    }

    /// `getScore`: the sticker's frecency now, if it was ever used.
    pub fn score(&self, id: Id, now_ms: u64) -> Option<f64> {
        self.frecency().score(&id.to_string(), now_ms)
    }

    /// `pD`: the picker's categories, favourites and recents first, then the guilds (the open one first), then the
    /// packs. A category with no stickers is dropped, except the open guild's upsell.
    pub fn categories(&self, input: &CategoryInputs, now_ms: u64) -> Vec<Category> {
        let favorites: Vec<Sticker> = input
            .favorites
            .iter()
            .filter_map(|id| self.sticker(*id))
            // `Y4`: a guild sticker of a server this account left is not listed.
            .filter(|sticker| {
                !sticker.is_guild()
                    || sticker
                        .guild_id
                        .is_some_and(|guild| input.guilds.iter().any(|(known, _)| *known == guild))
            })
            .cloned()
            .collect();
        let recent: Vec<Sticker> = input
            .frequent
            .iter()
            .filter_map(|id| self.sticker(*id))
            .filter(|sticker| {
                if sticker.is_guild() {
                    sticker.guild_id.is_some_and(|guild| {
                        self.guild_stickers(guild)
                            .iter()
                            .any(|known| known.id == sticker.id)
                    })
                } else {
                    sticker
                        .pack_id
                        .is_some_and(|pack| self.packs.iter().any(|known| known.id == pack))
                }
            })
            .cloned()
            .collect();
        let _ = now_ms;
        let mut guilds: Vec<Category> = input
            .guilds
            .iter()
            .filter_map(|(id, name)| {
                let stickers = self.guild_stickers(*id);
                (!stickers.is_empty()).then(|| Category {
                    kind: CategoryKind::Guild,
                    id: id.to_string(),
                    name: name.clone(),
                    stickers: stickers.to_vec(),
                })
            })
            .collect();
        if let Some(current) = input.current_guild {
            let at = guilds
                .iter()
                .position(|category| category.id == current.to_string());
            match at {
                Some(at) if at >= 1 => {
                    let category = guilds.remove(at);
                    guilds.insert(0, category);
                }
                None if input.can_manage_current => {
                    if let Some((_, name)) = input.guilds.iter().find(|(id, _)| *id == current) {
                        guilds.insert(
                            0,
                            Category {
                                kind: CategoryKind::EmptyGuildUpsell,
                                id: current.to_string(),
                                name: name.clone(),
                                stickers: Vec::new(),
                            },
                        );
                    }
                }
                _ => {}
            }
            if !input.can_use_external {
                guilds.retain(|category| category.id == current.to_string());
            }
        }
        let mut all = vec![
            Category {
                kind: CategoryKind::Favorite,
                id: "FAVORITE".to_owned(),
                name: input.favorites_name.to_owned(),
                stickers: favorites,
            },
            Category {
                kind: CategoryKind::Recent,
                id: "RECENT".to_owned(),
                name: input.recent_name.to_owned(),
                stickers: recent,
            },
        ];
        all.extend(guilds);
        all.extend(self.packs.iter().map(|pack| Category {
            kind: CategoryKind::Pack,
            id: pack.id.to_string(),
            name: pack.name.clone(),
            stickers: pack.stickers.clone(),
        }));
        all.retain(|category| {
            category.kind == CategoryKind::EmptyGuildUpsell || !category.stickers.is_empty()
        });
        all
    }

    /// The values `getStickerMetadataMap` keeps for one sticker, for `queryStickers` to match.
    pub fn metadata(
        &self,
        sticker: &Sticker,
        emoji: impl Fn(&str) -> Vec<String>,
        guild_name: impl Fn(Id) -> Option<String>,
    ) -> Vec<Meta> {
        let mut out = vec![Meta {
            kind: MetaKind::StickerName,
            value: sticker.name.trim().to_lowercase(),
        }];
        if sticker.is_guild() {
            if let Some(tags) = sticker.tags.as_deref() {
                out.push(Meta {
                    kind: MetaKind::Tag,
                    value: tags.trim().to_lowercase(),
                });
                if let Some(name) = sticker.guild_id.and_then(&guild_name) {
                    let name = name.trim().to_lowercase();
                    if !name.is_empty() {
                        out.push(Meta {
                            kind: MetaKind::GuildName,
                            value: name,
                        });
                    }
                }
                out.extend(emoji(tags).into_iter().map(|value| Meta {
                    kind: MetaKind::CorrelatedEmoji,
                    value,
                }));
            }
        } else if let Some(pack) = sticker.pack_id.and_then(|pack| self.pack(pack)) {
            out.push(Meta {
                kind: MetaKind::PackName,
                value: pack.name.clone(),
            });
        }
        out
    }

    /// `queryStickers([query], true)`: every sticker with a value that equals the query (10 times its weight), starts
    /// with it (7) or, for names, contains it (5), best first. The weights are 11 for a sticker name, 8 for a server
    /// or pack name, 6 for a related emoji and 1 for tags; a used sticker's score scales by its frecency over 100.
    pub fn search(
        &self,
        query: &str,
        now_ms: u64,
        emoji: impl Fn(&str) -> Vec<String>,
        guild_name: impl Fn(Id) -> Option<String>,
        keep: impl Fn(&Sticker) -> bool,
    ) -> Vec<Found> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let frecency = self.frecency();
        let mut seen: HashSet<Id> = HashSet::new();
        let mut found: Vec<Found> = Vec::new();
        let guild = self.guild.values().flatten();
        let pack = self.packs.iter().flat_map(|pack| &pack.stickers);
        for sticker in guild.chain(pack) {
            if !keep(sticker) || seen.contains(&sticker.id) {
                continue;
            }
            let mut best = 0f64;
            for meta in self.metadata(sticker, &emoji, &guild_name) {
                let weight = match meta.kind {
                    MetaKind::StickerName => 11.,
                    MetaKind::CorrelatedEmoji => 6.,
                    MetaKind::Tag => 1.,
                    MetaKind::GuildName | MetaKind::PackName => 8.,
                };
                let value = meta.value.to_lowercase();
                let score = if value == query {
                    10. * weight
                } else if value.starts_with(&query) {
                    7. * weight
                } else if matches!(
                    meta.kind,
                    MetaKind::GuildName | MetaKind::PackName | MetaKind::StickerName
                ) && value.contains(&query)
                {
                    5. * weight
                } else {
                    0.
                };
                best = best.max(score);
            }
            if let Some(frecent) = frecency.score(&sticker.id.to_string(), now_ms) {
                best *= frecent / 100.;
            }
            if best > 0. {
                seen.insert(sticker.id);
                found.push(Found {
                    sticker: sticker.clone(),
                    score: best,
                });
            }
        }
        // lodash `sortBy` is stable.
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        found
    }
}

/// Slots a guild at `premium_tier` offers (`aG`).
pub fn slots(premium_tier: u64) -> usize {
    STICKER_SLOTS[usize::try_from(premium_tier).unwrap_or(3).min(3)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MALLOW: &str = "796140620111544330";

    fn guild_sticker(id: &str, guild: &str, name: &str) -> Value {
        json!({"id": id, "name": name, "tags": "wave", "type": 2, "format_type": 1, "guild_id": guild, "available": true})
    }

    fn pack_body() -> Value {
        json!({"sticker_packs": [{
            "id": "796138864933863456", "name": "Mallow The Rascal", "sku_id": "796138864933863454",
            "cover_sticker_id": "796140731256799242",
            "stickers": [
                {"id": MALLOW, "name": "Happy", "tags": "mallow, happy", "type": 1, "format_type": 3,
                 "description": "Mallow dances a joyful jig", "pack_id": "796138864933863456"},
                {"id": "796140731256799242", "name": "Eyebrows", "type": 1, "format_type": 3,
                 "pack_id": "796138864933863456"}
            ]
        }]})
    }

    fn context(everywhere: bool, channel_guild: Option<Id>, external: bool) -> SendContext {
        SendContext {
            signed_in: true,
            everywhere,
            channel_guild,
            external,
        }
    }

    #[test]
    fn ready_and_guild_events_keep_the_guild_lists() {
        let mut store = StickersStore::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "10", "stickers": [guild_sticker("1", "10", "Wave")]},
                               {"id": "11", "stickers": [guild_sticker("2", "11", "Nod")]}]}),
        );
        assert_eq!(store.guild_stickers(10).len(), 1);
        store.dispatch(
            "GUILD_STICKERS_UPDATE",
            &json!({"guild_id": "10", "stickers": [guild_sticker("1", "10", "Wave"), guild_sticker("3", "10", "Hi")]}),
        );
        assert_eq!(store.guild_stickers(10).len(), 2);
        // A lurked guild is skipped, a deleted one is dropped, an outage keeps it.
        store.dispatch(
            "GUILD_CREATE",
            &json!({"id": "12", "joined_at": null, "stickers": [guild_sticker("4", "12", "X")]}),
        );
        assert!(store.guild_stickers(12).is_empty());
        store.dispatch("GUILD_DELETE", &json!({"id": "11", "unavailable": true}));
        assert_eq!(store.guild_stickers(11).len(), 1);
        store.dispatch("GUILD_DELETE", &json!({"id": "11"}));
        assert!(store.guild_stickers(11).is_empty());
        // READY drops guilds it no longer lists.
        store.dispatch("READY", &json!({"guilds": [{"id": "10", "stickers": []}]}));
        assert!(store.guild_stickers(10).is_empty());
    }

    #[test]
    fn a_partial_guild_writes_and_deletes() {
        let mut store = StickersStore::default();
        store.dispatch("GUILD_CREATE", &json!({"id": "10", "joined_at": "2026-01-01T00:00:00Z", "stickers": [guild_sticker("1", "10", "Wave"), guild_sticker("2", "10", "Nod")]}));
        store.dispatch(
            "GUILD_CREATE",
            &json!({"id": "10", "joined_at": "2026-01-01T00:00:00Z", "data_mode": "partial",
                    "partial_updates": {"stickers": [guild_sticker("2", "10", "Nodding"), guild_sticker("3", "10", "Hi")],
                                        "deleted_sticker_ids": ["1"]}}),
        );
        let names: Vec<&str> = store
            .guild_stickers(10)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Nodding", "Hi"]);
    }

    #[test]
    fn an_update_without_an_uploader_keeps_the_known_one() {
        let mut store = StickersStore::default();
        let mut first = guild_sticker("1", "10", "Wave");
        first["user"] = json!({"id": "99"});
        store.dispatch(
            "GUILD_STICKERS_UPDATE",
            &json!({"guild_id": "10", "stickers": [first]}),
        );
        store.dispatch(
            "GUILD_STICKERS_UPDATE",
            &json!({"guild_id": "10", "stickers": [guild_sticker("1", "10", "Wave")]}),
        );
        assert_eq!(store.guild_stickers(10)[0].user_id, Some(99));
    }

    #[test]
    fn packs_know_their_cover_and_their_stickers() {
        let mut store = StickersStore::default();
        assert!(!store.has_loaded_packs());
        store.set_packs(&pack_body());
        assert!(store.has_loaded_packs());
        let pack = &store.packs()[0];
        assert_eq!(pack.cover().unwrap().name, "Eyebrows");
        let happy = store.sticker(MALLOW.parse().unwrap()).unwrap();
        assert_eq!(happy.pack_id, Some(796_138_864_933_863_456));
        assert_eq!(happy.kind, STANDARD);
        assert_eq!(happy.format(), Some(Format::Lottie));
        assert_eq!(
            happy.alt_text(),
            "Sticker, Happy, Mallow dances a joyful jig"
        );
    }

    #[test]
    fn sendability_follows_the_official_table() {
        let own = Sticker::from_value(&guild_sticker("1", "10", "Wave")).unwrap();
        let member = |guild: Id| guild == 10;
        let known = |_: Id| true;
        // The channel's own guild: always.
        assert_eq!(
            context(false, Some(10), false).sendability(&own, member, known),
            Sendability::Sendable
        );
        // Another guild: Nitro and the permission, else the upsell or nothing.
        assert_eq!(
            context(false, Some(20), true).sendability(&own, member, known),
            Sendability::SendableWithPremium
        );
        assert_eq!(
            context(true, Some(20), true).sendability(&own, member, known),
            Sendability::Sendable
        );
        assert_eq!(
            context(true, Some(20), false).sendability(&own, member, known),
            Sendability::Nonsendable
        );
        // A DM needs no permission.
        assert_eq!(
            context(true, None, false).sendability(&own, member, known),
            Sendability::Sendable
        );
        // Not a member any more, or a lost Boost level.
        assert_eq!(
            context(true, Some(10), true).sendability(&own, |_| false, known),
            Sendability::Nonsendable
        );
        let mut lost = own.clone();
        lost.available = false;
        assert_eq!(
            context(true, Some(10), true).sendability(&lost, member, known),
            Sendability::SendableWithBoostedGuild
        );
        // A standard sticker needs its pack known, nothing else.
        let standard =
            Sticker::from_value(&pack_body()["sticker_packs"][0]["stickers"][0]).unwrap();
        let mut standard = standard;
        standard.pack_id = Some(5);
        standard.kind = STANDARD;
        assert_eq!(
            context(false, Some(20), false).sendability(&standard, member, known),
            Sendability::Sendable
        );
        assert_eq!(
            context(false, Some(20), false).sendability(&standard, member, |_| false),
            Sendability::Nonsendable
        );
        let signed_out = SendContext {
            signed_in: false,
            ..context(true, Some(10), true)
        };
        assert_eq!(
            signed_out.sendability(&own, member, known),
            Sendability::Nonsendable
        );
    }

    fn inputs<'a>(
        guilds: &'a [(Id, String)],
        current: Option<Id>,
        manage: bool,
    ) -> CategoryInputs<'a> {
        CategoryInputs {
            favorites: &[],
            guilds,
            current_guild: current,
            can_manage_current: manage,
            can_use_external: true,
            frequent: &[],
            favorites_name: "Favorites",
            recent_name: "Frequently Used",
        }
    }

    #[test]
    fn categories_list_the_open_guild_first_then_packs() {
        let mut store = StickersStore::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "10", "stickers": [guild_sticker("1", "10", "Wave")]},
                               {"id": "11", "stickers": [guild_sticker("2", "11", "Nod")]}]}),
        );
        store.set_packs(&pack_body());
        let guilds = vec![(10, "ten".to_owned()), (11, "eleven".to_owned())];
        let kinds: Vec<(CategoryKind, String)> = store
            .categories(&inputs(&guilds, Some(11), false), 0)
            .into_iter()
            .map(|category| (category.kind, category.name))
            .collect();
        assert_eq!(
            kinds,
            [
                (CategoryKind::Guild, "eleven".to_owned()),
                (CategoryKind::Guild, "ten".to_owned()),
                (CategoryKind::Pack, "Mallow The Rascal".to_owned())
            ]
        );
    }

    #[test]
    fn an_empty_open_guild_gets_the_upsell_only_for_those_who_manage_it() {
        let mut store = StickersStore::default();
        store.dispatch("READY", &json!({"guilds": [{"id": "10", "stickers": []}]}));
        store.set_packs(&pack_body());
        let guilds = vec![(10, "ten".to_owned())];
        let managed = store.categories(&inputs(&guilds, Some(10), true), 0);
        assert_eq!(managed[0].kind, CategoryKind::EmptyGuildUpsell);
        assert_eq!(managed[0].name, "ten");
        assert_eq!(managed.len(), 2);
        let plain = store.categories(&inputs(&guilds, Some(10), false), 0);
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].kind, CategoryKind::Pack);
    }

    #[test]
    fn without_external_emoji_only_the_open_guild_is_listed() {
        let mut store = StickersStore::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "10", "stickers": [guild_sticker("1", "10", "Wave")]},
                               {"id": "11", "stickers": [guild_sticker("2", "11", "Nod")]}]}),
        );
        let guilds = vec![(10, "ten".to_owned()), (11, "eleven".to_owned())];
        let mut input = inputs(&guilds, Some(11), false);
        input.can_use_external = false;
        let listed = store.categories(&input, 0);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "eleven");
    }

    #[test]
    fn favorites_and_recents_are_the_first_categories() {
        let mut store = StickersStore::default();
        store.set_packs(&pack_body());
        let happy: Id = MALLOW.parse().unwrap();
        store.track(&[happy], 1_000);
        let frequent = store.frequently(1_000);
        assert_eq!(frequent, [happy]);
        let favorites = [happy];
        let mut input = inputs(&[], None, false);
        input.favorites = &favorites;
        input.frequent = &frequent;
        let listed = store.categories(&input, 1_000);
        assert_eq!(listed[0].kind, CategoryKind::Favorite);
        assert_eq!(listed[1].kind, CategoryKind::Recent);
        assert_eq!(listed[2].kind, CategoryKind::Pack);
    }

    fn no_emoji(_: &str) -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_contains() {
        let mut store = StickersStore::default();
        store.set_packs(&json!({"sticker_packs": [{
        "id": "1", "name": "Pack",
        "stickers": [
            {"id": "11", "name": "Sad", "type": 1, "format_type": 3, "pack_id": "1"},
            {"id": "12", "name": "Sadness", "type": 1, "format_type": 3, "pack_id": "1"},
            {"id": "13", "name": "Not sad", "type": 1, "format_type": 3, "pack_id": "1"},
            {"id": "14", "name": "Happy", "type": 1, "format_type": 3, "pack_id": "1"}
        ]}]}));
        let found = store.search("sad", 0, no_emoji, |_| None, |_| true);
        let names: Vec<&str> = found.iter().map(|hit| hit.sticker.name.as_str()).collect();
        assert_eq!(names, ["Sad", "Sadness", "Not sad"]);
        assert_eq!(found[0].score, 110.);
        assert_eq!(found[1].score, 77.);
        assert_eq!(found[2].score, 55.);
        assert!(store.search("", 0, no_emoji, |_| None, |_| true).is_empty());
        assert!(
            store
                .search("sad", 0, no_emoji, |_| None, |sticker| sticker.id != 11)
                .iter()
                .all(|hit| hit.sticker.id != 11)
        );
    }

    #[test]
    fn search_reads_tags_servers_and_related_emoji() {
        let mut store = StickersStore::default();
        store.dispatch(
            "READY",
            &json!({"guilds": [{"id": "10", "stickers": [guild_sticker("1", "10", "Greeting")]}]}),
        );
        // Tags count for 1 only as a whole, a guild name 8, the related emoji 6.
        let by_tag = store.search("wave", 0, no_emoji, |_| None, |_| true);
        assert_eq!(by_tag[0].score, 10.);
        let by_guild = store.search(
            "fixt",
            0,
            no_emoji,
            |_| Some("Gravecord Fixture".to_owned()),
            |_| true,
        );
        assert_eq!(by_guild[0].score, 40.);
        let by_guild = store.search(
            "grave",
            0,
            no_emoji,
            |_| Some("Gravecord Fixture".to_owned()),
            |_| true,
        );
        assert_eq!(by_guild[0].score, 56.);
        let by_emoji = store.search(
            "\u{1f44b}",
            0,
            |tags| {
                if tags == "wave" {
                    vec!["\u{1f44b}".to_owned()]
                } else {
                    Vec::new()
                }
            },
            |_| None,
            |_| true,
        );
        assert_eq!(by_emoji[0].score, 60.);
    }

    #[test]
    fn a_used_sticker_scales_by_its_frecency() {
        let mut store = StickersStore::default();
        store.set_packs(&json!({"sticker_packs": [{
        "id": "1", "name": "Pack",
        "stickers": [
            {"id": "11", "name": "Sad", "type": 1, "format_type": 3, "pack_id": "1"},
            {"id": "12", "name": "Sad again", "type": 1, "format_type": 3, "pack_id": "1"}
        ]}]}));
        store.track(&[12], 1_000);
        store.track(&[12], 2_000);
        let found = store.search("sad", 2_000, no_emoji, |_| None, |_| true);
        // 12: prefix match 77 times (2 uses * score 100) / 100 = 154; 11: exact 110.
        assert_eq!(found[0].sticker.id, 12);
        assert_eq!(found[0].score, 154.);
        assert_eq!(found[1].score, 110.);
    }

    #[test]
    fn slots_follow_the_boost_level() {
        assert_eq!(slots(0), 5);
        assert_eq!(slots(2), 30);
        assert_eq!(slots(7), 60);
    }
}
