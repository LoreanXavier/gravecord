//! The Nitro page (Home rail, `/store`): hero, What's New, Best of Nitro, plan cards, the comparison table and the
//! footer call to action, with the real plans and prices from the API. Geometry is from
//! `design/dom/home/nitro-store-darker-cozy.json` (1280x720, darker, cozy); sections are laid out at their measured
//! heights so the scroll offsets of the nav bar match.
//!
//! Not drawn: the hero and bento videos, and the Rive buttons' glow. Those are video and
//! canvas content in the official page (`BLOCKED.md`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_state::premium::{self, Plan, PremiumType, Price, sku};
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, ScrollHandle, Window, canvas, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;

pub(in crate::ui) mod home;
mod perk_card;
mod tenure;
mod trinkets;

pub(super) const NITRO_PREFIX: &str = "nitro:";
const PLANS_KEY: &str = "nitro:plans:";
const SOURCES_KEY: &str = "nitro:sources";
const SUBSCRIPTIONS_KEY: &str = "nitro:subscriptions";
const PAYMENTS_KEY: &str = "nitro:payments";
const GIFT_CODES_KEY: &str = "nitro:gift-codes";

/// `font-family` of the `nitro-md` text variants.
pub(super) const NITRO_FONT: &str = "ABC Ginto Discord Nord";

const CDN: &str = "https://cdn.discordapp.com/assets/content/";
/// The hero trinkets, the plan card mascot and the footer mascot (`img` sources of the capture).
const ART: [(&str, &str); 8] = [
    (
        "bolt",
        "ae5638c61a572593c6b03b92e80d3846e0cfe7a9e893f3faf05aecd670a4017d.png",
    ),
    (
        "car",
        "6cb761e3e97838c7927f738882b67bd825d5eeed89633e0af126bda5f9d4e71d.png",
    ),
    (
        "hammer",
        "b39a5f2755e6da320fce10c8a4a10bdebef9926b671256b1681976198c5656d4.png",
    ),
    (
        "key",
        "7c23a220a6f31150648930e2ebb435aa7cc89ad57895275bed6f1900869f4de0.png",
    ),
    (
        "star",
        "0b1a59149e615fc048010a3c7f109f8695c8b2004712e99417fcb0dec43fcb44.png",
    ),
    (
        "rocket",
        "3aa94cb4beecb43100d482e94a5a707f188e2e2315d9b15865689b51409c4d56.png",
    ),
    (
        "jump",
        "30b4235a9a15735cae3f814c3389942356e6138fe5651945028afff3b421202b.png",
    ),
    (
        "youtube",
        "1db181f0a98c20928da64ee63fe3184fd6b60f1b7d4c4fb69560aeb802b55c28.svg",
    ),
];

/// Art of the purchase modals: the plan card's clouds and mascot (`discord.com/assets`) and the gift banner icon.
pub(super) const MODAL_ART: [(&str, &str); 9] = [
    (
        "cloud-big",
        "https://discord.com/assets/2752cc3e62a7cfd5.svg",
    ),
    (
        "cloud-small",
        "https://discord.com/assets/fde6959de82ed00e.svg",
    ),
    ("wumpus", "https://discord.com/assets/0793a123535de37d.svg"),
    (
        "gift-box",
        "https://cdn.discordapp.com/assets/content/b88750c7565c6dc55140754c3de85c45809121217d3aefb842e3999f887e1931.png",
    ),
    (
        "gift-champion",
        "https://cdn.discordapp.com/assets/content/101d1f232a0b465978b0607e2258fbc711806858ab8255b893b164652c8533c1.png",
    ),
    (
        "gift-luminary",
        "https://cdn.discordapp.com/assets/content/8b43ae51e2e88a961420044d8c2e30316a7dc0f8c3e50d1ea1d6810bdab69fec.png",
    ),
    (
        "gift-icon",
        "https://cdn.discordapp.com/assets/content/9c25b98950c3a34c327b5e038ccf117f08ac150338bc39398caac7cef8038e25.png",
    ),
    (
        "gift-hero",
        "https://cdn.discordapp.com/assets/content/ad23878cb00ef0c00e0b7484513f5e1f064b78a6f875dacc549120570f7ec87f.png",
    ),
    (
        "gift-legend",
        "https://cdn.discordapp.com/assets/content/e063f16d36efcebb21c7057160081afb9c564c93b6df629c61e45613886d1aee.png",
    ),
];

pub(super) fn art_key(name: &str) -> String {
    format!("nitro-art:{name}")
}

/// The SKUs whose plans the page lists (the official page asks for all four: Basic, Classic, Nitro, Boost).
const PLAN_SKUS: [u64; 4] = [sku::TIER_0, sku::TIER_1, sku::TIER_2, sku::GUILD];

/// Section tops inside the page, from the capture (`sectionsContainer` children minus the container's top).
const SECTION_TOPS: [(&str, f32); 5] = [
    ("Home", 0.),
    ("What's New", 400.),
    ("Best of Nitro", 1576.),
    ("Plans", 3119.),
    ("Compare", 4705.),
];

/// The page's state.
pub(crate) struct NitroState {
    pub(super) plans: Vec<Plan>,
    pub(super) requested: bool,
    pub(super) billing_requested: bool,
    pub(super) payments: Option<Value>,
    pub(super) gift_codes: Option<Value>,
    pub(super) scroll: ScrollHandle,
    /// The last offset the page was drawn at, so a scroll redraws the nav bar and the floating bar.
    seen: Rc<Cell<f32>>,
    /// The yearly option of the Nitro and Nitro Basic cards is selected (the capture's default).
    pub(super) nitro_yearly: Option<bool>,
    pub(super) basic_yearly: Option<bool>,
    pub(super) purchase: Option<super::nitro_purchase::Purchase>,
    /// The key and load id of a browser hand-off in flight.
    #[cfg(not(feature = "capture"))]
    pub(super) handoff: Option<(String, String)>,
    pub(super) sources: Option<Value>,
    /// A capture step set `sources`: the account's own read must not replace it.
    pub(super) sources_injected: bool,
    pub(super) subscriptions: Option<Value>,
    /// A capture step set `subscriptions`: the account's own read must not replace it.
    pub(super) subscriptions_injected: bool,
    /// The selected filter pill of the Nitro subscriber home's favorites (`sV`).
    pub(super) favorites_tab: usize,
    /// The perk card under the pointer on the subscriber home.
    pub(super) hovered_perk: Option<&'static str>,
    /// The favorites' horizontal scroller.
    pub(super) favorites_scroll: ScrollHandle,
    /// The subscriber home's container width as last laid out (0 before the first layout).
    pub(super) home_width: Rc<Cell<f32>>,
    /// `GET /users/@me/program-rewards` was sent, and its body once answered (`{}` after a failure).
    pub(super) rewards_requested: bool,
    pub(super) rewards: Option<Value>,
    /// When the subscriber home was opened (wall clock, motion clock), and the pictures already logged as drawn.
    pub(super) home_opened: Option<(std::time::Instant, f64)>,
    pub(super) art_logged: std::collections::HashSet<String>,
    /// The hero's Wumpus clip, decoded once.
    pub(super) wumpus: Option<Rc<trinkets::Clip>>,
    pub(super) wumpus_decoding: bool,
}

impl Default for NitroState {
    fn default() -> Self {
        Self {
            plans: Vec::new(),
            requested: false,
            billing_requested: false,
            payments: None,
            gift_codes: None,
            scroll: ScrollHandle::new(),
            seen: Rc::new(Cell::new(f32::NAN)),
            nitro_yearly: Some(true),
            basic_yearly: None,
            purchase: None,
            #[cfg(not(feature = "capture"))]
            handoff: None,
            sources: None,
            sources_injected: false,
            subscriptions: None,
            subscriptions_injected: false,
            favorites_tab: 0,
            hovered_perk: None,
            favorites_scroll: ScrollHandle::new(),
            home_width: Rc::new(Cell::new(0.)),
            rewards_requested: false,
            rewards: None,
            home_opened: None,
            art_logged: std::collections::HashSet::new(),
            wumpus: None,
            wumpus_decoding: false,
        }
    }
}

/// `oklab(L a b / alpha)` as sRGB, for the capture's gradient stops.
pub(super) fn oklab(l: f32, a: f32, b: f32, alpha: f32) -> Hsla {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let linear = [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ];
    let encode = |value: f32| {
        let value = value.clamp(0., 1.);
        let encoded = if value <= 0.003_130_8 {
            12.92 * value
        } else {
            1.055 * value.powf(1. / 2.4) - 0.055
        };
        (encoded * 255.).round() as u32
    };
    let packed = (encode(linear[0]) << 24)
        | (encode(linear[1]) << 16)
        | (encode(linear[2]) << 8)
        | (alpha.clamp(0., 1.) * 255.).round() as u32;
    rgba(packed).into()
}

pub(super) fn rgb(red: u32, green: u32, blue: u32) -> Hsla {
    rgba((red << 24) | (green << 16) | (blue << 8) | 0xff).into()
}

/// One bento box (`container_pmhtZL`): the text, the button, and which side the art sits on.
struct Bento {
    id: &'static str,
    large: bool,
    art_left: bool,
    /// Red for the partner offer, else the purple wash.
    youtube: bool,
    caption: Option<&'static str>,
    title: &'static str,
    body: &'static str,
    link: Option<&'static str>,
    button: Option<&'static str>,
    /// The box video's `poster`, shown as the still art.
    poster: &'static str,
    /// Box height from the capture.
    height: f32,
}

