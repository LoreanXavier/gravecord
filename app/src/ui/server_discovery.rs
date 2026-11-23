//! Server Settings > Access > Discoverable (module 989433 `A3`, `AP`, `AM`, `A1`; view rule module 716610; store module
//! 306846; requests module 460760).
//!
//! Choosing Discoverable reads `GET /guilds/{id}/discovery-requirements`, `GET /guilds/{id}/discovery-metadata` and
//! `GET /discovery/categories`. What the requirements say picks the view (`716610 l`): a server that fails them gets the
//! checklist (`AP`) and a bar that can only be reset; one that passes gets the listing form (`A1`).
//! Measured in `design/dom/settings/guild-access-disc-*`, `guild-access-elig-*`.
//!
//! Not drawn: the removed-from-Discovery header (`n1LDQM`), which the fixture cannot reach, and the activity-health
//! dialog behind "Click here for more details" (module 247920).

use super::{Root, color, icon};
use crate::editor::{Editor, EditorEvent};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Subscription, div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// Test runs: the fixture server fails the requirements, so the listing form is shown as if it passed them.
#[cfg(feature = "capture")]
pub(crate) static FORCE_ELIGIBLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub(super) const DISCOVERY_PREFIX: &str = "guild-discovery:";
const REQUIREMENTS_PREFIX: &str = "guild-discovery:requirements:";
const METADATA_PREFIX: &str = "guild-discovery:metadata:";
const CATEGORIES_PREFIX: &str = "guild-discovery:categories:";
const GUILD_PREFIX: &str = "guild-discovery:guild:";
const CATEGORY_PREFIX: &str = "guild-discovery:category:";
pub(super) const SAVED_GUILD_PREFIX: &str = "guild-discovery:saved-guild:";
pub(super) const SAVED_META_PREFIX: &str = "guild-discovery:saved-meta:";
/// Modules 848887 and 617416: the clipboard before and after the requirements are met.
const CLIPBOARD_FAILING: &str = "https://discord.com/assets/47fa1a531cbfd55c.svg";
const CLIPBOARD_PASSING: &str = "https://discord.com/assets/eaee201e4840b582.svg";
/// The banner behind the preview card before one is chosen.
pub(super) const DEFAULT_SPLASH: &str = "https://discord.com/assets/b00e3f0a7090b61f.svg";
const TERMS_URL: &str = "https://discord.com/terms";
/// `maxTags` of the tags field in `AZ`.
pub(super) const MAX_TAGS: usize = 10;

/// `AW.v`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Ineligible,
    Disqualified,
    EligibleDisabled,
    EligibleEnabled,
}

/// Module 716610 `l`.
pub(crate) fn settings_view(
    features: &[String],
    requirements: &Value,
    now: chrono::DateTime<chrono::Utc>,
) -> View {
    let has = |name: &str| features.iter().any(|feature| feature == name);
    let discoverable = has("DISCOVERABLE");
    if has("VERIFIED") {
        return if discoverable {
            View::EligibleEnabled
        } else {
            View::EligibleDisabled
        };
    }
    let sufficient = requirements["sufficient"] == true;
    if discoverable {
        let in_grace = requirements["sufficient_without_grace_period"] == true
            && requirements["grace_period_end_date"]
                .as_str()
                .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                .is_some_and(|end| end > now);
        return if sufficient || in_grace {
            View::EligibleEnabled
        } else {
            View::Disqualified
        };
    }
    if sufficient {
        View::EligibleDisabled
    } else {
        View::Ineligible
    }
}

/// `AM`'s input: one row of the checklist.
struct Item {
    name: String,
    description: Row,
    /// `Some(true)` passes, `Some(false)` fails, `None` is not known yet (never shown as passing).
    checked: bool,
    pending: bool,
}

/// A row's text: plain, or with a link in it.
enum Row {
    Text(String),
    Link {
        before: String,
        link: String,
        after: String,
        action: LinkAction,
    },
    Lines(Vec<String>),
}

#[derive(Clone, Copy)]
enum LinkAction {
    Safety,
    Url(&'static str),
}

/// `guildMetadata` in the settings store: what `GET /guilds/{id}/discovery-metadata` and its `PATCH` carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Meta {
    pub primary: u64,
    pub secondary: Vec<u64>,
    pub keywords: Vec<String>,
    pub emoji: bool,
    pub published: bool,
    /// Kept as the server sent them: the landing page edits these, this page sends them back unchanged.
    pub reasons: Value,
    pub social: Value,
    pub about: Value,
    pub partner_actioned: Value,
    pub partner_application: Value,
}

impl Meta {
    pub(crate) fn from_body(body: &Value) -> Self {
        let numbers = |name: &str| -> Vec<u64> {
            body[name]
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default()
        };
        Self {
            primary: body["primary_category_id"].as_u64().unwrap_or(0),
            secondary: numbers("category_ids"),
            keywords: body["keywords"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|tag| tag.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            emoji: body["emoji_discoverability_enabled"] == true,
            published: body["is_published"] == true,
            reasons: body["reasons_to_join"].clone(),
            social: body["social_links"].clone(),
            about: body["about"].clone(),
            partner_actioned: body["partner_actioned_timestamp"].clone(),
            partner_application: body["partner_application_timestamp"].clone(),
        }
    }

    /// The body of `PATCH /guilds/{id}/discovery-metadata` (`ux.Oh`).
    pub(crate) fn patch_body(&self) -> Value {
        // A link row nobody filled in is not sent (`filter(Boolean)` in `ux.Oh`).
        let social = match self.social.as_array() {
            Some(list) => Value::Array(
                list.iter()
                    .filter(|link| link.as_str().is_some_and(|link| !link.is_empty()))
                    .cloned()
                    .collect(),
            ),
            None => self.social.clone(),
        };
        json!({
            "primary_category_id": self.primary,
            "emoji_discoverability_enabled": self.emoji,
            "partner_actioned_timestamp": self.partner_actioned,
            "partner_application_timestamp": self.partner_application,
            "keywords": self.keywords,
            "is_published": self.published,
            "reasons_to_join": self.reasons,
            "social_links": social,
            "about": self.about,
        })
    }

    /// What Save Changes would send: the subcategories go through their own calls the moment they are picked.
    fn differs(&self, saved: &Self) -> bool {
        let mut left = self.clone();
        left.secondary.clone_from(&saved.secondary);
        left != *saved
    }
}

/// One entry of `GET /discovery/categories`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Category {
    pub id: u64,
    pub name: String,
    pub primary: bool,
}

