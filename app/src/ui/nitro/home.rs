//! The Nitro home of an account whose premium type is exactly Nitro (module 886268 `sQ`, `PREMIUM_SUBSCRIBER_NITRO_HOME`;
//! `L.YE` is `premiumType === TIER_2`). Basic and Classic keep the marketing home.
//!
//! The page (`sQ`): the scroller (`mainPageScroller_IRnLy8`), the top gradient (`sz`) and the sticky bar (`tP`) of
//! `sX`, the greeting hero (`tT` -> `tm`, with the floating art of `to`), "Explore What's New" (`sK`/`sL`), the
//! rewards section (`iy`), "Check Out These Nitro Favorites" (`sV`), the gift banner (module 175020), the footer
//! spacing and the bottom illustration. Until the account's profile and its program rewards are known it shows the
//! bar over a spinner, as `sQ` does.
//!
//! The section grid is the page's container query (`premium-marketing-page-container`): one column, two from 720px,
//! three from 960px; content is `calc(100% - 64px)` up to 1200px.
//!
//! Not built: the rewards section `iy` (every card in it is a partner promotion behind an experiment or a campaign
//! this client has no data for), the Quest-multiplier, premium-group and file-upload cards (experiments, off by
//! default), the churn banner of a canceled subscription, the hero pill and buttons (fractional Nitro and trials),
//! the avatar decoration and display name style in the greeting, and the light-theme gradient.

use super::perk_card::{Action, Asset, Card, OrbsFooter};
use super::{Root, art_key, tenure, trinkets};
use crate::net::Request;
use crate::ui::{avatar_source, color, icon, motion};
use dc_state::premium::PremiumType;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, Window, canvas, div,
    img, linear_color_stop, linear_gradient, prelude::*, px,
};
use std::time::Instant;

const GIFT_BANNER_KEY: &str = "nitro-home:gift-banner";
/// The default-variant banner picture of module 175020.
const GIFT_BANNER_URL: &str = "https://cdn.discordapp.com/assets/content/577cd1f06ad2e0559c8a531a20a840448c66d6a6251f5c0bac311ceba4d37396.png";
const BOTTOM_KEY: &str = "nitro-home:bottom";
/// `sq`: the bottom illustration, 112 x 85 (a bundled asset).
const BOTTOM_URL: &str = "https://discord.com/assets/cd2be35d285d4675.svg";
const BACKGROUND_KEY: &str = "nitro-home:background";
/// `sz`: the page's top gradient for a dark theme (`/assets/2ee0f277372e56e4.svg` for a light one).
const BACKGROUND_URL: &str = "https://discord.com/assets/3ebfa123a3805f56.svg";
/// `PROGRAM_REWARDS` (module 961250).
pub(super) const REWARDS_KEY: &str = "nitro:program-rewards";
const ASSETS: &str = "https://discord.com/assets/";
const CDN: &str = "https://cdn.discordapp.com/assets/content/";

/// `--custom-premium-home-max-width`.
const MAX_WIDTH: f32 = 1200.;
/// The 24px gap of `cardContainer_KD8RnX`.
const GRID_GAP: f32 = 24.;
/// `cardContainer_cpKOhU`: the favorites' slot width, and their row's gap.
const FAVORITE_WIDTH: f32 = 280.;
const FAVORITE_GAP: f32 = 20.;

/// `Wvc/I+`, `d+0STx`, `CqsxKI` by the local hour: morning is 5 to 12, afternoon 12 to 17, else evening (`tm`).
pub(super) fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => dc_intl::t("Wvc/I+"),
        12..=16 => dc_intl::t("d+0STx"),
        _ => dc_intl::t("CqsxKI"),
    }
}

/// `cardContainer_KD8RnX`: one column, two from a 720px container, three from 960px.
pub(super) fn grid_columns(container: f32) -> usize {
    if container >= 960. {
        3
    } else if container >= 720. {
        2
    } else {
        1
    }
}

/// `--custom-content-width-with-padding` under `--custom-premium-home-max-width`.
pub(super) fn content_width(container: f32) -> f32 {
    (container - 64.).clamp(0., MAX_WIDTH)
}

/// The width of a grid card.
pub(super) fn card_width(container: f32) -> f32 {
    let columns = grid_columns(container) as f32;
    ((content_width(container) - GRID_GAP * (columns - 1.)) / columns).max(0.)
}

/// One of the module's own image components: its picture and intrinsic size.
fn sized(id: &str, file: &str, width: f32, height: f32) -> Asset {
    Asset::Sized {
        key: art_key(id),
        url: format!("{CDN}{file}"),
        width,
        height,
        sticker: false,
    }
}

/// A string `primaryAsset`.
fn image(id: &str, url: String) -> Asset {
    Asset::Image {
        key: art_key(id),
        url,
        max_height: 180.,
        shrink: false,
    }
}

fn take_me_there() -> String {
    dc_intl::t("jVcuVY").to_owned()
}

fn card(
    id: &'static str,
    title: String,
    description: String,
    asset: Asset,
    cta: Option<(String, Action)>,
    categories: &'static [&'static str],
) -> Card {
    Card {
        id,
        title,
        description,
        pill: None,
        caption: None,
        progress: None,
        asset,
        cta,
        footer: None,
        categories,
    }
}