const WHATS_NEW: [Bento; 3] = [
    Bento {
        id: "bento-youtube",
        large: true,
        art_left: false,
        youtube: true,
        caption: Some("youtube"),
        title: "Level up your YouTube with Premium",
        body: "If you like YouTube, you'll love Premium. Take 20% off your next 6 months, whether you're new to YouTube Premium or already in. ",
        link: Some("Terms apply"),
        button: Some("Unlock Offer"),
        poster: "https://cdn.discordapp.com/assets/content/67348bc7420d5ab8029fb93d7d514fcdbd39b9273c238123bb1ea90b656bfe13.webp",
        height: 424.,
    },
    Bento {
        id: "bento-cod",
        large: false,
        art_left: false,
        youtube: false,
        caption: None,
        title: "Bring Nitro to your loadout",
        body: "Get the exclusive blueprint for Call of Duty: Modern Warfare 4 made just for Nitro members.",
        link: None,
        button: Some("Unlock Offer"),
        poster: "https://cdn.discordapp.com/promotions/premium-marketing/1562520828854140978/1562521352869773412.png",
        height: 490.,
    },
    Bento {
        id: "bento-orbs",
        large: false,
        art_left: false,
        youtube: false,
        caption: Some("BETA"),
        title: "Unlock more Orbs with Nitro",
        body: "Grow your stash with monthly Orb drops and a 1.2x multiplier on Quest Rewards.",
        link: None,
        button: Some("Check it out"),
        poster: "https://cdn.discordapp.com/assets/content/de126b095fb3d2353650e750d46c54b7156297482f9205ca1645a45fb0082169.png",
        height: 490.,
    },
];

const BEST_OF: [Bento; 4] = [
    Bento {
        id: "bento-style",
        large: true,
        art_left: false,
        youtube: false,
        caption: None,
        title: "Show off your style",
        body: "Add an animated avatar, profile theme, banner image, and more.",
        link: None,
        button: Some("Take me there"),
        poster: "https://cdn.discordapp.com/assets/content/b1476d645dd5e91c5ce647dcaa93964348a69e91306f74d19384330afd07ad94.png",
        height: 424.,
    },
    Bento {
        id: "bento-space",
        large: true,
        art_left: true,
        youtube: false,
        caption: None,
        title: "Make this place your space",
        body: "Choose from 20+ color app themes and custom app icons.",
        link: None,
        button: Some("Take me there"),
        poster: "https://cdn.discordapp.com/assets/content/14a4e932f2091109eafab5febe77a0adb77ed2e811abdd59bd28c8b8ba0d50e8.png",
        height: 424.,
    },
    Bento {
        id: "bento-emoji",
        large: false,
        art_left: false,
        youtube: false,
        caption: None,
        title: "Up your emoji game",
        body: "Hype, roast, and meme with custom emoji anywhere.",
        link: None,
        button: None,
        poster: "https://cdn.discordapp.com/assets/content/dd32ed38e28109d89cc6d785f682cb0791307c378333c9b022bfe6a69dd0f94b.png",
        height: 401.,
    },
    Bento {
        id: "bento-limits",
        large: false,
        art_left: false,
        youtube: false,
        caption: None,
        title: "No limits, all fun",
        body: "Upload larger files and stream in HD up to 4K 60FPS.",
        link: None,
        button: None,
        poster: "https://cdn.discordapp.com/assets/content/4d379ffac2c0b98c7b2e790c9800a35916cea7915569758b57b3d1f7f9b91682.png",
        height: 401.,
    },
];

/// The plan cards' feature lines, `(icon, text)`.
pub(super) const NITRO_FEATURES: [(&str, &str); 6] = [
    ("nitroplan_1gb_uploads", "1GB uploads"),
    ("nitroplan_custom_emoji_anywhere", "Custom emoji anywhere"),
    (
        "nitroplan_unlimited_super_reactions",
        "Unlimited Super Reactions",
    ),
    ("nitroplan_hd_video_streaming", "HD video streaming"),
    ("nitroplan_2_server_boosts", "2 Server Boosts"),
    (
        "nitroplan_custom_profiles_and_more",
        "Custom profiles and more!",
    ),
];
pub(super) const BASIC_FEATURES: [(&str, &str); 4] = [
    ("nitroplan_50mb_uploads", "50MB uploads"),
    ("nitroplan_custom_emoji_anywhere", "Custom emoji anywhere"),
    (
        "nitroplan_unlimited_super_reactions",
        "Unlimited Super Reactions",
    ),
    (
        "nitroplan_special_nitro_badge_on_your_profile",
        "Special Nitro badge on your profile",
    ),
];

/// The comparison table: groups of `(feature, basic, nitro)`; a value is included, not included, or text.
enum Cell3 {
    Yes,
    No,
    Text(&'static str),
}

use Cell3::{No, Text, Yes};

#[allow(clippy::type_complexity)]
const COMPARE: [(&str, &[(&str, Cell3, Cell3)]); 3] = [
    (
        "Show off your style",
        &[
            ("Nitro profile badge", Yes, Yes),
            ("Custom per-server profiles", No, Yes),
            ("Personalized entrance sounds", No, Yes),
            ("Display Name Styles", No, Yes),
        ],
    ),
    (
        "Make this place your space",
        &[
            ("Animated and custom emojis anywhere", Yes, Yes),
            ("Custom stickers anywhere", Yes, Yes),
            ("Custom app icons", Yes, Yes),
            ("Custom Soundboard sounds anywhere", No, Yes),
            ("20+ app color themes", No, Yes),
        ],
    ),
    (
        "No limits, all fun",
        &[
            ("Larger File Uploads", Text("50 MB"), Text("1 GB")),
            ("Custom video backgrounds", Yes, Yes),
            ("Super Reactions", Yes, Yes),
            ("Keep Quest avatar decorations rewards longer", No, Yes),
            ("HD streaming", No, Yes),
            ("2 Boosts + 30% off extra Boosts", No, Yes),
            ("3 Nitro friend passes", No, Yes),
            ("Shop member pricing, and exclusive items", No, Yes),
            ("Early Access", No, Yes),
            ("Longer messages up to 4000 characters", No, Yes),
            ("Join up to 200 servers", No, Yes),
        ],
    ),
];

/// Nitro Trial / Discount Terms Lines (module 222719: `2CGBri`, `z2oQtA`, `3ZiutU`, `9vyovu`,
/// `sJTwHQ`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum NitroTermsKind {
    /// Active free trial matching card (`2CGBri`).
    ActiveTrial { remaining_days: i64, price: String },
    /// Active yearly Tier 2 churn discount (`z2oQtA`).
    ActiveYearlyDiscount {
        percent: u32,
        regular_price: String,
        renewal_date: String,
    },
    /// Active monthly churn discount (`3ZiutU`).
    ActiveMonthlyDiscount {
        percent: u32,
        regular_price: String,
        num_months: u32,
    },
    /// Unredeemed trial offer (`9vyovu`).
    EligibleTrial {
        plan_name: String,
        duration: String,
        price: String,
    },
    /// Unredeemed discount offer (`sJTwHQ`).
    EligibleDiscount {
        num_months: u32,
        discounted_price: String,
        regular_price: String,
    },
}

/// Formats a `222719` terms line with `$b` bold spans rendered as `--text-strong`.
pub(super) fn format_nitro_terms(kind: &NitroTermsKind) -> gpui::StyledText {
    use dc_intl::Arg;
    use std::borrow::Cow;
    let runs = match kind {
        NitroTermsKind::ActiveTrial {
            remaining_days,
            price,
        } => dc_intl::format_runs(
            "2CGBri",
            &[
                ("remainingTime", Arg::Num((*remaining_days).max(0) as f64)),
                ("price", Arg::Str(Cow::Borrowed(price))),
            ],
        ),
        NitroTermsKind::ActiveYearlyDiscount {
            percent,
            regular_price,
            renewal_date,
        } => dc_intl::format_runs(
            "z2oQtA",
            &[
                ("percent", Arg::Num(f64::from(*percent))),
                ("regularPrice", Arg::Str(Cow::Borrowed(regular_price))),
                ("renewalDate", Arg::Str(Cow::Borrowed(renewal_date))),
            ],
        ),
        NitroTermsKind::ActiveMonthlyDiscount {
            percent,
            regular_price,
            num_months,
        } => dc_intl::format_runs(
            "3ZiutU",
            &[
                ("percent", Arg::Num(f64::from(*percent))),
                ("regularPrice", Arg::Str(Cow::Borrowed(regular_price))),
                ("numMonths", Arg::Num(f64::from(*num_months))),
            ],
        ),
        NitroTermsKind::EligibleTrial {
            plan_name,
            duration,
            price,
        } => dc_intl::format_runs(
            "9vyovu",
            &[
                ("planName", Arg::Str(Cow::Borrowed(plan_name))),
                ("duration", Arg::Str(Cow::Borrowed(duration))),
                ("price", Arg::Str(Cow::Borrowed(price))),
            ],
        ),
        NitroTermsKind::EligibleDiscount {
            num_months,
            discounted_price,
            regular_price,
        } => dc_intl::format_runs(
            "sJTwHQ",
            &[
                ("numMonths", Arg::Num(f64::from(*num_months))),
                ("discountedPrice", Arg::Str(Cow::Borrowed(discounted_price))),
                ("regularPrice", Arg::Str(Cow::Borrowed(regular_price))),
            ],
        ),
    };
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$b") && start < text.len() {
            highlights.push((
                start..text.len(),
                gpui::HighlightStyle {
                    font_weight: Some(FontWeight(700.)),
                    color: Some(color("--text-strong").into()),
                    ..gpui::HighlightStyle::default()
                },
            ));
        }
    }
    gpui::StyledText::new(text).with_highlights(highlights)
}