/// Module 997590 `GUILD_DISCOVERY_CATEGORY_FETCH_SUCCESS`: sorted by name, General is not offered.
pub(crate) fn categories_from(body: &Value) -> Vec<Category> {
    let mut list: Vec<Category> = body
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    Some(Category {
                        id: entry["id"].as_u64()?,
                        name: entry["name"].as_str()?.to_owned(),
                        primary: entry["is_primary"] == true,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by(|left, right| left.name.cmp(&right.name));
    list.retain(|category| category.id != 0);
    list
}

/// `getPrimaryCategories`: Other (13) comes after the rest.
pub(crate) fn primary_categories(all: &[Category]) -> Vec<&Category> {
    let mut list: Vec<&Category> = all
        .iter()
        .filter(|category| category.primary && category.id != 13)
        .collect();
    list.extend(all.iter().filter(|category| category.id == 13));
    list
}

/// `getAllCategories`: Other is not a subcategory.
pub(crate) fn all_categories(all: &[Category]) -> Vec<&Category> {
    all.iter().filter(|category| category.id != 13).collect()
}

/// The guild fields this page edits (`discoverySplash`, `description`, `preferredLocale`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Bits {
    pub description: Option<String>,
    pub locale: String,
    /// The hash on the server, or a data URI for a picked file.
    pub splash: Option<String>,
}

impl Bits {
    fn from_body(body: &Value) -> Self {
        Self {
            description: body["description"].as_str().map(str::to_owned),
            locale: body["preferred_locale"]
                .as_str()
                .unwrap_or("en-US")
                .to_owned(),
            splash: body["discovery_splash"].as_str().map(str::to_owned),
        }
    }
}

/// The three selects on the listing form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Select {
    Category,
    Subcategories,
    Language,
}

impl Select {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Category => "discovery-category",
            Self::Subcategories => "discovery-subcategories",
            Self::Language => "discovery-language",
        }
    }
}

impl Discovery {
    /// `f` of the tags field (module 513521) with `AZ`'s `o`: an empty entry does nothing; with `maxTags` reached the
    /// typed text stays and `onAddTagError` says "Cannot add anymore tags."; otherwise a new tag is added, the error
    /// goes and the field empties. Returns what the field holds afterwards.
    pub(super) fn submit_tag(&mut self, typed: &str) -> String {
        let tag = typed.trim();
        if tag.is_empty() {
            return typed.to_owned();
        }
        if self.meta.keywords.len() >= MAX_TAGS {
            self.tag_error = Some(dc_intl::t("Xx7XeB").to_owned());
            return typed.to_owned();
        }
        if !self.meta.keywords.iter().any(|known| known == tag) {
            self.meta.keywords.push(tag.to_owned());
            self.tag_error = None;
        }
        String::new()
    }
}

/// What the reads brought and what the form holds meanwhile.
#[derive(Default)]
pub(crate) struct Discovery {
    /// The requirements body, once read.
    pub(super) requirements: Option<Value>,
    pub(super) guild: Option<Id>,
    pub(super) saved_bits: Option<Bits>,
    pub(super) bits: Bits,
    pub(super) saved_meta: Option<Meta>,
    pub(super) meta: Meta,
    pub(super) categories: Vec<Category>,
    pub(super) open: Option<Select>,
    pub(super) description: Option<Entity<Editor>>,
    pub(super) tag_input: Option<Entity<Editor>>,
    /// The text box of an open select: it filters the list.
    pub(super) filter: Option<Entity<Editor>>,
    pub(super) scroll: gpui::ScrollHandle,
    pub(super) tag_error: Option<String>,
    pub(super) error: Option<String>,
    /// Save Changes is waiting for the server.
    pub(super) saving: bool,
    /// The "List on Discovery" agreement and its three boxes.
    pub(super) agreement: Option<[bool; 3]>,
    /// The promises were ticked: the next save goes through.
    pub(super) agreed: bool,
    /// Members the guild counts (`approximate_member_count`), for the preview card.
    pub(super) members: u64,
    pub(super) _events: Vec<Subscription>,
}

impl Discovery {
    /// The form is ready once the guild and its listing are read.
    pub(super) fn loaded(&self) -> bool {
        self.saved_bits.is_some() && self.saved_meta.is_some()
    }

    /// `guildDirty`: something Save Changes would send.
    pub(super) fn dirty(&self) -> bool {
        match (&self.saved_bits, &self.saved_meta) {
            (Some(bits), Some(meta)) => self.bits != *bits || self.meta.differs(meta),
            _ => false,
        }
    }

    /// `S` in `Il`: a description, a category and a tag are what listing needs.
    pub(super) fn listable(&self) -> bool {
        self.bits.description.is_some() && self.meta.primary != 0 && !self.meta.keywords.is_empty()
    }
}

