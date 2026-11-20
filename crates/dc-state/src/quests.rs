//! Quests: the official `QuestStore` (module 859703) and the Orbs balance it sits beside, `VirtualCurrencyStore`
//! (module 505274). The Quest Home page (module 635668) reads both.
//!
//! Wire shapes. `GET /quests/@me` (module 343559, `fetchCurrentQuests`):
//! `{"quests": [], "excluded_quests": [], "quest_enrollment_blocked_until": null, "quest_access_suspended_until": null}`
//! (captured in `TRAFFIC/identify.json`; the quest objects are decoded by `rO`/`E`/`A` of module 247512, whose field
//! names are followed here). `GET /quests/@me/claimed` answers `{"quests": [...]}` decoded by `S` of the same module.
//! `GET /users/@me/virtual-currency/balance` answers `{"balance": 0}` (`TRAFFIC/nitro-store.json`).
//!
//! The page only reads. [`enroll_request`], [`claim_reward_request`] and [`progress_request`] describe the account
//! writes the official client sends; nothing in this client sends them.

use chrono::DateTime;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const CURRENT_ROUTE: &str = "/quests/@me";
pub const CLAIMED_ROUTE: &str = "/quests/@me/claimed";
pub const BALANCE_ROUTE: &str = "/users/@me/virtual-currency/balance";

/// `QuestRewardType` (module 440703).
pub mod reward {
    pub const REWARD_CODE: u64 = 1;
    pub const IN_GAME: u64 = 2;
    pub const COLLECTIBLE: u64 = 3;
    pub const VIRTUAL_CURRENCY: u64 = 4;
    pub const FRACTIONAL_PREMIUM: u64 = 5;
}

/// `QuestTaskType` (module 412703): the keys of `task_config_v2.tasks`.
pub mod task {
    pub const STREAM_ON_DESKTOP: &str = "STREAM_ON_DESKTOP";
    pub const PLAY_ON_DESKTOP: &str = "PLAY_ON_DESKTOP";
    pub const PLAY_ON_XBOX: &str = "PLAY_ON_XBOX";
    pub const PLAY_ON_PLAYSTATION: &str = "PLAY_ON_PLAYSTATION";
    pub const WATCH_VIDEO: &str = "WATCH_VIDEO";
    pub const WATCH_VIDEO_ON_MOBILE: &str = "WATCH_VIDEO_ON_MOBILE";
    pub const PLAY_ACTIVITY: &str = "PLAY_ACTIVITY";
    pub const ACHIEVEMENT_IN_GAME: &str = "ACHIEVEMENT_IN_GAME";
    pub const ACHIEVEMENT_IN_ACTIVITY: &str = "ACHIEVEMENT_IN_ACTIVITY";
}

/// `QuestFeature` (module 902173, `Li` of module 221596).
pub mod feature {
    pub const CLOUD_GAMING_ACTIVITY: u64 = 25;
    pub const NON_GAMING_PLAY_QUEST: u64 = 26;
    pub const PACKAGE_ACTION_ADVENTURE: u64 = 28;
    pub const PACKAGE_RPG_MMO: u64 = 29;
    pub const PACKAGE_RACING_SPORTS: u64 = 30;
    pub const PACKAGE_SANDBOX_CREATIVE: u64 = 31;
    pub const PACKAGE_FAMILY_FRIENDLY: u64 = 32;
    pub const PACKAGE_HOLIDAY_SEASON: u64 = 33;
    pub const PACKAGE_NEW_YEARS: u64 = 34;
    pub const CLOUD_GAMING_PROVIDER_NVIDIA: u64 = 43;
}

/// `QuestRewardAssignmentMethod` & collectible `expirationMode` (module 417404).
pub mod reward_mode {
    pub const ASSIGNMENT_TIERED: u64 = 2;
    pub const EXPIRATION_PREMIUM_EXTENSION: u64 = 2;
    pub const EXPIRATION_PREMIUM_PERMANENT: u64 = 3;
}

/// The special "any Activity" application id (`ej` of module 221596) and badge-intro video quest (`Fw`).
pub const ANY_ACTIVITY_APP_ID: &str = "545364944258990091";
pub const ORBS_INTRO_QUEST_ID: &str = "1333839522189938740";

/// `QuestCtaState` (`UA` of module 79545).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestCtaState {
    Unenrolled,
    Enrolled,
    Incomplete,
    Completed,
    Claimed,
    Expired,
    ExpiredClaimable,
    ExpiredClaimed,
}

/// `QuestTaskPlatform` (`fO` of module 221596).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestPlatform {
    Desktop,
    Console,
}

/// `ActiveQuestBarScreen` (`X0` of module 506988, `Qo` of module 61387).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivePlatformScreen {
    Select,
    Desktop,
    Console,
}

/// Console platforms a quest can target (`bg` of module 574112).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsolePlatform {
    Xbox,
    PlayStation,
}

impl ConsolePlatform {
    pub fn connection_type(self) -> &'static str {
        match self {
            ConsolePlatform::Xbox => "xbox",
            ConsolePlatform::PlayStation => "playstation",
        }
    }
}

/// A quest that sorts first while it is unfinished (`er.aJ` of module 221596).
const PINNED_QUEST: &str = "1410358070831480904";

/// `Qh`'s sort methods (`er.kL`, module 221596). The page's menu lists them in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMethod {
    #[default]
    Suggested,
    MostRecent,
    ExpiringSoon,
    RecentlyEnrolled,
}

impl SortMethod {
    pub const ALL: [SortMethod; 4] = [
        SortMethod::Suggested,
        SortMethod::MostRecent,
        SortMethod::ExpiringSoon,
        SortMethod::RecentlyEnrolled,
    ];

    /// The `sort` query value of the route.
    pub fn wire(self) -> &'static str {
        match self {
            SortMethod::Suggested => "suggested",
            SortMethod::MostRecent => "most_recent",
            SortMethod::ExpiringSoon => "expiring_soon",
            SortMethod::RecentlyEnrolled => "recently_enrolled",
        }
    }

    /// `Js` of module 651892 (strings `gBfXPZ`, `K6oEu2`, `IB22n3`, `BB+2tX`).
    pub fn label(self) -> &'static str {
        match self {
            SortMethod::Suggested => "Suggested",
            SortMethod::MostRecent => "Most Recent",
            SortMethod::ExpiringSoon => "Expiring Soon",
            SortMethod::RecentlyEnrolled => "Started",
        }
    }
}

/// One filter option (`WQ` and `pc` of module 221596): the group it belongs to and its wire value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Filter {
    TaskPlay,
    TaskVideo,
    RewardVirtualCurrency,
    RewardCollectible,
    RewardInGame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterGroup {
    Reward,
    Task,
}

impl Filter {
    /// `er.pc` in the page's order: the Reward group first, then Quest Type.
    pub const GROUPS: [(FilterGroup, &'static str, [Option<Filter>; 3]); 2] = [
        (
            FilterGroup::Reward,
            "Reward",
            [
                Some(Filter::RewardVirtualCurrency),
                Some(Filter::RewardCollectible),
                Some(Filter::RewardInGame),
            ],
        ),
        (
            FilterGroup::Task,
            "Quest Type",
            [Some(Filter::TaskPlay), Some(Filter::TaskVideo), None],
        ),
    ];

    pub fn group(self) -> FilterGroup {
        match self {
            Filter::TaskPlay | Filter::TaskVideo => FilterGroup::Task,
            _ => FilterGroup::Reward,
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Filter::TaskPlay => "task_play",
            Filter::TaskVideo => "task_video",
            Filter::RewardVirtualCurrency => "reward_virtual_currency",
            Filter::RewardCollectible => "reward_collectible",
            Filter::RewardInGame => "reward_in_game",
        }
    }

    /// `WQ`: a wire value to its option, none for anything else.
    pub fn from_wire(value: &str) -> Option<Filter> {
        [
            Filter::TaskPlay,
            Filter::TaskVideo,
            Filter::RewardVirtualCurrency,
            Filter::RewardCollectible,
            Filter::RewardInGame,
        ]
        .into_iter()
        .find(|filter| filter.wire() == value)
    }

    /// `up` of module 651892 (strings `ElYQFS`, `Jg17Ut`, `O/J2kr`, `e0iISA`, `1nJR4p`).
    pub fn label(self) -> &'static str {
        match self {
            Filter::RewardVirtualCurrency => "Orbs",
            Filter::RewardCollectible => "Avatar Decoration",
            Filter::RewardInGame => "In-Game Rewards",
            Filter::TaskVideo => "Watch",
            Filter::TaskPlay => "Play",
        }
    }
}