impl Root {
    /// Resolves the `222719` trial/discount terms line for a plan card (`sku::TIER_0` or `sku::TIER_2`).
    pub(super) fn plan_card_terms(&self, sku_id: u64) -> Option<NitroTermsKind> {
        let month_price = self.plan_price(sku_id, false).map(|p| p.display())?;
        let regular_price = format!("{month_price}/month");
        if let Some(sub) = self.premium_subscription()
            && sub
                .premium_item()
                .and_then(|item| premium::plan_sku(item.plan_id))
                == Some(sku_id)
        {
            if sub.trial_id.is_some() || sub.trial_ends_at.is_some() {
                let remaining_days = sub
                    .trial_ends_at
                    .as_deref()
                    .and_then(|iso| chrono::DateTime::parse_from_rfc3339(iso).ok())
                    .map(|end| {
                        (end.with_timezone(&chrono::Utc) - chrono::Utc::now())
                            .num_days()
                            .max(0)
                    })
                    .unwrap_or(0);
                return Some(NitroTermsKind::ActiveTrial {
                    remaining_days,
                    price: regular_price,
                });
            }
            if let Some(offer) = self.fetched.get("user-offer")
                && let Some(pct) = offer["discount"]["amount"].as_u64()
            {
                let is_yearly_t2 = sub
                    .premium_item()
                    .is_some_and(|i| i.plan_id == premium::plan_id::YEAR_TIER_2);
                if is_yearly_t2 {
                    let renewal_date = sub
                        .current_period_end
                        .as_deref()
                        .and_then(|s| s.get(..10))
                        .unwrap_or("Dec 2, 2026")
                        .to_owned();
                    return Some(NitroTermsKind::ActiveYearlyDiscount {
                        percent: pct as u32,
                        regular_price,
                        renewal_date,
                    });
                }
                let num_months = offer["discount"]["user_usage_limit"]
                    .as_u64()
                    .unwrap_or(1)
                    .max(1) as u32;
                return Some(NitroTermsKind::ActiveMonthlyDiscount {
                    percent: pct as u32,
                    regular_price,
                    num_months,
                });
            }
        }
        if sku_id == sku::TIER_2
            && let Some(offer) = self.fetched.get("user-offer")
        {
            if let Some(pct) = offer["discount"]["amount"].as_u64() {
                let num_months = offer["discount"]["user_usage_limit"]
                    .as_u64()
                    .unwrap_or(1)
                    .max(1) as u32;
                let base_cents = self
                    .plan_price(sku_id, false)
                    .map(|p| p.amount)
                    .unwrap_or(999);
                let disc_cents =
                    base_cents.saturating_mul((100u64.saturating_sub(pct)) as i64) / 100;
                let discounted_price = format!("${:.2}", disc_cents as f64 / 100.);
                return Some(NitroTermsKind::EligibleDiscount {
                    num_months,
                    discounted_price,
                    regular_price,
                });
            }
            if offer.get("subscription_trial").is_some() || offer.get("user_trial_offer").is_some()
            {
                return Some(NitroTermsKind::EligibleTrial {
                    plan_name: "Nitro".to_owned(),
                    duration: "1 month".to_owned(),
                    price: regular_price,
                });
            }
        }
        None
    }
}

impl Root {
    /// Pre-requests shared Nitro assets after startup so opening the Nitro tab renders immediately.
    pub(super) fn warm_nitro_art(&mut self) {
        if !super::start_cache::enabled() {
            return;
        }
        for (name, file) in ART {
            self.request_image(art_key(name), format!("{CDN}{file}"));
        }
        if self.premium() == PremiumType::Nitro {
            self.warm_subscriber_home();
        } else {
            for item in WHATS_NEW.iter().chain(BEST_OF.iter()) {
                self.request_image(item.poster.to_owned(), item.poster.to_owned());
            }
        }
    }