/// The four fixed cards of `sL`, in order.
fn whats_new_fixed() -> Vec<Card> {
    vec![
        card(
            "display-name-styles",
            dc_intl::t("OLtTrt").to_owned(),
            dc_intl::t("di/pXR").to_owned(),
            image(
                "display-name-styles",
                format!(
                    "{CDN}cd580e29aa6ad4aa731dba64c23331d6bc556ad2e236ec1b5781206f6e71cb50.svg"
                ),
            ),
            // `T ? C : I`: the profile settings (the profile-editing experiment is off by default).
            Some((take_me_there(), Action::Settings("account"))),
            &[],
        ),
        card(
            "client-themes",
            dc_intl::t("acc6h6").to_owned(),
            dc_intl::format("WQazjs", &[("themeCount", 20.into())]),
            image(
                "client-themes",
                format!(
                    "{CDN}c0c412ad58e2520901e2cb967308eb432d0b349b3b5f54b14f141e12b80f7d42.svg"
                ),
            ),
            Some((take_me_there(), Action::CustomTheme)),
            &[],
        ),
        card(
            "permadecos",
            dc_intl::t("L14NZN").to_owned(),
            dc_intl::t("eCZkAI").to_owned(),
            sized(
                "permadecos",
                "ef081d5f86c6c1d63f7f026517c0226c59bba2618b3d65a7ce91dc10019b35ad.svg",
                288.,
                192.,
            ),
            Some((take_me_there(), Action::Settings("account"))),
            &[],
        ),
        card(
            "custom-app-icons",
            dc_intl::t("GU+wqh").to_owned(),
            dc_intl::t("1uPk1Z").to_owned(),
            image(
                "custom-app-icons",
                format!(
                    "{CDN}7bb191dd09571f8910a367c7cf35b500ea7b64dde29309c7c74831cc16b1dc1d.png"
                ),
            ),
            Some((
                dc_intl::t("y9TxXV").to_owned(),
                Action::Settings("appearance"),
            )),
            &[],
        ),
    ]
}

/// `E` of `sV` without the cards that are null by default, in the module's order. `badge` is the picture of the
/// subscriber badge card (`s`); `light` picks the light video-backgrounds picture.
fn favorites_all(badge: Asset, light: bool) -> Vec<Card> {
    vec![
        card(
            "server-boosts",
            dc_intl::format(
                "pWySes",
                &[("boostCount", 2.into()), ("percentageOff", 30.into())],
            ),
            dc_intl::format(
                "cWFUoT",
                &[("boostCount", 2.into()), ("percentageOff", 30.into())],
            ),
            sized(
                "server-boosts",
                "a9ce5551f37c49862694d063777be6e02b9190ff9a205c7c62f5fe9cecb673de.svg",
                288.,
                192.,
            ),
            Some((take_me_there(), Action::Settings("server-boost"))),
            &["bestof", "upgrades"],
        ),
        card(
            "profiles",
            dc_intl::t("xDRab3").to_owned(),
            dc_intl::t("yn6fWA").to_owned(),
            sized(
                "profiles",
                "a89ff8a07704142beff857453a6c8aa15400cf711f8fe3cbbfa7ff4f0b2a334f.svg",
                288.,
                162.,
            ),
            Some((take_me_there(), Action::Settings("account"))),
            &["bestof", "appearance"],
        ),
        card(
            "hd-video",
            dc_intl::t("/mQ5gg").to_owned(),
            dc_intl::t("7WwAXh").to_owned(),
            sized(
                "hd-video",
                "2af3a2846a762c266ce197f82418f1f084b3b7aa12a3f38d4865682a2f715072.svg",
                288.,
                192.,
            ),
            None,
            &["bestof", "upgrades"],
        ),
        card(
            "client-themes",
            dc_intl::t("acc6h6").to_owned(),
            dc_intl::format("WQazjs", &[("themeCount", 20.into())]),
            image(
                "client-themes",
                format!(
                    "{CDN}c0c412ad58e2520901e2cb967308eb432d0b349b3b5f54b14f141e12b80f7d42.svg"
                ),
            ),
            Some((take_me_there(), Action::Settings("appearance"))),
            &["bestof", "appearance"],
        ),
        card(
            "more-emojis",
            dc_intl::t("D8vIDT").to_owned(),
            dc_intl::t("DRMecB").to_owned(),
            sized(
                "more-emojis",
                "96dd57d4bf82369b228168d944da9a2d72bdf1f73640cd50802379ffcf925354.svg",
                288.,
                192.,
            ),
            None,
            &["bestof", "upgrades"],
        ),
        card(
            "large-uploads",
            dc_intl::t("nL1WZV").to_owned(),
            // `M6`: the legacy copy unless the file-upload rollout is on (off by default).
            dc_intl::format("k8LC1w", &[("maxSizeMb", 500.into())]),
            sized(
                "large-uploads",
                "804d87fc1c1330e6413159eaacc359de89bdfbec7b4602532b1bd074868783c8.svg",
                288.,
                192.,
            ),
            None,
            &["bestof", "upgrades"],
        ),
        card(
            "custom-app-icons",
            dc_intl::t("GU+wqh").to_owned(),
            dc_intl::t("1uPk1Z").to_owned(),
            image(
                "custom-app-icons",
                format!(
                    "{CDN}7bb191dd09571f8910a367c7cf35b500ea7b64dde29309c7c74831cc16b1dc1d.png"
                ),
            ),
            Some((take_me_there(), Action::Settings("appearance"))),
            &["appearance"],
        ),
        card(
            "entrance-sounds",
            dc_intl::t("WJfCPi").to_owned(),
            dc_intl::t("liQKJR").to_owned(),
            image(
                "entrance-sounds",
                format!(
                    "{CDN}61471321446262d980f72210a31bbce561d7021e51f4ea2988d63e413df9fe04.svg"
                ),
            ),
            // `SOUNDBOARD_CATEGORY` lives in Voice & Video.
            Some((take_me_there(), Action::Settings("voice-video"))),
            &["appearance"],
        ),
        card(
            "display-name-styles",
            dc_intl::t("OLtTrt").to_owned(),
            dc_intl::t("di/pXR").to_owned(),
            image(
                "display-name-styles",
                format!(
                    "{CDN}cd580e29aa6ad4aa731dba64c23331d6bc556ad2e236ec1b5781206f6e71cb50.svg"
                ),
            ),
            Some((take_me_there(), Action::Settings("account"))),
            &["appearance"],
        ),
        card(
            "custom-sounds",
            dc_intl::t("Cu/oFd").to_owned(),
            dc_intl::t("czj2aa").to_owned(),
            sized(
                "custom-sounds",
                "2d617cf5c40353ca4ec6ac76e1a48f6781439c18db06b8cfab0f194358abe555.svg",
                288.,
                162.,
            ),
            None,
            &["upgrades"],
        ),
        card(
            "special-stickers",
            dc_intl::t("MQoVeb").to_owned(),
            dc_intl::t("HGCLZX").to_owned(),
            Asset::Sized {
                key: art_key("special-stickers"),
                url: format!(
                    "{CDN}c561ce5c9866c0bdb3f8c9dc43164f873414c3d905d1ab4beace8020dc4933fc.svg"
                ),
                width: 288.,
                height: 162.,
                sticker: true,
            },
            None,
            &["upgrades"],
        ),
        card(
            "super-reactions",
            dc_intl::t("qERvAA").to_owned(),
            dc_intl::t("WkUWzx").to_owned(),
            sized(
                "super-reactions",
                "8a9a80f2b924da025612a160c7dc74c5e0fe675a90e3424737e6c5927739a6d0.svg",
                288.,
                162.,
            ),
            None,
            &["upgrades"],
        ),
        card(
            "video-backgrounds",
            dc_intl::t("ssVDYQ").to_owned(),
            dc_intl::t("aUSRMa").to_owned(),
            image(
                if light {
                    "video-backgrounds-light"
                } else {
                    "video-backgrounds"
                },
                format!(
                    "{ASSETS}{}",
                    if light {
                        "983b60e4fcaf973b.svg"
                    } else {
                        "1eb1b74667b4c0f0.svg"
                    }
                ),
            ),
            None,
            &["upgrades"],
        ),
        card(
            "early-access",
            dc_intl::t("g/KRY6").to_owned(),
            dc_intl::t("JzAmJc").to_owned(),
            sized(
                "early-access",
                "3ba8fce33a4733cdbadc10ac378d1b5523b0c0961e7edc74372c159e3009727f.svg",
                288.,
                162.,
            ),
            None,
            &["vip"],
        ),
        card(
            "badge",
            dc_intl::t("Bn3CtB").to_owned(),
            dc_intl::t("LmENwu").to_owned(),
            badge,
            None,
            &["vip"],
        ),
        card(
            "special-pricing",
            dc_intl::t("MTD+7w").to_owned(),
            dc_intl::t("Bhs0s6").to_owned(),
            sized(
                "special-pricing",
                "1314d17fb23c01337cac4bfdaaff48c5e6ac1f515b0647f0964922b988f095c1.svg",
                288.,
                162.,
            ),
            Some((dc_intl::t("dBJVnZ").to_owned(), Action::Shop)),
            &["vip"],
        ),
        card(
            "permadecos",
            dc_intl::t("L14NZN").to_owned(),
            dc_intl::t("eCZkAI").to_owned(),
            sized(
                "permadecos",
                "ef081d5f86c6c1d63f7f026517c0226c59bba2618b3d65a7ce91dc10019b35ad.svg",
                288.,
                192.,
            ),
            None,
            &["vip"],
        ),
    ]
}