/// Per-task progress inside `user_status.progress[task_type]` (module 792620).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskProgress {
    pub event_name: String,
    pub value: f64,
    pub updated_at: Option<String>,
    pub heartbeat_expires_at: Option<String>,
    pub heartbeat_last_beat_at: Option<String>,
}

impl TaskProgress {
    fn from_value(key: &str, value: &Value) -> Self {
        let text = |v: &Value| v.as_str().map(str::to_owned);
        Self {
            event_name: value["event_name"].as_str().unwrap_or(key).to_owned(),
            value: value["value"].as_f64().unwrap_or(0.),
            updated_at: text(&value["updated_at"]),
            heartbeat_expires_at: text(&value["heartbeat"]["expires_at"]),
            heartbeat_last_beat_at: text(&value["heartbeat"]["last_beat_at"]),
        }
    }
}

/// The user's status for one quest (`A` of module 247512).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserStatus {
    pub quest_id: String,
    pub enrolled_at: Option<String>,
    pub completed_at: Option<String>,
    pub claimed_at: Option<String>,
    pub claimed_tier: Option<u64>,
    pub orb_quantity_claimed: Option<u64>,
    pub stream_progress_seconds: f64,
    pub progress: BTreeMap<String, TaskProgress>,
}

impl UserStatus {
    pub fn from_value(value: &Value) -> Self {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        let progress = value["progress"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), TaskProgress::from_value(k, v)))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            quest_id: text("quest_id").unwrap_or_default(),
            enrolled_at: text("enrolled_at"),
            completed_at: text("completed_at"),
            claimed_at: text("claimed_at"),
            claimed_tier: value["claimed_tier"].as_u64(),
            orb_quantity_claimed: value["orb_quantity_claimed"].as_u64(),
            stream_progress_seconds: value["stream_progress_seconds"].as_f64().unwrap_or(0.),
            progress,
        }
    }
}

/// One task entry from `config.task_config_v2.tasks[type]` (module 792620).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestTask {
    pub kind: String,
    pub target: f64,
    pub applications: Vec<String>,
    pub task_title: Option<String>,
    pub task_description: Option<String>,
    pub video_title: Option<String>,
}