/// `c2` in module 989433: the tags suggested for each primary category.
const POPULAR_TAGS: &[(u64, &[&str])] = &[
    (
        14,
        &[
            "gaming",
            "roblox",
            "youtube",
            "twitch",
            "minecraft",
            "community",
            "streamer",
            "fortnite",
            "anime",
            "roleplay",
        ],
    ),
    (
        1,
        &[
            "minecraft",
            "roblox",
            "twitch",
            "youtube",
            "fortnite",
            "esports",
            "roleplay",
            "valorant",
            "leagueoflegends",
            "fivem",
        ],
    ),
    (
        2,
        &[
            "edm",
            "rap",
            "kpop",
            "youtube",
            "music-production",
            "hiphop",
            "singing",
            "gaming",
            "bts",
            "metal",
        ],
    ),
    (
        3,
        &[
            "anime", "youtube", "gaming", "twitch", "manga", "memes", "streamer", "social",
            "reddit",
        ],
    ),
    (
        4,
        &[
            "art",
            "anime",
            "design",
            "youtube",
            "animation",
            "3d",
            "nft",
            "manga",
            "drawing",
            "photography",
        ],
    ),
    (
        5,
        &[
            "programming",
            "tech",
            "coding",
            "github",
            "opensource",
            "javascript",
            "cryptocurrency",
            "hardware",
            "software",
            "linux",
        ],
    ),
    (
        6,
        &[
            "study",
            "language",
            "school",
            "english",
            "learning",
            "homework",
            "programming",
            "college",
            "stocks",
            "youtube",
        ],
    ),
    (
        7,
        &[
            "football",
            "nba",
            "nfl",
            "soccer",
            "basketball",
            "formula1",
            "premier-league",
            "racing",
            "reddit",
            "fantasy-football",
        ],
    ),
    (
        8,
        &[
            "fashion",
            "streetwear",
            "sneakers",
            "clothing",
            "reddit",
            "yeezy",
            "supreme",
            "hypebeast",
            "style",
        ],
    ),
    (9, &["lgbtq", "anime", "social", "friends", "community"]),
    (
        10,
        &[
            "food",
            "disney",
            "beverage",
            "snacks",
            "youtube",
            "travel",
            "disneyworld",
        ],
    ),
    (
        11,
        &[
            "gym",
            "bodybuilding",
            "exercise",
            "calisthenics",
            "mental-health",
            "self-improvement",
            "education",
            "crossfit",
            "nutrition",
            "sports",
        ],
    ),
    (
        12,
        &[
            "stocks",
            "trading",
            "investing",
            "bitcoin",
            "cryptocurrency",
            "ethereum",
            "options",
            "defi",
            "forex",
        ],
    ),
    (
        13,
        &[
            "gaming",
            "youtube",
            "twitch",
            "community",
            "anime",
            "events",
            "social",
            "memes",
        ],
    ),
    (
        43,
        &[
            "emoji",
            "emotes",
            "nitro",
            "anime",
            "cute",
            "giveaways",
            "aesthetic",
            "art",
            "memes",
        ],
    ),
    (
        49,
        &[
            "apps",
            "bots",
            "discord bot",
            "giveaways",
            "support",
            "fun",
            "memes",
        ],
    ),
];

pub(crate) fn popular_tags(primary: u64) -> &'static [&'static str] {
    POPULAR_TAGS
        .iter()
        .find(|(id, _)| *id == primary)
        .map_or(&[], |(_, tags)| tags)
}

/// `Math.ceil(minimumGuildAge / 7)` weeks, in en-US plural.
fn weeks(days: u64) -> String {
    let weeks = days.div_ceil(7);
    if weeks == 1 {
        "1 week".to_owned()
    } else {
        format!("{weeks} weeks")
    }
}

/// `toLocaleString` for a count.
pub(super) fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `moment(...).format("LL")` in en-US: "January 2, 2027".
fn long_date(at: chrono::DateTime<chrono::Utc>) -> String {
    use chrono::Datelike;
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let local = at.with_timezone(&chrono::Local);
    format!(
        "{} {}, {}",
        MONTHS[local.month0() as usize],
        local.day(),
        local.year()
    )
}