    /// Starts the reads the page needs: the plans of each SKU, the saved payment sources and the subscriptions.
    pub(super) fn request_nitro(&mut self) {
        // A Nitro subscriber sees the subscriber home: its pictures go first, in parallel.
        if self.premium() == PremiumType::Nitro {
            self.request_subscriber_art();
        }
        for (name, file) in ART {
            self.request_image(art_key(name), format!("{CDN}{file}"));
        }
        for (name, url) in MODAL_ART {
            self.request_image(art_key(name), url.to_owned());
        }
        for (name, url, _, _) in super::settings::nitro_page::PERK_ART {
            self.request_image(art_key(name), url.to_owned());
        }
        for item in WHATS_NEW.iter().chain(BEST_OF.iter()) {
            self.request_image(item.poster.to_owned(), item.poster.to_owned());
        }
        self.request_billing();
        if self.nitro.requested {
            return;
        }
        self.nitro.requested = true;
        for sku in PLAN_SKUS {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{PLANS_KEY}{sku}")),
                method: reqwest::Method::GET,
                route: format!(
                    "/store/published-listings/skus/{sku}/subscription-plans?revenue_surface=0"
                ),
                body: None,
            });
        }
    }

    /// The reads of the billing pages: saved payment sources, subscriptions and the payment history.
    pub(super) fn request_billing(&mut self) {
        if self.nitro.billing_requested {
            return;
        }
        self.nitro.billing_requested = true;
        for (key, route) in [
            (SOURCES_KEY, "/users/@me/billing/payment-sources"),
            (
                SUBSCRIPTIONS_KEY,
                "/users/@me/billing/subscriptions?sync_level=1",
            ),
            (PAYMENTS_KEY, "/users/@me/billing/payments?limit=20"),
            (GIFT_CODES_KEY, "/users/@me/entitlements/gift-codes"),
            ("user-offer", "/users/@me/billing/user-offer"),
            // Module 579245 (`589152.b`): Nitro friends / affinity social proof banner.
            ("nitro:affinity", "/users/@me/billing/nitro-affinity"),
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(key.to_owned()),
                method: reqwest::Method::GET,
                route: route.to_owned(),
                body: None,
            });
        }
    }

    pub(super) fn nitro_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.get(key).cloned().unwrap_or_default();
        #[cfg(not(feature = "capture"))]
        if key == super::nitro_purchase::HANDOFF_KEY {
            self.checkout_handoff_ready(cx);
            return;
        }
        if key == home::REWARDS_KEY {
            // A failed read leaves the store ready with nothing in it (`isError`).
            self.nitro.rewards = Some(if report["ok"] == true {
                report["body"].clone()
            } else {
                Value::Object(Default::default())
            });
        } else if key.starts_with(PLANS_KEY) {
            if report["ok"] == true {
                let incoming = premium::parse_plans(&report["body"]);
                self.nitro
                    .plans
                    .retain(|plan| !incoming.iter().any(|new| new.sku_id == plan.sku_id));
                self.nitro.plans.extend(incoming);
            }
        } else if key == SOURCES_KEY {
            if report["ok"] == true && !self.nitro.sources_injected {
                self.nitro.sources = Some(report["body"].clone());
                self.request_payment_source_art();
            }
        } else if key == GIFT_CODES_KEY {
            if report["ok"] == true {
                self.nitro.gift_codes = Some(report["body"].clone());
            }
        } else if key == PAYMENTS_KEY {
            if report["ok"] == true {
                self.nitro.payments = Some(report["body"].clone());
            }
        } else if key == SUBSCRIPTIONS_KEY
            && report["ok"] == true
            && !self.nitro.subscriptions_injected
        {
            self.nitro.subscriptions = Some(report["body"].clone());
        }
        cx.notify();
    }

    /// The price of a SKU's monthly or yearly plan for the default purchase type.
    pub(super) fn plan_price(&self, sku: u64, yearly: bool) -> Option<Price> {
        self.plan_price_for(sku, yearly, false)
    }

    /// The same for a purchase type: 0 is a subscription for oneself, 1 is a gift (its own prices, in USD).
    pub(super) fn plan_price_for(&self, sku: u64, yearly: bool, gift: bool) -> Option<Price> {
        premium::find_plan(&self.nitro.plans, sku, yearly)?.price(u32::from(gift), None)
    }

    pub(super) fn open_nitro(&mut self, cx: &mut Context<Self>) {
        if self.nitro_open && self.guild.is_none() && self.channel.is_none() {
            return;
        }
        let from = self.route();
        self.history.visit(
            from,
            super::history::Route {
                guild: None,
                channel: None,
                favorites: false,
                nitro: true,
                guild_guide: false,
                channel_browser: false,
                discover: None,
                quests: false,
                shop: false,
                message_requests: false,
                library: false,
            },
        );
        self.favorites_view = false;
        self.discover.open = false;
        self.guild = None;
        self.channel = None;
        self.nitro_open = true;
        self.guild_guide_open = false;
        self.channel_browser_open = false;
        // Each visit mounts the page again: its art animations start over and its pictures are timed again.
        self.nitro.home_opened = None;
        self.nitro.art_logged.clear();
        self.nitro.hovered_perk = None;
        // Module 105390 / 432458: acknowledge an unacked Nitro trial/discount offer when opening Nitro.
        if self.unacknowledged_nitro_offer_count() > 0 && !crate::storage::is_capture_run() {
            let _ = self.requests.send(Request::Rest {
                key: Some("user-offer".to_owned()),
                method: reqwest::Method::POST,
                route: "/users/@me/billing/user-offer/ack".to_owned(),
                body: Some(serde_json::json!({})),
            });
        }
        self.request_nitro();
        cx.notify();
    }

    /// "Plans start at only X/month. Cancel anytime": the cheapest monthly plan, Nitro Basic.
    fn nitro_starting_price(&self) -> Option<String> {
        self.plan_price(sku::TIER_0, false)
            .map(|price| price.display())
    }

    pub(super) fn nitro_text(
        text: impl Into<gpui::SharedString>,
        size: f32,
        line: f32,
        weight: f32,
        tint: &str,
    ) -> gpui::Div {
        div()
            .text_size(px(size))
            .line_height(px(line))
            .font_weight(FontWeight(weight))
            .text_color(color(tint))
            .child(text.into())
    }

    /// `nitro-md` headings: the Nitro display face, bold italic.
    pub(super) fn nitro_heading(text: &str, size: f32, line: f32, upper: bool) -> gpui::Div {
        div()
            .font_family(NITRO_FONT)
            .italic()
            .text_size(px(size))
            .line_height(px(line))
            .font_weight(FontWeight(700.))
            .text_color(color("--text-strong"))
            .child(if upper {
                text.to_uppercase()
            } else {
                text.to_owned()
            })
    }

    pub(super) fn nitro_art(&self, name: &str, width: f32, height: f32) -> AnyElement {
        match self.images.get(&art_key(name)) {
            Some(image) => img(image.clone())
                .w(px(width))
                .h(px(height))
                .into_any_element(),
            None => div().w(px(width)).h(px(height)).into_any_element(),
        }
    }

    /// The white `expressive` button: Subscribe, with the Nitro wheel.
    ///
    /// `target`: the plan a card has selected, which goes straight to checkout; `None` opens Select Plan. A card
    /// with nothing selected passes `enabled = false`.
    ///
    /// `cta` is the button's text and state for the account's subscription (module 572079): Upgrade, Current Plan, or
    /// a disabled Subscribe. A disabled button is half opaque, not-allowed, does nothing and may explain itself.
    fn subscribe_button(
        &self,
        id: &'static str,
        full: bool,
        target: Option<(u64, bool)>,
        enabled: bool,
        cta: premium::SubscribeCta,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = enabled && !cta.disabled;
        let button = div()
            .id(id)
            .when(!enabled, |this| this.opacity(0.5))
            .when(full, |this| this.w_full())
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(rgba(0xffffffff))
            .when(enabled, |this| this.cursor_pointer())
            .when(!enabled, |this| this.cursor_not_allowed())
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(rgba(0x000000ff))
            .child(icon("nitro_wheel", 20., "--black"))
            .child(cta.text)
            .on_click(cx.listener(move |this, _, _, cx| {
                if !enabled {
                    return;
                }
                match target {
                    Some((sku, yearly)) => this.open_checkout(sku, yearly, cx),
                    None => this.open_purchase(false, cx),
                }
            }));
        match cta.tooltip {
            Some(tip) => self
                .with_tip(
                    format!("{id}-tip"),
                    super::tooltip::Tip::text(tip, super::tooltip::Side::Top),
                    button,
                    cx,
                )
                .when(full, |this| this.w_full())
                .into_any_element(),
            None => button.into_any_element(),
        }
    }

    fn gift_button(&self, id: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(id)
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--control-secondary-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(icon("gift", 20., "--text-strong"))
            .child("Gift Nitro")
            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(true, cx)))
            .into_any_element()
    }

    fn nitro_hero(&self, cx: &mut Context<Self>) -> AnyElement {
        let note = self
            .nitro_starting_price()
            .map(|price| format!("Plans start at only {price}/month. Cancel anytime"))
            .unwrap_or_default();
        let trinket = |name: &str, x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .child(self.nitro_art(name, w, h))
        };
        // Offsets are relative to the centered 809px stage inside sectionsContainer.
        div()
            .h(px(400.))
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(809.))
                    .h(px(400.))
                    .child(trinket("star", 398. - 375., 117. - 33., 83., 90.))
                    .child(trinket("hammer", 959. - 375., 103. - 33., 100., 103.))
                    .child(trinket("car", 562. - 375., 242. - 33., 70., 59.))
                    .child(trinket("key", 463. - 375., 349. - 33., 78., 72.))
                    .child(trinket("rocket", 1048. - 375., 214. - 33., 195., 195.))
                    .child(trinket("bolt", 1181. - 375., 348. - 33., 62., 87.))
                    .child(
                        div()
                            .absolute()
                            .top(px(120.))
                            .left(px(119.5))
                            .w(px(650.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                Self::nitro_heading(
                                    "Unlock a World of Perks with Nitro",
                                    48.,
                                    48.,
                                    true,
                                )
                                .w_full()
                                .text_center(),
                            )
                            .child(
                                div()
                                    .mt(px(24.))
                                    .flex()
                                    .gap(px(16.))
                                    .child(self.subscribe_button(
                                        "nitro-hero-subscribe",
                                        false,
                                        None,
                                        true,
                                        self.subscribe_cta(self.hero_upgrade_sku()),
                                        cx,
                                    ))
                                    .child(self.gift_button("nitro-hero-gift", cx)),
                            )
                            .child(
                                Self::nitro_text(note, 12., 16., 500., "--text-muted")
                                    .mt(px(24.))
                                    .text_center(),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn bento_section(
        &self,
        title: &'static str,
        id: &'static str,
        boxes: &[Bento],
        bento_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut grid = div().flex().flex_wrap().gap(px(32.)).w_full();
        for item in boxes {
            grid = grid.child(self.bento_box(item, bento_w, cx));
        }
        div()
            .id(id)
            .flex()
            .flex_col()
            .items_center()
            .w(px(bento_w))
            .child(Self::nitro_heading(title, 32., 34., true))
            .child(div().w_full().mt(px(32.)).child(grid))
            .into_any_element()
    }

    fn bento_box(&self, item: &Bento, bento_w: f32, cx: &mut Context<Self>) -> AnyElement {
        let _ = cx;
        let large_art_w = ((bento_w - 24.) * (457. / 785.)).round();
        let small_card_w = (bento_w - 32.) / 2.;
        let wash = if item.youtube {
            linear_gradient(
                180.,
                linear_color_stop(rgb(62, 0, 12).opacity(0.75), 0.),
                linear_color_stop(color("--background-base-lower"), 1.),
            )
        } else {
            linear_gradient(
                270.,
                linear_color_stop(oklab(0.344934, 0.0582462, -0.160853, 0.3), 0.),
                linear_color_stop(oklab(0.615888, 0.118784, -0.190369, 0.3), 1.),
            )
        };
        let text = div()
            .flex()
            .flex_col()
            .justify_center()
            .px(px(if item.large { 24. } else { 12. }))
            .when(item.large, |this| this.flex_1().min_w(px(300.)).h(px(400.)))
            .when(!item.large, |this| this.mt(px(24.)).w_full().pl(px(20.)))
            .children(
                item.caption
                    .filter(|caption| *caption != "youtube")
                    .map(|caption| {
                        div()
                            .mb(px(8.))
                            .h(px(18.))
                            .px(px(6.))
                            .rounded(px(4.))
                            .bg(color("--background-brand"))
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--white"))
                            .child(caption)
                    }),
            )
            .children(
                (item.caption == Some("youtube")).then(|| self.nitro_art("youtube", 112., 28.)),
            )
            .child(
                Self::nitro_text(
                    item.title,
                    if item.large { 32. } else { 24. },
                    if item.large { 40. } else { 32. },
                    600.,
                    "--text-strong",
                )
                .mt(px(8.)),
            )
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(item.body)
                    .children(
                        item.link
                            .map(|link| div().text_color(color("--text-link")).child(link)),
                    ),
            )
            .children(item.button.map(|label| {
                div()
                    .mt(px(24.))
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .w_auto()
                    .self_start()
                    .bg(color("--control-secondary-background-default"))
                    .flex()
                    .items_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label)
            }));
        let art = div()
            .rounded(px(if item.large { 8. } else { 12. }))
            .when(item.large, |this| {
                this.flex_none().w(px(large_art_w)).h(px(400.))
            })
            .when(!item.large, |this| this.w_full().h(px(225.)))
            .overflow_hidden()
            .children(self.images.get(item.poster).map(|image| {
                img(image.clone())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
            }));
        let promo_partner = match item.id {
            "bento-youtube" => Some(super::third_party_promos::PartnerId::YouTube),
            "bento-cod" => Some(super::third_party_promos::PartnerId::CallOfDuty),
            _ => None,
        };
        div()
            .id(item.id)
            .relative()
            .flex_none()
            .when_some(promo_partner, |el, partner| {
                el.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_third_party_promo_modal(partner, cx);
                    }))
            })
            .rounded(px(16.))
            .p(px(12.))
            .bg(color("--background-surface-high"))
            .h(px(item.height))
            .when(item.large, |this| {
                this.w(px(bento_w))
                    .flex()
                    .when(item.art_left, |this| this.flex_row_reverse())
            })
            .when(!item.large, |this| {
                this.w(px(small_card_w)).flex().flex_col()
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(16.))
                    .opacity(0.3)
                    .bg(wash),
            )
            .child(if item.large {
                // Text and art side by side.
                div()
                    .relative()
                    .flex()
                    .flex_none()
                    .w_full()
                    .when(item.art_left, |this| this.flex_row_reverse())
                    .child(text)
                    .child(art)
                    .into_any_element()
            } else {
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(art)
                    .child(text)
                    .into_any_element()
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_option(
        &self,
        id: &'static str,
        label: &str,
        sub: Option<String>,
        price: String,
        selected: bool,
        cx: &mut Context<Self>,
        yearly: bool,
        nitro: bool,
        disabled: bool,
    ) -> AnyElement {
        div()
            .id(id)
            .when(!disabled, |this| this.cursor_pointer())
            .when(disabled, |this| this.cursor_not_allowed().opacity(0.3))
            .relative()
            .w_full()
            .rounded(px(16.))
            .px(px(16.))
            .py(px(12.))
            .min_h(px(76.))
            .when(selected, |this| {
                this.bg(rgba(0x97979f14))
                    .border_1()
                    .border_color(rgb(171, 93, 138))
            })
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_none()
                    .size(px(24.))
                    .rounded_full()
                    .border_2()
                    .border_color(color(if selected {
                        "--background-brand"
                    } else {
                        "--text-muted"
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.child(
                            div()
                                .size(px(12.))
                                .rounded_full()
                                .bg(color("--background-brand")),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(Self::nitro_text(
                        label.to_owned(),
                        16.,
                        20.,
                        600.,
                        "--text-strong",
                    ))
                    .children(sub.map(|sub| Self::nitro_text(sub, 14., 18., 500., "--text-muted"))),
            )
            .child(div().flex_none().child(Self::nitro_text(
                price,
                20.,
                24.,
                700.,
                "--text-strong",
            )))
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                if nitro {
                    this.nitro.nitro_yearly = Some(yearly);
                } else {
                    this.nitro.basic_yearly = Some(yearly);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn plan_card(&self, nitro: bool, cx: &mut Context<Self>) -> AnyElement {
        let sku = if nitro { sku::TIER_2 } else { sku::TIER_0 };
        let yearly_selected = if nitro {
            self.nitro.nitro_yearly
        } else {
            self.nitro.basic_yearly
        };
        // Module 572079: the card of the plan the account is already subscribed to is locked (both periods).
        let owned = self
            .premium_subscription()
            .and_then(|s| s.premium_item().map(|item| premium::plan_sku(item.plan_id)))
            .flatten()
            == Some(sku);
        let yearly = self.plan_price(sku, true);
        let monthly = self.plan_price(sku, false);
        let features: &[(&str, &str)] = if nitro {
            &NITRO_FEATURES
        } else {
            &BASIC_FEATURES
        };
        let terms_line = self.plan_card_terms(sku).map(|terms| {
            // Module 222719 (`.trialHeader_Q2LuyA`): margin-bottom 18px, max-width 60%.
            div()
                .mt(px(8.))
                .max_w(px(230.))
                .text_size(px(13.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(format_nitro_terms(&terms))
        });
        let list = div()
            .mt(px(if terms_line.is_some() { 18. } else { 37. }))
            .flex()
            .flex_col()
            .gap(px(12.))
            .children(nitro.then(|| {
                Self::nitro_text(
                    "Everything in Basic, plus",
                    14.,
                    18.,
                    600.,
                    "--text-default",
                )
                .mb(px(4.))
            }))
            .children(features.iter().map(|(glyph, line)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(20.))
                    .child(icon(glyph, 18., "--text-default"))
                    .child(Self::nitro_text(*line, 16., 20., 500., "--text-default"))
            }));
        let options = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(yearly.clone().map(|price| {
                self.plan_option(
                    if nitro {
                        "nitro-nitro-yearly"
                    } else {
                        "nitro-basic-yearly"
                    },
                    "Yearly",
                    (yearly_selected == Some(true)).then(|| {
                        format!(
                            "{}/month billed annually",
                            price.per_month_of_year().display()
                        )
                    }),
                    price.display(),
                    yearly_selected == Some(true),
                    cx,
                    true,
                    nitro,
                    owned,
                )
            }))
            .children(monthly.map(|price| {
                self.plan_option(
                    if nitro {
                        "nitro-nitro-monthly"
                    } else {
                        "nitro-basic-monthly"
                    },
                    "Monthly",
                    None,
                    price.display(),
                    yearly_selected == Some(false),
                    cx,
                    false,
                    nitro,
                    owned,
                )
            }));
        // Module 543213: top-rim offer pill + Wumpus mascot accessible alt labels (`02VBaY` / `XP8vWR`).
        let rim_pill_text: Option<&'static str> =
            super::premium_upsell::resolve_upsell_offer(self.fetched.get("user-offer"), sku).1;
        let mascot_alt = if nitro {
            dc_intl::t("02VBaY")
        } else {
            dc_intl::t("XP8vWR")
        };
        div()
            .relative()
            .w(px(400.))
            .h(px(620.))
            .rounded(px(24.))
            .bg(color("--background-surface-higher"))
            .when_some(rim_pill_text, |card, pill| {
                card.child(
                    div()
                        .absolute()
                        .top(px(-14.))
                        .left(px(110.))
                        .right(px(110.))
                        .h(px(26.))
                        .px(px(10.))
                        .rounded(px(999.))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(11.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .truncate()
                        .child(pill),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(24.))
                    .when(nitro, |this| {
                        this.bg(linear_gradient(
                            270.,
                            linear_color_stop(oklab(0.395045, -0.0154365, -0.253271, 0.4), 0.),
                            linear_color_stop(oklab(0.449387, 0.167872, -0.086863, 0.4), 1.),
                        ))
                    }),
            )
            .when(nitro, |this| {
                this.child(
                    div()
                        .id("nitro-tier2-mascot")
                        .aria_label(mascot_alt)
                        .absolute()
                        .left(px(170.))
                        .top(px(-55.))
                        .child(self.nitro_art("rocket", 380., 270.)),
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px(24.))
                    .top(px(24.))
                    .w(px(350.))
                    .child(Self::nitro_heading(
                        if nitro { "Nitro" } else { "Nitro Basic" },
                        32.,
                        34.,
                        true,
                    ))
                    .children(terms_line)
                    .child(list),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .w_full()
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(options)
                    .child(self.subscribe_button(
                        if nitro {
                            "nitro-card-subscribe"
                        } else {
                            "basic-card-subscribe"
                        },
                        true,
                        yearly_selected.map(|yearly| (sku, yearly)),
                        yearly_selected.is_some(),
                        self.subscribe_cta(Some(sku)),
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Module 579245: Nitro Friends / Affinity Social Proof Banner (`GET /users/@me/billing/nitro-affinity`):
    /// up to 3 overlapping avatars (`ml(-8px)`, surface border cutout) + `c7ETJH` / `st8Rh/` / `dpjXPL`.
    pub(super) fn nitro_affinity_banner(&self, brand_refresh: bool) -> Option<AnyElement> {
        let raw = self.fetched.get("nitro:affinity")?;
        let list = raw
            .get("body")
            .and_then(Value::as_array)
            .or_else(|| raw.as_array())?;
        if list.is_empty() {
            return None;
        }
        let mut users: Vec<(String, Option<String>)> = list
            .iter()
            .take(3)
            .map(|u| {
                let name = u["global_name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| u["username"].as_str())
                    .unwrap_or("Friend")
                    .to_owned();
                let id = u["id"].as_str().map(str::to_owned);
                (name, id)
            })
            .collect();
        users.reverse();
        let copy = match users.len() {
            3 => dc_intl::format(
                "c7ETJH",
                &[(
                    "username",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&users[2].0)),
                )],
            ),
            2 => dc_intl::format(
                "st8Rh/",
                &[
                    (
                        "username",
                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&users[1].0)),
                    ),
                    (
                        "otherUsername",
                        dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&users[0].0)),
                    ),
                ],
            ),
            _ => dc_intl::format(
                "dpjXPL",
                &[(
                    "username",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(&users[0].0)),
                )],
            ),
        };
        let avatar_px = if brand_refresh { 24. } else { 32. };
        let avatars = div()
            .flex()
            .items_center()
            .children(users.iter().enumerate().map(|(idx, (name, uid))| {
                let pic = uid
                    .as_deref()
                    .and_then(|s| s.parse::<dc_state::Id>().ok())
                    .and_then(|id| self.stores.users.get(&id))
                    .and_then(|u| {
                        let (k, _) = super::avatar_source(u);
                        self.images.get(&k).cloned()
                    });
                let initial = name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                div()
                    .when(idx > 0, |d| d.ml(px(-8.)))
                    .size(px(avatar_px))
                    .rounded_full()
                    .border_2()
                    .border_color(color("--background-surface-higher"))
                    .bg(color("--background-brand"))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match pic {
                        Some(image) => img(image)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .into_any_element(),
                        None => {
                            Self::nitro_text(&initial, 11., 14., 700., "--white").into_any_element()
                        }
                    })
            }));
        let banner = if brand_refresh {
            div()
                .id("nitro-affinity-banner")
                .mt(px(16.))
                .px(px(16.))
                .py(px(12.))
                .rounded(px(10.))
                .bg(color("--background-mod-normal"))
                .flex()
                .items_center()
                .gap(px(9.))
                .child(avatars)
                .child(Self::nitro_text(&copy, 14., 18., 500., "--text-strong"))
        } else {
            div()
                .id("nitro-affinity-banner")
                .mt(px(16.))
                .h(px(50.))
                .px(px(20.))
                .rounded(px(10.))
                .bg(gpui::rgba(0xffffff14))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(9.))
                .child(avatars)
                .child(Self::nitro_text(&copy, 16., 20., 500., "--text-strong"))
        };
        Some(banner.into_any_element())
    }

    fn nitro_plans(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("nitro-plans")
            .flex()
            .flex_col()
            .items_center()
            .w_full()
            .child(Self::nitro_heading("Pick Your Plan", 32., 34., true))
            .children(self.nitro_affinity_banner(true))
            .child(
                div()
                    .mt(px(112.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(self.plan_card(true, cx))
                    .child(self.plan_card(false, cx)),
            )
            .into_any_element()
    }

    fn compare_mark(value: &Cell3) -> AnyElement {
        match value {
            Yes => icon("nitrocmp_check", 18., "--text-default").into_any_element(),
            No => icon("nitrocmp_cross", 18., "--text-default").into_any_element(),
            Text(text) => {
                Self::nitro_text(*text, 16., 20., 600., "--text-strong").into_any_element()
            }
        }
    }

    fn nitro_compare(&self) -> AnyElement {
        let basic = self
            .plan_price(sku::TIER_0, false)
            .map(|price| price.display());
        let nitro = self
            .plan_price(sku::TIER_2, false)
            .map(|price| price.display());
        let column = |label: &'static str, price: Option<String>| {
            div()
                .w(px(180.))
                .h(px(63.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("nitro_wheel", 24., "--text-strong"))
                        .child(Self::nitro_text(label, 14., 18., 600., "--text-strong")),
                )
                .children(
                    price.map(|price| Self::nitro_text(price, 14., 18., 600., "--text-strong")),
                )
        };
        let mut groups = div().flex().flex_col();
        for (group, items) in COMPARE {
            let mut body = div()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(color("--border-strong"))
                .child(
                    div()
                        .h(px(66.))
                        .flex()
                        .items_start()
                        .pt(px(42.))
                        .child(Self::nitro_text(group, 20., 24., 600., "--text-strong")),
                );
            for (index, (name, basic_value, nitro_value)) in items.iter().enumerate() {
                body = body.child(
                    div()
                        .h(px(69.))
                        .flex()
                        .items_center()
                        .when(index > 0, |this| {
                            this.border_t_1().border_color(color("--border-strong"))
                        })
                        .child(div().flex_1().child(Self::nitro_text(
                            *name,
                            16.,
                            20.,
                            500.,
                            "--text-default",
                        )))
                        .child(
                            div()
                                .w(px(180.))
                                .flex()
                                .justify_center()
                                .child(Self::compare_mark(basic_value)),
                        )
                        .child(
                            div()
                                .w(px(180.))
                                .flex()
                                .justify_center()
                                .child(Self::compare_mark(nitro_value)),
                        ),
                );
            }
            groups = groups.child(body);
        }
        // The Nitro column: a 2px pink frame outside the 180px cells, 31px above the table, fading from purple.
        let fade = |from: Hsla, to: Hsla, top: f32, height: f32| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(top))
                .h(px(height))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(from, 0.),
                    linear_color_stop(to, 1.),
                ))
        };
        let frame = div()
            .absolute()
            .left(px(579.))
            .top(px(-31.))
            .w(px(184.))
            .h(px(1673.))
            .rounded(px(12.))
            .border_2()
            .border_color(rgb(235, 81, 214))
            .overflow_hidden()
            .child(fade(rgb(143, 31, 156), rgb(45, 39, 85), 0., 82.))
            .child(fade(rgb(45, 39, 85), rgb(18, 18, 20), 82., 110.));
        let pill = div()
            .absolute()
            .left(px(635.))
            .top(px(-44.))
            .w(px(71.))
            .h(px(29.))
            .rounded(px(15.))
            .bg(rgb(89, 31, 114))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .line_height(px(16.))
            .font_weight(FontWeight(700.))
            .text_color(color("--white"))
            .child("POPULAR");
        div()
            .id("nitro-compare")
            .relative()
            .w(px(761.))
            .flex()
            .flex_col()
            .items_center()
            .child(Self::nitro_heading("Compare Our Plans", 32., 34., true))
            .child(
                div()
                    .relative()
                    .mt(px(64.))
                    .w_full()
                    .child(frame)
                    .child(pill)
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(63.))
                                    .flex()
                                    .items_start()
                                    .child(div().flex_1().child(Self::nitro_text(
                                        "Pricing and Features",
                                        24.,
                                        32.,
                                        600.,
                                        "--text-strong",
                                    )))
                                    .child(column("Basic", basic))
                                    .child(column("Nitro", nitro)),
                            )
                            .child(groups),
                    ),
            )
            .into_any_element()
    }

    /// "What are you waiting for?": the page's last section, 100px below the table (the capture's `container_oEOllb`),
    /// with the trinkets around the heading. The call to action is the floating bar.
    fn nitro_footer(&self, _cx: &mut Context<Self>) -> AnyElement {
        let trinket = |name: &str, x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .child(self.nitro_art(name, w, h))
        };
        div()
            .relative()
            .mt(px(100.))
            .mb(px(64.))
            .h(px(750.))
            .w(px(880.))
            .child(trinket("jump", 299., 15., 277., 276.))
            .child(trinket("star", 93., 127., 83., 90.))
            .child(trinket("car", 682., 212., 70., 59.))
            .child(trinket("hammer", -27., 353., 100., 103.))
            .child(trinket("bolt", 39., 493., 62., 87.))
            .child(trinket("key", 708., 504., 78., 72.))
            .child(
                div()
                    .absolute()
                    .top(px(303.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        Self::nitro_heading("What are you waiting for?", 72., 72., true)
                            .w(px(800.))
                            .text_center(),
                    ),
            )
            .into_any_element()
    }

    /// `521440.l` (`NitroWishlistButton`): toggles `682901.pe.TIER_2` (`521847234246082599`) in the user's wishlist
    /// with tooltips `cjmnm6` ("Add Nitro to Wishlist") / `yr9TTf` ("Remove from Wishlist").
    pub(super) fn nitro_wishlist_button(
        &self,
        id: &'static str,
        overlay_surface: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const NITRO_TIER_2_SKU: &str = "521847234246082599";
        let wished = self.shop.wishlist.contains(NITRO_TIER_2_SKU);
        let tip = dc_intl::t(if wished { "yr9TTf" } else { "cjmnm6" });
        let btn = div()
            .id(id)
            .size(px(32.))
            .rounded(px(8.))
            .when(overlay_surface, |this| {
                this.bg(color("--control-overlay-secondary-background-default"))
                    .hover(|s| s.bg(color("--control-overlay-secondary-background-hover")))
            })
            .when(!overlay_surface, |this| {
                this.border_1()
                    .border_color(color("--control-secondary-border-default"))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(
                if wished { "heart" } else { "golive_heart" },
                16.,
                if wished {
                    "--text-feedback-critical"
                } else {
                    "--text-strong"
                },
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.shop_toggle_wishlist(NITRO_TIER_2_SKU, cx);
            }));
        self.with_tip(
            format!("{id}-tip"),
            super::tooltip::Tip::text(tip, super::tooltip::Side::Bottom),
            btn,
            cx,
        )
        .into_any_element()
    }

    /// Module 107834 (`NitroHeaderBar` on `HeaderBar` 742589): 48px solid navigation header with Nitro wheel +
    /// `"Nitro"` (`Ipxkog`) on the left, and `headerToolbar_8iSxT3` (wishlist button 521440 + Gift Nitro 65470) on
    /// the right. Rendered during loading/unauthenticated states in `204504` and `886268`.
    pub(super) fn nitro_header_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("nitro-header-bar")
            .flex_none()
            .w_full()
            .h(px(48.))
            .px(px(16.))
            .bg(color("--background-base-lower"))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("nitro_wheel", 24., "--icon-strong"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("Ipxkog")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(self.nitro_wishlist_button("nitro-header-wishlist", false, cx))
                    .child(
                        div()
                            .id("nitro-header-gift")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(icon("gift", 16., "--control-secondary-text-default"))
                            .child(dc_intl::t("Ve9Ge6"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(true, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The page, filling the content column.
    pub(super) fn nitro_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.request_nitro();
        if self.stores.current_user.is_none() {
            // Module 204504: NitroHeaderBar (107834) over loading cubes before currentUser is ready.
            return div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .child(self.nitro_header_bar(cx))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(super::login::wandering_cubes()),
                )
                .into_any_element();
        }
        // `L.YE` of module 886268: exactly Nitro gets the subscriber home (`sQ`), every other account the marketing one.
        if self.premium() == PremiumType::Nitro {
            return self.nitro_subscriber_home(window, cx);
        }
        let scroll = self.nitro.scroll.clone();
        let offset = -f32::from(scroll.offset().y);
        let active = SECTION_TOPS
            .iter()
            .rposition(|(_, top)| offset >= top - 50.)
            .unwrap_or(0);
        let show_nav_border = super::theme() != dc_tokens::Theme::Midnight;
        let seen = self.nitro.seen.clone();
        let hero_gradient = |top: Hsla, bottom: Hsla, height: f32| {
            div().w_full().h(px(height)).bg(linear_gradient(
                180.,
                linear_color_stop(top, 0.),
                linear_color_stop(bottom, 1.),
            ))
        };
        let nav_items: Vec<AnyElement> = SECTION_TOPS
            .iter()
            .enumerate()
            .map(|(index, (label, top))| {
                let top = *top;
                let scroll = scroll.clone();
                div()
                    .id(*label)
                    .relative()
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(*label)
                    .when(index == active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(px(1.5))
                                .h(px(2.))
                                .bg(color("--text-default")),
                        )
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        scroll.set_offset(gpui::point(px(0.), px(-top)));
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let nav = div()
            .absolute()
            .top_0()
            .left_0()
            .right(px(16.))
            .h(px(49.))
            .flex()
            .items_center()
            .px(px(16.))
            .backdrop_blur(px(11.5))
            .when(show_nav_border, |this| {
                this.border_b_1().border_color(color("--border-strong"))
            })
            .child(icon("nitro_wheel", 24., "--text-strong"))
            .child(div().ml(px(24.)).flex().gap(px(20.)).children(nav_items))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.nitro_wishlist_button("nitro-wishlist", true, cx))
                    .child(
                        div()
                            .id("nitro-nav-gift")
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(rgba(0x0000008f))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(icon("gift", 16., "--white"))
                            .child(dc_intl::t("Ve9Ge6"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_purchase(true, cx))),
                    ),
            );
        // The floating Subscribe / Gift bar shows once the hero's own buttons are out of view.
        let floating = offset > 300.;
        let vw = f32::from(window.viewport_size().width);
        let bento_w = (vw - self.combined_sidebar_width() - 16. - 80.).clamp(320., 1105.);
        let sections = div()
            .w_full()
            .px(px(40.))
            .flex()
            .flex_col()
            .items_center()
            .child(self.nitro_hero(cx))
            .child(self.bento_section("What's New", "nitro-whats-new", &WHATS_NEW, bento_w, cx))
            .child(div().h(px(164.)))
            .child(self.bento_section("Best of Nitro", "nitro-best-of", &BEST_OF, bento_w, cx))
            .child(div().h(px(164.)))
            .child(self.nitro_plans(cx))
            .child(div().h(px(164.)))
            .child(self.nitro_compare())
            .child(self.nitro_footer(cx));
        div()
            .relative()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .child(
                div()
                    .id("nitro-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .flex()
                                    .flex_col()
                                    .child(hero_gradient(
                                        rgb(204, 81, 215),
                                        rgb(170, 79, 202),
                                        120.,
                                    ))
                                    .child(hero_gradient(
                                        rgb(170, 79, 202),
                                        rgb(122, 69, 168),
                                        130.,
                                    ))
                                    .child(hero_gradient(rgb(122, 69, 168), rgb(41, 34, 65), 130.))
                                    .child(hero_gradient(rgb(41, 34, 65), rgb(18, 18, 20), 120.)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .flex()
                                    .flex_col()
                                    .child(hero_gradient(rgb(18, 18, 20), rgb(48, 39, 79), 123.))
                                    .child(hero_gradient(rgb(48, 39, 79), rgb(117, 70, 173), 120.))
                                    .child(hero_gradient(
                                        rgb(117, 70, 173),
                                        rgb(204, 83, 216),
                                        260.,
                                    )),
                            )
                            .child(div().w_full().pr(px(16.)).child(sections)),
                    ),
            )
            .child(nav)
            .child(super::settings::auto_scrollbar(scroll.clone()))
            .when(floating, |this| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(0.))
                        .left_0()
                        .right(px(16.))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .mb(px(64.))
                                .p(px(12.))
                                .rounded(px(12.))
                                .bg(rgba(0x2c2d32f2))
                                .flex()
                                .gap(px(8.))
                                .child(self.subscribe_button(
                                    "nitro-float-subscribe",
                                    false,
                                    None,
                                    true,
                                    self.subscribe_cta(self.hero_upgrade_sku()),
                                    cx,
                                ))
                                .child(self.gift_button("nitro-float-gift", cx)),
                        ),
                )
            })
            // A scroll redraws the page: the nav underline and the floating bar follow the offset.
            .child(canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    if (seen.get() - offset).abs() > 0.01 {
                        seen.set(offset);
                        window.refresh();
                    }
                },
            ))
            .into_any_element()
    }

    /// Whether a custom emoji is locked for the account in a message: another server's emoji need
    /// `EmojisEverywhere`, animated ones `AnimatedEmojis` (module 722178).
    pub(super) fn custom_emoji_locked(
        &self,
        id: impl PartialEq<u64> + Copy,
        animated: bool,
    ) -> bool {
        let tier = self.premium();
        let own = self
            .guild
            .and_then(|current| self.stores.guilds.iter().find(|guild| guild.id == current))
            .is_some_and(|guild| guild.emojis.iter().any(|emoji| id == emoji.id));
        (!own && !tier.has(premium::Feature::EmojisEverywhere))
            || (animated && !tier.has(premium::Feature::AnimatedEmojis))
    }

    /// Capture helper for Phase 3ah visual surfaces (`trial-upsell`, `affinity`, `guide-bar`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_phase3ah(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "trial-upsell" => {
                self.fetched.insert(
                    "user-offer".to_owned(),
                    serde_json::json!({
                        "user_trial_offer": {
                            "subscription_trial": {
                                "sku_id": sku::TIER_2.to_string(),
                                "interval": 1,
                                "interval_count": 1
                            }
                        }
                    }),
                );
                self.open_try_it_out_upsell(cx);
                "phase3ah: trial-upsell".to_owned()
            }
            "affinity" => {
                self.fetched.insert(
                    "nitro:affinity".to_owned(),
                    serde_json::json!([
                        { "id": "101", "username": "wren", "global_name": "Wren" },
                        { "id": "102", "username": "loreanxavier", "global_name": "lorean xavier" },
                        { "id": "103", "username": "plu", "global_name": "plü" }
                    ]),
                );
                self.open_nitro(cx);
                cx.notify();
                "phase3ah: affinity".to_owned()
            }
            "guide-bar" => {
                if let (Some(gid), Some(cid)) = (self.guild, self.channel) {
                    if let Some(g) = self.stores.guilds.iter_mut().find(|g| g.id == gid) {
                        for f in ["COMMUNITY", "GUILD_SERVER_GUIDE", "GUILD_ONBOARDING"] {
                            if !g.features.iter().any(|x| x == f) {
                                g.features.push(f.to_owned());
                            }
                        }
                        g.my_pending = false;
                        g.my_flags &= !64;
                    }
                    self.fetched.insert(
                        format!("guild-guide:home:{gid}"),
                        serde_json::json!({
                            "welcome_message": { "author_ids": [], "message": "Welcome!" },
                            "new_member_actions": [
                                {
                                    "channel_id": cid.to_string(),
                                    "action_type": 1,
                                    "title": "Introduce yourself in #general",
                                    "description": "Say hello to the server!"
                                }
                            ],
                            "resource_channels": []
                        }),
                    );
                }
                cx.notify();
                "phase3ah: guide-bar".to_owned()
            }
            other => format!("phase3ah: unknown {other}"),
        }
    }

    /// Capture harness step `premium N`: the account's tier, changed in this process only, with the monthly
    /// subscription of that tier a real subscriber's account has (the test account has none).
    #[cfg(feature = "capture")]
    pub(crate) fn script_premium(&mut self, tier: Option<u64>, cx: &mut Context<Self>) {
        self.stores.account.premium_type = tier;
        let plan = tier.and_then(|tier| match PremiumType::from_wire(Some(tier)) {
            PremiumType::Basic => Some(premium::plan_id::MONTH_TIER_0),
            PremiumType::Classic => Some(premium::plan_id::MONTH_TIER_1),
            PremiumType::Nitro => Some(premium::plan_id::MONTH_TIER_2),
            PremiumType::None => None,
        });
        self.script_subscription(plan, "ok", cx);
    }

    /// Capture harness step `subscription <plan id> <state>`: the body of `GET /users/@me/billing/subscriptions`
    /// as one Nitro subscription would return it (`none` clears it). The states are `ok`, `past_due`, `trial` and
    /// `pending` (a plan change to the monthly Nitro plan).
    #[cfg(feature = "capture")]
    pub(crate) fn script_subscription(
        &mut self,
        plan: Option<u64>,
        state: &str,
        cx: &mut Context<Self>,
    ) {
        let user = self
            .stores
            .current_user
            .as_ref()
            .map_or(0, |user| user.id)
            .to_string();
        let rows: Vec<Value> = plan
            .map(|plan| {
                let item = |plan: u64| serde_json::json!({"id": "1", "plan_id": plan.to_string(), "quantity": 1});
                serde_json::json!({
                    "id": "1", "type": 1, "user_id": user,
                    "status": if state == "past_due" { premium::status::PAST_DUE } else { premium::status::ACTIVE },
                    "items": [item(plan)],
                    "created_at": "2025-03-14T09:30:00.000000+00:00",
                    "streak_started_at": "2024-02-10T09:30:00.000000+00:00",
                    "current_period_start": "2026-11-02T09:30:00.000000+00:00",
                    "current_period_end": "2026-12-02T09:30:00.000000+00:00",
                    "currency": "usd",
                    "payment_source_id": null,
                    "trial_id": if state == "trial" { Value::from("1") } else { Value::Null },
                    "trial_ends_at": if state == "trial" {
                        Value::from("2026-12-02T09:30:00.000000+00:00")
                    } else { Value::Null },
                    "renewal_mutations": if state == "pending" {
                        serde_json::json!({"items": [item(premium::plan_id::MONTH_TIER_2)]})
                    } else { Value::Null },
                })
            })
            .into_iter()
            .collect();
        self.nitro.subscriptions = Some(Value::Array(rows));
        self.nitro.subscriptions_injected = true;
        cx.notify();
    }

    /// Capture harness step `payment-sources demo|none`: the body of `GET /users/@me/billing/payment-sources` as an
    /// account with a card (the default), PayPal, an iDEAL account and a Venmo account would return it; nothing is
    /// sent.
    #[cfg(feature = "capture")]
    pub(crate) fn script_payment_sources(&mut self, demo: bool, cx: &mut Context<Self>) {
        let rows = if demo {
            serde_json::json!([
                {"id": "1", "type": 1, "brand": "visa", "last_4": "4242", "expires_month": 3,
                 "expires_year": 2029, "default": true, "invalid": false, "country": "US"},
                {"id": "2", "type": 2, "email": "player@example.com", "default": false, "invalid": false},
                {"id": "3", "type": 16, "bank": "rabobank", "default": false, "invalid": true},
                {"id": "4", "type": 11, "username": "player", "default": false, "invalid": false},
            ])
        } else {
            serde_json::json!([])
        };
        self.nitro.sources = Some(rows);
        self.nitro.sources_injected = true;
        self.request_payment_source_art();
        cx.notify();
    }

    /// The account's Nitro subscription, as the subscription store holds it (`getPremiumTypeSubscription`): none
    /// until the account's own read has returned one.
    pub(super) fn premium_subscription(&self) -> Option<premium::Subscription> {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        premium::premium_type_subscription(self.nitro.subscriptions.as_ref()?, me)
    }

    /// The SKU the Nitro pages' hero asks for: Nitro, from a subscriber of any other plan (`C` of module 886268);
    /// nothing from a Nitro subscriber or an account without a subscription.
    pub(in crate::ui) fn hero_upgrade_sku(&self) -> Option<u64> {
        let subscription = self.premium_subscription()?;
        let own = premium::plan_sku(subscription.premium_item()?.plan_id)?;
        (own != sku::TIER_2).then_some(sku::TIER_2)
    }

    /// The text and state of a subscribe button naming `cta_sku` (module 572079).
    pub(super) fn subscribe_cta(&self, cta_sku: Option<u64>) -> premium::SubscribeCta {
        premium::subscribe_cta(
            cta_sku,
            self.premium_subscription().as_ref(),
            self.premium(),
        )
    }

    /// Whether the account has any Nitro (`premium_type`).
    pub(super) fn premium(&self) -> PremiumType {
        PremiumType::from_wire(self.premium_wire())
    }

    /// The account's `premium_type` as `UserStore` gives it: `premiumTypeOverride ?? premiumType`. Not in the pinned
    /// client: NoNitroUpsell's override (`plugin_hooks.rs`).
    pub(super) fn premium_wire(&self) -> Option<u64> {
        super::plugin_hooks::premium_type_override().or(self.stores.account.premium_type)
    }
}