/// `sL`'s ordering: the program cards first (here the Orbs card), the fixed four, the tenure card spliced in front
/// (or second after a featured card; none is featured here), at most six.
pub(super) fn arrange_whats_new(tenure: Card, orbs: Option<Card>, fixed: Vec<Card>) -> Vec<Card> {
    let mut cards: Vec<Card> = orbs.into_iter().chain(fixed).collect();
    cards.insert(0, tenure);
    cards.truncate(6);
    cards
}

/// `sV`'s list without the cards What's New already shows.
pub(super) fn favorites_without(all: Vec<Card>, shown: &[&str]) -> Vec<Card> {
    all.into_iter()
        .filter(|card| !shown.contains(&card.id))
        .collect()
}

/// The filter tabs (`sB`): id and label key.
const TABS: [(&str, &str); 4] = [
    ("bestof", "q1u7nQ"),
    ("appearance", "CUnZkZ"),
    ("upgrades", "KC5q8v"),
    ("vip", "DjEAcv"),
];

/// The favorites' horizontal scroller (module 151869, `scrollStep: "page"`): where a click on an arrow takes it.
/// Items are `FAVORITE_WIDTH` apart by `FAVORITE_GAP`, the first 8px in (`margin-inline-start`), and the view is
/// `view` wide at `offset`.
pub(super) fn page_step(count: usize, view: f32, offset: f32, right: bool) -> f32 {
    let left_of = |index: usize| 8. + index as f32 * (FAVORITE_WIDTH + FAVORITE_GAP);
    let total = if count == 0 {
        0.
    } else {
        left_of(count - 1) + FAVORITE_WIDTH + 8.
    };
    let max = (total - view).max(0.);
    let clamp = |value: f32| value.clamp(0., max);
    if count == 0 {
        return 0.;
    }
    // `y(direction, scroller, scrollLeft, 16)`: the first (left) or last (right) item that shows past 16px of edge.
    let (lo, hi) = (offset + 16., offset + view - 16.);
    let visible = |index: usize| left_of(index) + FAVORITE_WIDTH > lo && left_of(index) < hi;
    let (index, hidden) = if right {
        (0..count)
            .rev()
            .find(|index| visible(*index))
            .map_or((count - 1, false), |index| {
                (index, left_of(index) + FAVORITE_WIDTH > hi + 1.)
            })
    } else {
        (0..count)
            .find(|index| visible(*index))
            .map_or((0, false), |index| (index, left_of(index) < lo - 1.))
    };
    let step: isize = if right { 1 } else { -1 };
    let target = if hidden {
        index
    } else {
        (index as isize + step).clamp(0, count as isize - 1) as usize
    };
    // `I(scroller, index, right ? "start" : "end", step)`.
    let place = |index: usize| {
        if right {
            clamp(left_of(index) - 16.)
        } else {
            clamp(left_of(index) + FAVORITE_WIDTH - view + 16.)
        }
    };
    let at = place(target);
    if (at - offset).abs() > 0.5 {
        return at;
    }
    let next = (target as isize + step).clamp(0, count as isize - 1) as usize;
    place(next)
}