/// The six rows of `AP`, in the order the bundle lists them.
fn checklist_items(requirements: &Value, guild: Id, partnered: bool) -> Vec<Item> {
    let flag = |name: &str| requirements[name] == true;
    let minimum_size = requirements["minimum_size"].as_u64().unwrap_or(1000);
    let minimum_age = requirements["minimum_age"].as_u64().unwrap_or(56);
    let created =
        chrono::DateTime::from_timestamp_millis(((guild >> 22) + 1_420_070_400_000) as i64)
            .unwrap_or_default();
    let pass_date = long_date(created + chrono::Duration::days(minimum_age as i64));
    let size = grouped(minimum_size);
    let age = weeks(minimum_age);
    let pending = flag("health_score_pending");
    let mut items = Vec::new();
    items.push(Item {
        name: if flag("safe_environment") {
            "Safe Environment".to_owned()
        } else {
            "Flagged By Trust & Safety".to_owned()
        },
        description: if flag("safe_environment") {
            Row::Text(
                "Thanks for abiding by our Community Guidelines and keeping your server safe!"
                    .to_owned(),
            )
        } else {
            Row::Link {
                before: "Your server was in violation of our ".to_owned(),
                link: "Terms of Service".to_owned(),
                after: " or Community Server Guidelines.".to_owned(),
                action: LinkAction::Url(TERMS_URL),
            }
        },
        checked: flag("safe_environment"),
        pending: false,
    });
    items.push(Item {
        name: if flag("size") {
            format!("{size}+ Members")
        } else {
            format!("Fewer Than {size} Members")
        },
        description: Row::Text(if flag("size") {
            String::new()
        } else if partnered {
            format!(
                "Unfortunately, Discovery is limited to only servers with more than {size} members."
            )
        } else {
            format!("Your server must have at least {size} members to be eligible.")
        }),
        checked: flag("size"),
        pending: false,
    });
    items.push(Item {
        name: if flag("age") {
            "Meets Age Requirement".to_owned()
        } else {
            "Server Too Young".to_owned()
        },
        description: Row::Text(if flag("age") {
            format!("Servers in Discovery have to be at least {age} old.")
        } else {
            format!(
                "Servers in Discovery need to be at least {age} old. Please check back on {pass_date}."
            )
        }),
        checked: flag("age"),
        pending: false,
    });
    let healthy = flag("healthy");
    let mut health_lines =
        vec!["Uh oh, your server does not currently pass our activity requirements.".to_owned()];
    if !flag("engagement_healthy") {
        health_lines.push("Your server needs more members who visit and talk weekly.".to_owned());
    }
    if !flag("retention_healthy") {
        health_lines.push(
            "Your retention is too low. Try improving your new member onboarding experience."
                .to_owned(),
        );
    }
    items.push(Item {
        name: if pending {
            "Waiting on Server Activity Metrics".to_owned()
        } else if healthy {
            "Passed Server Activity Requirements".to_owned()
        } else {
            "Failed Server Activity Requirements".to_owned()
        },
        description: if pending {
            Row::Text(if flag("size") {
                "Servers in Discovery must meet certain activity requirements. This takes time to calculate for newer or smaller servers, so check back in about a week.".to_owned()
            } else {
                "We\u{2019}re unable to calculate your server\u{2019}s activity metrics until your server has at least 200 members. Servers in Discovery must meet certain activity requirements.".to_owned()
            })
        } else if healthy {
            Row::Text("Your community metrics look good! Your server has enough members that regularly visit and talk.".to_owned())
        } else {
            Row::Lines(health_lines)
        },
        checked: healthy,
        pending,
    });
    let nsfw = &requirements["nsfw_properties"];
    let clean = nsfw
        .as_object()
        .is_none_or(|properties| properties.is_empty());
    let mut bad = Vec::new();
    if nsfw["channels"].is_array() || nsfw["channels_banned_keywords"].is_object() {
        bad.push("You need to change one or more channel names".to_owned());
    }
    if nsfw["name"].is_string() {
        bad.push("You need to change your server name".to_owned());
    }
    if nsfw["description"].is_string() {
        bad.push("You need to change your description".to_owned());
    }
    if ["icon", "discovery_splash", "invite_splash", "banner"]
        .iter()
        .any(|name| !nsfw[*name].is_null())
    {
        bad.push("One or more of your server assets are not appropriate for Discovery!".to_owned());
    }
    items.push(Item {
        name: if clean {
            "No Bad Content".to_owned()
        } else {
            "Bad Content Found".to_owned()
        },
        description: if clean {
            Row::Text("Your server assets look appropriate for Discovery!".to_owned())
        } else {
            Row::Lines(bad)
        },
        checked: clean,
        pending: false,
    });
    let protected = flag("protected");
    items.push(Item {
        name: if protected {
            "2FA Requirement For Moderation Enabled".to_owned()
        } else {
            "Please Enable the [2FA Requirement For Moderation] Setting".to_owned()
        },
        description: if protected {
            Row::Text("Members with moderation powers (e.g. admins or mods) are required to have 2FA enabled to perform moderation actions.".to_owned())
        } else {
            Row::Link {
                before: "This setting will require all members with moderation powers to have 2FA enabled before they can perform moderation actions. This can help protect against compromised admin or mod accounts. ".to_owned(),
                link: "Enable at the bottom of the Safety Setup tab.".to_owned(),
                after: String::new(),
                action: LinkAction::Safety,
            }
        },
        checked: protected,
        pending: false,
    });
    items
}

/// The words the page falls back on when the server sends none (`aTVNes`).
fn generic_error() -> String {
    "Oops! You've caught an ultra rare error. This is probably our fault, so please try again or check our status page."
        .to_owned()
}

/// `GUILD_DISCOVERY_SPLASH`: where a saved banner lives.
pub(super) fn splash_url(guild: Id, hash: &str) -> String {
    format!("https://cdn.discordapp.com/discovery-splashes/{guild}/{hash}.png?size=512")
}

/// The features Save Changes sends for a listing: Discoverable added, manual approval dropped.
pub(crate) fn listed_features(features: &[String]) -> Vec<String> {
    let mut list: Vec<String> = features
        .iter()
        .filter(|feature| *feature != "MEMBER_VERIFICATION_MANUAL_APPROVAL")
        .cloned()
        .collect();
    if !list.iter().any(|feature| feature == "DISCOVERABLE") {
        list.push("DISCOVERABLE".to_owned());
    }
    list
}

impl Root {
    fn discovery_mut(&mut self) -> Option<&mut Discovery> {
        self.server_settings
            .as_mut()
            .map(|open| &mut open.discovery)
    }

    /// Escape: an open category or language list closes before the page does.
    pub(super) fn close_discovery_layer(&mut self) -> bool {
        self.discovery_mut().is_some_and(|discovery| {
            let was = discovery.open.take().is_some();
            if was {
                discovery.filter = None;
            }
            was
        })
    }