/// Full Nitro perk card catalog and eligibility selector (`useBentoBoxes`, module 221168).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum BentoKind {
    ServerProfiles,
    ReferralProgram,
    CustomThemes,
    DisplayNameStyles,
    PremiumGroup,
    ShowYourStyle,
    YourSpace,
    Emojis,
    NoLimits,
    ThreePPromotions,
    OrbRewards,
    OrbMultiplier,
    CombinedOrbs,
    YouTube,
    XboxPartnerPass,
}

/// Eligibility flags read by module 221168 `ew` to order the What's New bento boxes.
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
pub(crate) struct BentoEligibility {
    pub bonus_orbs: bool,
    pub bonus_orb_multiplier: u32,
    pub youtube_eligible: bool,
    pub xbox_eligible: bool,
    pub three_p_eligible: bool,
    pub premium_group_eligible: bool,
    pub nitro_rewards_v2: bool,
}

/// Resolved perk card copy for any `BentoKind` (module 221168 `eL`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BentoCardCopy {
    pub title: String,
    pub description: String,
    pub cta: Option<String>,
    pub badge: Option<String>,
}

/// Builds the localized copy of a Nitro perk card (`eL` of module 221168).
#[allow(dead_code)]
pub(crate) fn bento_card_copy(kind: BentoKind, e: BentoEligibility) -> BentoCardCopy {
    let mult = e.bonus_orb_multiplier.max(2) as i64;
    match kind {
        BentoKind::ServerProfiles => BentoCardCopy {
            title: dc_intl::t("I9TYMg").to_owned(),
            description: dc_intl::t("HMSHeH").to_owned(),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::ReferralProgram => BentoCardCopy {
            title: dc_intl::t("tPY4o9").to_owned(),
            description: dc_intl::format(
                "jRPQUH",
                &[(
                    "learnMoreLink",
                    "https://support.discord.com/hc/en-us/articles/18570422416919".into(),
                )],
            ),
            cta: Some(dc_intl::t("hvVgAZ").to_owned()),
            badge: None,
        },
        BentoKind::CustomThemes => BentoCardCopy {
            title: dc_intl::t("Wme3nX").to_owned(),
            description: dc_intl::t("/aAIqV").to_owned(),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::DisplayNameStyles => BentoCardCopy {
            title: dc_intl::t("Ij3Zmv").to_owned(),
            description: dc_intl::t("UsOUxY").to_owned(),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::PremiumGroup => BentoCardCopy {
            title: "Nitro Squad".to_owned(),
            description: dc_intl::t("tPY4o9").to_owned(),
            cta: Some(dc_intl::t("hvVgAZ").to_owned()),
            badge: Some(dc_intl::t("oW0eUd").to_uppercase()),
        },
        BentoKind::ShowYourStyle => BentoCardCopy {
            title: dc_intl::t("Ij3Zmv").to_owned(),
            description: dc_intl::t("UsOUxY").to_owned(),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::YourSpace => BentoCardCopy {
            title: dc_intl::t("Wme3nX").to_owned(),
            description: dc_intl::t("/aAIqV").to_owned(),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::Emojis => BentoCardCopy {
            title: dc_intl::t("zY5PPb").to_owned(),
            description: dc_intl::t("R5Xag2").to_owned(),
            cta: None,
            badge: None,
        },
        BentoKind::NoLimits => BentoCardCopy {
            title: dc_intl::t("6b3ydG").to_owned(),
            description: dc_intl::t("Y+IJyg").to_owned(),
            cta: None,
            badge: None,
        },
        BentoKind::ThreePPromotions => BentoCardCopy {
            title: if e.nitro_rewards_v2 {
                dc_intl::t("E4U4SS").to_owned()
            } else {
                dc_intl::t("NG1e6l").to_owned()
            },
            description: if e.nitro_rewards_v2 {
                dc_intl::t("B4uSy/").to_owned()
            } else {
                dc_intl::format(
                    "LOYRxB",
                    &[(
                        "helpCenterLink",
                        "https://support.discord.com/hc/en-us/articles/4409494125719".into(),
                    )],
                )
            },
            cta: Some(if e.nitro_rewards_v2 {
                dc_intl::t("RzWDqY").to_owned()
            } else {
                dc_intl::t("7ioAjs").to_owned()
            }),
            badge: (!e.nitro_rewards_v2).then(|| dc_intl::t("y2b7CA").to_uppercase()),
        },
        BentoKind::OrbRewards => BentoCardCopy {
            title: dc_intl::t("Tzhw6e").to_owned(),
            description: dc_intl::format("djzJx6", &[("bonusOrbMultiplier", mult.into())]),
            cta: Some(dc_intl::t("hvVgAZ").to_owned()),
            badge: (!e.nitro_rewards_v2).then(|| dc_intl::t("oW0eUd").to_uppercase()),
        },
        BentoKind::OrbMultiplier => BentoCardCopy {
            title: dc_intl::t("Csf5Ol").to_owned(),
            description: dc_intl::format("G5k+lZ", &[("bonusOrbMultiplier", mult.into())]),
            cta: Some(dc_intl::t("jVcuVY").to_owned()),
            badge: None,
        },
        BentoKind::CombinedOrbs => BentoCardCopy {
            title: dc_intl::t("Tzhw6e").to_owned(),
            description: dc_intl::format("djzJx6", &[("bonusOrbMultiplier", mult.into())]),
            cta: Some(dc_intl::t("RzWDqY").to_owned()),
            badge: (!e.nitro_rewards_v2).then(|| dc_intl::t("oW0eUd").to_uppercase()),
        },
        BentoKind::YouTube => BentoCardCopy {
            title: "Level up your YouTube with Premium".to_owned(),
            description: dc_intl::t("B4uSy/").to_owned(),
            cta: Some(dc_intl::t("RzWDqY").to_owned()),
            badge: None,
        },
        BentoKind::XboxPartnerPass => BentoCardCopy {
            title: "Xbox Game Pass".to_owned(),
            description: dc_intl::t("B4uSy/").to_owned(),
            cta: Some(dc_intl::t("2pG5Ga").to_owned()),
            badge: None,
        },
    }
}

/// Orders eligible What's New bento cards exactly as module 221168 `ew` does.
#[allow(dead_code)]
pub(crate) fn select_whats_new_bentos(e: BentoEligibility) -> Vec<BentoKind> {
    let mut out = Vec::new();
    if e.youtube_eligible {
        out.push(BentoKind::YouTube);
    }
    if e.xbox_eligible && e.nitro_rewards_v2 {
        out.push(BentoKind::XboxPartnerPass);
    }
    if e.three_p_eligible {
        out.push(BentoKind::ThreePPromotions);
    }
    if e.bonus_orbs {
        out.push(BentoKind::CombinedOrbs);
    }
    if e.premium_group_eligible {
        out.push(BentoKind::PremiumGroup);
    }
    if !e.bonus_orbs {
        out.push(BentoKind::OrbRewards);
    }
    out.extend_from_slice(&[
        BentoKind::DisplayNameStyles,
        BentoKind::CustomThemes,
        BentoKind::ServerProfiles,
        BentoKind::ReferralProgram,
    ]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_matches_the_capture_tokens() {
        // `--background-base-lowest` is oklab(0.183087 ...) = rgb(18, 18, 20) in the capture's pixels.
        let c = oklab(0.183087, 0.00112148, -0.00387992, 1.);
        let rgba = c.to_rgb();
        assert!((rgba.r * 255. - 18.).abs() < 1.5, "{rgba:?}");
        assert!((rgba.b * 255. - 20.).abs() < 1.5, "{rgba:?}");
    }

    #[test]
    fn section_tops_are_ordered() {
        assert!(SECTION_TOPS.windows(2).all(|pair| pair[0].1 < pair[1].1));
    }

    #[test]
    fn compare_table_rows_add_up_to_the_capture_heights() {
        // 66 px group heading plus 69 px rows: 342, 411 and 825 as measured.
        let heights: Vec<usize> = COMPARE
            .iter()
            .map(|(_, rows)| 66 + 69 * rows.len())
            .collect();
        assert_eq!(heights, [342, 411, 825]);
    }

    #[test]
    fn all_221168_nitro_bento_catalog_strings_and_selector_work() {
        for key in [
            "I9TYMg", "HMSHeH", "jVcuVY", "tPY4o9", "jRPQUH", "hvVgAZ", "oW0eUd", "Ij3Zmv",
            "UsOUxY", "Wme3nX", "/aAIqV", "zY5PPb", "R5Xag2", "6b3ydG", "Y+IJyg", "E4U4SS",
            "B4uSy/", "RzWDqY", "NG1e6l", "7ioAjs", "LOYRxB", "y2b7CA", "Csf5Ol", "G5k+lZ",
            "Tzhw6e", "djzJx6", "2pG5Ga",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in nitro 221168"
            );
        }
        let e = BentoEligibility {
            bonus_orbs: true,
            bonus_orb_multiplier: 2,
            youtube_eligible: true,
            three_p_eligible: true,
            ..Default::default()
        };
        let picked = select_whats_new_bentos(e);
        assert_eq!(picked[0], BentoKind::YouTube);
        assert!(picked.contains(&BentoKind::CombinedOrbs));
        assert!(picked.contains(&BentoKind::ServerProfiles));
        let orbs = bento_card_copy(BentoKind::OrbMultiplier, e);
        assert!(orbs.description.contains("2x"));
        let sp = bento_card_copy(BentoKind::ServerProfiles, e);
        assert_eq!(sp.title, "Elevate your per-server profile");
    }

    #[test]
    fn nitro_terms_222719_keys_resolve_all_variants() {
        for key in ["2CGBri", "z2oQtA", "3ZiutU", "9vyovu", "sJTwHQ"] {
            assert!(dc_intl::has_key(key), "missing key {key}");
        }
        let _ = format_nitro_terms(&NitroTermsKind::ActiveTrial {
            remaining_days: 5,
            price: "$9.99/month".to_owned(),
        });
        let _ = format_nitro_terms(&NitroTermsKind::ActiveYearlyDiscount {
            percent: 20,
            regular_price: "$99.99/year".to_owned(),
            renewal_date: "Dec 2, 2026".to_owned(),
        });
        let _ = format_nitro_terms(&NitroTermsKind::ActiveMonthlyDiscount {
            percent: 30,
            regular_price: "$9.99/month".to_owned(),
            num_months: 3,
        });
        let _ = format_nitro_terms(&NitroTermsKind::EligibleTrial {
            plan_name: "Nitro".to_owned(),
            duration: "1 month".to_owned(),
            price: "$9.99/month".to_owned(),
        });
        let _ = format_nitro_terms(&NitroTermsKind::EligibleDiscount {
            num_months: 2,
            discounted_price: "$6.99".to_owned(),
            regular_price: "$9.99/month".to_owned(),
        });
    }
}