impl QuestTask {
    fn from_value(key: &str, value: &Value) -> Self {
        let applications = value["applications"]
            .as_array()
            .map(|apps| {
                apps.iter()
                    .filter_map(|app| app["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            kind: value["type"].as_str().unwrap_or(key).to_owned(),
            target: value["target"].as_f64().unwrap_or(0.),
            applications,
            task_title: value["messages"]["task_title"].as_str().map(str::to_owned),
            task_description: value["messages"]["task_description"]
                .as_str()
                .map(str::to_owned),
            video_title: value["messages"]["video_title"].as_str().map(str::to_owned),
        }
    }
}

/// One reward entry from `config.rewards_config.rewards[]` (module 417404).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestReward {
    pub kind: u64,
    pub name: String,
    pub name_with_article: String,
    pub orb_quantity: Option<u64>,
    pub premium_orb_quantity: Option<u64>,
    pub expires_at: Option<String>,
    pub expiration_mode: Option<u64>,
    pub quantity: u64,
    pub sku_id: Option<String>,
    pub redemption_instructions_by_platform: BTreeMap<u64, String>,
}

impl QuestReward {
    fn from_value(value: &Value) -> Self {
        let name = value["messages"]["name"]
            .as_str()
            .or_else(|| value["name"].as_str())
            .unwrap_or_default()
            .to_owned();
        let name_with_article = value["messages"]["name_with_article"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| name.clone());
        let redemption_instructions_by_platform =
            value["messages"]["redemption_instructions_by_platform"]
                .as_object()
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| Some((k.parse::<u64>().ok()?, v.as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default();
        Self {
            kind: value["type"].as_u64().unwrap_or_default(),
            name,
            name_with_article,
            orb_quantity: value["orb_quantity"].as_u64(),
            premium_orb_quantity: value["premium_orb_quantity"].as_u64(),
            expires_at: value["expires_at"].as_str().map(str::to_owned),
            expiration_mode: value["expiration_mode"].as_u64(),
            quantity: value["quantity"].as_u64().unwrap_or(1),
            sku_id: value["sku_id"].as_str().map(str::to_owned),
            redemption_instructions_by_platform,
        }
    }
}

/// Decoded progress snapshot for a quest's primary task (`Yh` / `R` of module 792620).
#[derive(Clone, Debug, PartialEq)]
pub struct TaskDetails {
    pub progress_seconds: f64,
    pub target_seconds: f64,
    pub target_minutes: u64,
    pub percent_complete: f64,
    pub task_type: String,
    pub applications: Vec<String>,
}

/// Decoded third-party / achievement task (`JC` of module 792620).
#[derive(Clone, Debug, PartialEq)]
pub struct ThirdPartyTaskDetails {
    pub title: String,
    pub description: String,
    pub target: f64,
    pub progress: f64,
    pub percent_complete: f64,
}

/// What the page reads of a quest's `config` (`E` of module 247512, config version 2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestConfig {
    pub starts_at: String,
    pub expires_at: String,
    pub quest_name: String,
    pub game_title: String,
    pub game_publisher: String,
    /// The keys of `task_config_v2.tasks`.
    pub tasks: BTreeSet<String>,
    /// Decoded tasks keyed by task type (`task_config_v2.tasks`).
    pub task_details: BTreeMap<String, QuestTask>,
    /// `rewards_config.rewards[].type`, in order.
    pub reward_types: Vec<u64>,
    /// Decoded rewards (`rewards_config.rewards[]`).
    pub rewards: Vec<QuestReward>,
    pub rewards_assignment_method: u64,
    pub rewards_expire_at: Option<String>,
    pub platforms: usize,
    pub features: Vec<u64>,
    pub cta_link: String,
    pub cta_button_label: String,
    pub cta_subtitle: Option<String>,
    pub cosponsor_name: Option<String>,
    pub assets_hero: Option<String>,
    pub assets_logotype: Option<String>,
    pub assets_logotype_dark: Option<String>,
    pub assets_game_tile: Option<String>,
    pub assets_reward_tile: Option<String>,
    pub assets_video_thumbnail: Option<String>,
}

impl QuestConfig {
    fn from_value(value: &Value) -> Self {
        let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
        let opt_text = |value: &Value| value.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let task_details: BTreeMap<String, QuestTask> = value["task_config_v2"]["tasks"]
            .as_object()
            .map(|tasks| {
                tasks
                    .iter()
                    .map(|(k, v)| (k.clone(), QuestTask::from_value(k, v)))
                    .collect()
            })
            .unwrap_or_default();
        let rewards: Vec<QuestReward> = value["rewards_config"]["rewards"]
            .as_array()
            .map(|rewards| rewards.iter().map(QuestReward::from_value).collect())
            .unwrap_or_default();
        let assets = &value["assets"];
        Self {
            starts_at: text(&value["starts_at"]),
            expires_at: text(&value["expires_at"]),
            quest_name: text(&value["messages"]["quest_name"]),
            game_title: text(&value["messages"]["game_title"]),
            game_publisher: text(&value["messages"]["game_publisher"]),
            tasks: task_details.keys().cloned().collect(),
            task_details,
            reward_types: rewards.iter().map(|r| r.kind).collect(),
            rewards,
            rewards_assignment_method: value["rewards_config"]["assignment_method"]
                .as_u64()
                .unwrap_or(1),
            rewards_expire_at: opt_text(&value["rewards_config"]["rewards_expire_at"]),
            platforms: value["rewards_config"]["platforms"]
                .as_array()
                .map_or(0, Vec::len),
            features: value["features"]
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default(),
            cta_link: text(&value["cta_config"]["link"]),
            cta_button_label: text(&value["cta_config"]["button_label"]),
            cta_subtitle: opt_text(&value["cta_config"]["subtitle"]),
            cosponsor_name: opt_text(&value["cosponsor_metadata"]["name"]),
            assets_hero: opt_text(&assets["hero"]),
            assets_logotype: opt_text(&assets["logotype"]),
            assets_logotype_dark: opt_text(&assets["logotype_dark"]),
            assets_game_tile: opt_text(&assets["game_tile"]),
            assets_reward_tile: opt_text(&assets["reward_tile"]).or_else(|| {
                value["rewards_config"]["rewards"]
                    .as_array()
                    .and_then(|list| list.first())
                    .and_then(|r| opt_text(&r["asset"]))
            }),
            assets_video_thumbnail: opt_text(&assets["video_player_thumbnail"]),
        }
    }

    pub fn has_reward(&self, kind: u64) -> bool {
        self.reward_types.contains(&kind)
    }

    pub fn has_task(&self, kind: &str) -> bool {
        self.tasks.contains(kind)
    }

    pub fn has_feature(&self, flag: u64) -> bool {
        self.features.contains(&flag)
    }

    /// `nj` of module 417404: the collectible reward entry, if any.
    pub fn collectible_reward(&self) -> Option<&QuestReward> {
        self.rewards.iter().find(|r| r.kind == reward::COLLECTIBLE)
    }

    /// `JX` of module 417404: collectible reward has a Nitro extension or permanent perk.
    pub fn has_collectible_premium_benefit(&self) -> bool {
        self.collectible_reward()
            .and_then(|r| r.expiration_mode)
            .is_some_and(|mode| {
                mode == reward_mode::EXPIRATION_PREMIUM_EXTENSION
                    || mode == reward_mode::EXPIRATION_PREMIUM_PERMANENT
            })
    }

    /// `Y7` of module 417404: collectible reward becomes permanent with Nitro.
    pub fn is_collectible_premium_permanent(&self) -> bool {
        self.collectible_reward().and_then(|r| r.expiration_mode)
            == Some(reward_mode::EXPIRATION_PREMIUM_PERMANENT)
    }

    /// `ks` of module 417404: has an Orbs (virtual currency) reward.
    pub fn has_virtual_currency_reward(&self) -> bool {
        self.has_reward(reward::VIRTUAL_CURRENCY)
    }

    /// `r7` of module 417404: has a reward-code reward.
    pub fn has_reward_code(&self) -> bool {
        self.has_reward(reward::REWARD_CODE)
    }

    /// `_Z` / `wo` of module 417404: orb quantity (with optional Nitro multiplier).
    pub fn orb_quantity(&self, is_nitro: bool) -> Option<u64> {
        let r = self
            .rewards
            .iter()
            .find(|r| r.kind == reward::VIRTUAL_CURRENCY)?;
        if is_nitro {
            r.premium_orb_quantity.or(r.orb_quantity)
        } else {
            r.orb_quantity
        }
    }

    /// `kr` of module 417404: tiered assignment where every reward is a reward code.
    pub fn is_tiered_reward_code(&self) -> bool {
        self.rewards_assignment_method == reward_mode::ASSIGNMENT_TIERED
            && !self.rewards.is_empty()
            && self.rewards.iter().all(|r| r.kind == reward::REWARD_CODE)
    }

    /// `xv` of module 417404 / 651892: primary reward name (or with article).
    pub fn primary_reward_name(&self, with_article: bool) -> String {
        if let Some(first) = self.rewards.first() {
            if with_article && !first.name_with_article.is_empty() {
                first.name_with_article.clone()
            } else if !first.name.is_empty() {
                first.name.clone()
            } else {
                self.quest_name.clone()
            }
        } else {
            self.quest_name.clone()
        }
    }

    /// `Xi` of module 792620: video title from the watch-video task.
    pub fn video_title(&self) -> Option<&str> {
        self.task_details
            .get(task::WATCH_VIDEO_ON_MOBILE)
            .or_else(|| self.task_details.get(task::WATCH_VIDEO))
            .and_then(|t| t.video_title.as_deref())
    }
}

/// A quest (`rO` of module 247512).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quest {
    pub id: String,
    pub preview: bool,
    pub config: QuestConfig,
    pub user_status: Option<UserStatus>,
}

impl Quest {
    /// `s2` then `rO`: only config version 2 is understood, anything else is dropped.
    pub fn from_value(value: &Value) -> Option<Quest> {
        if value["config"]["config_version"].as_u64() != Some(2) {
            return None;
        }
        Some(Quest {
            id: value["id"].as_str()?.to_owned(),
            preview: value["preview"].as_bool().unwrap_or(false),
            config: QuestConfig::from_value(&value["config"]),
            user_status: value["user_status"]
                .is_object()
                .then(|| UserStatus::from_value(&value["user_status"])),
        })
    }

    pub fn is_claimed(&self) -> bool {
        self.user_status
            .as_ref()
            .is_some_and(|status| status.claimed_at.is_some())
    }

    pub fn is_enrolled(&self) -> bool {
        self.user_status
            .as_ref()
            .is_some_and(|status| status.enrolled_at.is_some())
    }

    pub fn is_completed(&self) -> bool {
        self.user_status
            .as_ref()
            .is_some_and(|status| status.completed_at.is_some())
    }

    /// `Ic` of module 977416: the end date has passed. An unreadable date never counts as passed.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        parse_ms(&self.config.expires_at).is_some_and(|end| end <= now_ms)
    }

    /// `if` of module 710969: rewards have not expired yet.
    pub fn is_claimable(&self, now_ms: i64) -> bool {
        self.config
            .rewards_expire_at
            .as_deref()
            .and_then(parse_ms)
            .is_none_or(|end| end > now_ms)
    }

    /// `Y.GR` of module 710969: finished but not yet claimed.
    pub fn awaits_claim(&self) -> bool {
        self.is_completed() && !self.is_claimed()
    }

    // ---- Official task predicates (modules 792620, 814793) ----

    /// `t` / `xZ` of module 792620.
    pub fn is_play_on_desktop(&self) -> bool {
        self.config.has_task(task::PLAY_ON_DESKTOP)
    }

    /// `fE` of module 792620.
    pub fn is_stream_on_desktop(&self) -> bool {
        self.config.has_task(task::STREAM_ON_DESKTOP)
    }

    /// `g5` / `uD` of module 792620.
    pub fn is_play_on_console(&self) -> bool {
        self.config.has_task(task::PLAY_ON_XBOX) || self.config.has_task(task::PLAY_ON_PLAYSTATION)
    }

    /// `no` of module 792620: desktop-only play quest (not console).
    pub fn is_desktop_only_play(&self) -> bool {
        !self.is_play_on_console() && self.is_play_on_desktop()
    }

    /// `IO` of module 792620.
    pub fn is_watch_video_desktop(&self) -> bool {
        self.config.has_task(task::WATCH_VIDEO)
    }

    /// `K$` of module 792620: mobile-only video quest.
    pub fn is_watch_video_mobile_only(&self) -> bool {
        self.config.has_task(task::WATCH_VIDEO_ON_MOBILE) && !self.is_watch_video_desktop()
    }

    /// `vv` of module 792620: any watch-video quest.
    pub fn is_watch_video(&self) -> bool {
        self.is_watch_video_desktop() || self.config.has_task(task::WATCH_VIDEO_ON_MOBILE)
    }

    /// `vl` of module 792620.
    pub fn is_play_activity(&self) -> bool {
        self.config.has_task(task::PLAY_ACTIVITY)
    }

    /// `I6` of module 792620.
    pub fn is_achievement_in_game(&self) -> bool {
        self.config.has_task(task::ACHIEVEMENT_IN_GAME)
    }

    /// `Ov` of module 792620.
    pub fn is_achievement_in_activity(&self) -> bool {
        self.config.has_task(task::ACHIEVEMENT_IN_ACTIVITY)
    }

    /// `Cr` of module 792620: in-game or in-activity achievement task.
    pub fn is_achievement_quest(&self) -> bool {
        self.is_achievement_in_game() || self.is_achievement_in_activity()
    }

    /// `GL` of module 792620: play/stream/console/in-game quest.
    pub fn is_game_play_quest(&self) -> bool {
        self.is_play_on_desktop()
            || self.is_stream_on_desktop()
            || self.is_play_on_console()
            || self.is_achievement_in_game()
    }

    /// `vA` of module 814793: activity or achievement-in-activity quest.
    pub fn is_activity_quest(&self) -> bool {
        self.is_play_activity() || self.is_achievement_in_activity()
    }

    /// `ui` of module 814793: multi-game play quest (`PLAY_ON_DESKTOP` with >1 applications).
    pub fn is_multi_game_play_quest(&self) -> bool {
        self.config
            .task_details
            .get(task::PLAY_ON_DESKTOP)
            .is_some_and(|t| t.applications.len() > 1)
    }

    /// `_e` of module 814793: "play any Activity on Discord" quest.
    pub fn is_any_activity_quest(&self) -> bool {
        self.config
            .task_details
            .get(task::PLAY_ACTIVITY)
            .and_then(|t| t.applications.first())
            .is_some_and(|id| id == ANY_ACTIVITY_APP_ID)
    }

    /// `bg` of module 574112: console platforms supported by this quest.
    pub fn supported_consoles(&self) -> Vec<ConsolePlatform> {
        let mut out = Vec::new();
        if self.config.has_task(task::PLAY_ON_XBOX) {
            out.push(ConsolePlatform::Xbox);
        }
        if self.config.has_task(task::PLAY_ON_PLAYSTATION) {
            out.push(ConsolePlatform::PlayStation);
        }
        out
    }

    /// `UR` of module 574112: task platforms (Desktop / Console) supported by this quest.
    pub fn supported_platforms(&self) -> Vec<QuestPlatform> {
        let mut out = Vec::new();
        if self.is_play_on_desktop()
            || self.is_stream_on_desktop()
            || self.is_watch_video()
            || self.is_play_activity()
            || self.is_achievement_in_game()
            || self.is_achievement_in_activity()
        {
            out.push(QuestPlatform::Desktop);
        }
        if self.is_play_on_console() {
            out.push(QuestPlatform::Console);
        }
        out
    }

    /// `YL` of module 792620: active console heartbeat not yet expired.
    pub fn is_progressing_on_console(&self, now_ms: i64) -> bool {
        let Some(status) = self.user_status.as_ref() else {
            return false;
        };
        [task::PLAY_ON_XBOX, task::PLAY_ON_PLAYSTATION]
            .iter()
            .any(|kind| {
                status
                    .progress
                    .get(*kind)
                    .and_then(|p| p.heartbeat_expires_at.as_deref())
                    .and_then(parse_ms)
                    .is_some_and(|exp| exp > now_ms)
            })
    }

    /// `JC` of module 792620: third-party / achievement task details if present.
    pub fn third_party_task_details(&self) -> Option<ThirdPartyTaskDetails> {
        let entry = self
            .config
            .task_details
            .get(task::ACHIEVEMENT_IN_ACTIVITY)
            .or_else(|| self.config.task_details.get(task::ACHIEVEMENT_IN_GAME))?;
        let progress = self
            .user_status
            .as_ref()
            .and_then(|s| s.progress.get(&entry.kind))
            .map_or(0., |p| p.value);
        let pct = if entry.target > 0. {
            ((progress / entry.target).min(1.) * 10_000.).floor() / 10_000.
        } else {
            0.
        };
        Some(ThirdPartyTaskDetails {
            title: entry.task_title.clone().unwrap_or_default(),
            description: entry.task_description.clone().unwrap_or_default(),
            target: entry.target,
            progress,
            percent_complete: pct,
        })
    }

    /// `Yh` / `R` of module 792620: resolved task progress details.
    pub fn task_details(&self) -> TaskDetails {
        let chosen_key: Option<&str> = if self.is_achievement_quest() {
            return TaskDetails {
                progress_seconds: 0.,
                target_seconds: 1.,
                target_minutes: 1,
                percent_complete: 0.,
                task_type: task::STREAM_ON_DESKTOP.to_owned(),
                applications: Vec::new(),
            };
        } else if self.is_play_on_console() {
            // Prefer the task with the highest progress if any, otherwise Xbox/PlayStation/Desktop.
            self.user_status
                .as_ref()
                .and_then(|s| {
                    s.progress
                        .iter()
                        .filter(|(k, _)| self.config.task_details.contains_key(*k))
                        .max_by(|a, b| {
                            a.1.value
                                .partial_cmp(&b.1.value)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(k, _)| k.as_str())
                })
                .or_else(|| {
                    [
                        task::PLAY_ON_XBOX,
                        task::PLAY_ON_PLAYSTATION,
                        task::PLAY_ON_DESKTOP,
                    ]
                    .into_iter()
                    .find(|k| self.config.task_details.contains_key(*k))
                })
        } else if self.is_watch_video() {
            [task::WATCH_VIDEO_ON_MOBILE, task::WATCH_VIDEO]
                .into_iter()
                .find(|k| self.config.task_details.contains_key(*k))
        } else if self.is_play_on_desktop() {
            Some(task::PLAY_ON_DESKTOP)
        } else if self.is_play_activity() {
            Some(task::PLAY_ACTIVITY)
        } else {
            self.config.task_details.keys().next().map(String::as_str)
        };
        let Some(entry) = chosen_key.and_then(|k| self.config.task_details.get(k)) else {
            return TaskDetails {
                progress_seconds: 0.,
                target_seconds: 1.,
                target_minutes: 1,
                percent_complete: 0.,
                task_type: task::STREAM_ON_DESKTOP.to_owned(),
                applications: Vec::new(),
            };
        };
        let target = entry.target.max(1.);
        let raw_progress = if self.is_completed() {
            target
        } else {
            let v = self
                .user_status
                .as_ref()
                .and_then(|s| {
                    s.progress
                        .get(&entry.kind)
                        .map(|p| p.value)
                        .or(Some(s.stream_progress_seconds))
                })
                .unwrap_or(0.);
            v.min(target * 0.99).max(0.)
        };
        let percent = ((raw_progress / target).min(1.) * 10_000.).floor() / 10_000.;
        let applications = self
            .config
            .task_details
            .get(task::PLAY_ON_DESKTOP)
            .map(|t| t.applications.clone())
            .unwrap_or_default();
        TaskDetails {
            progress_seconds: raw_progress,
            target_seconds: target,
            target_minutes: (target / 60.).ceil().max(1.) as u64,
            percent_complete: percent,
            task_type: entry.kind.clone(),
            applications,
        }
    }

    /// `Pd` of module 79545: CTA state machine across all 8 quest states.
    pub fn cta_state(&self, now_ms: i64) -> QuestCtaState {
        let enrolled = self.is_enrolled();
        let completed = self.is_completed();
        let claimed = self.is_claimed();
        let progress = self.third_party_task_details().map_or_else(
            || self.task_details().percent_complete,
            |d| d.percent_complete,
        );
        if self.is_expired(now_ms) {
            if claimed {
                QuestCtaState::ExpiredClaimed
            } else if self.is_claimable(now_ms) && completed {
                QuestCtaState::ExpiredClaimable
            } else {
                QuestCtaState::Expired
            }
        } else if claimed {
            QuestCtaState::Claimed
        } else if enrolled && completed {
            QuestCtaState::Completed
        } else if enrolled && progress > 0. {
            QuestCtaState::Incomplete
        } else if enrolled {
            QuestCtaState::Enrolled
        } else {
            QuestCtaState::Unenrolled
        }
    }

    /// `cc` of module 79545: primary task type driving CTA labels.
    pub fn primary_task_type(&self) -> &str {
        if self.is_watch_video_desktop() {
            task::WATCH_VIDEO
        } else {
            self.config
                .task_details
                .values()
                .next()
                .map_or(task::STREAM_ON_DESKTOP, |t| t.kind.as_str())
        }
    }

    /// `Qo` of module 61387: active platform screen (Select / Desktop / Console).
    pub fn active_platform_screen(
        &self,
        selected: Option<QuestPlatform>,
        now_ms: i64,
    ) -> ActivePlatformScreen {
        if self.is_progressing_on_console(now_ms) {
            return ActivePlatformScreen::Console;
        }
        let details = self.task_details();
        if details.percent_complete > 0. {
            return match details.task_type.as_str() {
                task::PLAY_ON_XBOX | task::PLAY_ON_PLAYSTATION => ActivePlatformScreen::Console,
                _ => ActivePlatformScreen::Desktop,
            };
        }
        if let Some(sel) = selected {
            return match sel {
                QuestPlatform::Desktop => ActivePlatformScreen::Desktop,
                QuestPlatform::Console => ActivePlatformScreen::Console,
            };
        }
        let platforms = self.supported_platforms();
        let has_desktop = platforms.contains(&QuestPlatform::Desktop);
        let has_console = platforms.contains(&QuestPlatform::Console);
        if has_desktop && has_console {
            ActivePlatformScreen::Select
        } else if has_console {
            ActivePlatformScreen::Console
        } else {
            ActivePlatformScreen::Desktop
        }
    }

    /// `eA`/`eE` of module 61387: whether a quest matches one filter option.
    pub fn matches(&self, filter: Filter) -> bool {
        let config = &self.config;
        match filter {
            // `ee.vv`: a video task on any platform.
            Filter::TaskVideo => {
                config.has_task(task::WATCH_VIDEO) || config.has_task(task::WATCH_VIDEO_ON_MOBILE)
            }
            // `ee.t`, `fE`, `vl`, `g5` and `Cr`: a desktop, stream, activity, console or in-game achievement task.
            Filter::TaskPlay => {
                config.has_task(task::PLAY_ON_DESKTOP)
                    || config.has_task(task::STREAM_ON_DESKTOP)
                    || config.has_task(task::PLAY_ACTIVITY)
                    || config.has_task(task::PLAY_ON_XBOX)
                    || config.has_task(task::PLAY_ON_PLAYSTATION)
                    || config.has_task(task::ACHIEVEMENT_IN_GAME)
                    || config.has_task(task::ACHIEVEMENT_IN_ACTIVITY)
            }
            Filter::RewardVirtualCurrency => config.has_reward(reward::VIRTUAL_CURRENCY),
            Filter::RewardCollectible => config.has_reward(reward::COLLECTIBLE),
            Filter::RewardInGame => {
                config.has_reward(reward::IN_GAME) || config.has_reward(reward::REWARD_CODE)
            }
        }
    }
}

/// `p` of module 247512: a claimed quest as `/quests/@me/claimed` returns it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaimedQuest {
    pub id: String,
    pub quest_name: String,
    /// `config.rewards[].type` and `.name`, in order.
    pub rewards: Vec<(u64, String)>,
    pub user_status: Option<UserStatus>,
    /// The quest as the service sent it, for the assets of its card (`s$`).
    pub raw: Value,
}

impl ClaimedQuest {
    /// `sn`: the reward of the claimed tier.
    pub fn claimed_reward(&self) -> &Value {
        let tier = self
            .user_status
            .as_ref()
            .and_then(|s| s.claimed_tier)
            .unwrap_or(0) as usize;
        &self.raw["config"]["rewards"][tier]
    }