    /// `i` in `Ie`: the page asks again every time Discoverable is chosen.
    pub(super) fn request_discovery(&mut self, guild: Id) {
        use crate::net::Request;
        if let Some(discovery) = self.discovery_mut() {
            *discovery = Discovery {
                guild: Some(guild),
                ..Discovery::default()
            };
        }
        for (key, route) in [
            (
                format!("{REQUIREMENTS_PREFIX}{guild}"),
                format!("/guilds/{guild}/discovery-requirements"),
            ),
            (
                format!("{METADATA_PREFIX}{guild}"),
                format!("/guilds/{guild}/discovery-metadata"),
            ),
            (
                format!("{CATEGORIES_PREFIX}{guild}"),
                "/discovery/categories?locale=en-US&primary_only=false".to_owned(),
            ),
            (
                format!("{GUILD_PREFIX}{guild}"),
                format!("/guilds/{guild}?with_counts=true"),
            ),
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(key),
                method: reqwest::Method::GET,
                route,
                body: None,
            });
        }
    }

    pub(super) fn discovery_send(
        &mut self,
        key: String,
        method: reqwest::Method,
        route: String,
        body: Option<Value>,
    ) {
        let _ = self.requests.send(crate::net::Request::Rest {
            key: Some(key),
            method,
            route,
            body,
        });
    }

    pub(super) fn discovery_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        let ok = report["ok"] == true;
        let body = report["body"].clone();
        let message = body["message"].as_str().map(str::to_owned);
        let guild = self.discovery_mut().and_then(|discovery| discovery.guild);
        if key.starts_with(REQUIREMENTS_PREFIX) {
            if ok && let Some(discovery) = self.discovery_mut() {
                discovery.requirements = Some(body);
            }
        } else if key.starts_with(GUILD_PREFIX) {
            if ok {
                self.discovery_read_guild(&body, cx);
            }
        } else if key.starts_with(METADATA_PREFIX) {
            if ok {
                self.discovery_read_meta(&body, cx);
            }
        } else if key.starts_with(CATEGORIES_PREFIX) {
            if ok && let Some(discovery) = self.discovery_mut() {
                discovery.categories = categories_from(&body);
            }
        } else if key.starts_with(CATEGORY_PREFIX) {
            if !ok {
                if let Some(discovery) = self.discovery_mut() {
                    discovery.error = message;
                }
                if let Some(guild) = guild {
                    self.discovery_send(
                        format!("{METADATA_PREFIX}{guild}"),
                        reqwest::Method::GET,
                        format!("/guilds/{guild}/discovery-metadata"),
                        None,
                    );
                }
            }
        } else if key.starts_with(SAVED_GUILD_PREFIX) {
            let Some(guild) = guild else { return };
            if ok {
                let meta = self.discovery_mut().map(|discovery| discovery.meta.clone());
                if let Some(meta) = meta {
                    self.discovery_send(
                        format!("{SAVED_META_PREFIX}{guild}"),
                        reqwest::Method::PATCH,
                        format!("/guilds/{guild}/discovery-metadata"),
                        Some(meta.patch_body()),
                    );
                }
            } else if let Some(discovery) = self.discovery_mut() {
                discovery.saving = false;
                discovery.error = Some(message.unwrap_or_else(generic_error));
            }
        } else if key.starts_with(SAVED_META_PREFIX) {
            let Some(guild) = guild else { return };
            if ok {
                self.request_discovery(guild);
                self.request_access_form(guild);
            } else if let Some(discovery) = self.discovery_mut() {
                discovery.saving = false;
                discovery.error = Some(message.unwrap_or_else(generic_error));
            }
        }
        cx.notify();
    }

    fn discovery_read_guild(&mut self, body: &Value, cx: &mut Context<Self>) {
        let bits = Bits::from_body(body);
        let members = body["approximate_member_count"].as_u64().unwrap_or(0);
        let editor = cx.new(|cx| {
            // `.textArea_uI0xjn` keeps `line-height: normal`: 21px for gg sans at 16px.
            let mut style = super::editor_style();
            style.line_height = 21.;
            let mut editor = Editor::new(style, cx);
            editor.set_placeholder("Tell the world a bit about this server.".to_owned());
            editor.set_text(bits.description.as_deref().unwrap_or_default());
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            let text = editor.read(cx).text().to_owned();
            match event {
                EditorEvent::Submit(submitted) => {
                    let submitted = submitted.clone();
                    editor.update(cx, |editor, _| editor.set_text(&submitted));
                }
                EditorEvent::Edited => {
                    let cut: String = text.chars().take(300).collect();
                    if cut != text {
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                    if let Some(discovery) = this.discovery_mut() {
                        discovery.bits.description = (!cut.is_empty()).then_some(cut);
                    }
                }
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        let splash = bits
            .splash
            .clone()
            .zip(self.discovery_mut().and_then(|discovery| discovery.guild));
        if let Some(discovery) = self.discovery_mut() {
            discovery.members = members;
            discovery.saved_bits = Some(bits.clone());
            discovery.bits = bits;
            discovery.description = Some(editor);
            discovery._events.push(events);
        }
        if let Some((hash, guild)) = splash {
            let url = splash_url(guild, &hash);
            self.request_image(url.clone(), url);
        }
    }

    fn discovery_read_meta(&mut self, body: &Value, cx: &mut Context<Self>) {
        let meta = Meta::from_body(body);
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder(String::new());
            editor
        });
        let events = cx.subscribe(&editor, |this, editor, event: &EditorEvent, cx| {
            let text = editor.read(cx).text().to_owned();
            match event {
                EditorEvent::Submit(submitted) => {
                    let submitted = submitted.clone();
                    this.discovery_submit_tag(&editor, &submitted, cx);
                }
                EditorEvent::Edited => {
                    let cut: String = text.chars().take(30).collect();
                    if cut != text {
                        editor.update(cx, |editor, _| editor.set_text(&cut));
                    }
                }
                EditorEvent::Cancel => {}
            }
            cx.notify();
        });
        let filter = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Select...".to_owned());
            editor
        });
        let filter_events = cx.subscribe(&filter, |_, _, _: &EditorEvent, cx| cx.notify());
        if let Some(discovery) = self.discovery_mut() {
            discovery.saved_meta = Some(meta.clone());
            discovery.meta = meta;
            discovery.tag_input = Some(editor);
            discovery.filter = Some(filter);
            discovery._events.push(events);
            discovery._events.push(filter_events);
        }
        self.landing_sync(cx);
    }

    /// `o` in `AZ`: ten tags at most, none twice; an added tag clears the field's error.
    pub(super) fn discovery_add_tag(&mut self, tag: String, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut()
            && !tag.is_empty()
            && discovery.meta.keywords.len() < MAX_TAGS
            && !discovery.meta.keywords.contains(&tag)
        {
            discovery.meta.keywords.push(tag);
            discovery.tag_error = None;
        }
        cx.notify();
    }

    /// `f` of the tags field (module 513521): with `maxTags` reached the typed tag stays in the field and
    /// `onAddTagError` shows "Cannot add anymore tags."; otherwise it is added and the field empties.
    fn discovery_submit_tag(
        &mut self,
        editor: &Entity<Editor>,
        typed: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(discovery) = self.discovery_mut() else {
            return;
        };
        let kept = discovery.submit_tag(typed);
        editor.update(cx, |editor, _| editor.set_text(&kept));
        cx.notify();
    }

    pub(super) fn discovery_remove_tag(&mut self, tag: &str, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.meta.keywords.retain(|known| known != tag);
        }
        cx.notify();
    }

    /// `GX`: the primary category, 0 when cleared.
    pub(super) fn discovery_set_primary(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.meta.primary = id;
            discovery.open = None;
        }
        cx.notify();
    }

    /// `up`'s `onSelectionChange`: each pick or removal is its own call, five at most.
    pub(super) fn discovery_toggle_secondary(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(discovery) = self.discovery_mut() else {
            return;
        };
        let Some(guild) = discovery.guild else {
            return;
        };
        let held = discovery.meta.secondary.contains(&id);
        if !held && discovery.meta.secondary.len() >= 5 {
            return;
        }
        let method = if held {
            discovery.meta.secondary.retain(|known| *known != id);
            reqwest::Method::DELETE
        } else {
            discovery.meta.secondary.push(id);
            reqwest::Method::PUT
        };
        if let Some(saved) = discovery.saved_meta.as_mut() {
            saved.secondary.clone_from(&discovery.meta.secondary);
        }
        self.discovery_send(
            format!("{CATEGORY_PREFIX}{guild}"),
            method,
            format!("/guilds/{guild}/discovery-categories/{id}"),
            None,
        );
        cx.notify();
    }

    pub(super) fn discovery_set_emoji(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.meta.emoji = on;
        }
        cx.notify();
    }

    pub(super) fn discovery_set_locale(&mut self, locale: &str, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.bits.locale = locale.to_owned();
            discovery.open = None;
        }
        cx.notify();
    }

    pub(super) fn discovery_remove_splash(&mut self, cx: &mut Context<Self>) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.bits.splash = None;
        }
        cx.notify();
    }

    pub(super) fn discovery_pick_splash(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(uri) = paths
                    .first()
                    .and_then(|path| super::server_settings::data_uri(path))
            {
                let _ = this.update(cx, |this, cx| {
                    this.onboarding_keep_picture(&uri);
                    if let Some(discovery) = this.discovery_mut() {
                        discovery.bits.splash = Some(uri);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// `AW.v` for the server as it is now, or nothing until the requirements are read.
    pub(super) fn discovery_view(&self, guild: Id) -> Option<View> {
        let requirements = self
            .server_settings
            .as_ref()?
            .discovery
            .requirements
            .as_ref()?;
        let features = self.stores.guild(guild)?.features.clone();
        #[cfg(feature = "capture")]
        if FORCE_ELIGIBLE.load(std::sync::atomic::Ordering::Relaxed) {
            return Some(settings_view(
                &features,
                &json!({"sufficient": true}),
                chrono::Utc::now(),
            ));
        }
        Some(settings_view(&features, requirements, chrono::Utc::now()))
    }

    /// Something on the listing form would be saved.
    pub(super) fn discovery_dirty(&self) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.discovery.dirty())
    }

    /// What Save Changes sends for Discoverable once the form is agreed to (`A`, `case DISCOVERABLE`): the guild with
    /// the feature added and manual approval dropped, then the listing.
    pub(super) fn save_discovery(&mut self, guild: Id) {
        let features: Vec<String> = self
            .stores
            .guild(guild)
            .map(|known| known.features.clone())
            .unwrap_or_default();
        let enabled = features.iter().any(|feature| feature == "DISCOVERABLE");
        let Some(discovery) = self.discovery_mut() else {
            return;
        };
        discovery.agreed = false;
        if enabled && !discovery.dirty() {
            return;
        }
        discovery.error = None;
        let mut body = json!({ "features": listed_features(&features) });
        let (saved, now) = (
            discovery.saved_bits.clone().unwrap_or_default(),
            discovery.bits.clone(),
        );
        if saved.description != now.description {
            body["description"] = json!(now.description);
        }
        if saved.locale != now.locale {
            body["preferred_locale"] = json!(now.locale);
        }
        if saved.splash != now.splash {
            body["discovery_splash"] = json!(now.splash);
        }
        discovery.saving = true;
        self.discovery_send(
            format!("{SAVED_GUILD_PREFIX}{guild}"),
            reqwest::Method::PATCH,
            format!("/guilds/{guild}"),
            Some(body),
        );
    }

    fn discovery_state_description(&self) -> Option<Entity<Editor>> {
        self.server_settings
            .as_ref()
            .and_then(|open| open.discovery.description.clone())
    }

    /// `A3`.
    pub(super) fn access_discovery_section(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        // The description box is shared with the Server Web Page, which words its hint differently.
        if let Some(editor) = self.discovery_state_description() {
            editor.update(cx, |editor, _| {
                editor.set_placeholder("Tell the world a bit about this server.".to_owned())
            });
        }
        match self.discovery_view(guild) {
            None => Self::discovery_checking(),
            Some(View::Ineligible | View::Disqualified) => self.discovery_checklist(guild, cx),
            Some(View::EligibleDisabled | View::EligibleEnabled) => self.discovery_form(guild, cx),
        }
    }

    /// `S` in `Il`: a description, a category and a tag are what listing needs.
    pub(super) fn discovery_listable(&self, _cx: &Context<Self>) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.discovery.listable())
    }

    pub(super) fn discovery_agreed(&self, _guild: Id) -> bool {
        self.server_settings
            .as_ref()
            .is_some_and(|open| open.discovery.agreed)
    }

    pub(super) fn discovery_ask_agreement(&mut self) {
        if let Some(discovery) = self.discovery_mut() {
            discovery.agreement = Some([false; 3]);
        }
    }

    /// `AP` before the answer arrives.
    pub(super) fn discovery_checking() -> AnyElement {
        div()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .p(px(16.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child("Checking if your server passes requirements..."),
            )
            .into_any_element()
    }

    /// `AP`: the header and one row for each requirement.
    fn discovery_checklist(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(requirements) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.discovery.requirements.as_ref())
        else {
            return Self::discovery_checking();
        };
        let partnered = self
            .stores
            .guild(guild)
            .is_some_and(|known| known.features.iter().any(|feature| feature == "PARTNERED"));
        let passing = requirements["sufficient"] == true;
        let items = checklist_items(requirements, guild, partnered);
        let art = self
            .images
            .get(if passing {
                CLIPBOARD_PASSING
            } else {
                CLIPBOARD_FAILING
            })
            .cloned();
        let mut list = div()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(16.))
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(40.))
                            .flex_none()
                            .children(art.map(|art| img(art).size(px(40.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(
                                div()
                                    .flex()
                                    .child("Your server currently ")
                                    .child(
                                        div()
                                            .text_color(color("--text-feedback-critical"))
                                            .child("does not"),
                                    )
                                    .child(" meet the requirements for Server Discovery"),
                            ),
                    ),
            );
        let count = items.len();
        for (index, item) in items.into_iter().enumerate() {
            let glyph: AnyElement = if item.pending {
                icon("checklist_pending", 20., "--yellow-300").into_any_element()
            } else if item.checked {
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--brand-500"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("checkbox_check", 18., "--white"))
                    .into_any_element()
            } else {
                icon("circle_x_small", 20., "--red-400").into_any_element()
            };
            let body: AnyElement = match item.description {
                Row::Text(text) => div().w_full().child(text).into_any_element(),
                Row::Lines(lines) => div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(lines.into_iter().map(|line| div().child(line)))
                    .into_any_element(),
                Row::Link {
                    before,
                    link,
                    after,
                    action,
                } => self.text_with_link(
                    "discovery-checklist-link",
                    &before,
                    &link,
                    &after,
                    cx,
                    move |this, cx| match action {
                        LinkAction::Safety => this.discovery_open_safety(cx),
                        LinkAction::Url(url) => cx.open_url(url),
                    },
                ),
            };
            list = list
                .child(
                    div()
                        .p(px(16.))
                        .flex()
                        .gap(px(16.))
                        .child(div().w(px(20.)).flex_none().child(glyph))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(item.name),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-muted"))
                                        .child(body),
                                ),
                        ),
                )
                .when(index + 1 < count, |this| {
                    this.child(div().mx(px(16.)).h(px(1.)).bg(color("--border-muted")))
                });
        }
        list.into_any_element()
    }

    /// `ex.A.setSection(SAFETY, SAFETY_PERMISSIONS)`: the Safety Setup page, which has no subsection of its own here.
    fn discovery_open_safety(&mut self, cx: &mut Context<Self>) {
        use super::server_settings::Page;
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        open.page = Page::SafetySetup;
        let guild = open.guild;
        self.request_server_page(guild, Page::SafetySetup, cx);
    }

    /// The images the checklist draws, asked for when the page opens.
    pub(super) fn request_discovery_art(&mut self) {
        for url in [CLIPBOARD_FAILING, CLIPBOARD_PASSING, DEFAULT_SPLASH] {
            self.request_image(url.to_owned(), url.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-11-09T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn features(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_view_follows_the_requirements_and_the_feature() {
        let passing = json!({"sufficient": true});
        let failing = json!({"sufficient": false});
        assert_eq!(
            settings_view(&features(&[]), &failing, now()),
            View::Ineligible
        );
        assert_eq!(
            settings_view(&features(&[]), &passing, now()),
            View::EligibleDisabled
        );
        assert_eq!(
            settings_view(&features(&["DISCOVERABLE"]), &passing, now()),
            View::EligibleEnabled
        );
        assert_eq!(
            settings_view(&features(&["DISCOVERABLE"]), &failing, now()),
            View::Disqualified
        );
        // A verified server never fails.
        assert_eq!(
            settings_view(&features(&["VERIFIED"]), &failing, now()),
            View::EligibleDisabled
        );
    }

    #[test]
    fn a_server_in_its_grace_period_stays_listed() {
        let requirements = json!({
            "sufficient": false,
            "sufficient_without_grace_period": true,
            "grace_period_end_date": "2026-11-20T00:00:00Z",
        });
        assert_eq!(
            settings_view(&features(&["DISCOVERABLE"]), &requirements, now()),
            View::EligibleEnabled
        );
        let over = json!({
            "sufficient": false,
            "sufficient_without_grace_period": true,
            "grace_period_end_date": "2026-11-01T00:00:00Z",
        });
        assert_eq!(
            settings_view(&features(&["DISCOVERABLE"]), &over, now()),
            View::Disqualified
        );
    }

    #[test]
    fn the_rows_read_like_the_checklist() {
        let requirements = json!({
            "safe_environment": true, "size": false, "age": false, "healthy": false,
            "health_score_pending": true, "protected": false, "minimum_size": 1000,
            "minimum_age": 56, "nsfw_properties": {},
        });
        let items = checklist_items(&requirements, 1568692564637524109, false);
        let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Safe Environment",
                "Fewer Than 1,000 Members",
                "Server Too Young",
                "Waiting on Server Activity Metrics",
                "No Bad Content",
                "Please Enable the [2FA Requirement For Moderation] Setting",
            ]
        );
        assert!(items[3].pending);
        assert!(items[4].checked);
        assert!(!items[5].checked);
    }

    #[test]
    fn counts_and_weeks_read_in_english() {
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(999), "999");
        assert_eq!(weeks(56), "8 weeks");
        assert_eq!(weeks(7), "1 week");
        assert_eq!(weeks(8), "2 weeks");
    }

    #[test]
    fn the_listing_reads_what_the_server_sent_and_sends_it_back() {
        let body = json!({
            "guild_id": "1", "primary_category_id": 1, "keywords": ["test"],
            "emoji_discoverability_enabled": true, "partner_actioned_timestamp": null,
            "partner_application_timestamp": null, "is_published": false,
            "reasons_to_join": [], "social_links": null, "about": null, "category_ids": [15, 34],
        });
        let meta = Meta::from_body(&body);
        assert_eq!(meta.primary, 1);
        assert_eq!(meta.secondary, [15, 34]);
        assert_eq!(meta.keywords, ["test"]);
        let sent = meta.patch_body();
        assert_eq!(sent["primary_category_id"], 1);
        assert_eq!(sent["keywords"], json!(["test"]));
        assert_eq!(sent["reasons_to_join"], json!([]));
        assert!(sent["social_links"].is_null());
        assert!(sent.get("category_ids").is_none());
    }

    #[test]
    fn categories_sort_by_name_and_other_goes_last_among_the_primary_ones() {
        let all = categories_from(&json!([
            {"id": 0, "is_primary": true, "name": "General"},
            {"id": 13, "is_primary": true, "name": "Other"},
            {"id": 2, "is_primary": true, "name": "Music"},
            {"id": 15, "is_primary": false, "name": "Esports"},
            {"id": 1, "is_primary": true, "name": "Gaming"},
        ]));
        let primary: Vec<&str> = primary_categories(&all)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(primary, ["Gaming", "Music", "Other"]);
        let every: Vec<&str> = all_categories(&all)
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(every, ["Esports", "Gaming", "Music"]);
    }

    #[test]
    fn an_eleventh_tag_stays_typed_and_says_no_more_tags() {
        let mut state = Discovery::default();
        for index in 0..MAX_TAGS {
            assert_eq!(state.submit_tag(&format!(" tag{index} ")), "");
        }
        assert_eq!(state.meta.keywords.len(), MAX_TAGS);
        assert_eq!(state.meta.keywords[0], "tag0");
        assert_eq!(state.submit_tag("one more"), "one more");
        assert_eq!(state.tag_error.as_deref(), Some("Cannot add anymore tags."));
        assert_eq!(state.meta.keywords.len(), MAX_TAGS);
        // A removed tag makes room; the next one clears the error.
        state.meta.keywords.pop();
        assert_eq!(state.submit_tag("one more"), "");
        assert!(state.tag_error.is_none());
    }

    #[test]
    fn listing_needs_a_description_a_category_and_a_tag() {
        let mut state = Discovery::default();
        assert!(!state.listable());
        state.bits.description = Some("About".to_owned());
        state.meta.primary = 1;
        assert!(!state.listable());
        state.meta.keywords = vec!["test".to_owned()];
        assert!(state.listable());
        state.meta.primary = 0;
        assert!(!state.listable());
    }

    #[test]
    fn only_the_fields_save_changes_sends_make_the_form_dirty() {
        let mut state = Discovery {
            saved_bits: Some(Bits::default()),
            saved_meta: Some(Meta::default()),
            ..Discovery::default()
        };
        assert!(!state.dirty());
        // A subcategory is saved on its own the moment it is picked.
        state.meta.secondary.push(15);
        assert!(!state.dirty());
        state.meta.keywords.push("test".to_owned());
        assert!(state.dirty());
        state.meta.keywords.clear();
        state.bits.locale = "tr".to_owned();
        assert!(state.dirty());
    }

    #[test]
    fn listing_adds_discoverable_and_drops_manual_approval() {
        let held = features(&["COMMUNITY", "MEMBER_VERIFICATION_MANUAL_APPROVAL"]);
        assert_eq!(listed_features(&held), ["COMMUNITY", "DISCOVERABLE"]);
        let listed = listed_features(&features(&["COMMUNITY", "DISCOVERABLE"]));
        assert_eq!(listed, ["COMMUNITY", "DISCOVERABLE"]);
    }

    #[test]
    fn each_category_suggests_its_own_tags() {
        assert_eq!(popular_tags(1)[0], "minecraft");
        assert_eq!(popular_tags(1).len(), 10);
        assert!(popular_tags(0).is_empty());
    }
}