impl Root {
    /// The width of the page's container (`premium-marketing-page-container`), as last laid out; before the first
    /// layout, the window less the rail and the sidebar.
    fn home_width(&self, window: &Window) -> f32 {
        let measured = self.nitro.home_width.get();
        if measured > 0. {
            measured
        } else {
            (f32::from(window.viewport_size().width) - 375.).max(0.)
        }
    }

    /// The account's own profile (`fetchProfile` on mount of `sQ`) and its program rewards (`w.Ay`).
    fn request_subscriber_data(&mut self) {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let key = format!("profile:{me}");
        if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
            let _ = self.requests.send(Request::Fetch {
                key,
                route: format!("/users/{me}/profile"),
            });
        }
        if !self.nitro.rewards_requested {
            self.nitro.rewards_requested = true;
            let _ = self.requests.send(Request::Rest {
                key: Some(REWARDS_KEY.to_owned()),
                method: reqwest::Method::GET,
                route: "/users/@me/program-rewards".to_owned(),
                body: None,
            });
        }
    }

    /// `sQ` renders once the profile is there and the rewards store is ready (fetched, failed, or not eligible).
    fn subscriber_home_ready(&self) -> bool {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return false;
        };
        self.fetched.contains_key(&format!("profile:{me}"))
            && (self.nitro.rewards.is_some() || self.premium() != PremiumType::Nitro)
    }

    fn own_profile(&self) -> Option<&serde_json::Value> {
        let me = self.stores.current_user.as_ref()?.id;
        self.fetched
            .get(&format!("profile:{me}"))
            .filter(|body| body.get("fetch_failed").is_none())
    }

    /// The tenure badge card (`v` in `sL`).
    fn tenure_card(&self, now: chrono::DateTime<chrono::Utc>) -> Card {
        let subscription_since = (self.premium() == PremiumType::Nitro)
            .then(|| self.premium_subscription())
            .flatten()
            .and_then(|subscription| subscription.premium_since().and_then(tenure::parse_time));
        let facts = tenure::Facts {
            profile: self.own_profile(),
            subscription_since,
            paid_nitro: self.premium() == PremiumType::Nitro,
        };
        let badge = facts.badge(now);
        let since = facts.since();
        let next = tenure::next_tier(badge);
        let until = tenure::until_next(next, since, now);
        let reduced = motion::reduced();
        let asset = badge.map_or(Asset::None, |badge| {
            let pending = badge.status != tenure::Status::Earned;
            let still = reduced && !pending;
            Asset::Badge {
                key: art_key(&format!("tenure:{}:{}", badge.tier.id, pending || still)),
                url: if pending || still {
                    badge.tier.standard_url()
                } else {
                    badge.tier.ambient_large_url()
                },
                dimmed: pending,
                still,
            }
        });
        Card {
            id: "tenure-badge",
            title: badge.map_or(String::new(), |badge| {
                dc_intl::t(badge.tier.name_key).to_owned()
            }),
            description: tenure::description(badge, until).unwrap_or_default(),
            pill: Some(dc_intl::t("jyYgZ+").to_owned()),
            caption: since.map(|since| {
                dc_intl::format(
                    "Hu4jfi",
                    &[(
                        "date",
                        dc_intl::Arg::When {
                            date: tenure::short_date(since).into(),
                            time: String::new().into(),
                        },
                    )],
                )
            }),
            progress: tenure::progress(badge, next, since, now),
            asset,
            cta: Some((take_me_there(), Action::TenureBadge)),
            footer: None,
            categories: &[],
        }
    }

    /// The Orbs card (`nitroOrbsRewardsCard` of `so`): only when the Nitro program gives Orbs.
    fn orbs_card(&self, now: chrono::DateTime<chrono::Utc>) -> Option<Card> {
        let body = self.nitro.rewards.as_ref()?;
        let reward = tenure::nitro_reward(body)?;
        let amount = reward["reward_amount"]
            .as_i64()
            .filter(|amount| *amount > 0)?;
        let footer = tenure::passes_ui_invariant(reward, now).then(|| OrbsFooter {
            days: reward["next_reward_date"]
                .as_str()
                .filter(|date| !date.is_empty())
                .and_then(|date| tenure::days_until(date, now))
                .map(|days| days.max(1)),
            total: reward["total_rewarded_from_program"].as_i64().unwrap_or(0),
        });
        Some(Card {
            id: "nitro-orbs-rewards",
            title: dc_intl::t("hx5AFp").to_owned(),
            description: dc_intl::format("wq3CF2", &[("orbsCount", amount.into())]),
            pill: None,
            caption: None,
            progress: None,
            asset: Asset::Image {
                key: art_key("nitro-orbs-rewards"),
                url: format!("{ASSETS}8f530451dce1ccc0.svg"),
                max_height: 234.,
                shrink: true,
            },
            cta: Some((dc_intl::t("BxjHiu").to_owned(), Action::Shop)),
            footer,
            categories: &[],
        })
    }

    /// The subscriber badge card's picture (`s` of `sV`): the standard art of the badge the profile shows, else the
    /// Nitro mark.
    fn subscriber_badge_asset(&self) -> Asset {
        let listed = self.own_profile().and_then(|profile| {
            profile["badges"]
                .as_array()?
                .iter()
                .rev()
                .find_map(|badge| tenure::tier(badge["id"].as_str()?))
        });
        match listed {
            Some(tier) => Asset::Wide {
                key: art_key(&format!("tenure:{}:true", tier.id)),
                url: tier.standard_url(),
                width: 160.,
            },
            None => Asset::NitroMark,
        }
    }

    /// Every card of the page: What's New, then the favorites.
    fn subscriber_cards(&self) -> (Vec<Card>, Vec<Card>) {
        let now = chrono::Utc::now();
        let whats_new = arrange_whats_new(
            self.tenure_card(now),
            self.orbs_card(now),
            whats_new_fixed(),
        );
        let shown: Vec<&str> = whats_new.iter().map(|card| card.id).collect();
        let light = super::super::theme() == dc_tokens::Theme::Light;
        let favorites =
            favorites_without(favorites_all(self.subscriber_badge_asset(), light), &shown);
        (whats_new, favorites)
    }

    /// Starts every download the page shows at once, before its first frame: the cards' art (all tabs), the hero art,
    /// the gift banner, the gradient and the bottom picture.
    pub(super) fn request_subscriber_art(&mut self) {
        if self.nitro.home_opened.is_none() {
            self.nitro.home_opened = Some((Instant::now(), motion::now()));
            tracing::debug!("nitro home: opened");
        }
        self.request_subscriber_data();
        let (whats_new, favorites) = self.subscriber_cards();
        let sources: Vec<(String, String)> = whats_new
            .iter()
            .chain(&favorites)
            .filter_map(|card| card.asset.source())
            .map(|(key, url)| (key.to_owned(), url.to_owned()))
            .collect();
        for (key, url) in sources {
            self.request_image(key, url);
        }
        for piece in &trinkets::PIECES {
            self.request_image(piece.key.to_owned(), piece.url());
        }
        self.request_image(
            trinkets::WUMPUS_STILL_KEY.to_owned(),
            format!(
                "https://cdn.discordapp.com/assets/content/{}",
                trinkets::WUMPUS_STILL
            ),
        );
        self.request_image(BACKGROUND_KEY.to_owned(), BACKGROUND_URL.to_owned());
        self.request_image(GIFT_BANNER_KEY.to_owned(), GIFT_BANNER_URL.to_owned());
        self.request_image(BOTTOM_KEY.to_owned(), BOTTOM_URL.to_owned());
        if let Some(user) = self.stores.current_user.clone() {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
    }

    /// Decodes the Wumpus clip off the UI thread once its file is here (once per run of the app).
    fn decode_wumpus(&mut self, cx: &mut Context<Self>) {
        if self.nitro.wumpus.is_some() || self.nitro.wumpus_decoding {
            return;
        }
        let Some(image) = self.images.get(trinkets::PIECES[0].key).cloned() else {
            return;
        };
        self.nitro.wumpus_decoding = true;
        cx.spawn(async move |this, cx| {
            let clip = cx
                .background_executor()
                .spawn(async move { trinkets::Clip::decode(image.bytes()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.nitro.wumpus = clip.map(std::rc::Rc::new);
                cx.notify();
            });
        })
        .detach();
    }

    /// Debug lines with the time from opening the page to each picture's first frame.
    fn log_art_arrivals(&mut self, keys: &[String]) {
        let Some((opened, _)) = self.nitro.home_opened else {
            return;
        };
        for key in keys {
            if self.images.contains_key(key) && self.nitro.art_logged.insert(key.clone()) {
                tracing::debug!(
                    "nitro home: {key} ready after {} ms",
                    opened.elapsed().as_millis()
                );
            }
        }
    }

    /// The greeting hero (`tm`): the time-of-day line and the avatar with the display name (`eL`) in the left column
    /// (`treatmentAContentColumn`, at most 636 wide), the floating art in the right box.
    fn nitro_greeting(&self, container: f32) -> AnyElement {
        let user = self.stores.current_user.clone().unwrap_or_default();
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let hour = chrono::Timelike::hour(&chrono::Local::now());
        let scroll = -f32::from(self.nitro.scroll.offset().y);
        // `eg`: how much of the hero's row is on screen. The row starts 81px down (the bar's 1px, the 80px padding);
        // its height is the heading (34), the 12px gap and the 67.2px avatar box.
        let row = 34. + 12. + 67.2;
        let visibility = ((row - (scroll - 81.).max(0.)) / row).clamp(0., 1.);
        let column = div()
            .relative()
            .w_full()
            .max_w(px(636.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                Self::nitro_heading(greeting(hour), 32., 34., true)
                    .text_color(color("--text-default")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    // `avatar_uKEKOI`: 56px avatar in a 1.2x box.
                    .child(
                        div()
                            .flex_none()
                            .size(px(67.2))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(56.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .children(
                                        avatar.map(|image| img(image).size(px(56.)).rounded_full()),
                                    ),
                            ),
                    )
                    // `displayName_uKEKOI`: 44px (42 at 720, 38 at 540), one line, at most 380 wide.
                    .child(
                        div()
                            .max_w(px(380.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(if container <= 540. {
                                38.
                            } else if container <= 720. {
                                42.
                            } else {
                                44.
                            }))
                            .line_height(px(48.))
                            .text_color(color("--text-default"))
                            .child(user.display_name().to_owned()),
                    ),
            );
        let art =
            trinkets::art_width(container).map(|width| self.hero_art(width, container, visibility));
        let wide = container > 1140.;
        div()
            .w_full()
            .pt(px(80.))
            .pb(px(64.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .relative()
                    .w(px(content_width(container)))
                    .flex()
                    .when(container <= 960., |this| this.flex_col())
                    .items_start()
                    .gap(px(32.))
                    .child(column)
                    .children(art.map(|art| {
                        if wide {
                            // `align-self: stretch`: as tall as the column.
                            div()
                                .relative()
                                .flex_none()
                                .h(px(row))
                                .ml_auto()
                                .child(art)
                                .into_any_element()
                        } else {
                            div()
                                .absolute()
                                .top_0()
                                .right_0()
                                .child(art)
                                .into_any_element()
                        }
                    })),
            )
            .into_any_element()
    }

    /// `to`: the trinkets and the Wumpus in a box `width` wide, each at its place, moving.
    fn hero_art(&self, width: f32, container: f32, visibility: f32) -> AnyElement {
        let reduced = motion::reduced();
        let elapsed = self
            .nitro
            .home_opened
            .map_or(0., |(_, at)| (motion::now() - at).max(0.));
        trinkets::keep_moving();
        let mut layer = div().relative().w(px(width)).h_full();
        // `ti`'s trinkets first, the Wumpus over them.
        for index in [1usize, 2, 3, 4, 0] {
            let piece = &trinkets::PIECES[index];
            let (left, right, top, height) = trinkets::place(index, container);
            let (dx, dy, scale, opacity) = if reduced {
                (0., 0., 1., 1.)
            } else {
                trinkets::pose(piece, index, elapsed, visibility)
            };
            // The Wumpus is a looping clip (`tl`): its frames once decoded, the still under reduced motion.
            let source: gpui::ImageSource = if index == 0 && reduced {
                match self.images.get(trinkets::WUMPUS_STILL_KEY) {
                    Some(image) => image.clone().into(),
                    None => continue,
                }
            } else if index == 0 {
                match self
                    .nitro
                    .wumpus
                    .as_ref()
                    .and_then(|clip| clip.frame(elapsed))
                    .map(gpui::ImageSource::from)
                    .or_else(|| {
                        self.images
                            .get(piece.key)
                            .cloned()
                            .map(gpui::ImageSource::from)
                    }) {
                    Some(frame) => frame,
                    None => continue,
                }
            } else {
                match self.images.get(piece.key) {
                    Some(image) => image.clone().into(),
                    None => continue,
                }
            };
            let base_w = height * piece.aspect;
            let (w, h) = (base_w * scale, height * scale);
            // `transform: scale()` grows from the centre of the box.
            let shift_x = (base_w - w) / 2. + dx;
            let shift_y = (height - h) / 2. + dy;
            let mut holder = div().absolute().top(px(top + shift_y));
            holder = match (left, right) {
                (Some(left), _) => holder.left(px(left + shift_x)),
                (None, Some(right)) => holder.right(px(right - shift_x)),
                (None, None) => holder,
            };
            layer = layer.child(
                holder
                    .opacity(opacity)
                    .child(img(source).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)),
            );
        }
        layer.into_any_element()
    }

    /// `sK`: "Explore What's New" over the card grid.
    fn nitro_whats_new(
        &self,
        cards: &[Card],
        container: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let columns = grid_columns(container);
        let width = card_width(container);
        let mut rows = div().flex().flex_col().gap(px(GRID_GAP));
        for chunk in cards.chunks(columns) {
            let mut row = div().flex().gap(px(GRID_GAP));
            for card in chunk {
                row = row.child(self.home_perk_card(card, width, window, cx));
            }
            rows = rows.child(row);
        }
        div()
            .mb(px(88.))
            .w(px(content_width(container)))
            .child(
                Self::nitro_heading(dc_intl::t("Aw5DRm"), 24., 26., true)
                    .text_color(color("--text-strong"))
                    .mb(px(24.)),
            )
            .child(rows)
            .into_any_element()
    }

    /// `sV`: the heading, the four filter pills (`top-pill` tab bar, look `custom`) and the scroller of 280px cards.
    fn nitro_favorites(
        &self,
        cards: &[Card],
        container: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.nitro.favorites_tab.min(TABS.len() - 1);
        let tabs = div()
            .mt(px(32.))
            .mb(px(20.))
            .ml(px(-8.))
            .flex()
            .flex_wrap()
            .gap_y(px(8.))
            .children(TABS.iter().enumerate().map(|(index, (id, label))| {
                let chosen = index == selected;
                div()
                    .id(SharedString::from(format!("nitro-favorites-tab-{id}")))
                    .mx(px(8.))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(99.))
                    .border_1()
                    .border_color(if chosen {
                        color("--brand-500")
                    } else {
                        gpui::transparent_black().into()
                    })
                    .bg(color("--control-secondary-background-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
                    .active(|style| style.bg(color("--control-secondary-background-active")))
                    .flex()
                    .items_center()
                    .when(!chosen, |this| this.cursor_pointer())
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.nitro.favorites_tab != index {
                            this.nitro.favorites_tab = index;
                            // A new tab is a new list (`key: n`): it starts at the left.
                            this.nitro
                                .favorites_scroll
                                .set_offset(gpui::point(px(0.), px(0.)));
                            cx.notify();
                        }
                    }))
            }));
        let shown: Vec<&Card> = cards
            .iter()
            .filter(|card| card.categories.contains(&TABS[selected].0))
            .collect();
        let count = shown.len();
        let view = content_width(container) + 16.;
        let offset = -f32::from(self.nitro.favorites_scroll.offset().x);
        let total =
            16. + count as f32 * FAVORITE_WIDTH + count.saturating_sub(1) as f32 * FAVORITE_GAP;
        let overflowing = total > view + 0.5;
        let can_left = offset > 0.5;
        let can_right = offset < total - view - 1.;
        let mut row = div()
            .id("nitro-favorites-row")
            .flex()
            .gap(px(FAVORITE_GAP))
            .overflow_x_scroll()
            .track_scroll(&self.nitro.favorites_scroll);
        for (index, card) in shown.iter().enumerate() {
            row = row.child(
                div()
                    .flex_none()
                    .mt(px(16.))
                    .mb(px(24.))
                    .when(index == 0, |this| this.ml(px(8.)))
                    .when(index + 1 == count, |this| this.mr(px(8.)))
                    .child(self.home_perk_card(card, FAVORITE_WIDTH, window, cx)),
            );
        }
        let arrow = |id: &'static str, glyph: &'static str, enabled: bool, right: bool| {
            let fade = motion::value(
                motion::key(&("nitro-favorites-arrow", id)),
                if enabled { 1. } else { 0. },
                motion::Css::new(100., motion::Ease::Linear),
            );
            div()
                .id(id)
                .size(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-overlay-secondary-border-default"))
                .bg(color("--control-overlay-secondary-background-default"))
                .hover(|style| style.bg(color("--control-overlay-secondary-background-hover")))
                .active(|style| style.bg(color("--control-overlay-secondary-background-active")))
                .flex()
                .items_center()
                .justify_center()
                .opacity(fade)
                .when(enabled, |this| this.cursor_pointer())
                .child(icon(glyph, 20., "--control-overlay-secondary-icon-default"))
                .when(enabled, |this| {
                    this.on_click(cx.listener(move |this, _, _, cx| {
                        let handle = &this.nitro.favorites_scroll;
                        let at = -f32::from(handle.offset().x);
                        let to = page_step(count, view, at, right);
                        handle.set_offset(gpui::point(px(-to), px(0.)));
                        cx.notify();
                    }))
                })
        };
        let actions = overflowing.then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .p(px(12.))
                .flex()
                .items_center()
                .justify_between()
                .child(arrow(
                    "nitro-favorites-left",
                    "discover_arrow_left",
                    can_left,
                    false,
                ))
                .child(arrow(
                    "nitro-favorites-right",
                    "discover_arrow_right",
                    can_right,
                    true,
                ))
        });
        div()
            .mb(px(66.))
            .w(px(content_width(container)))
            .child(Self::nitro_heading(dc_intl::t("Uh3+CA"), 24., 26., true))
            .child(tabs)
            // `scrollContainer_cpKOhU` (8px past each side) around the hscroll container (4px padding, -4px margin).
            .child(div().relative().mx(px(-8.)).child(row).children(actions))
            .into_any_element()
    }

    /// `Y.A` (module 175020), default variant: a bordered card with the banner picture, "Gift Nitro", a line of copy and
    /// the Gift Nitro button.
    fn nitro_gift_banner(&self, container: f32, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(content_width(container)))
            .child(
                div()
                    .w_full()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color("--border-muted"))
                    .bg(color("--card-background-default"))
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    // `bannerImage_gLR1C5`: 300px slot, the picture contained at 200px.
                    .child(
                        div()
                            .h(px(300.))
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(self.images.get(GIFT_BANNER_KEY).cloned().map(|image| {
                                img(image).max_h(px(200.)).object_fit(ObjectFit::Contain)
                            })),
                    )
                    .child(
                        div()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .text_center()
                            .child(
                                div()
                                    .mb(px(12.))
                                    .text_size(px(40.))
                                    .line_height(px(44.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::tr("Gift Nitro")),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::tr("Gift someone Nitro to give them access to profile customization, emoji, and superpowered chat features.")),
                            )
                            .child(
                                div()
                                    .mt(px(24.))
                                    .child(self.gift_button("nitro-home-gift", cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `tP`: the sticky bar of `sX` (`navBar_jHfUxk`: blurred, a 1px strong bottom border, 16px side padding). The
    /// Nitro wheel and the wordmark on the left (`brand_GB6rv4`, 8px apart), the wishlist heart and Gift Nitro on
    /// the right (`navBarActions`, 8px apart).
    fn nitro_subscriber_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(49.))
            .px(px(16.))
            .border_b_1()
            .border_color(color("--border-strong"))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("nitro_wheel", 24., "--icon-strong"))
                    .child(Self::nitro_heading(dc_intl::t("Ipxkog"), 14., 16., true)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        // The wishlist heart (module 521440): adding Nitro to the wishlist is the Shop's write.
                        div()
                            .id("nitro-home-wishlist")
                            .size(px(32.))
                            .rounded(px(8.))
                            .bg(color("--control-overlay-secondary-background-default"))
                            .hover(|style| {
                                style.bg(color("--control-overlay-secondary-background-hover"))
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("golive_heart", 16., "--text-strong")),
                    )
                    .child(
                        div()
                            .id("nitro-home-bar-gift")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(color("--control-overlay-secondary-background-default"))
                            .hover(|style| {
                                style.bg(color("--control-overlay-secondary-background-hover"))
                            })
                            .active(|style| {
                                style.bg(color("--control-overlay-secondary-background-active"))
                            })
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(icon("gift", 16., "--white"))
                            .child(dc_intl::tr("Gift Nitro"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(true, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The whole page (`sQ`).
    pub(super) fn nitro_subscriber_home(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.request_subscriber_art();
        let probe_width = self.nitro.home_width.clone();
        // The page's width, measured each layout; a change draws again at the new width.
        let probe = canvas(
            move |bounds, window, _| {
                let width = f32::from(bounds.size.width);
                if (probe_width.get() - width).abs() > 0.5 {
                    probe_width.set(width);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let page_color = color("--background-base-lower");
        if !self.subscriber_home_ready() {
            // `loading_IRnLy8 treatmentLoading_IRnLy8`: the bar over a spinner in a 520px box.
            return div()
                .relative()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .bg(page_color)
                .child(probe)
                .child(
                    div()
                        .h(px(520.))
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::ui::login::wandering_cubes()),
                )
                .child(self.nitro_subscriber_bar(cx))
                .into_any_element();
        }
        let container = self.home_width(window);
        let (whats_new, favorites) = self.subscriber_cards();
        // Decode every card's picture now, the other tabs' too, so a tab shows its art on its first frame.
        let keys: Vec<String> = whats_new
            .iter()
            .chain(&favorites)
            .filter_map(|card| card.asset.source())
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in &keys {
            if let Some(image) = self.images.get(key).cloned() {
                let _ = image.use_render_image(window, cx);
            }
        }
        self.log_art_arrivals(&keys);
        self.decode_wumpus(cx);
        let scroll = self.nitro.scroll.clone();
        let background = self.images.get(BACKGROUND_KEY).cloned();
        let bottom = self.images.get(BOTTOM_KEY).cloned();
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .bg(page_color)
            .child(
                div()
                    .id("nitro-subscriber-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .child(probe)
                            // `gradient_u8qKJM`: 984px tall at the top, masked to nothing from 60% down; the mask is
                            // drawn as a gradient of the page color over the lower 40%.
                            .children(background.map(|image| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w_full()
                                    .h(px(984.))
                                    .overflow_hidden()
                                    .child(
                                        img(image).w_full().h(px(984.)).object_fit(ObjectFit::Fill),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .bottom_0()
                                            .w_full()
                                            .h(px(394.))
                                            .bg(linear_gradient(
                                                180.,
                                                linear_color_stop(page_color.opacity(0.), 0.),
                                                linear_color_stop(page_color, 1.),
                                            )),
                                    )
                            }))
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    // The sticky bar takes 49px and gives back 48 (`margin-bottom` of the header
                                    // height).
                                    .pt(px(1.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(self.nitro_greeting(container))
                                    .child(self.nitro_whats_new(&whats_new, container, window, cx))
                                    .child(self.nitro_favorites(&favorites, container, window, cx))
                                    .child(self.nitro_gift_banner(container, cx))
                                    // `footerSpacing_IRnLy8`; the 112 x 85 `bottomIllustration` sits at the bottom
                                    // of the page, centred.
                                    .child(div().h(px(180.)).w_full())
                                    .child(
                                        div()
                                            .absolute()
                                            .bottom_0()
                                            .left_0()
                                            .right_0()
                                            .flex()
                                            .justify_center()
                                            .child(
                                                div().w(px(112.)).h(px(85.)).children(bottom.map(
                                                    |image| img(image).w(px(112.)).h(px(85.)),
                                                )),
                                            ),
                                    ),
                            ),
                    ),
            )
            .child(self.nitro_subscriber_bar(cx))
            .child(crate::ui::settings::auto_scrollbar(scroll))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_follows_the_local_hour() {
        assert_eq!(greeting(4), "Good Evening,");
        assert_eq!(greeting(5), "Good Morning,");
        assert_eq!(greeting(11), "Good Morning,");
        assert_eq!(greeting(12), "Good Afternoon,");
        assert_eq!(greeting(16), "Good Afternoon,");
        assert_eq!(greeting(17), "Good Evening,");
        assert_eq!(greeting(0), "Good Evening,");
    }

    #[test]
    fn the_grid_follows_the_container_queries() {
        assert_eq!(grid_columns(719.), 1);
        assert_eq!(grid_columns(720.), 2);
        assert_eq!(grid_columns(959.), 2);
        assert_eq!(grid_columns(960.), 3);
        // A 2000px window less the rail and sidebar: the content stops at 1200, three cards of 384.
        assert_eq!(content_width(1625.), 1200.);
        assert!((card_width(1625.) - 384.).abs() < 1e-3);
        assert!((card_width(905.) - (841. - 24.) / 2.).abs() < 1e-3);
        assert_eq!(content_width(40.), 0.);
    }

    fn named(id: &'static str) -> Card {
        card(id, String::new(), String::new(), Asset::None, None, &[])
    }

    #[test]
    fn whats_new_puts_the_tenure_card_first_and_keeps_six() {
        let ids = |cards: Vec<Card>| cards.into_iter().map(|card| card.id).collect::<Vec<_>>();
        let fixed = || {
            vec![
                named("display-name-styles"),
                named("client-themes"),
                named("permadecos"),
                named("custom-app-icons"),
            ]
        };
        assert_eq!(
            ids(arrange_whats_new(named("tenure-badge"), None, fixed())),
            [
                "tenure-badge",
                "display-name-styles",
                "client-themes",
                "permadecos",
                "custom-app-icons"
            ]
        );
        // With the Orbs reward the grid has two full rows of three.
        assert_eq!(
            ids(arrange_whats_new(
                named("tenure-badge"),
                Some(named("nitro-orbs-rewards")),
                fixed()
            )),
            [
                "tenure-badge",
                "nitro-orbs-rewards",
                "display-name-styles",
                "client-themes",
                "permadecos",
                "custom-app-icons"
            ]
        );
        let mut more = fixed();
        more.push(named("extra"));
        assert_eq!(
            arrange_whats_new(named("tenure-badge"), Some(named("orbs")), more).len(),
            6
        );
    }

    #[test]
    fn each_tab_lists_the_favorites_of_its_category() {
        let favorites = favorites_without(
            favorites_all(Asset::NitroMark, false),
            &[
                "tenure-badge",
                "display-name-styles",
                "client-themes",
                "permadecos",
                "custom-app-icons",
            ],
        );
        let ids = |tab: &str| {
            favorites
                .iter()
                .filter(|card| card.categories.contains(&tab))
                .map(|card| card.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids("bestof"),
            [
                "server-boosts",
                "profiles",
                "hd-video",
                "more-emojis",
                "large-uploads"
            ]
        );
        assert_eq!(ids("appearance"), ["profiles", "entrance-sounds"]);
        assert_eq!(
            ids("upgrades"),
            [
                "server-boosts",
                "hd-video",
                "more-emojis",
                "large-uploads",
                "custom-sounds",
                "special-stickers",
                "super-reactions",
                "video-backgrounds"
            ]
        );
        assert_eq!(ids("vip"), ["early-access", "badge", "special-pricing"]);
        // Every card that is not the badge mark has a picture to load.
        assert!(
            favorites
                .iter()
                .filter(|card| card.id != "badge")
                .all(|card| card.asset.source().is_some())
        );
    }

    #[test]
    fn the_copy_comes_from_the_catalog() {
        let all = favorites_all(Asset::NitroMark, false);
        assert_eq!(all[0].title, "2 Server Boosts + 30% off");
        assert_eq!(
            all[0].description,
            "Support the servers you love with 2 included Boosts and 30% off when you buy more."
        );
        let uploads = all.iter().find(|card| card.id == "large-uploads").unwrap();
        assert_eq!(
            uploads.description,
            "Send files up to 500MB. Perfect for game clips, art, and more."
        );
        let fixed = whats_new_fixed();
        assert!(fixed[1].description.contains("20+ themes"));
        assert_eq!(dc_intl::t("hx5AFp"), "Orbs on repeat");
        assert_eq!(dc_intl::t("Aw5DRm"), "Explore What's New");
    }

    #[test]
    fn the_arrows_page_through_the_favorites() {
        // Twelve cards in a 1216px view: 8 + 12 * 280 + 11 * 20 + 8 = 3596 wide, so at most 2380 scrolled.
        let view = 1216.;
        let right = page_step(12, view, 0., true);
        // The last card in full view is the fourth (908 to 1188): the fifth becomes the first, 16px in.
        assert_eq!(right, 8. + 4. * 300. - 16.);
        // From there the ninth would go 16px in, past the end: the scroller stops at its end.
        let end = page_step(12, view, right, true);
        assert_eq!(end, 3596. - view);
        assert_eq!(page_step(12, view, end, true), end);
        // Back: the first card in view (the ninth, from 2408) is whole, so the eighth ends 16px from the right.
        assert_eq!(page_step(12, view, end, false), 2108. + 280. - view + 16.);
        assert_eq!(page_step(0, view, 0., true), 0.);
        assert_eq!(page_step(3, view, 0., true), 0.);
    }
}