    pub fn from_value(value: &Value) -> Option<ClaimedQuest> {
        Some(ClaimedQuest {
            id: value["id"].as_str()?.to_owned(),
            quest_name: value["config"]["messages"]["quest_name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            rewards: value["config"]["rewards"]
                .as_array()
                .map(|rewards| {
                    rewards
                        .iter()
                        .map(|reward| {
                            (
                                reward["type"].as_u64().unwrap_or_default(),
                                reward["name"].as_str().unwrap_or_default().to_owned(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            user_status: value["user_status"]
                .is_object()
                .then(|| UserStatus::from_value(&value["user_status"])),
            raw: value.clone(),
        })
    }
}

fn parse_ms(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

/// `ea`: the Suggested order of two quests.
fn suggested(a: &Quest, b: &Quest, now_ms: i64) -> std::cmp::Ordering {
    use std::cmp::Ordering::{Greater, Less};
    let pinned = |quest: &Quest| quest.id == PINNED_QUEST && !quest.is_completed();
    if pinned(a) != pinned(b) {
        return if pinned(a) { Less } else { Greater };
    }
    let stalled = |quest: &Quest| {
        // `e7`: enrolled more than 20 minutes ago and not finished.
        quest.is_enrolled()
            && !quest.is_completed()
            && quest
                .user_status
                .as_ref()
                .and_then(|status| status.enrolled_at.as_deref())
                .and_then(parse_ms)
                .is_some_and(|at| now_ms - at > 20 * 60 * 1000)
    };
    let (a_stalled, b_stalled) = (stalled(a), stalled(b));
    if !a.is_expired(now_ms) {
        if a_stalled != b_stalled && (a_stalled || b_stalled) {
            return if a_stalled { Greater } else { Less };
        }
        if a.is_claimed() != b.is_claimed() {
            return if a.is_claimed() { Greater } else { Less };
        }
        if a.is_enrolled() != b.is_enrolled() {
            return if a.is_enrolled() { Less } else { Greater };
        }
        return a.config.expires_at.cmp(&b.config.expires_at);
    }
    if a.is_claimed() != b.is_claimed() {
        return if a.is_claimed() { Less } else { Greater };
    }
    if a.is_enrolled() != b.is_enrolled() {
        return if a.is_enrolled() { Less } else { Greater };
    }
    b.config.expires_at.cmp(&a.config.expires_at)
}

/// `ed`: Started, the quests the user enrolled in first, the latest enrolment first.
fn recently_enrolled(a: &Quest, b: &Quest) -> std::cmp::Ordering {
    let enrolled = |quest: &Quest| {
        quest
            .user_status
            .as_ref()
            .and_then(|status| status.enrolled_at.clone())
    };
    match (enrolled(a), enrolled(b)) {
        (None, None) => b.config.expires_at.cmp(&a.config.expires_at),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => b.cmp(&a),
    }
}

/// `eI` of module 61387: the quests matching the filters (a group matches when any of its options does, every group
/// that has a selection must match), in the order of `sort`. Unexpired quests come before expired ones in every order.
pub fn filter_and_sort<'a>(
    quests: impl IntoIterator<Item = &'a Quest>,
    sort: SortMethod,
    filters: &[Filter],
    now_ms: i64,
) -> Vec<&'a Quest> {
    let mut kept: Vec<&Quest> = quests
        .into_iter()
        .filter(|quest| {
            [FilterGroup::Task, FilterGroup::Reward]
                .iter()
                .all(|group| {
                    let chosen: Vec<&Filter> = filters
                        .iter()
                        .filter(|filter| filter.group() == *group)
                        .collect();
                    chosen.is_empty() || chosen.iter().any(|filter| quest.matches(**filter))
                })
        })
        .collect();
    kept.sort_by(|a, b| {
        let (a_live, b_live) = (!a.is_expired(now_ms), !b.is_expired(now_ms));
        if a_live != b_live {
            return if a_live {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        match sort {
            SortMethod::Suggested => suggested(a, b, now_ms),
            SortMethod::MostRecent => b.config.starts_at.cmp(&a.config.starts_at),
            SortMethod::RecentlyEnrolled => recently_enrolled(a, b),
            SortMethod::ExpiringSoon => a.config.expires_at.cmp(&b.config.expires_at),
        }
    });
    kept
}

/// The Orbs balance (`VirtualCurrencyStore`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VirtualCurrencyStore {
    pub balance: Option<u64>,
    pub is_fetching_balance: bool,
    pub fetch_balance_failed: bool,
}

impl VirtualCurrencyStore {
    pub fn begin(&mut self) {
        self.is_fetching_balance = true;
        self.fetch_balance_failed = false;
    }

    /// `VIRTUAL_CURRENCY_BALANCE_FETCH_SUCCESS`: `{"balance": n}`.
    pub fn succeed(&mut self, body: &Value) {
        self.is_fetching_balance = false;
        self.balance = body["balance"].as_u64();
        self.fetch_balance_failed = self.balance.is_none();
    }

    pub fn fail(&mut self) {
        self.is_fetching_balance = false;
        self.fetch_balance_failed = true;
    }

    /// The hook `W` of module 309954 fetches once while there is neither a balance nor an error.
    pub fn should_fetch(&self) -> bool {
        self.balance.is_none() && !self.fetch_balance_failed && !self.is_fetching_balance
    }

    /// What the pill shows: the balance, or 0 until it arrives.
    pub fn shown(&self) -> u64 {
        self.balance.unwrap_or(0)
    }
}

/// The quest store: the current quests, the claimed ones and the fetch flags.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuestStore {
    /// `quests`, in the order the server sent them.
    pub quests: Vec<Quest>,
    pub excluded_quests: Vec<String>,
    claimed: BTreeMap<String, ClaimedQuest>,
    pub is_fetching_current_quests: bool,
    pub is_fetching_claimed_quests: bool,
    /// Set when a current-quests answer arrived (`lastFetchedCurrentQuests > 0`).
    pub fetched_current_quests: bool,
    pub enrollment_blocked_until: Option<String>,
    pub access_suspended_until: Option<String>,
    /// Selected task platform per quest id (`selectedTaskPlatform` of module 859703 / `tZ` of module 61387).
    pub selected_task_platform: BTreeMap<String, QuestPlatform>,
    /// Console launch/connection error hints per quest id (module 178540).
    pub console_error_hints: BTreeMap<String, Vec<String>>,
}

impl QuestStore {
    pub fn get_quest(&self, id: &str) -> Option<&Quest> {
        self.quests.iter().find(|quest| quest.id == id)
    }

    pub fn selected_platform(&self, quest_id: &str) -> Option<QuestPlatform> {
        self.selected_task_platform.get(quest_id).copied()
    }

    pub fn set_selected_platform(&mut self, quest_id: &str, platform: Option<QuestPlatform>) {
        if let Some(platform) = platform {
            self.selected_task_platform
                .insert(quest_id.to_owned(), platform);
        } else {
            self.selected_task_platform.remove(quest_id);
        }
    }

    pub fn error_hints(&self, quest_id: &str) -> &[String] {
        self.console_error_hints
            .get(quest_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn set_error_hints(&mut self, quest_id: &str, hints: Vec<String>) {
        if hints.is_empty() {
            self.console_error_hints.remove(quest_id);
        } else {
            self.console_error_hints.insert(quest_id.to_owned(), hints);
        }
    }

    pub fn clear_error_hints(&mut self, quest_id: &str) {
        self.console_error_hints.remove(quest_id);
    }

    pub fn claimed_quests(&self) -> impl Iterator<Item = &ClaimedQuest> {
        self.claimed.values()
    }

    pub fn is_quest_access_suspended(&self) -> bool {
        self.access_suspended_until.is_some()
    }

    pub fn begin_current(&mut self) {
        self.is_fetching_current_quests = true;
    }

    /// `QUESTS_FETCH_CURRENT_QUESTS_FAILURE`.
    pub fn fail_current(&mut self) {
        self.fetched_current_quests = false;
        self.is_fetching_current_quests = false;
    }

    /// `fetchCurrentQuests` then `QUESTS_FETCH_CURRENT_QUESTS_SUCCESS`: quests of an unknown config version are dropped,
    /// and so are those that are neither claimed nor tied to a platform.
    pub fn apply_current(&mut self, body: &Value) {
        self.quests = body["quests"]
            .as_array()
            .map(|quests| {
                quests
                    .iter()
                    .filter_map(Quest::from_value)
                    .filter(|quest| quest.is_claimed() || quest.config.platforms > 0)
                    .collect()
            })
            .unwrap_or_default();
        self.excluded_quests = body["excluded_quests"]
            .as_array()
            .map(|excluded| {
                excluded
                    .iter()
                    .filter_map(|quest| quest["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        self.enrollment_blocked_until = body["quest_enrollment_blocked_until"]
            .as_str()
            .map(str::to_owned);
        self.access_suspended_until = body["quest_access_suspended_until"]
            .as_str()
            .map(str::to_owned);
        self.fetched_current_quests = true;
        self.is_fetching_current_quests = false;
    }

    pub fn begin_claimed(&mut self) {
        self.is_fetching_claimed_quests = true;
    }

    pub fn fail_claimed(&mut self) {
        self.is_fetching_claimed_quests = false;
    }

    /// `QUESTS_FETCH_CLAIMED_QUESTS_SUCCESS`.
    pub fn apply_claimed(&mut self, body: &Value) {
        self.claimed = body["quests"]
            .as_array()
            .map(|quests| {
                quests
                    .iter()
                    .filter_map(ClaimedQuest::from_value)
                    .map(|quest| (quest.id.clone(), quest))
                    .collect()
            })
            .unwrap_or_default();
        self.is_fetching_claimed_quests = false;
    }

    /// The Claimed Quests tab's order (`sZ` of module 635668): the latest claim first.
    pub fn claimed_by_date(&self) -> Vec<&ClaimedQuest> {
        let mut list: Vec<&ClaimedQuest> = self.claimed.values().collect();
        list.sort_by(|a, b| {
            let claimed = |quest: &ClaimedQuest| {
                quest
                    .user_status
                    .as_ref()
                    .and_then(|status| status.claimed_at.clone())
                    .unwrap_or_default()
            };
            claimed(b).cmp(&claimed(a))
        });
        list
    }

    /// `QUESTS_USER_STATUS_UPDATE` (the gateway's `quests_user_status_update`): the status of a known quest changes.
    pub fn apply_user_status(&mut self, status: UserStatus) {
        if let Some(quest) = self
            .quests
            .iter_mut()
            .find(|quest| quest.id == status.quest_id)
        {
            quest.user_status = Some(status);
        }
    }

    /// `QUESTS_USER_STATUS_UPDATE {user_status}` as it arrives on the gateway (module 859703: `$(quest_id, {userStatus})`,
    /// which only touches a quest the store already holds).
    pub fn apply_gateway_status(&mut self, data: &Value) {
        if data["user_status"]["quest_id"].is_string() {
            self.apply_user_status(UserStatus::from_value(&data["user_status"]));
        }
    }

    /// `QUESTS_USER_COMPLETION_UPDATE {quest_enrollment_blocked_until}` (module 859703: `O = t ? new Date(t) : null`).
    pub fn apply_completion_update(&mut self, data: &Value) {
        self.enrollment_blocked_until = data["quest_enrollment_blocked_until"]
            .as_str()
            .map(str::to_owned);
    }

    pub fn clear(&mut self) {
        *self = QuestStore::default();
    }
}

/// A request the official client would send for a quest. Nothing here is sent by this client.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestRequest {
    pub method: &'static str,
    pub route: String,
    pub body: Option<Value>,
}

/// `QuestContent.QUEST_HOME_DESKTOP` (module 696292): the `location` of requests made from this page.
pub const QUEST_HOME_DESKTOP: u64 = 11;

/// Enrol in a quest: `POST /quests/{id}/enroll` (`enrollInQuest`, module 343559). The body is the location, the
/// ad-decision fields (`Kc` of module 710969, `{is_targeted: false}` without a decision) and the two sealed blobs,
/// null here because this page has no ad decision.
pub fn enroll_request(quest_id: &str, location: u64) -> QuestRequest {
    QuestRequest {
        method: "POST",
        route: format!("/quests/{quest_id}/enroll"),
        body: Some(json!({
            "location": location,
            "is_targeted": false,
            "metadata_sealed": null,
            "traffic_metadata_sealed": null,
        })),
    }
}

/// Claim a reward: `POST /quests/{id}/claim-reward` (`claimQuestReward`), the same body plus the `platform`.
pub fn claim_reward_request(quest_id: &str, platform: u64, location: u64) -> QuestRequest {
    QuestRequest {
        method: "POST",
        route: format!("/quests/{quest_id}/claim-reward"),
        body: Some(json!({
            "platform": platform,
            "location": location,
            "is_targeted": false,
            "metadata_sealed": null,
            "traffic_metadata_sealed": null,
        })),
    }
}

/// Progress of a desktop play or stream task: `POST /quests/{id}/heartbeat` (`sendHeartbeat`).
pub fn heartbeat_request(
    quest_id: &str,
    stream_key: &str,
    application_id: Option<&str>,
) -> QuestRequest {
    QuestRequest {
        method: "POST",
        route: format!("/quests/{quest_id}/heartbeat"),
        body: Some(json!({
            "stream_key": stream_key,
            "application_id": application_id,
            "terminal": false,
            "executable_path": null,
            "executable_fingerprint": null,
        })),
    }
}

/// Progress of a video task: `POST /quests/{id}/video-progress` with `{timestamp}` in seconds.
pub fn video_progress_request(quest_id: &str, timestamp: f64) -> QuestRequest {
    QuestRequest {
        method: "POST",
        route: format!("/quests/{quest_id}/video-progress"),
        body: Some(json!({"timestamp": timestamp})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn quest(id: &str, starts: &str, expires: &str, tasks: &[&str], rewards: &[u64]) -> Value {
        json!({
            "id": id,
            "preview": false,
            "config": {
                "config_version": 2,
                "id": id,
                "starts_at": starts,
                "expires_at": expires,
                "messages": {"quest_name": id, "game_title": "g", "game_publisher": "p"},
                "task_config_v2": {
                    "tasks": tasks.iter().map(|task| (task.to_string(), json!({"type": task}))).collect::<serde_json::Map<_, _>>(),
                },
                "rewards_config": {
                    "rewards": rewards.iter().map(|kind| json!({"type": kind})).collect::<Vec<_>>(),
                    "rewards_expire_at": null,
                    "platforms": [0],
                },
            },
            "user_status": null,
        })
    }

    fn store(quests: Vec<Value>) -> QuestStore {
        let mut store = QuestStore::default();
        store.apply_current(&json!({"quests": quests, "excluded_quests": []}));
        store
    }

    #[test]
    fn the_captured_empty_answer_leaves_no_quests() {
        let mut store = QuestStore::default();
        store.begin_current();
        assert!(store.is_fetching_current_quests && !store.fetched_current_quests);
        store.apply_current(&json!({
            "quests": [],
            "excluded_quests": [],
            "quest_enrollment_blocked_until": null,
            "quest_access_suspended_until": null,
        }));
        assert!(store.quests.is_empty() && store.fetched_current_quests);
        assert!(!store.is_fetching_current_quests && !store.is_quest_access_suspended());
    }

    #[test]
    fn a_gateway_status_update_changes_a_known_quest_only() {
        let mut store = store(vec![quest(
            "a",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            &[],
            &[],
        )]);
        store.apply_gateway_status(&json!({"user_status": {"quest_id": "a", "enrolled_at": "2026-02-01T00:00:00Z", "completed_at": null, "claimed_at": null}}));
        assert_eq!(
            store.quests[0]
                .user_status
                .as_ref()
                .and_then(|s| s.enrolled_at.as_deref()),
            Some("2026-02-01T00:00:00Z")
        );
        store.apply_gateway_status(&json!({"user_status": {"quest_id": "zzz"}}));
        assert_eq!(store.quests.len(), 1);
        store.apply_gateway_status(&json!({}));
        assert_eq!(store.quests.len(), 1);
        store.apply_completion_update(
            &json!({"quest_enrollment_blocked_until": "2027-01-01T00:00:00Z"}),
        );
        assert_eq!(
            store.enrollment_blocked_until.as_deref(),
            Some("2027-01-01T00:00:00Z")
        );
        store.apply_completion_update(&json!({"quest_enrollment_blocked_until": null}));
        assert_eq!(store.enrollment_blocked_until, None);
    }

    #[test]
    fn a_failed_fetch_clears_the_fetched_mark() {
        let mut store = QuestStore::default();
        store.apply_current(&json!({"quests": []}));
        store.begin_current();
        store.fail_current();
        assert!(!store.fetched_current_quests && !store.is_fetching_current_quests);
    }

    #[test]
    fn unknown_config_versions_and_platformless_quests_are_dropped() {
        let mut old = quest(
            "a",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            &[],
            &[4],
        );
        old["config"]["config_version"] = json!(1);
        let mut bare = quest(
            "b",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            &[],
            &[4],
        );
        bare["config"]["rewards_config"]["platforms"] = json!([]);
        let mut claimed = bare.clone();
        claimed["id"] = json!("c");
        claimed["user_status"] = json!({"quest_id": "c", "claimed_at": "2026-02-02T00:00:00Z"});
        let ok = quest(
            "d",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            &[],
            &[4],
        );
        let store = store(vec![old, bare, claimed, ok]);
        let ids: Vec<&str> = store.quests.iter().map(|quest| quest.id.as_str()).collect();
        assert_eq!(ids, ["c", "d"]);
    }

    #[test]
    fn claimed_quests_list_the_latest_claim_first() {
        let mut store = QuestStore::default();
        store.begin_claimed();
        store.apply_claimed(&json!({"quests": [
            {"id": "1", "config": {"messages": {"quest_name": "One"}, "rewards": [{"type": 4, "name": "700 Orbs"}]},
             "user_status": {"quest_id": "1", "claimed_at": "2026-03-01T00:00:00Z", "orb_quantity_claimed": 700}},
            {"id": "2", "config": {"messages": {"quest_name": "Two"}, "rewards": []},
             "user_status": {"quest_id": "2", "claimed_at": "2026-04-01T00:00:00Z"}},
        ]}));
        assert!(!store.is_fetching_claimed_quests);
        let order: Vec<&str> = store
            .claimed_by_date()
            .iter()
            .map(|quest| quest.id.as_str())
            .collect();
        assert_eq!(order, ["2", "1"]);
        assert_eq!(
            store.claimed_by_date()[1]
                .user_status
                .as_ref()
                .unwrap()
                .orb_quantity_claimed,
            Some(700)
        );
        assert_eq!(
            store.claimed_by_date()[1].rewards[0],
            (4, "700 Orbs".to_owned())
        );
    }

    #[test]
    fn filters_match_by_group_and_option() {
        let list = [
            Quest::from_value(&quest(
                "play-orbs",
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                &[task::PLAY_ON_DESKTOP],
                &[reward::VIRTUAL_CURRENCY],
            ))
            .unwrap(),
            Quest::from_value(&quest(
                "watch-decoration",
                "2026-01-02T00:00:00Z",
                "2027-01-01T00:00:00Z",
                &[task::WATCH_VIDEO],
                &[reward::COLLECTIBLE],
            ))
            .unwrap(),
            Quest::from_value(&quest(
                "play-game",
                "2026-01-03T00:00:00Z",
                "2027-01-01T00:00:00Z",
                &[task::ACHIEVEMENT_IN_GAME],
                &[reward::IN_GAME],
            ))
            .unwrap(),
        ];
        let ids = |filters: &[Filter]| -> Vec<String> {
            filter_and_sort(&list, SortMethod::MostRecent, filters, NOW)
                .iter()
                .map(|quest| quest.id.clone())
                .collect()
        };
        assert_eq!(ids(&[]).len(), 3);
        assert_eq!(ids(&[Filter::TaskVideo]), ["watch-decoration"]);
        assert_eq!(ids(&[Filter::TaskPlay]), ["play-game", "play-orbs"]);
        // Two options of one group widen the result; a second group narrows it.
        assert_eq!(
            ids(&[Filter::RewardVirtualCurrency, Filter::RewardInGame]).len(),
            2
        );
        assert_eq!(
            ids(&[Filter::TaskPlay, Filter::RewardInGame]),
            ["play-game"]
        );
        assert!(ids(&[Filter::TaskVideo, Filter::RewardInGame]).is_empty());
    }

    #[test]
    fn sorts_follow_the_official_comparators() {
        let mut enrolled = quest(
            "enrolled",
            "2026-01-01T00:00:00Z",
            "2027-06-01T00:00:00Z",
            &[],
            &[4],
        );
        enrolled["user_status"] =
            json!({"quest_id": "enrolled", "enrolled_at": "2026-05-05T00:00:00Z"});
        let soon = quest(
            "soon",
            "2026-02-01T00:00:00Z",
            "2027-02-01T00:00:00Z",
            &[],
            &[4],
        );
        let late = quest(
            "late",
            "2026-03-01T00:00:00Z",
            "2027-09-01T00:00:00Z",
            &[],
            &[4],
        );
        let expired = quest(
            "expired",
            "2026-04-01T00:00:00Z",
            "2026-05-01T00:00:00Z",
            &[],
            &[4],
        );
        let list: Vec<Quest> = [enrolled, soon, late, expired]
            .iter()
            .filter_map(Quest::from_value)
            .collect();
        let order = |sort| -> Vec<String> {
            filter_and_sort(&list, sort, &[], NOW)
                .iter()
                .map(|quest| quest.id.clone())
                .collect()
        };
        // Expired quests always go last. Suggested sinks a quest enrolled long ago and still unfinished
        // (`e7`), then orders by the earliest end.
        assert_eq!(
            order(SortMethod::Suggested),
            ["soon", "late", "enrolled", "expired"]
        );
        assert_eq!(
            order(SortMethod::MostRecent),
            ["late", "soon", "enrolled", "expired"]
        );
        assert_eq!(
            order(SortMethod::ExpiringSoon),
            ["soon", "enrolled", "late", "expired"]
        );
        assert_eq!(
            order(SortMethod::RecentlyEnrolled),
            ["enrolled", "late", "soon", "expired"]
        );
    }

    #[test]
    fn expiry_reads_the_end_date() {
        let past = Quest::from_value(&quest(
            "p",
            "2026-01-01T00:00:00Z",
            "2026-05-01T00:00:00Z",
            &[],
            &[4],
        ))
        .unwrap();
        let unreadable =
            Quest::from_value(&quest("u", "2026-01-01T00:00:00Z", "soon", &[], &[4])).unwrap();
        assert!(past.is_expired(NOW));
        assert!(!unreadable.is_expired(NOW));
    }

    #[test]
    fn a_user_status_update_replaces_the_status_of_a_known_quest() {
        let mut store = store(vec![quest(
            "q",
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            &[],
            &[4],
        )]);
        assert!(!store.quests[0].is_enrolled());
        store.apply_user_status(UserStatus::from_value(
            &json!({"quest_id": "q", "enrolled_at": "2026-06-06T00:00:00Z"}),
        ));
        assert!(store.get_quest("q").unwrap().is_enrolled());
        store.apply_user_status(UserStatus::from_value(&json!({"quest_id": "other"})));
        assert_eq!(store.quests.len(), 1);
    }

    #[test]
    fn the_balance_is_fetched_once_and_reads_zero_until_it_arrives() {
        let mut balance = VirtualCurrencyStore::default();
        assert!(balance.should_fetch() && balance.shown() == 0);
        balance.begin();
        assert!(!balance.should_fetch());
        balance.succeed(&json!({"balance": 0}));
        assert_eq!((balance.balance, balance.shown()), (Some(0), 0));
        assert!(!balance.should_fetch());
        let mut failed = VirtualCurrencyStore::default();
        failed.begin();
        failed.fail();
        assert!(!failed.should_fetch() && failed.shown() == 0);
        balance.succeed(&json!({"balance": 1250}));
        assert_eq!(balance.shown(), 1250);
    }

    #[test]
    fn sort_and_filter_names_match_the_page() {
        let labels: Vec<&str> = SortMethod::ALL.iter().map(|sort| sort.label()).collect();
        assert_eq!(
            labels,
            ["Suggested", "Most Recent", "Expiring Soon", "Started"]
        );
        let groups: Vec<(&str, Vec<&str>)> = Filter::GROUPS
            .iter()
            .map(|(_, heading, options)| {
                (
                    *heading,
                    options
                        .iter()
                        .flatten()
                        .map(|filter| filter.label())
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            groups,
            [
                (
                    "Reward",
                    vec!["Orbs", "Avatar Decoration", "In-Game Rewards"]
                ),
                ("Quest Type", vec!["Play", "Watch"]),
            ]
        );
        for filter in Filter::GROUPS
            .iter()
            .flat_map(|group| group.2.iter().flatten())
        {
            assert_eq!(Filter::from_wire(filter.wire()), Some(*filter));
        }
        assert_eq!(Filter::from_wire("everything"), None);
    }

    #[test]
    fn the_account_writes_are_described_but_not_sent() {
        let enroll = enroll_request("42", QUEST_HOME_DESKTOP);
        assert_eq!(
            (enroll.method, enroll.route.as_str()),
            ("POST", "/quests/42/enroll")
        );
        assert_eq!(
            enroll.body,
            Some(json!({
                "location": 11,
                "is_targeted": false,
                "metadata_sealed": null,
                "traffic_metadata_sealed": null,
            }))
        );
        let claim = claim_reward_request("42", 4, QUEST_HOME_DESKTOP);
        assert_eq!(claim.route, "/quests/42/claim-reward");
        assert_eq!(claim.body.as_ref().unwrap()["platform"], json!(4));
        assert_eq!(claim.body.as_ref().unwrap()["location"], json!(11));
        let beat = heartbeat_request("42", "call:1:2", Some("7"));
        assert_eq!(beat.route, "/quests/42/heartbeat");
        assert_eq!(beat.body.as_ref().unwrap()["stream_key"], json!("call:1:2"));
        assert_eq!(beat.body.as_ref().unwrap()["terminal"], json!(false));
        let video = video_progress_request("42", 12.5);
        assert_eq!(video.route, "/quests/42/video-progress");
        assert_eq!(video.body, Some(json!({"timestamp": 12.5})));
    }
}
