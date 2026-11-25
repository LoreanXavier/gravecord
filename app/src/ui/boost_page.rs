//! The Server Boost page the server dropdown's "Server Boost" row and the channel list's boost bar open: a full
//! window page (module 355097's `GuildBoostingModal` family; layout classes `*_mQ3T6n`, `*_0jpYct`, `*_xZJtlG`,
//! `*_RL7BaR`, `*_twYJsJ`, `*_c_ash1`, `*_EVXXpI` of `196063.6435217f69e9b808.css`). Measured in
//! `design/dom/boost/dropdown-boost-modal-*-darker-cozy.json`.
//!
//! Art is requested at run time from the CDN addresses the DOM dump and the modules name (none of it is in the
//! snapshot's assets). Not drawn: the header's animated backdrop (a looping `.webm`; its dark-theme still is shown),
//! the Rive glow around the two header buttons (no `.riv` in the snapshot), the text shadow of the headline and the
//! progress bar's context menu.
//! The headline's letter spacing is applied per glyph. "Boost This Server" goes as far as the existing Server Boost
//! settings page and never past payment.

use super::discover::{gutter_scrollbar, text};
use super::{Root, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, RenderImage, ScrollHandle,
    SharedString, deferred, div, img, linear_color_stop, linear_gradient, prelude::*, px, rgb,
    rgba,
};
use std::sync::Arc;

/// Module 235780 ("How Boosting Works" marketing video modal): official CDN video & poster URLs.
pub(crate) const BOOST_MARKETING_VIDEO_URL: &str = "https://cdn.discordapp.com/assets/content/f0b9a61f96ee69dc341c22c04fe6458660d9317caae3d381c05cf8348fca5b95.mp4";
pub(crate) const BOOST_MARKETING_POSTER_URL: &str = "https://cdn.discordapp.com/assets/content/7b8d6c9c9f8bc8092089a2f55081695d40f7f303f053fcc6f944bf4b38403573.png";

/// Module 235780: WebVTT caption URL (`None` for English locales, official CDN path otherwise).
pub(crate) fn boost_marketing_captions_url(locale: &str) -> Option<String> {
    if locale.starts_with("en") {
        None
    } else {
        Some(format!(
            "https://cdn.discordapp.com/assets/boosting/marketing/captions/boosting_marketing_video_captions_{locale}.vtt"
        ))
    }
}

/// One parsed WebVTT cue (`start_ms..=end_ms`, caption text) for module 235780 captions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WebVttCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[allow(dead_code)]
fn parse_vtt_ts(raw: &str) -> Option<u64> {
    let clean = raw.split_whitespace().next()?;
    let (hms, ms_str) = clean.split_once('.')?;
    let ms: u64 = ms_str.get(..3).unwrap_or(ms_str).parse().ok()?;
    let parts: Vec<u64> = hms.split(':').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [m, s] => Some((m * 60 + s) * 1000 + ms),
        [h, m, s] => Some((h * 3600 + m * 60 + s) * 1000 + ms),
        _ => None,
    }
}

/// Parses a WebVTT caption track into cues for module 235780.
#[allow(dead_code)]
pub(crate) fn parse_webvtt_cues(vtt: &str) -> Vec<WebVttCue> {
    let mut cues = Vec::new();
    let mut lines = vtt.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some((start_s, end_s)) = line.split_once("-->")
            && let (Some(start_ms), Some(end_ms)) = (parse_vtt_ts(start_s), parse_vtt_ts(end_s))
        {
            let mut text_lines = Vec::new();
            while let Some(&next) = lines.peek() {
                if next.trim().is_empty() {
                    lines.next();
                    break;
                }
                text_lines.push(lines.next().unwrap_or_default().trim());
            }
            if !text_lines.is_empty() {
                cues.push(WebVttCue {
                    start_ms,
                    end_ms,
                    text: text_lines.join("\n"),
                });
            }
        }
    }
    cues
}

/// Returns the active caption text at `position_ms`, if any.
pub(crate) fn active_webvtt_cue(cues: &[WebVttCue], position_ms: u64) -> Option<&str> {
    cues.iter()
        .find(|c| position_ms >= c.start_ms && position_ms <= c.end_ms)
        .map(|c| c.text.as_str())
}

/// Module 235780: "How Boosting Works" XXL modal state (video playback + WebVTT captions toggle).
#[derive(Clone, Debug)]
pub(crate) struct HowBoostingWorksModal {
    pub guild_id: Option<Id>,
    pub playing: bool,
    pub captions_enabled: bool,
    pub position_ms: u64,
    pub cues: Vec<WebVttCue>,
    /// Module 338548: true when user is a non-owner Nitro Squad member who cannot buy Boosts.
    pub boost_purchasing_unavailable: bool,
}

/// Module 338548 (`5xN/C1` + `aFBQ3d`): tooltip text for Boost Purchasing Currently Unavailable pill.
pub(crate) fn boost_unavailable_tooltip_text() -> String {
    dc_intl::format(
        "5xN/C1",
        &[("premiumGroupProductName", dc_intl::t("aFBQ3d").into())],
    )
}

/// Module 338548 (`.guildBoostUnavailableNotice_Jru8ph`, CSS `285644`): pill-shaped notice banner
/// shown when a Nitro Squad member cannot purchase Server Boosts.
pub(crate) fn render_boost_unavailable_pill(
    root: &Root,
    always_white: bool,
    is_light_theme: bool,
    cx: &mut Context<Root>,
) -> AnyElement {
    let tip = boost_unavailable_tooltip_text();
    let border_col = if !always_white && is_light_theme {
        rgba(0x000000ff)
    } else {
        color("--white")
    };
    let pill = div()
        .id("boost-purchasing-unavailable-pill")
        .px(px(16.))
        .py(px(8.))
        .rounded(px(9999.))
        .bg(rgba(0xffffff1a))
        .border_2()
        .border_color(border_col)
        .text_size(px(14.))
        .font_weight(FontWeight(600.))
        .text_color(if always_white {
            color("--white")
        } else {
            color("--text-strong")
        })
        .child(dc_intl::t("5nrJDO").to_uppercase());
    root.with_tip(
        "boost-unavailable-338548",
        super::tooltip::Tip::body(
            super::tooltip::Body::Text(tip.into()),
            super::tooltip::Side::Top,
        ),
        pill,
        cx,
    )
    .into_any_element()
}

/// Guild powerup SKU IDs (`512750` / `945942.js`).
pub(super) const SKU_SERVER_TAG: &str = "1351706802684952639";
pub(super) const SKU_ROLE_STYLES: &str = "1354906318279807056";
pub(super) const SKU_VANITY_URL: &str = "1387197800336330924";
pub(super) const SKU_GUILD_THEME: &str = "1493634428604252161";
/// Module 512750 (`YG` / `OJ` / `jF` / `Ht` / `tv`): Boost Level 3 & Server Tag Badge Pack SKUs for module 179413 warnings.
pub(crate) const SKU_BOOST_LEVEL_3: &str = "1341586379779604623";
pub(crate) const SKU_TAG_BADGE_PACKS: [&str; 4] = [
    "1395150519886024775",
    "1395150923734581339",
    "1466209416922667288",
    "1466209416931055898",
];

/// Module 179413: computes SKU-specific `(text, is_critical)` deactivation warning segments.
pub(crate) fn powerup_deactivate_warnings(
    sku_id: &str,
    perk_name: &str,
    member_count: u64,
    gradient_role_members: u64,
    has_vanity_url: bool,
) -> Vec<(String, bool)> {
    let mut warnings = Vec::new();
    if sku_id == SKU_ROLE_STYLES {
        if gradient_role_members > 0 {
            warnings.push((
                dc_intl::format(
                    "4jSvr1",
                    &[
                        ("perk", perk_name.into()),
                        (
                            "memberCount",
                            dc_intl::Arg::Num(gradient_role_members as f64),
                        ),
                    ],
                ),
                false,
            ));
        } else {
            warnings.push((
                dc_intl::format("cavtEo", &[("perk", perk_name.into())]),
                false,
            ));
        }
    } else if sku_id == SKU_VANITY_URL {
        let key = if has_vanity_url { "hN75yb" } else { "Du91Rb" };
        warnings.push((dc_intl::t(key).to_owned(), true));
    } else if SKU_TAG_BADGE_PACKS.contains(&sku_id) {
        warnings.push((dc_intl::t("Vf2ZcR").to_owned(), false));
    } else {
        warnings.push((
            dc_intl::format(
                "4jSvr1",
                &[
                    ("perk", perk_name.into()),
                    ("memberCount", dc_intl::Arg::Num(member_count as f64)),
                ],
            ),
            false,
        ));
    }
    if has_vanity_url && sku_id == SKU_BOOST_LEVEL_3 {
        warnings.push((dc_intl::t("M4XL5n").to_owned(), true));
    }
    warnings
}

const VANITY_HELP_URL: &str = "https://support.discord.com/hc/en-us/articles/115001542132";

/// Open state of `GuildPowerupPerkModal` (module 945942, Layouts A & B + `933337` deactivate confirm).
#[derive(Clone, Debug)]
pub(super) struct PowerupPerkModal {
    pub guild_id: Id,
    pub sku_id: &'static str,
    pub title: String,
    pub description: String,
    pub cost: u32,
    pub is_active: bool,
    pub level_activated: bool,
    pub cooldown_days: u32,
    pub can_manage: bool,
    pub error: Option<String>,
    pub confirm_disable: bool,
    pub selected_tag_badge: usize,
    pub selected_gradient: usize,
    pub vanity_code: String,
}

impl PowerupPerkModal {
    pub(super) fn for_sku(guild_id: Id, sku_id: &'static str, can_manage: bool) -> Self {
        match sku_id {
            SKU_GUILD_THEME => Self {
                guild_id,
                sku_id,
                title: "Server Theme".to_owned(),
                description: "Give your server a custom gradient theme visible to all members."
                    .to_owned(),
                cost: 3,
                is_active: false,
                level_activated: false,
                cooldown_days: 0,
                can_manage,
                error: None,
                confirm_disable: false,
                selected_tag_badge: 0,
                selected_gradient: 0,
                vanity_code: String::new(),
            },
            SKU_SERVER_TAG => Self {
                guild_id,
                sku_id,
                title: "Server Tag".to_owned(),
                description: "Let your members represent your server everywhere on Discord."
                    .to_owned(),
                cost: 3,
                is_active: false,
                level_activated: false,
                cooldown_days: 7,
                can_manage,
                error: None,
                confirm_disable: false,
                selected_tag_badge: 0,
                selected_gradient: 0,
                vanity_code: String::new(),
            },
            SKU_VANITY_URL => Self {
                guild_id,
                sku_id,
                title: "Custom Invite Link".to_owned(),
                description: "Create a memorable custom invite link for your server.".to_owned(),
                cost: 14,
                is_active: false,
                level_activated: false,
                cooldown_days: 0,
                can_manage,
                error: None,
                confirm_disable: false,
                selected_tag_badge: 0,
                selected_gradient: 0,
                vanity_code: "my-server".to_owned(),
            },
            _ => Self {
                guild_id,
                sku_id: SKU_ROLE_STYLES,
                title: "Enhanced Role Styles".to_owned(),
                description: "Make certain roles stand out with animated, vibrant gradients."
                    .to_owned(),
                cost: 3,
                is_active: false,
                level_activated: false,
                cooldown_days: 0,
                can_manage,
                error: None,
                confirm_disable: false,
                selected_tag_badge: 0,
                selected_gradient: 0,
                vanity_code: String::new(),
            },
        }
    }
}

/// Module 784140 (`GuildPowerupLevelModal`): Boost Level 1/2/3 detail modal state.
#[derive(Clone, Debug)]
pub(super) struct PowerupLevelModal {
    pub guild_id: Id,
    pub sku_id: String,
    pub tier: u8,
    pub is_active: bool,
    pub is_tier_override: bool,
    pub can_manage: bool,
    pub disabled_reason: Option<String>,
    pub error: Option<String>,
    pub confirm_disable: bool,
}

/// Module 784140: required boost subscriptions per Boost Level (`M2T`: 2 / 7 / 14).
pub(super) fn powerup_level_boost_count(tier: u8) -> u32 {
    match tier {
        1 => 2,
        2 => 7,
        _ => 14,
    }
}

/// Module 784140: `pob/cL` (`"{subscriptions} Boosts"`) subtitle for a level modal.
pub(super) fn powerup_level_subtitle(tier: u8) -> String {
    dc_intl::format(
        "pob/cL",
        &[(
            "subscriptions",
            dc_intl::Arg::Num(f64::from(powerup_level_boost_count(tier))),
        )],
    )
}

/// Module 784140: `5HQUzD` (`"Get {boostCount} Boost(s) back."`) disable button tooltip.
pub(super) fn powerup_level_disable_tooltip(boost_count: u32) -> String {
    dc_intl::format(
        "5HQUzD",
        &[("boostCount", dc_intl::Arg::Num(f64::from(boost_count)))],
    )
}

impl PowerupLevelModal {
    pub(super) fn for_tier(guild_id: Id, tier: u8, is_active: bool, can_manage: bool) -> Self {
        let clamped = tier.clamp(1, 3);
        Self {
            guild_id,
            sku_id: format!("guild-level-{clamped}"),
            tier: clamped,
            is_active,
            is_tier_override: false,
            can_manage,
            disabled_reason: None,
            error: None,
            confirm_disable: false,
        }
    }

    pub(super) fn can_show_action(&self) -> bool {
        !self.is_tier_override && self.can_manage
    }
}

/// Status of an individual card inside `PowerupMultiPerkModal` (module 789267).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MultiPerkStatus {
    Inactive,
    Active,
    Expiring,
    #[allow(dead_code)]
    Removing,
}

/// Single perk card entry inside `GuildPowerupsMultiPerkModal` (module 789267).
#[derive(Clone, Debug)]
pub(super) struct MultiPerkItem {
    pub sku_id: String,
    pub title: String,
    pub description: String,
    pub cost: u32,
    pub is_new: bool,
    pub status: MultiPerkStatus,
    pub accent: u32,
}

/// Module 789267 (`GuildPowerupsMultiPerkModal`): wrapping 220px perk-group cards modal.
#[derive(Clone, Debug)]
pub(super) struct PowerupMultiPerkModal {
    pub guild_id: Id,
    pub title: String,
    pub subtitle: String,
    pub can_manage: bool,
    pub perks: Vec<MultiPerkItem>,
    pub footer_info: Option<String>,
    pub footer_action_label: Option<String>,
}

/// Module 789267: `t2Wbo1` (`"{required} Boosts"`) boost-cost pill label.
pub(super) fn multi_perk_cost_label(required: u32) -> String {
    dc_intl::format(
        "t2Wbo1",
        &[
            ("required", dc_intl::Arg::Num(f64::from(required))),
            ("decorator", "".into()),
        ],
    )
}

impl PowerupMultiPerkModal {
    pub(super) fn badge_packs(guild_id: Id, can_manage: bool) -> Self {
        Self {
            guild_id,
            title: "Server Tag Badge Packs".to_owned(),
            subtitle: "Unlock themed icon packs for your server's members to wear everywhere."
                .to_owned(),
            can_manage,
            perks: vec![
                MultiPerkItem {
                    sku_id: "1351706802684952641".to_owned(),
                    title: "Fantasy Badge Pack".to_owned(),
                    description: "Swords, shields, crowns, and arcane sigils for guild tags."
                        .to_owned(),
                    cost: 3,
                    is_new: false,
                    status: MultiPerkStatus::Active,
                    accent: 0x5865f2,
                },
                MultiPerkItem {
                    sku_id: "1351706802684952642".to_owned(),
                    title: "Cyber Badge Pack".to_owned(),
                    description: "Neon glyphs and retro-futuristic emblems for your server tag."
                        .to_owned(),
                    cost: 3,
                    is_new: true,
                    status: MultiPerkStatus::Inactive,
                    accent: 0xb845c1,
                },
                MultiPerkItem {
                    sku_id: "1351706802684952643".to_owned(),
                    title: "Cozy Badge Pack".to_owned(),
                    description: "Teacups, plants, stars, and warm icons for community tags."
                        .to_owned(),
                    cost: 3,
                    is_new: false,
                    status: MultiPerkStatus::Expiring,
                    accent: 0x2d7d46,
                },
            ],
            footer_info: Some(
                "Disabling a pack removes its badges from your Server Tag picker.".to_owned(),
            ),
            footer_action_label: Some(dc_intl::t("g5Ds69").to_owned()),
        }
    }
}

/// Module 722523 (`GuildPowerupCardActionFooter`): action CTA shown at the bottom of a guild
/// powerup / boost perk card for admin vs non-admin viewers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PowerupCardAction {
    /// Active configurable perk for admin (`g5Ds69` "Configure" -> SKU-specific Server Settings page).
    Configure,
    /// Level card or active non-configurable perk (`0Q61kF` "View Perks").
    ViewPerks,
    /// Non-admin non-level perk card (`Xa11Ep` "View Perk").
    ViewPerk,
    /// Inactive toggleable perk for admin (`439156` boost activate toggle).
    ToggleBoost,
    None,
}

/// Module 722523 (`Ay` / `f` / `v`): exact admin vs non-admin CTA selection for a powerup card.
pub(crate) fn powerup_card_action(
    is_admin: bool,
    is_active: bool,
    is_configurable: bool,
    show_toggle: bool,
    is_level: bool,
) -> PowerupCardAction {
    if is_admin {
        if is_active && is_configurable {
            PowerupCardAction::Configure
        } else if is_active {
            PowerupCardAction::ViewPerks
        } else if show_toggle {
            PowerupCardAction::ToggleBoost
        } else {
            PowerupCardAction::None
        }
    } else if is_level {
        PowerupCardAction::ViewPerks
    } else {
        PowerupCardAction::ViewPerk
    }
}

/// Module 722523 / 689906: maps a configurable powerup SKU to its Server Settings destination page.
pub(crate) fn powerup_settings_page_for_sku(sku_id: &str) -> super::server_settings::Page {
    match sku_id {
        SKU_SERVER_TAG => super::server_settings::Page::Tag,
        SKU_ROLE_STYLES => super::server_settings::Page::Roles,
        _ => super::server_settings::Page::BoostPerks,
    }
}

/// Builds `POST` or `DELETE /guilds/{guildId}/skus/{skuId}` (`GUILD_POWERUP_TOGGLE`, module 945942 / 533993 / 722523).
pub(super) fn toggle_guild_powerup_request(guild_id: Id, sku_id: &str, enable: bool) -> Request {
    Request::Rest {
        key: Some(format!("guild-powerup-toggle:{guild_id}:{sku_id}")),
        method: if enable {
            reqwest::Method::POST
        } else {
            reqwest::Method::DELETE
        },
        route: format!("/guilds/{guild_id}/skus/{sku_id}"),
        body: enable.then(|| serde_json::json!({})),
    }
}

/// The page's open state: where it is scrolled and which FAQ row is open.
pub(super) struct BoostPage {
    pub scroll: ScrollHandle,
    /// The open FAQ row; one at a time (module 232122: `onClick: setOpen(open === index ? null : index)`).
    pub faq_open: Option<usize>,
    /// `.headerGradient_0jpYct`.
    header_gradient: Arc<RenderImage>,
    /// `.card_RL7BaR`'s radial glow, one image for both perk cards.
    card_glow: Arc<RenderImage>,
    /// `.overlay_hKMjy9` over the video card's picture.
    video_overlay: Arc<RenderImage>,
}

const CDN: &str = "https://cdn.discordapp.com/assets/content/";
/// Art the page shows, from the `img`/`source` elements of `design/dom/boost/*` and module 233824: the dark theme's
/// still of the header video (`fallbackImage` of `ea`; the `.webm` itself is not played), the video card's
/// thumbnail (module 519636), the three tier gems (`el.v`, `eo.w`, `ec.H`) and the two perk illustrations (the
/// powerup catalog's `staticImageUrl`s).
const ART: [(&str, &str); 7] = [
    (
        "boost:backdrop",
        "f91111a24ca4c59e87a462e8a3523938628e03e3723c31e5681991a07b0acf48.png",
    ),
    (
        "boost:thumbnail",
        "7b8d6c9c9f8bc8092089a2f55081695d40f7f303f053fcc6f944bf4b38403573.png",
    ),
    (
        "boost:tier1",
        "33e98755a49cf85d07b8189a0001926c17d91599d53662a39999329f5253254f.svg",
    ),
    (
        "boost:tier2",
        "3bc18204c2bf43975ded85a602c7816d032bc07cf9b1d5589e52404f9bc1d687.svg",
    ),
    (
        "boost:tier3",
        "cb869ee40a8ed5d6210e69a02cebbf03025be47006b310ec2564f74f6ad1dff8.svg",
    ),
    (
        "boost:roles",
        "93c84331a5f11ba05946bce6fb4f85594d98cb52c8452e9b6869291da41cf62c.png",
    ),
    (
        "boost:tag",
        "478ce7fb9a64e4d6c4792c82d93f724c2aa0444ac2dfc3e2dfdf43dd11a39c79.png",
    ),
];
/// The second "be recognized" card's picture (`icon_c_ash1`, an `img` of the capture).
const MEMBER_ICON: (&str, &str) = (
    "boost:badge-picture",
    "https://discord.com/assets/185ece790f20d870.svg",
);

/// The headline's spacing per glyph: the CSS `letter-spacing` of -1.26px plus the 0.94px per glyph that setting each
/// glyph in its own box loses in kerning, so both lines come out at the captured 768.7 and 558 pixels (measured
/// from frames of both lines: 823.5 and 595.4 wide without tracking).
const HEADING_TRACKING: f32 = -2.2;

/// The open FAQ row after pressing `index`: that row, or none when it was the open one.
fn toggled_faq(open: Option<usize>, index: usize) -> Option<usize> {
    (open != Some(index)).then_some(index)
}

/// `text` with `spacing` pixels added after every glyph (CSS `letter-spacing`).
///
/// Each glyph is shifted by its own offset (a margin does not shrink the line's measured width), and the line by
/// half the total so it stays centred in its box.
fn tracked(text: &'static str, spacing: f32) -> gpui::Div {
    let glyphs = text.chars().count();
    div()
        .relative()
        .left(px(-spacing * (glyphs - 1) as f32 / 2.))
        .w_full()
        .flex()
        .justify_center()
        .children(text.chars().enumerate().map(|(index, glyph)| {
            div()
                .relative()
                .left(px(spacing * index as f32))
                .child(if glyph == ' ' {
                    "\u{a0}".to_owned()
                } else {
                    glyph.to_string()
                })
        }))
}

/// A colour stop of a CSS gradient: the position along the ray (0 to 1) and straight RGBA (0 to 255, alpha 0 to 1).
type Stop = (f32, [f32; 4]);

/// A CSS `radial-gradient(ellipse rx ry at cx cy, stops)` as pixels: stops are interpolated premultiplied (the
/// default for CSS gradients) and the last colour extends past the end. The atlas takes BGRA straight alpha.
fn radial_image(
    (width, height): (u32, u32),
    (cx, cy): (f32, f32),
    (rx, ry): (f32, f32),
    stops: &[Stop],
) -> Arc<RenderImage> {
    let mut pixels = image::RgbaImage::new(width, height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let dx = (x as f32 + 0.5 - cx) / rx;
        let dy = (y as f32 + 0.5 - cy) / ry;
        let [r, g, b, a] = stop_color(stops, (dx * dx + dy * dy).sqrt());
        let channel = |value: f32| (value.clamp(0., 255.) + 0.5) as u8;
        // Back from premultiplied.
        let (r, g, b) = if a > 0. {
            (r / a, g / a, b / a)
        } else {
            (0., 0., 0.)
        };
        *pixel = image::Rgba([channel(b), channel(g), channel(r), channel(a * 255.)]);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
}

/// The premultiplied colour at `t` along the stops.
fn stop_color(stops: &[Stop], t: f32) -> [f32; 4] {
    let premultiplied = |[r, g, b, a]: [f32; 4]| [r * a, g * a, b * a, a];
    let Some(first) = stops.first() else {
        return [0.; 4];
    };
    if t <= first.0 {
        return premultiplied(first.1);
    }
    for pair in stops.windows(2) {
        let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
        if t <= to_at {
            let mix = if to_at > from_at {
                (t - from_at) / (to_at - from_at)
            } else {
                1.
            };
            let (from, to) = (premultiplied(from), premultiplied(to));
            return std::array::from_fn(|index| from[index] + (to[index] - from[index]) * mix);
        }
    }
    premultiplied(stops[stops.len() - 1].1)
}

/// `.content_0jpYct` and the cards' shared width.
const COLUMN: f32 = 1200.;
/// The scroller's gutter (`scrollbarGutterStable`): the content is 1264 wide in a 1280 window.
const GUTTER: f32 = 16.;
/// Where the hero's buttons end; the persistent bar appears once they are scrolled away.
const HERO_CTA_BOTTOM: f32 = 392.;

/// The tiers: level, boosts, the four perks the card lists (existing perk icons stand in for the art).
type Tier = (
    &'static str,
    &'static str,
    [(&'static str, &'static str); 4],
);

const TIERS: [Tier; 3] = [
    (
        "Level 1",
        "2 Boosts",
        [
            ("boost_perk_3", "100 Emoji Slots"),
            ("clips_soundboard", "24 Soundboard Slots"),
            ("boost_perk_gif", "Animated Server Icon"),
            ("voice_select_headphones", "Better Quality Audio"),
        ],
    ),
    (
        "Level 2",
        "7 Boosts",
        [
            ("boost_perk_2", "HD Streaming"),
            ("ob_upload", "50MB File Uploads"),
            ("boost_perk_0", "Custom Role Icons"),
            ("boost_perk_images", "Server Banner"),
        ],
    ),
    (
        "Level 3",
        "14 Boosts",
        [
            ("boost_perk_v0", "Custom Server Invite Link"),
            ("ob_upload", "100MB File Uploads"),
            ("voice_select_headphones", "Highest Quality Audio"),
            ("boost_perk_gif", "Animated Server Banner"),
        ],
    ),
];

/// A cell of the comparison table.
#[derive(Clone, Copy)]
enum Cell {
    Text(&'static str),
    Yes,
    No,
}

/// The table, row by row: the perk, then Unboosted, Level 1, 2 and 3.
const TABLE: [(&str, [Cell; 4]); 12] = [
    (
        "Emoji Slots",
        [
            Cell::Text("50"),
            Cell::Text("100"),
            Cell::Text("150"),
            Cell::Text("250"),
        ],
    ),
    (
        "Sticker Slots",
        [
            Cell::Text("5"),
            Cell::Text("15"),
            Cell::Text("30"),
            Cell::Text("60"),
        ],
    ),
    (
        "Soundboard Slots",
        [
            Cell::Text("8"),
            Cell::Text("24"),
            Cell::Text("36"),
            Cell::Text("48"),
        ],
    ),
    (
        "Stream Quality",
        [
            Cell::Text("720p & 30fps"),
            Cell::Text("720p & 60fps"),
            Cell::Text("1440p & 60fps"),
            Cell::Text("1440p & 60fps"),
        ],
    ),
    (
        "Audio Quality",
        [
            Cell::Text("96kbps"),
            Cell::Text("128kbps"),
            Cell::Text("256kbps"),
            Cell::Text("384kbps"),
        ],
    ),
    (
        "Upload Size Limit",
        [
            Cell::Text("10MB"),
            Cell::Text("10MB"),
            Cell::Text("50MB"),
            Cell::Text("100MB"),
        ],
    ),
    (
        "Video Stage Seats",
        [
            Cell::Text("50"),
            Cell::Text("50"),
            Cell::Text("150"),
            Cell::Text("300"),
        ],
    ),
    (
        "Animated Server Icon",
        [Cell::No, Cell::Yes, Cell::Yes, Cell::Yes],
    ),
    (
        "Server Invite Background",
        [Cell::No, Cell::Yes, Cell::Yes, Cell::Yes],
    ),
    (
        "Server Banner",
        [
            Cell::No,
            Cell::No,
            Cell::Text("Static"),
            Cell::Text("Animated"),
        ],
    ),
    (
        "Custom Role Icons",
        [Cell::No, Cell::No, Cell::Yes, Cell::Yes],
    ),
    (
        "Custom Invite Link",
        [Cell::No, Cell::No, Cell::No, Cell::Yes],
    ),
];

/// The four "be recognized" cards (`c_ash1`): the icon and the line under it.
const RECOGNIZED: [(&str, &str); 4] = [
    ("boosts", "Display your support with a Member List icon"),
    (
        BADGE_PICTURE,
        "Unlock a profile badge that evolves over time",
    ),
    ("role_shield", "Get an exclusive Booster role"),
    ("shop_heart", "Show support for your community"),
];
/// Marks the one "be recognized" card that shows a downloaded picture rather than a muted icon.
const BADGE_PICTURE: &str = "@picture";

/// The FAQ, in the frame's order: the question, the answer (`en-US` strings of module 232122) and, for the one
/// answer with a Help Center link, its text and what follows it.
type Faq = (
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
);

const FAQ: [Faq; 9] = [
    (
        "What is a Server Boost?",
        "Boosts allow you to support your favorite Servers, by unlocking perks for everyone in them. The more Boosts a server has, the more epic the perks that can be unlocked. See the chart above for more details.",
        None,
    ),
    (
        "How do I Boost a server?",
        "You can Boost this server directly on this page! If you want to Boost a different server, you can find step-by-step instructions ",
        Some(("in our Help Center", ".")),
    ),
    (
        "Can I Boost a Server multiple times?",
        "Yes, you can Boost a server as many times as you want! The more Boosts the server gets, the better the perks that will be unlocked.",
        None,
    ),
    (
        "Can I continue Boosting a server after it reaches Level 3?",
        "Absolutely! Server Boosts are a great way to support your favorite servers. When you Boost a server, you'll be recognized with a special Member List Icon, a Boosting role, and a Profile Badge.",
        None,
    ),
    (
        "What are 'additional perks' and how are they different from Levels?",
        "Additional Perks are new features you can unlock directly with Boosts, separate from your server's Boost Level. Currently, there are two available: Enhanced Role Styles (which add gradient and holographic effects to roles) and Server Tags (4-character identifiers members can display on their profiles across Discord). Unlike traditional server levels, these perks can be unlocked individually without reaching specific level thresholds.",
        None,
    ),
    (
        "How much do the new additional perks cost?",
        "Enhanced Role Styles and Server Tags each cost 3 Boosts. If you want both perks, you'll need 6 Boosts total (3 for each perk).",
        None,
    ),
    (
        "Are additional perks replacing the current server level system?",
        "No, this supplements the existing level system. All current server level perks (emoji slots, file upload limits, etc.) remain exactly the same. This new system simply gives you additional ways to use your Boosts for targeted perks without affecting your server's level progression.",
        None,
    ),
    (
        "Do additional perks use separate Boosts from my server level?",
        "Yes, Boosts used for additional perks are dedicated specifically to those perks and don't count toward your server's level progression. Similarly, Boosts allocated to your server level can't be used for additional perks. You'll need separate Boosts for each - so if you want Enhanced Role Styles, you need 3 Boosts just for that perk, plus whatever Boosts you want to maintain your current server level.",
        None,
    ),
    (
        "Are additional perks a one-time purchase, or do they require ongoing Boosts?",
        "Each additional perk requires an ongoing allocation of its required Boosts to stay active, just like levels. If your server drops below the required number of Boosts, the perk will be disabled until the necessary Boosts are reallocated.",
        None,
    ),
];

impl BoostPage {
    pub(super) fn new() -> Self {
        let sqrt2 = std::f32::consts::SQRT_2;
        Self {
            scroll: ScrollHandle::new(),
            faq_open: None,
            // `radial-gradient(ellipse 100% 70% at 50% 0, rgba(156,89,209,.6) 0, rgba(88,47,140,.3) 50%,
            // transparent 80%)` over the 1264x1280 box (`headerBackground`'s `min-height`).
            header_gradient: radial_image(
                (1264, 1280),
                (632., 0.),
                (1264., 896.),
                &[
                    (0., [156., 89., 209., 0.6]),
                    (0.5, [88., 47., 140., 0.3]),
                    (0.8, [0., 0., 0., 0.]),
                ],
            ),
            // `radial-gradient(ellipse 355px 208px at 50% -31.5px, rgba(86,22,138,.5) 0, rgba(86,22,138,0) 100%)`
            // over the card's 586x316 padding box.
            card_glow: radial_image(
                (586, 316),
                (293., -31.5),
                (355., 208.),
                &[(0., [86., 22., 138., 0.5]), (1., [86., 22., 138., 0.])],
            ),
            // `radial-gradient(ellipse at center, rgba(25,22,58,.2) 0, rgba(25,22,58,.9) 100%)`: farthest-corner,
            // so the radii are the half sides times the square root of two.
            video_overlay: radial_image(
                (770, 433),
                (385., 216.5),
                (385. * sqrt2, 216.5 * sqrt2),
                &[(0., [25., 22., 58., 0.2]), (1., [25., 22., 58., 0.9])],
            ),
        }
    }
}

/// The recommended tier's column highlight index (Level 2 is `recommendedTierHighlightTier2`).
const RECOMMENDED: usize = 2;

impl Root {
    /// The dropdown's Server Boost row and the progress bar's click.
    pub(super) fn open_boost_page(&mut self, cx: &mut Context<Self>) {
        if self.current_guild().is_none() {
            return;
        }
        for (key, file) in ART {
            self.request_image(key.to_owned(), format!("{CDN}{file}"));
        }
        self.request_image(MEMBER_ICON.0.to_owned(), MEMBER_ICON.1.to_owned());
        self.boost_page = Some(BoostPage::new());
        cx.notify();
    }

    /// A downloaded picture of the page at `width` x `height`, or an empty block of that size until it arrives.
    fn boost_art(&self, key: &str, width: f32, height: f32) -> AnyElement {
        match self.images.get(key) {
            Some(image) => img(image.clone())
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().w(px(width)).h(px(height)).into_any_element(),
        }
    }

    pub(super) fn close_boost_page(&mut self) -> bool {
        self.boost_page.take().is_some()
    }

    /// "Boost This Server": the account's free Boosts go to this server (`boost_apply.rs`); with none, as far as
    /// the account's Server Boost page, since nothing here buys anything.
    fn boost_page_boost(&mut self, cx: &mut Context<Self>) {
        match self.guild {
            Some(guild) => self.boost_this_server(guild, cx),
            None => {
                self.boost_page = None;
                self.open_settings_section("Server Boost", cx);
            }
        }
    }

    fn boost_cta_button(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        // `expressive` primary on the dark header: white, black label, the gem 20px, label 16px medium.
        div()
            .id(id)
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(gpui::rgb(0xffffff))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .child(
                gpui::svg()
                    .path("icons/boost_gem.svg")
                    .size(px(20.))
                    .text_color(gpui::rgb(0x000000)),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(gpui::rgb(0x000000))
                    .child("Boost This Server"),
            )
            .on_click(cx.listener(|this, _, _, cx| this.boost_page_boost(cx)))
    }

    pub(super) fn boost_page_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let page = self.boost_page.as_ref()?;
        let guild = self.current_guild()?;
        let gift = offers_gift(self.premium());
        let scroll = page.scroll.clone();
        let scrolled = -f32::from(scroll.offset().y);
        let name = guild.name.clone();
        let acronym = guild.acronym();
        let boosts = guild.premium_subscription_count;
        let count_text = match boosts {
            0 => "No Boosts".to_owned(),
            1 => "1 Boost".to_owned(),
            n => format!("{n} Boosts"),
        };
        let faq_open = page.faq_open;

        // The server's picture when it has one (the rail has loaded it), its initials otherwise.
        let picture = self.images.get(&format!("guild:{}", guild.id)).cloned();
        let guild_chip = |size: f32, radius: f32| {
            let chip = div()
                .size(px(size))
                .flex_none()
                .rounded(px(radius))
                .border_1()
                .border_color(gpui::hsla(0., 0., 1., 0.16))
                .bg(color("--background-base-low"))
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(if size > 40. { 20. } else { 12. }))
                .line_height(px(if size > 40. { 24. } else { 16. }))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"));
            match picture.clone() {
                Some(picture) => chip.child(
                    gpui::img(picture)
                        .size(px(size - 2.))
                        .rounded(px(radius - 1.)),
                ),
                None => chip.child(acronym.clone()),
            }
        };

        // The header: the gradient the backdrop video sits on, the server, the headline, the two buttons.
        let header = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            // `.headerBackground_0jpYct`: the 1280px gradient with the header video (here its still frame, 632px
            // tall at the page's width) above it.
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .right(px(0.))
                    .h(px(1280.))
                    .overflow_hidden()
                    .child(
                        img(page.header_gradient.clone())
                            .absolute()
                            .top(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(px(1280.)),
                    )
                    .children(self.images.get("boost:backdrop").map(|backdrop| {
                        img(backdrop.clone())
                            .absolute()
                            .top(px(0.))
                            .left(px(0.))
                            .w_full()
                            .h(px(632.))
                            .object_fit(ObjectFit::Cover)
                    })),
            )
            .child(
                div()
                    .relative()
                    .w(px(COLUMN))
                    .pt(px(64.))
                    .pb(px(32.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(24.))
                            .mb(px(40.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(guild_chip(72., 19.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(4.))
                                            .child(
                                                text(20., 24., 600., "--text-strong")
                                                    .text_color(gpui::rgb(0xffffff))
                                                    .child(name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .h(px(20.))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(2.))
                                                    .child(
                                                        gpui::svg()
                                                            .path("icons/boost_gem.svg")
                                                            .size(px(18.))
                                                            .text_color(if boosts > 0 {
                                                                super::hsla(
                                                                    "--guild-boosting-pink-refresh",
                                                                )
                                                            } else {
                                                                gpui::hsla(0., 0., 1., 0.6)
                                                            }),
                                                    )
                                                    .child(
                                                        text(16., 20., 600., "--text-strong")
                                                            .text_color(gpui::rgb(0xffffff))
                                                            .child(count_text),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .parity("boost-hero")
                                    .w(px(770.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(40.))
                                    .child(
                                        // `.heading_mQ3T6n`: 770px wide, `letter-spacing: -1.26px`; gpui has no tracking,
                                        // so each glyph is its own box with the negative margin, and the two lines
                                        // (the wrap the official page makes at 770px) are set explicitly.
                                        text(42., 46.2, 700., "--text-strong")
                                            .font_family(super::nitro::NITRO_FONT)
                                            .w_full()
                                            // Two lines of 46.2px (the line box is rounded here); the glyphs sit 4px
                                            // higher in the capture's box (dropdown-boost-modal dom: 222, rows 224/270).
                                            .relative()
                                            .top(px(-4.))
                                            .left(px(1.))
                                            .h(px(92.4))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .whitespace_nowrap()
                                            .child(tracked(
                                                "BOOST THIS SERVER & UNLOCK",
                                                HEADING_TRACKING,
                                            ))
                                            .child(tracked("PERKS FOR EVERYONE", HEADING_TRACKING)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(12.))
                                            .child(self.boost_cta_button("boost-page-cta", cx))
                                            .child(
                                                div()
                                                    .id("boost-page-nitro")
                                                    .h(px(40.))
                                                    .px(px(16.))
                                                    .rounded(px(8.))
                                                    // Translucent gray over the gradient: the capture reads
                                                    // (59,45,76) over (46,30,64), which is `hsla(240,4%,61%,.12)`.
                                                    .bg(gpui::hsla(240. / 360., 0.04, 0.61, 0.12))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(gpui::rgb(0xffffff))
                                                    .when(gift, |this| {
                                                        this.gap(px(4.))
                                                            .child(icon("gift", 20., "--white"))
                                                    })
                                                    .child(if gift {
                                                        "Gift Nitro"
                                                    } else {
                                                        "Get Nitro with 2 Free Boosts"
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.boost_page = None;
                                                            if gift {
                                                                this.open_purchase(true, cx);
                                                            } else {
                                                                this.open_nitro(cx);
                                                            }
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    // `container_hKMjy9`: the video card, 770 wide plus the 1px border, radius lg; the thumbnail
                    // (module 519636) under `overlay_hKMjy9`, and `CirclePlayIcon` 76px white in the middle.
                    .child(
                        div()
                            .id("boost-page-video")
                            .parity_with_border("boost-video", 1.)
                            .relative()
                            .w(px(772.))
                            .h(px(435.1))
                            .flex_none()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                let gid = this.guild;
                                this.open_how_boosting_works(gid, cx);
                            }))
                            // 32px, plus the .53px the official layout carries down from the heading's 2 x 46.2 and
                            // the thumbnail's 433.1 (layout here rounds each box to whole pixels).
                            .mb(px(32.53))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .overflow_hidden()
                            .children(self.images.get("boost:thumbnail").map(|thumbnail| {
                                img(thumbnail.clone())
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .w(px(770.))
                                    .h(px(433.1))
                                    .object_fit(ObjectFit::Cover)
                                    .rounded(px(15.))
                            }))
                            .child(
                                img(page.video_overlay.clone())
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .w(px(770.))
                                    .h(px(433.1))
                                    .rounded(px(15.)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        gpui::svg()
                                            .path("icons/boost_circle_play.svg")
                                            .size(px(76.))
                                            .text_color(gpui::rgb(0xffffff)),
                                    ),
                            ),
                    )
                    .child(self.boost_tier_cards(cx)),
            );

        // The middle: additional perks (Enhanced Role Styles, Server Tag).
        let perks_cards = div()
            .w(px(COLUMN))
            .px(px(0.))
            .pt(px(32.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(32.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        text(32., 40., 600., "--text-default")
                            .child("Boost this server with additional perks"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(rgba(0xaaaab1ff))
                            .child("Unlock these perks individually - no Boost Level required.")
                            .child(div().text_color(rgba(0x5197edff)).child("Learn More")),
                    ),
            )
            .child(
                div().parity("boost-perks").flex().gap(px(24.)).children(
                    [
                        (
                            "boost-perk-roles",
                            SKU_ROLE_STYLES,
                            "boost:roles",
                            "Enhanced Role Styles",
                            "Make certain roles stand out with animated, vibrant gradients.",
                            3u32,
                            Some("NEW"),
                        ),
                        (
                            "boost-perk-tag",
                            SKU_SERVER_TAG,
                            "boost:tag",
                            "Server Tag",
                            "Let your members represent your server everywhere on Discord.",
                            3u32,
                            Some("BETA"),
                        ),
                    ]
                    .map(|(id, sku_id, art, title, body, cost, badge)| {
                        // `.card_RL7BaR` (module 658937) + module 577930 (`NI`/`Ft`/`$L` powerup perk card primitives:
                        // active/expiring/removing borders, corner NEW/BETA badge, `t2Wbo1` boost cost).
                        let perk_active = self.current_guild().is_some_and(|g| {
                            (sku_id == SKU_ROLE_STYLES
                                && g.features.iter().any(|f| f == "ENHANCED_ROLE_COLORS"))
                                || (sku_id == SKU_SERVER_TAG
                                    && g.features.iter().any(|f| f == "GUILD_TAGS"))
                        });
                        let cost_label = dc_intl::format(
                            "t2Wbo1",
                            &[("required", (cost as f64).into()), ("decorator", "".into())],
                        );
                        div()
                            .id(id)
                            .cursor_pointer()
                            .relative()
                            .w(px(588.))
                            .h(px(318.))
                            .flex_none()
                            .rounded(px(16.))
                            .backdrop_blur(px(40.))
                            .border_1()
                            .border_color(if perk_active {
                                gpui::Hsla::from(color("--status-positive"))
                            } else {
                                gpui::hsla(240. / 360., 0.04, 0.61, 0.12)
                            })
                            .bg(gpui::hsla(240. / 360., 0.04, 0.61, 0.08))
                            .hover(|s| s.border_color(gpui::hsla(240. / 360., 0.04, 0.75, 0.28)))
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(24.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let guild_id = this.guild.unwrap_or(1);
                                let can_manage = this.my_permission(
                                    guild_id,
                                    None,
                                    dc_state::permissions::MANAGE_GUILD,
                                );
                                this.powerup_perk_modal =
                                    Some(PowerupPerkModal::for_sku(guild_id, sku_id, can_manage));
                                cx.notify();
                            }))
                            .child(
                                img(page.card_glow.clone())
                                    .absolute()
                                    .top(px(0.))
                                    .left(px(0.))
                                    .w(px(586.))
                                    .h(px(316.))
                                    .rounded(px(15.)),
                            )
                            .when_some(badge, |card, pill| {
                                card.child(
                                    div()
                                        .absolute()
                                        .top(px(12.))
                                        .right(px(12.))
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(999.))
                                        .bg(color("--control-primary-background-default"))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child(pill),
                                )
                            })
                            .child(self.boost_art(art, 250., 140.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_center()
                                    .child(
                                        text(20., 24., 600., "--text-default")
                                            .text_color(rgba(0xefeff0ff))
                                            .child(title),
                                    )
                                    .child(
                                        text(16., 20., 400., "--text-default")
                                            .text_color(rgba(0xaaaab1ff))
                                            .child(body),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .h(px(32.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(gpui::hsla(240. / 360., 0.04, 0.61, 0.04))
                                            .bg(gpui::hsla(240. / 360., 0.04, 0.61, 0.08))
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .text_color(rgba(0x94959cff))
                                            .child(icon(
                                                "boost_gem",
                                                16.,
                                                "--guild-boosting-pink-refresh",
                                            ))
                                            .child(cost_label),
                                    )
                                    .when(perk_active, |row| {
                                        row.child(
                                            div()
                                                .h(px(32.))
                                                .px(px(10.))
                                                .rounded(px(8.))
                                                .bg(gpui::rgba(0x23a55a22))
                                                .flex()
                                                .items_center()
                                                .gap(px(4.))
                                                .child(icon(
                                                    "circle_check",
                                                    14.,
                                                    "--status-positive",
                                                ))
                                                .child(
                                                    text(
                                                        13.,
                                                        16.,
                                                        600.,
                                                        "--text-feedback-positive",
                                                    )
                                                    .child("Active"),
                                                ),
                                        )
                                    })
                                    .children({
                                        let guild_id = self.guild.unwrap_or(1);
                                        let is_admin = self.my_permission(
                                            guild_id,
                                            None,
                                            dc_state::permissions::ADMINISTRATOR,
                                        );
                                        let action = powerup_card_action(
                                            is_admin,
                                            perk_active,
                                            true,
                                            true,
                                            false,
                                        );
                                        let label = match action {
                                            PowerupCardAction::Configure => {
                                                Some(dc_intl::t("g5Ds69"))
                                            }
                                            PowerupCardAction::ViewPerks => {
                                                Some(dc_intl::t("0Q61kF"))
                                            }
                                            PowerupCardAction::ViewPerk => {
                                                Some(dc_intl::t("Xa11Ep"))
                                            }
                                            PowerupCardAction::ToggleBoost => {
                                                Some(dc_intl::t("Xa11Ep"))
                                            }
                                            PowerupCardAction::None => None,
                                        };
                                        label.map(|cta| {
                                            div()
                                                .id(SharedString::from(format!(
                                                    "powerup-card-cta-{id}"
                                                )))
                                                .h(px(32.))
                                                .px(px(14.))
                                                .rounded(px(8.))
                                                .bg(color("--control-primary-background-default"))
                                                .hover(|s| {
                                                    s.bg(color(
                                                        "--control-primary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--white"))
                                                .child(cta)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if action == PowerupCardAction::Configure {
                                                        this.open_server_settings_at(
                                                            guild_id,
                                                            powerup_settings_page_for_sku(sku_id),
                                                            cx,
                                                        );
                                                    } else {
                                                        let can_manage = this.my_permission(
                                                            guild_id,
                                                            None,
                                                            dc_state::permissions::MANAGE_GUILD,
                                                        );
                                                        this.powerup_perk_modal =
                                                            Some(PowerupPerkModal::for_sku(
                                                                guild_id, sku_id, can_manage,
                                                            ));
                                                        cx.notify();
                                                    }
                                                }))
                                        })
                                    }),
                            )
                    }),
                ),
            );

        let table = self.boost_table();
        let recognized = div()
            .w(px(COLUMN))
            .mt(px(80.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(32.))
            .child(
                text(32., 40., 600., "--text-default")
                    .child("Give your community a Boost & be recognized"),
            )
            .child(
                div()
                    .parity("boost-recognized")
                    .flex()
                    .gap(px(24.))
                    .w_full()
                    .children(RECOGNIZED.map(|(art, line)| {
                        // `.card_c_ash1`: `flex: 1 1 0; min-width: 0`, padding 24, 12px between icon and line.
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .h(px(150.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-base-low"))
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(12.))
                            .child(if art == BADGE_PICTURE {
                                self.boost_art("boost:badge-picture", 48., 48.)
                            } else {
                                icon(art, 48., "--icon-muted").into_any_element()
                            })
                            .child(
                                text(16., 20., 500., "--text-muted")
                                    .text_center()
                                    .child(line),
                            )
                    })),
            );

        let faq =
            div()
                .w(px(COLUMN))
                .mt(px(80.))
                .flex()
                .flex_col()
                .gap(px(32.))
                .child(
                    text(32., 40., 600., "--text-default")
                        .w_full()
                        .text_center()
                        .child("Boosting FAQ"),
                )
                .child(
                    div()
                        .parity("boost-faq")
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(FAQ.iter().enumerate().map(
                            |(index, (question, answer, link))| {
                                let open = faq_open == Some(index);
                                // `.listItem_EVXXpI`: padding 12, 1px border, radius 8; the question is
                                // `heading-sm/semibold` (muted, default while open or hovered), the chevron 18px turned
                                // over while open, the answer `body-sm/medium` muted 8px below.
                                div()
                                    .id(("boost-faq", index))
                                    .px(px(12.))
                                    .py(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(if open {
                                        color("--border-strong")
                                    } else {
                                        color("--border-subtle")
                                    })
                                    .bg(if open {
                                        color("--background-surface-highest")
                                    } else {
                                        color("--background-base-low")
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .flex_col()
                                    .text_color(if open {
                                        color("--text-default")
                                    } else {
                                        color("--text-muted")
                                    })
                                    .hover(|row| row.text_color(color("--text-default")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(page) = this.boost_page.as_mut() {
                                            page.faq_open = toggled_faq(page.faq_open, index);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(600.))
                                                    .child(*question),
                                            )
                                            .child(
                                                icon(
                                                    "chevron_down",
                                                    18.,
                                                    "--interactive-icon-default",
                                                )
                                                .when(open, |chevron| {
                                                    chevron.with_transformation(
                                                        gpui::Transformation::rotate(
                                                            gpui::percentage(0.5),
                                                        ),
                                                    )
                                                }),
                                            ),
                                    )
                                    .when(open, |row| {
                                        row.child(
                                            div()
                                                .pt(px(8.))
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-muted"))
                                                .child(match link {
                                                    None => gpui::StyledText::new(*answer),
                                                    Some((text, rest)) => {
                                                        let start = answer.len();
                                                        gpui::StyledText::new(format!(
                                                            "{answer}{text}{rest}"
                                                        ))
                                                        .with_highlights([(
                                                            start..start + text.len(),
                                                            gpui::HighlightStyle {
                                                                color: Some(
                                                                    color("--text-link").into(),
                                                                ),
                                                                ..Default::default()
                                                            },
                                                        )])
                                                    }
                                                }),
                                        )
                                    })
                            },
                        )),
                );

        let body = div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(header)
            .child(perks_cards)
            .child(div().w(px(COLUMN)).mt(px(64.)).child(table))
            .child(recognized)
            .child(faq)
            .child(div().h(px(120. + 32.)).flex_none());

        let bar = (scrolled > HERO_CTA_BOTTOM).then(|| {
            div()
                .absolute()
                .bottom(px(36.))
                .left(px(0.))
                .right(px(0.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .h(px(66.))
                        .px(px(12.))
                        .rounded(px(16.))
                        // `.innerWrapper_RcIw1d` (module 430815, `392028.css`): `hsla(0,0%,100%,.17)` and a 1px
                        // `hsla(0,0%,100%,.16)` border over a 16px backdrop blur.
                        .backdrop_blur(px(16.))
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.16))
                        .bg(gpui::hsla(0., 0., 1., 0.17))
                        .flex()
                        .items_center()
                        .gap(px(40.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(guild_chip(32., 8.))
                                .child(
                                    text(16., 20., 600., "--text-strong")
                                        .text_color(gpui::rgb(0xffffff))
                                        .child(name.clone()),
                                ),
                        )
                        .child(self.boost_cta_button("boost-page-bar-cta", cx)),
                )
        });

        let close = div()
            .id("boost-page-close")
            .absolute()
            .top(px(24.))
            .right(px(24.))
            .size(px(40.))
            .rounded(px(12.))
            .bg(rgba(0x10101280))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| {
                this.boost_page = None;
                cx.notify();
            }))
            // `XSmallIcon` at its `md` size, 24px.
            .child(icon("clips_x_small", 24., "--primary-330"));

        Some(
            gpui::deferred(
                div()
                    .id("boost-page")
                    .parity("boost-page")
                    .absolute()
                    .inset_0()
                    .bg(color("--background-base-lowest"))
                    .occlude()
                    .child(
                        div()
                            .id("boost-page-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                            .pr(px(GUTTER))
                            .child(body),
                    )
                    .child(gutter_scrollbar(scroll))
                    .children(bar)
                    .child(close),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// `wrapper_xZJtlG`: the three level cards.
    fn boost_tier_cards(&self, cx: &mut Context<Self>) -> AnyElement {
        let boosts = self
            .current_guild()
            .map_or(0, |guild| guild.premium_subscription_count);
        let needed = [2u64, 7, 14];
        div()
            .parity("boost-tiers")
            .w_full()
            .flex()
            .gap(px(24.))
            .children(
                TIERS
                    .iter()
                    .enumerate()
                    .map(|(level, (title, count, perks))| {
                        let active = boosts >= needed[level];
                        let tier_num = (level + 1) as u8;
                        let start = level == 0;
                        let end = level == 2;
                        // `.progressBarActive`: `linear-gradient(90deg, #ff4ebb, rgba(255,78,187,.4))`;
                        // `.progressBarInactive`: `#474464`.
                        let bar = if active {
                            gpui::linear_gradient(
                                90.,
                                gpui::linear_color_stop(rgba(0xff4ebbff), 0.),
                                gpui::linear_color_stop(rgba(0xff4ebb66), 1.),
                            )
                        } else {
                            rgba(0x474464ff).into()
                        };
                        // `.gemContainerLevel{1,2,3}`: the ring's colour per level; an active gem is pink and rings
                        // `#202024`.
                        let ring = if active {
                            0x202024ff
                        } else {
                            [0x202024ff, 0x211e40ff, 0x201d3dff][level]
                        };
                        div()
                            .id(SharedString::from(format!("boost-tier-card-{tier_num}")))
                            .cursor_pointer()
                            .flex_1()
                            .min_w(px(0.))
                            .h(px(306.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(if active {
                                gpui::hsla(240. / 360., 0.04, 0.61, 0.04)
                            } else {
                                super::hsla("--border-subtle")
                            })
                            .bg(color("--background-mod-subtle"))
                            .hover(|s| s.border_color(color("--border-strong")))
                            .overflow_hidden()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let guild_id = this.guild.unwrap_or(1);
                                let can_manage = this.my_permission(
                                    guild_id,
                                    None,
                                    dc_state::permissions::MANAGE_GUILD,
                                );
                                this.powerup_level_modal = Some(PowerupLevelModal::for_tier(
                                    guild_id, tier_num, active, can_manage,
                                ));
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .relative()
                                    .h(px(64.))
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(40.))
                                            .h(px(8.))
                                            .left(px(if start { 40. } else { 0. }))
                                            .right(px(if end { 24. } else { 0. }))
                                            .bg(bar)
                                            .when(start, |bar| bar.rounded_l(px(100.)))
                                            .when(end, |bar| bar.rounded_r(px(100.))),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top(px(24.))
                                            .left(px(24.))
                                            .size(px(40.))
                                            .rounded(px(20.))
                                            .border_5()
                                            .border_color(rgba(ring))
                                            .bg(if active {
                                                super::hsla("--guild-boosting-pink-refresh")
                                            } else {
                                                rgba(0x474464ff).into()
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(self.boost_art(
                                                ["boost:tier1", "boost:tier2", "boost:tier3"]
                                                    [level],
                                                24.,
                                                24.,
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .h(px(32.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                text(24., 32., 600., "--text-default")
                                                    .text_color(if active {
                                                        color("--text-default")
                                                    } else {
                                                        rgba(0x94959cff)
                                                    })
                                                    .child(*title),
                                            )
                                            .child(
                                                div()
                                                    .h(px(30.))
                                                    .px(px(8.))
                                                    .rounded(px(8.))
                                                    .bg(color("--background-mod-subtle"))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(rgba(0xaaaab1ff))
                                                    .child(icon("boost_gem", 16., "--text-muted"))
                                                    .child(
                                                        div()
                                                            .text_color(color("--text-default"))
                                                            .child(*count),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.))
                                            .children(perks.iter().map(|(art, line)| {
                                                div()
                                                    .h(px(20.))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(icon(art, 20., "--text-muted"))
                                                    .child(
                                                        text(16., 20., 500., "--text-default")
                                                            .child(*line),
                                                    )
                                            }))
                                            .child(
                                                text(16., 20., 500., "--text-default")
                                                    .text_color(rgba(0x94959cff))
                                                    .child("...and more!"),
                                            ),
                                    ),
                            )
                    }),
            )
            .into_any_element()
    }

    /// `table_twYJsJ`: Perks, Unboosted and the three levels, with the recommended column outlined.
    fn boost_table(&self) -> AnyElement {
        // `.cell_twYJsJ`: `flex: 1 1 0`, `padding: 0 16px`; five equal 240px cells in the 1200px table.
        let cell = |content: AnyElement| {
            div()
                .flex_1()
                .min_w(px(0.))
                .px(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(content)
        };
        // `heading-lg/semibold` (20/24) and `body-sm/medium` (14/18).
        let head = |title: &'static str, sub: &'static str| {
            cell(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(text(20., 24., 600., "--text-strong").child(title))
                    .child(text(14., 18., 500., "--text-muted").child(sub))
                    .into_any_element(),
            )
        };
        let header_row = div()
            .h(px(75.))
            .py(px(16.))
            .flex()
            .items_start()
            .border_b_1()
            .border_color(color("--border-normal"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .px(px(16.))
                    .child(text(20., 24., 600., "--text-strong").child("Perks")),
            )
            .child(head("Unboosted", "No Boosts"))
            .child(head("Level 1", "2 Boosts"))
            .child(head("Level 2", "7 Boosts"))
            .child(head("Level 3", "14 Boosts"));
        let rows = TABLE.iter().enumerate().map(|(index, (label, cells))| {
            let last = index == TABLE.len() - 1;
            let has_icon = cells.iter().any(|cell| !matches!(cell, Cell::Text(_)));
            div()
                // 16 + content + 16 + the 1px border, which the last row lacks.
                .h(px(
                    if has_icon { 57. } else { 53. } - if last { 1. } else { 0. }
                ))
                .py(px(16.))
                .flex()
                .items_center()
                .when(!last, |row| {
                    row.border_b_1().border_color(color("--border-normal"))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .px(px(16.))
                        .child(text(16., 20., 400., "--text-muted").child(*label)),
                )
                .children(cells.iter().enumerate().map(|(tier, value)| {
                    let content: AnyElement = match value {
                        Cell::Text(value) => text(
                            16.,
                            20.,
                            if tier == 0 { 400. } else { 600. },
                            if tier == 0 {
                                "--text-muted"
                            } else {
                                "--text-strong"
                            },
                        )
                        .child(*value)
                        .into_any_element(),
                        Cell::Yes => {
                            icon("checkmark_large", 24., "--text-strong").into_any_element()
                        }
                        // `XSmallIcon` (14px of its 24px box).
                        Cell::No => {
                            icon("clips_x_small", 24., "--interactive-muted").into_any_element()
                        }
                    };
                    cell(content)
                }))
        });
        div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(48.))
            .child(
                text(32., 40., 600., "--text-default")
                    .child("Uplevel this server with the best perks"),
            )
            .child(
                div()
                    .parity("boost-table")
                    .relative()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .child(header_row)
                            .children(rows),
                    )
                    // `.recommendedTierHighlight`: 20% wide plus its 2px border (content-box), pink, 20px past the
                    // table at both ends.
                    .child(
                        div()
                            .absolute()
                            .top(px(-20.))
                            .bottom(px(-20.))
                            .right(px(COLUMN * 0.2 * (4 - RECOMMENDED - 1) as f32))
                            .w(px(COLUMN * 0.2 + 4.))
                            .rounded(px(16.))
                            .border_2()
                            .border_color(super::hsla("--guild-boosting-pink-refresh")),
                    )
                    // `.recommendedTierHighlightTag`: `top: 0; left: 50%; translate(-50%, -50%)` of the highlight,
                    // so centred on its top edge's inner side (capture: 105.6 by 24 at x 817.2, 5.5 over a
                    // table at 35.5), drawn over the border line.
                    .child(
                        div()
                            .absolute()
                            .top(px(-30.))
                            .left(px(COLUMN
                                - COLUMN * 0.2 * (3 - RECOMMENDED) as f32
                                - 122.
                                - 52.8))
                            .w(px(105.6))
                            .h(px(24.))
                            .rounded(px(8.))
                            .bg(super::hsla("--guild-boosting-pink-refresh"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(gpui::rgb(0xffffff))
                            .child("RECOMMENDED"),
                    ),
            )
            .into_any_element()
    }
}

/// Module 233824 (`hasFreeBoosts(user) || isPremium(user, TIER_2)`): an account with free boosts already has them, so
/// the secondary button gifts Nitro; any other account is offered "Get Nitro with 2 Free Boosts".
fn offers_gift(premium: dc_state::premium::PremiumType) -> bool {
    use dc_state::premium::{Feature, PremiumType};
    premium.has(Feature::FreeBoosts) || premium == PremiumType::Nitro
}

impl Root {
    /// Capture helper for `GuildPowerupPerkModal` (module 945942: `powerup-perk theme|roles|tag|vanity|disable-confirm|close`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_powerup_perk(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.powerup_perk_modal = None;
            cx.notify();
            return "powerup-perk: closed".to_owned();
        }
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1);
        let mut modal = match mode {
            "theme" => PowerupPerkModal::for_sku(guild_id, SKU_GUILD_THEME, true),
            "tag" => PowerupPerkModal::for_sku(guild_id, SKU_SERVER_TAG, true),
            "vanity" => PowerupPerkModal::for_sku(guild_id, SKU_VANITY_URL, true),
            "disable-confirm" => {
                let mut m = PowerupPerkModal::for_sku(guild_id, SKU_SERVER_TAG, true);
                m.is_active = true;
                m.confirm_disable = true;
                m
            }
            _ => PowerupPerkModal::for_sku(guild_id, SKU_ROLE_STYLES, true),
        };
        if mode == "tag-active" {
            modal = PowerupPerkModal::for_sku(guild_id, SKU_SERVER_TAG, true);
            modal.is_active = true;
        }
        self.powerup_perk_modal = Some(modal);
        cx.notify();
        format!("powerup-perk: {mode}")
    }

    /// Renders `GuildPowerupPerkModal` (module 945942): Layout A (Guild Theme `480px`), Layout B
    /// (Standard Perk `850px` split modal w/ interactive Server Tag / Role Gradient / Vanity URL
    /// demos), and `933337` Deactivate Confirm dialog.
    pub(super) fn powerup_perk_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.powerup_perk_modal.as_ref()?;
        let guild_id = modal.guild_id;
        let sku_id = modal.sku_id;
        let cost = modal.cost;
        let is_active = modal.is_active;
        let level_activated = modal.level_activated;
        let can_manage = modal.can_manage;
        let cost_label = dc_intl::format(
            "gDsyB9",
            &[("numSubscriptions", dc_intl::Arg::Num(f64::from(cost)))],
        );

        // Module `179413` (`GuildPowerupDeactivateModal` + `DeactivateConfirmationModal` `O`, CSS `721721`):
        // `size="sm"` (400px) for Perks, title `iEBw1M({perk})`, subtitle `7o0K+2({perk})`, preview box
        // `.image_OgsNUm`, neutral `.warningContainer_OgsNUm` (`--background-mod-subtle`, `--border-muted`,
        // `--text-feedback-critical` `"Warning"` header + SKU-specific warning segments), inline error `.errorText_OgsNUm`.
        if modal.confirm_disable {
            let member_count = self
                .stores
                .member_counts
                .get_member_count(guild_id)
                .unwrap_or(1)
                .max(1);
            let has_vanity = !modal.vanity_code.is_empty();
            let warnings = powerup_deactivate_warnings(
                sku_id,
                &modal.title,
                member_count,
                member_count,
                has_vanity,
            );
            let confirm_title = dc_intl::format("iEBw1M", &[("perk", modal.title.as_str().into())]);
            let confirm_subtitle =
                dc_intl::format("7o0K+2", &[("perk", modal.title.as_str().into())]);
            let inline_error = modal.error.clone();
            let confirm_card = div()
                .id("powerup-disable-confirm")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(px(400.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .shadow_xl()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(text(20., 24., 700., "--text-strong").child(confirm_title))
                                .child(
                                    text(14., 20., 400., "--text-subtle").child(confirm_subtitle),
                                ),
                        )
                        .child(
                            div()
                                .id("powerup-disable-close")
                                .size(px(28.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.powerup_perk_modal.as_mut() {
                                        m.confirm_disable = false;
                                    }
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--interactive-text-default")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(18.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        // `.image_OgsNUm`: `--background-base-lowest`, `--radius-sm`, `max-height: 205px`.
                        .child(
                            div()
                                .w_full()
                                .h(px(108.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lowest"))
                                .px(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(10.))
                                .child(icon("boost_gem", 28., "--brand-500"))
                                .child(
                                    text(15., 20., 600., "--text-strong")
                                        .child(modal.title.clone()),
                                ),
                        )
                        // `.warningContainer_OgsNUm`: `--background-mod-subtle`, `1px solid --border-muted`.
                        .child(
                            div()
                                .p(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    text(14., 18., 600., "--text-feedback-critical")
                                        .child(dc_intl::t("OVt5CC")),
                                )
                                .child(div().flex().flex_col().gap(px(2.)).children(
                                    warnings.into_iter().map(|(msg, critical)| {
                                        text(
                                            14.,
                                            18.,
                                            if critical { 600. } else { 400. },
                                            if critical {
                                                "--text-feedback-critical"
                                            } else {
                                                "--text-default"
                                            },
                                        )
                                        .child(msg)
                                    }),
                                )),
                        )
                        .children(inline_error.map(|err| {
                            text(14., 18., 600., "--text-feedback-critical").child(err)
                        })),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .rounded_b(px(12.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("powerup-disable-cancel")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("ETE/oC"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.powerup_perk_modal.as_mut() {
                                        m.confirm_disable = false;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("powerup-disable-submit")
                                .h(px(38.))
                                .px(px(18.))
                                .rounded(px(8.))
                                .bg(color("--control-critical-primary-background-default"))
                                .hover(|s| {
                                    s.bg(color("--control-critical-primary-background-hover"))
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("PYPdl4"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(toggle_guild_powerup_request(
                                        guild_id, sku_id, false,
                                    ));
                                    this.powerup_perk_modal = None;
                                    cx.notify();
                                })),
                        ),
                );
            return Some(
                deferred(
                    div()
                        .id("powerup-disable-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.powerup_perk_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(confirm_card),
                )
                .with_priority(97)
                .into_any_element(),
            );
        }

        // Layout A — Guild Theme (`SKU_GUILD_THEME`, `modalGuildTheme_s4Cv6K`, 480px vertical).
        if sku_id == SKU_GUILD_THEME {
            let card = div()
                .id("powerup-perk-guild-theme")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .relative()
                .w(px(480.))
                .rounded(px(12.))
                .overflow_hidden()
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-low"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .w_full()
                        .h(px(176.))
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(rgb(0x5865f2), 0.),
                            linear_color_stop(rgb(0xeb459e), 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(240.))
                                .h(px(116.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(rgba(0xffffff33))
                                .bg(rgba(0x1e1f22cc))
                                .p(px(14.))
                                .flex()
                                .flex_col()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(icon(
                                            "boost_gem",
                                            20.,
                                            "--guild-boosting-pink-refresh",
                                        ))
                                        .child(
                                            text(14., 18., 700., "--white").child("Server Theme"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.))
                                        .child(
                                            div()
                                                .h(px(12.))
                                                .flex_1()
                                                .rounded(px(6.))
                                                .bg(rgba(0xffffff33)),
                                        )
                                        .child(
                                            div()
                                                .h(px(12.))
                                                .w(px(56.))
                                                .rounded(px(6.))
                                                .bg(rgba(0xeb459e88)),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("powerup-theme-close")
                        .absolute()
                        .top(px(10.))
                        .right(px(10.))
                        .size(px(28.))
                        .rounded(px(6.))
                        .bg(rgba(0x00000044))
                        .hover(|s| s.bg(rgba(0x00000066)))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.powerup_perk_modal = None;
                            cx.notify();
                        }))
                        .child(icon("close", 16., "--white")),
                )
                .child(
                    div()
                        .px(px(40.))
                        .pt(px(24.))
                        .pb(px(16.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .text_center()
                        .child(text(22., 28., 700., "--text-strong").child(modal.title.clone()))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(icon("boost_gem", 18., "--guild-boosting-pink-refresh"))
                                .child(text(15., 20., 600., "--text-muted").child(cost_label)),
                        )
                        .child(
                            text(14., 20., 400., "--text-default").child(modal.description.clone()),
                        )
                        .children(modal.error.as_ref().map(|e| {
                            text(13., 18., 600., "--text-feedback-critical").child(e.clone())
                        })),
                )
                .when(can_manage, |col| {
                    col.child(
                        div()
                            .px(px(32.))
                            .pt(px(12.))
                            .pb(px(28.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .when(!is_active, |row| {
                                row.child(
                                    div()
                                        .id("powerup-theme-try")
                                        .flex_1()
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(6.))
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("uw9zI7"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.powerup_perk_modal = None;
                                            this.open_guild_theme_nux(guild_id, cx);
                                        })),
                                )
                            })
                            .child(
                                div()
                                    .id("powerup-theme-toggle")
                                    .flex_1()
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .bg(if is_active {
                                        color("--control-secondary-background-default")
                                    } else {
                                        color("--control-primary-background-default")
                                    })
                                    .hover(move |s| {
                                        s.bg(if is_active {
                                            color("--control-secondary-background-hover")
                                        } else {
                                            color("--control-primary-background-hover")
                                        })
                                    })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(6.))
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(if is_active {
                                        color("--text-strong")
                                    } else {
                                        color("--white")
                                    })
                                    .when(!is_active, |b| {
                                        b.child(icon("boost_gem", 16., "--white"))
                                    })
                                    .child(if is_active {
                                        dc_intl::t("R9GHya")
                                    } else {
                                        dc_intl::t("JFfins")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if is_active {
                                            if let Some(m) = this.powerup_perk_modal.as_mut() {
                                                m.confirm_disable = true;
                                            }
                                        } else {
                                            let _ =
                                                this.requests.send(toggle_guild_powerup_request(
                                                    guild_id, sku_id, true,
                                                ));
                                            if let Some(m) = this.powerup_perk_modal.as_mut() {
                                                m.is_active = true;
                                            }
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                });
            return Some(
                deferred(
                    div()
                        .id("powerup-theme-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.powerup_perk_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(card),
                )
                .with_priority(97)
                .into_any_element(),
            );
        }

        // Layout B — Standard Perks (`850px` horizontal split modal with interactive right demo pane).
        let right_demo: AnyElement = match sku_id {
            SKU_SERVER_TAG => {
                const TAG_BADGES: [(&str, u32); 4] = [
                    ("⚡", 0xf0b232),
                    ("🔥", 0xda373c),
                    ("✦", 0x5865f2),
                    ("❄", 0x00a8fc),
                ];
                let sel = modal.selected_tag_badge.min(TAG_BADGES.len() - 1);
                let (glyph, tint) = TAG_BADGES[sel];
                div()
                    .w(px(460.))
                    .h_full()
                    .bg(color("--background-base-lowest"))
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(20.))
                    .child(
                        div()
                            .w(px(360.))
                            .p(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_start()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded_full()
                                    .bg(rgb(0x5865f2))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child("R"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(
                                                text(15., 20., 600., "--brand-400").child("Roka"),
                                            )
                                            .child(
                                                div()
                                                    .px(px(6.))
                                                    .py(px(1.))
                                                    .rounded(px(4.))
                                                    .bg(color("--background-surface-higher"))
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(rgb(tint))
                                                            .child(glyph),
                                                    )
                                                    .child(
                                                        text(11., 14., 700., "--text-strong")
                                                            .child("GGEZ"),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        text(14., 20., 400., "--text-default")
                                            .child(dc_intl::t("6OSasb")),
                                    ),
                            ),
                    )
                    .child(div().flex().items_center().gap(px(10.)).children(
                        TAG_BADGES.iter().enumerate().map(|(i, &(g, col))| {
                            let active = i == sel;
                            div()
                                .id(SharedString::from(format!("powerup-tag-badge-{i}")))
                                .size(px(40.))
                                .rounded(px(10.))
                                .bg(color("--background-base-lower"))
                                .border_2()
                                .border_color(if active {
                                    color("--brand-500")
                                } else {
                                    color("--border-subtle")
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(18.))
                                .text_color(rgb(col))
                                .child(g)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.powerup_perk_modal.as_mut() {
                                        m.selected_tag_badge = i;
                                    }
                                    cx.notify();
                                }))
                        }),
                    ))
                    .into_any_element()
            }
            SKU_VANITY_URL => {
                let code = if modal.vanity_code.is_empty() {
                    "my-server"
                } else {
                    modal.vanity_code.as_str()
                };
                div()
                    .relative()
                    .w(px(460.))
                    .h_full()
                    .bg(color("--background-base-lowest"))
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .w(px(381.))
                            .p(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(24.))
                                            .rounded_full()
                                            .bg(rgb(0x5865f2))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("sb_discord", 14., "--white")),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .child(
                                                div()
                                                    .opacity(0.5)
                                                    .text_color(color("--text-link"))
                                                    .child("discord.gg/"),
                                            )
                                            .child(
                                                div()
                                                    .max_w(px(180.))
                                                    .overflow_hidden()
                                                    .text_ellipsis()
                                                    .whitespace_nowrap()
                                                    .text_color(color("--text-link"))
                                                    .child(code.to_owned()),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(292.))
                                    .h(px(64.))
                                    .rounded(px(8.))
                                    .bg(color("--background-surface-higher"))
                                    .p(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .size(px(40.))
                                            .rounded(px(10.))
                                            .bg(color("--background-mod-subtle")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .child(
                                                div()
                                                    .h(px(10.))
                                                    .w(px(120.))
                                                    .rounded(px(4.))
                                                    .bg(color("--background-mod-strong")),
                                            )
                                            .child(
                                                div()
                                                    .h(px(8.))
                                                    .w(px(84.))
                                                    .rounded(px(4.))
                                                    .bg(color("--background-mod-subtle")),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(16.))
                            .text_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("u5iNbz")),
                    )
                    .into_any_element()
            }
            _ => {
                const GRADIENTS: [(u32, u32); 5] = [
                    (0xff6b9d, 0xc084fc),
                    (0x38bdf8, 0x818cf8),
                    (0x34d399, 0x22d3ee),
                    (0xfbbf24, 0xf97316),
                    (0xf43f5e, 0xfb7185),
                ];
                let sel = modal.selected_gradient.min(GRADIENTS.len() - 1);
                let (c1, c2) = GRADIENTS[sel];
                div()
                    .w(px(460.))
                    .h_full()
                    .bg(color("--background-base-lowest"))
                    .p(px(28.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(20.))
                    .child(
                        div()
                            .w(px(360.))
                            .p(px(16.))
                            .rounded(px(12.))
                            .bg(color("--background-base-lower"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_start()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded_full()
                                    .bg(linear_gradient(
                                        135.,
                                        linear_color_stop(rgb(c1), 0.),
                                        linear_color_stop(rgb(c2), 1.),
                                    ))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child("C"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(15.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(rgb(c1))
                                                    .child("Cherry"),
                                            )
                                            .child(div().h(px(10.)).w(px(36.)).rounded_full().bg(
                                                linear_gradient(
                                                    90.,
                                                    linear_color_stop(rgb(c1), 0.),
                                                    linear_color_stop(rgb(c2), 1.),
                                                ),
                                            )),
                                    )
                                    .child(
                                        text(14., 20., 400., "--text-default")
                                            .child(dc_intl::t("6OSasb")),
                                    ),
                            ),
                    )
                    .child(div().flex().items_center().gap(px(10.)).children(
                        GRADIENTS.iter().enumerate().map(|(i, &(a, b))| {
                            let active = i == sel;
                            div()
                                .id(SharedString::from(format!("powerup-grad-{i}")))
                                .size(px(32.))
                                .rounded_full()
                                .bg(linear_gradient(
                                    135.,
                                    linear_color_stop(rgb(a), 0.),
                                    linear_color_stop(rgb(b), 1.),
                                ))
                                .border_2()
                                .border_color(if active {
                                    color("--white")
                                } else {
                                    rgba(0x00000000)
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.powerup_perk_modal.as_mut() {
                                        m.selected_gradient = i;
                                    }
                                    cx.notify();
                                }))
                        }),
                    ))
                    .into_any_element()
            }
        };

        let left_col = div()
            .flex_1()
            .min_w_0()
            .p(px(28.))
            .flex()
            .flex_col()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(icon("boost_gem", 18., "--guild-boosting-pink-refresh"))
                            .child(text(13., 18., 600., "--text-muted").child(cost_label)),
                    )
                    .child(text(22., 28., 700., "--text-strong").child(modal.title.clone()))
                    .child(text(14., 20., 400., "--text-default").child(modal.description.clone()))
                    .when(modal.cooldown_days > 0, |col| {
                        col.child(
                            div()
                                .mt(px(4.))
                                .flex()
                                .items_start()
                                .gap(px(6.))
                                .child(icon("circle_info", 14., "--text-muted"))
                                .child(text(12., 16., 500., "--text-muted").child(
                                    dc_intl::format(
                                        "GMhQcE",
                                        &[(
                                            "cooldownDays",
                                            dc_intl::Arg::Num(f64::from(modal.cooldown_days)),
                                        )],
                                    ),
                                )),
                        )
                    })
                    .when(sku_id == SKU_VANITY_URL, |col| {
                        col.child(
                            text(12., 16., 400., "--text-muted")
                                .child(dc_intl::format("NAFGkH", &[("level", "Level 3".into())])),
                        )
                        .child(
                            div()
                                .id("powerup-vanity-learn-more")
                                .cursor_pointer()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-link"))
                                .child("Learn More")
                                .on_click(|_, _, cx| cx.open_url(VANITY_HELP_URL)),
                        )
                    })
                    .children(modal.error.as_ref().map(|e| {
                        text(13., 18., 600., "--text-feedback-critical").child(e.clone())
                    })),
            )
            .when(can_manage, |col| {
                let enable_text = format!("{} · {cost}", dc_intl::t("JFfins"));
                let disable_text = format!("{} · {cost}", dc_intl::t("TZsu1U"));
                let _alt_enable = dc_intl::format(
                    "ad2Mfj",
                    &[("boostCount", dc_intl::Arg::Num(f64::from(cost)))],
                );
                let _alt_disable = dc_intl::format(
                    "uzQpQd",
                    &[("boostCount", dc_intl::Arg::Num(f64::from(cost)))],
                );
                let _cant_disable = (dc_intl::t("9oYuvb"), dc_intl::t("WRRYUT"));
                col.child(
                    div()
                        .mt(px(20.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("powerup-perk-toggle")
                                .flex_1()
                                .h(px(40.))
                                .px(px(14.))
                                .rounded(px(8.))
                                .bg(if is_active {
                                    color("--control-secondary-background-default")
                                } else {
                                    color("--control-primary-background-default")
                                })
                                .when(!level_activated, |b| {
                                    b.cursor_pointer().hover(move |s| {
                                        s.bg(if is_active {
                                            color("--control-secondary-background-hover")
                                        } else {
                                            color("--control-primary-background-hover")
                                        })
                                    })
                                })
                                .when(level_activated, |b| b.opacity(0.5))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(px(6.))
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(if is_active {
                                    color("--text-strong")
                                } else {
                                    color("--white")
                                })
                                .child(icon(
                                    "boost_gem",
                                    16.,
                                    if is_active { "--text-muted" } else { "--white" },
                                ))
                                .child(if is_active { disable_text } else { enable_text })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if level_activated {
                                        return;
                                    }
                                    if is_active {
                                        if let Some(m) = this.powerup_perk_modal.as_mut() {
                                            m.confirm_disable = true;
                                        }
                                    } else {
                                        let _ = this.requests.send(toggle_guild_powerup_request(
                                            guild_id, sku_id, true,
                                        ));
                                        if let Some(m) = this.powerup_perk_modal.as_mut() {
                                            m.is_active = true;
                                        }
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(is_active, |row| {
                            row.child(
                                div()
                                    .id("powerup-perk-configure")
                                    .h(px(40.))
                                    .px(px(16.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("g5Ds69"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.powerup_perk_modal = None;
                                        this.open_server_settings_at(
                                            guild_id,
                                            powerup_settings_page_for_sku(sku_id),
                                            cx,
                                        );
                                    })),
                            )
                        }),
                )
            });

        let split_card = div()
            .id("powerup-perk-split-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .relative()
            .w(px(850_f32.min((self.viewport.0 - 48.).max(320.))))
            .h(px(520_f32.min((self.viewport.1 - 48.).max(320.))))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .child(left_col)
            .child(right_demo)
            .child(
                div()
                    .id("powerup-split-close")
                    .absolute()
                    .top(px(14.))
                    .right(px(14.))
                    .size(px(28.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(color("--background-mod-subtle")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.powerup_perk_modal = None;
                        cx.notify();
                    }))
                    .child(icon("close", 16., "--interactive-text-default")),
            );

        Some(
            deferred(
                div()
                    .id("powerup-perk-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.powerup_perk_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(split_card),
            )
            .with_priority(97)
            .into_any_element(),
        )
    }

    /// Capture-harness opener for module 784140 (`powerup-level 1|2|3|active|close`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_powerup_level(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.powerup_level_modal = None;
            cx.notify();
            return "powerup-level: closed".to_owned();
        }
        let guild_id = self.guild.unwrap_or(1);
        let (tier, active) = match mode {
            "1" => (1, false),
            "2" => (2, false),
            "3" => (3, false),
            "active" => (2, true),
            _ => (2, false),
        };
        let mut modal = PowerupLevelModal::for_tier(guild_id, tier, active, true);
        if mode == "disabled" {
            modal.disabled_reason =
                Some("This server needs more available Boosts to enable this level.".to_owned());
        }
        self.powerup_level_modal = Some(modal);
        cx.notify();
        format!("powerup-level: {mode}")
    }

    /// Renders `GuildPowerupLevelModal` (module 784140): tier header (`pob/cL`), 2-col perk grid
    /// (`container_Yo0T3x`, `gap: 12px`, `20px` icon `--text-subtle` + `body-sm`), disabled info notice,
    /// critical error, Enable (`AOQgki`) + `boost_gem` / Disable (`PYPdl4`) + `5HQUzD` tooltip.
    pub(super) fn powerup_level_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.powerup_level_modal.as_ref()?;
        let guild_id = modal.guild_id;
        let sku_id = modal.sku_id.clone();
        let tier = modal.tier.clamp(1, 3);
        let tier_idx = (tier - 1) as usize;
        let (tier_title, _, perks) = TIERS[tier_idx];
        let boost_count = powerup_level_boost_count(tier);
        let subtitle = powerup_level_subtitle(tier);
        let is_active = modal.is_active;
        let can_show_action = modal.can_show_action();
        let disabled_reason = modal.disabled_reason.clone();
        let error = modal.error.clone();
        let confirm_disable = modal.confirm_disable;

        // Module 179413 (`size="md"` 480px): Level deactivation confirmation modal with `.levelContainer_o7nzA2`
        // 2-col perk grid (`870247`) + `.warningContainer_OgsNUm` (`721721`, including extra `M4XL5n` on Level 3).
        if confirm_disable {
            let member_count = self
                .stores
                .member_counts
                .get_member_count(guild_id)
                .unwrap_or(1)
                .max(1);
            let effective_sku = if tier == 3 {
                SKU_BOOST_LEVEL_3
            } else {
                sku_id.as_str()
            };
            let warnings =
                powerup_deactivate_warnings(effective_sku, tier_title, member_count, 0, tier == 3);
            let confirm_title = dc_intl::format("iEBw1M", &[("perk", tier_title.into())]);
            let confirm_subtitle = dc_intl::format("7o0K+2", &[("perk", tier_title.into())]);
            let level_grid = div()
                .w_full()
                .p(px(16.))
                .rounded(px(8.))
                .bg(color("--background-base-lowest"))
                .flex()
                .flex_wrap()
                .gap(px(12.))
                .children(perks.iter().map(|(ico, label)| {
                    div()
                        .w(px(192.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon(ico, 20., "--text-subtle"))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(*label),
                        )
                }));
            let level_confirm = div()
                .id("powerup-level-disable-confirm")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(px(480.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .shadow_xl()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(text(20., 24., 700., "--text-strong").child(confirm_title))
                                .child(
                                    text(14., 20., 400., "--text-subtle").child(confirm_subtitle),
                                ),
                        )
                        .child(
                            div()
                                .id("powerup-level-disable-close")
                                .size(px(28.))
                                .rounded(px(6.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.powerup_level_modal.as_mut() {
                                        m.confirm_disable = false;
                                    }
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--interactive-text-default")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(18.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(level_grid)
                        .child(
                            div()
                                .p(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--background-mod-subtle"))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    text(14., 18., 600., "--text-feedback-critical")
                                        .child(dc_intl::t("OVt5CC")),
                                )
                                .child(div().flex().flex_col().gap(px(2.)).children(
                                    warnings.into_iter().map(|(msg, critical)| {
                                        text(
                                            14.,
                                            18.,
                                            if critical { 600. } else { 400. },
                                            if critical {
                                                "--text-feedback-critical"
                                            } else {
                                                "--text-default"
                                            },
                                        )
                                        .child(msg)
                                    }),
                                )),
                        )
                        .children(error.map(|err| {
                            text(14., 18., 600., "--text-feedback-critical").child(err)
                        })),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(16.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-base-lower"))
                        .rounded_b(px(12.))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("powerup-level-disable-cancel")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--border-muted"))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(dc_intl::t("ETE/oC"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.powerup_level_modal.as_mut() {
                                        m.confirm_disable = false;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("powerup-level-disable-submit")
                                .h(px(38.))
                                .px(px(18.))
                                .rounded(px(8.))
                                .bg(color("--control-critical-primary-background-default"))
                                .hover(|s| {
                                    s.bg(color("--control-critical-primary-background-hover"))
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("PYPdl4"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(toggle_guild_powerup_request(
                                        guild_id, &sku_id, false,
                                    ));
                                    this.powerup_level_modal = None;
                                    cx.notify();
                                })),
                        ),
                );
            return Some(
                deferred(
                    div()
                        .id("powerup-level-disable-backdrop")
                        .occlude()
                        .absolute()
                        .inset_0()
                        .bg(rgba(0x000000b8))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.powerup_level_modal = None;
                                cx.notify();
                            }),
                        )
                        .child(level_confirm),
                )
                .with_priority(97)
                .into_any_element(),
            );
        }

        let perk_grid = div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .children(perks.iter().map(|(ico, label)| {
                div()
                    .w(px(204.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .child(icon(ico, 20., "--text-subtle"))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(*label),
                    )
            }));

        let action_btn = can_show_action.then(|| {
            if is_active {
                let tip_text = powerup_level_disable_tooltip(boost_count);
                let btn = div()
                    .id("powerup-level-disable")
                    .h(px(38.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .bg(if confirm_disable {
                        color("--control-critical-primary-background-default")
                    } else {
                        color("--background-mod-subtle")
                    })
                    .hover(|s| {
                        s.bg(if confirm_disable {
                            color("--control-critical-primary-background-hover")
                        } else {
                            color("--background-mod-strong")
                        })
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .font_weight(FontWeight(600.))
                    .text_color(if confirm_disable {
                        color("--white")
                    } else {
                        color("--text-strong")
                    })
                    .child(dc_intl::t("PYPdl4"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = this.powerup_level_modal.as_mut() {
                            if m.confirm_disable {
                                let _ = this
                                    .requests
                                    .send(toggle_guild_powerup_request(guild_id, &sku_id, false));
                                m.is_active = false;
                                m.confirm_disable = false;
                            } else {
                                m.confirm_disable = true;
                            }
                        }
                        cx.notify();
                    }));
                self.with_tip(
                    "powerup-level-disable-tip",
                    super::tooltip::Tip::body(
                        super::tooltip::Body::Text(tip_text.into()),
                        super::tooltip::Side::Top,
                    ),
                    btn,
                    cx,
                )
                .into_any_element()
            } else {
                let disabled = disabled_reason.is_some();
                div()
                    .id("powerup-level-enable")
                    .h(px(38.))
                    .px(px(18.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .when(!disabled, |b| {
                        b.cursor_pointer()
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this
                                    .requests
                                    .send(toggle_guild_powerup_request(guild_id, &sku_id, true));
                                if let Some(m) = this.powerup_level_modal.as_mut() {
                                    m.is_active = true;
                                    m.error = None;
                                }
                                cx.notify();
                            }))
                    })
                    .when(disabled, |b| b.opacity(0.5))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .child(icon("boost_gem", 16., "--white"))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("AOQgki")),
                    )
                    .into_any_element()
            }
        });

        let card = div()
            .id("powerup-level-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(480.))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(40.))
                                    .rounded_full()
                                    .bg(if is_active {
                                        super::hsla("--guild-boosting-pink-refresh")
                                    } else {
                                        gpui::Hsla::from(color("--background-mod-strong"))
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("boost_gem", 20., "--white")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(tier_title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("powerup-level-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.powerup_level_modal = None;
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--interactive-text-default")),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(perk_grid)
                    .when_some(disabled_reason, |col, reason| {
                        col.child(
                            div()
                                .w_full()
                                .p(px(12.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(icon("circle_info", 16., "--text-subtle"))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(13.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(reason),
                                ),
                        )
                    })
                    .when_some(error, |col, err| {
                        col.child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-feedback-critical"))
                                .child(err),
                        )
                    }),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(14.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("powerup-level-badge-packs")
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-link"))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child("View Badge Packs")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let can_manage = this
                                    .powerup_level_modal
                                    .as_ref()
                                    .is_some_and(|m| m.can_manage);
                                this.powerup_level_modal = None;
                                this.powerup_multi_perk_modal =
                                    Some(PowerupMultiPerkModal::badge_packs(guild_id, can_manage));
                                cx.notify();
                            })),
                    )
                    .child(div().flex().items_center().gap(px(8.)).children(action_btn)),
            );

        Some(
            deferred(
                div()
                    .id("powerup-level-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.powerup_level_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(98)
            .into_any_element(),
        )
    }

    /// Capture-harness opener for module 789267 (`powerup-multi-perk open|close`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_powerup_multi_perk(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.powerup_multi_perk_modal = None;
            cx.notify();
            return "powerup-multi-perk: closed".to_owned();
        }
        let guild_id = self.guild.unwrap_or(1);
        self.powerup_multi_perk_modal = Some(PowerupMultiPerkModal::badge_packs(guild_id, true));
        cx.notify();
        "powerup-multi-perk: open".to_owned()
    }

    /// Renders `GuildPowerupsMultiPerkModal` (module 789267): wrapping row of `220px` cards
    /// (`perkCard_ua3BFY`, `204x140px` image box, optional `"NEW"` pill, description, active/expiring/
    /// removing status or `boost_gem` + `t2Wbo1` `"{required} Boosts"`, hover-revealed deactivate toggle,
    /// `footerInfoText` warning max 220px, optional `footerAction`).
    pub(super) fn powerup_multi_perk_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.powerup_multi_perk_modal.as_ref()?;
        let guild_id = modal.guild_id;
        let can_manage = modal.can_manage;
        let title = modal.title.clone();
        let subtitle = modal.subtitle.clone();
        let footer_info = modal.footer_info.clone();
        let footer_action = modal.footer_action_label.clone();

        let cards = modal.perks.iter().enumerate().map(|(idx, item)| {
            let group = SharedString::from(format!("multi-perk-card-{idx}"));
            let sku_id = item.sku_id.clone();
            let status = item.status;
            let accent_top = rgba((item.accent << 8) | 0xff);
            let accent_bot = rgba(((item.accent >> 1) & 0x7f7f7f) << 8 | 0xff);
            let border = match status {
                MultiPerkStatus::Active => gpui::Hsla::from(color("--status-positive")),
                MultiPerkStatus::Expiring => gpui::Hsla::from(color("--status-warning")),
                MultiPerkStatus::Removing => gpui::Hsla::from(color("--text-feedback-critical")),
                MultiPerkStatus::Inactive => gpui::Hsla::from(color("--border-subtle")),
            };

            let footer_row: AnyElement = match status {
                MultiPerkStatus::Active => div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("checkmark_large", 14., "--status-positive"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--status-positive"))
                                    .child("Active"),
                            ),
                    )
                    .when(can_manage, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("multi-perk-disable-{idx}")))
                                .px(px(8.))
                                .py(px(3.))
                                .rounded(px(6.))
                                .bg(color("--background-mod-subtle"))
                                .hover(|s| s.bg(color("--background-mod-strong")))
                                .cursor_pointer()
                                .invisible()
                                .group_hover(group.clone(), |b| b.visible())
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("PYPdl4"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(toggle_guild_powerup_request(
                                        guild_id, &sku_id, false,
                                    ));
                                    if let Some(m) = this.powerup_multi_perk_modal.as_mut()
                                        && let Some(p) = m.perks.get_mut(idx)
                                    {
                                        p.status = MultiPerkStatus::Inactive;
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .into_any_element(),
                MultiPerkStatus::Expiring => div()
                    .text_size(px(12.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--status-warning"))
                    .child("Expiring Soon")
                    .into_any_element(),
                MultiPerkStatus::Removing => div()
                    .text_size(px(12.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-feedback-critical"))
                    .child("Removing")
                    .into_any_element(),
                MultiPerkStatus::Inactive => div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("boost_gem", 14., "--guild-boosting-pink-refresh"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-subtle"))
                                    .child(multi_perk_cost_label(item.cost)),
                            ),
                    )
                    .when(can_manage, |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("multi-perk-enable-{idx}")))
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("AOQgki"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.requests.send(toggle_guild_powerup_request(
                                        guild_id, &sku_id, true,
                                    ));
                                    if let Some(m) = this.powerup_multi_perk_modal.as_mut()
                                        && let Some(p) = m.perks.get_mut(idx)
                                    {
                                        p.status = MultiPerkStatus::Active;
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .into_any_element(),
            };

            div()
                .id(SharedString::from(format!("multi-perk-card-el-{idx}")))
                .group(group)
                .w(px(220.))
                .p(px(8.))
                .rounded(px(12.))
                .border_1()
                .border_color(border)
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .relative()
                        .w(px(204.))
                        .h(px(140.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(linear_gradient(
                            135.,
                            linear_color_stop(accent_top, 0.),
                            linear_color_stop(accent_bot, 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("boost_gem", 44., "--white"))
                        .when(item.is_new, |box_div| {
                            box_div.child(
                                div()
                                    .absolute()
                                    .top(px(8.))
                                    .right(px(8.))
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(999.))
                                    .bg(color("--control-primary-background-default"))
                                    .text_size(px(10.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child("NEW"),
                            )
                        }),
                )
                .child(
                    div()
                        .px(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(item.title.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(item.description.clone()),
                        ),
                )
                .child(div().px(px(4.)).pt(px(2.)).child(footer_row))
        });

        let card = div()
            .id("powerup-multi-perk-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(740_f32.min((self.viewport.0 - 48.).max(320.))))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(22.))
                    .pb(px(14.))
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .id("powerup-multi-perk-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.powerup_multi_perk_modal = None;
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--interactive-text-default")),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .flex_wrap()
                    .gap(px(14.))
                    .children(cards),
            )
            .child(
                div()
                    .px(px(24.))
                    .py(px(14.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .max_w(px(220.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-feedback-warning"))
                            .children(footer_info),
                    )
                    .when_some(footer_action, |bar, label| {
                        bar.child(
                            div()
                                .id("powerup-multi-perk-footer-action")
                                .h(px(38.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.powerup_multi_perk_modal = None;
                                    this.open_server_settings_at(
                                        guild_id,
                                        super::server_settings::Page::Tag,
                                        cx,
                                    );
                                })),
                        )
                    }),
            );

        Some(
            deferred(
                div()
                    .id("powerup-multi-perk-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.powerup_multi_perk_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(98)
            .into_any_element(),
        )
    }
}

/// Module 148700: Guild Powerups & Game Servers coachmark popovers (all 11 official exports).
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PowerupCoachmarkKind {
    LevelUp {
        perk_name: String,
    },
    NewPerks,
    PurchasablePerks {
        guild_name: String,
        perk_names: Vec<String>,
        single_sku_id: Option<String>,
    },
    NewPerkAvailable {
        sku_id: String,
    },
    GameServerHosting {
        available_boosts: u32,
    },
    GameServersEnabled,
    BoostForGameServers,
    GuildThemeMember {
        needed_boosts: u32,
    },
    NewTitlesGameServers {
        is_admin: bool,
    },
    BoostToUnlock {
        perk_name: String,
        needed_boosts: u32,
    },
    ExpiringPerk {
        perk_name: Option<String>,
        is_game_server: bool,
        expiring_boosts: u32,
        days_until_expiry: u32,
    },
}

/// Module 148700 (`bo` / `X`): official SKU priority order for New Perk Available coachmark.
pub(crate) const SKU_BADGE_WAVE2_A: &str = "1466209416922667288";
pub(crate) const SKU_BADGE_WAVE2_B: &str = "1466209416931055898";
pub(crate) const SKU_BADGE_WAVE1_A: &str = "1395150519886024775";
pub(crate) const SKU_BADGE_WAVE1_B: &str = "1395150923734581339";
pub(crate) const SKU_UPLOADS_250MB: &str = "1479204648718958665";

pub(crate) const FEATURED_GAME_1: &str = "Windrose";
pub(crate) const FEATURED_GAME_2: &str = "Factorio";
pub(crate) const FEATURED_GAME_3: &str = "V Rising";

/// Module 148700 (`bo` / `X`): picks the highest-priority newly available powerup SKU.
#[allow(dead_code)]
pub(crate) fn select_new_perk_coachmark_sku<'a>(available_skus: &[&'a str]) -> Option<&'a str> {
    let priority = [
        SKU_GUILD_THEME,
        SKU_VANITY_URL,
        SKU_BADGE_WAVE2_A,
        SKU_BADGE_WAVE2_B,
        SKU_BADGE_WAVE1_A,
        SKU_BADGE_WAVE1_B,
        SKU_UPLOADS_250MB,
    ];
    priority
        .into_iter()
        .find(|sku| available_skus.contains(sku))
}

/// Module 148700: open pink-gradient coachmark popover state.
#[derive(Clone, Debug)]
pub(crate) struct PowerupCoachmarkState {
    pub guild_id: Id,
    pub kind: PowerupCoachmarkKind,
}

fn pc_copy(key: &str, fallback: &str) -> String {
    if dc_intl::has_key(key) {
        let s = dc_intl::t(key);
        if !s.trim().is_empty() {
            return s.to_owned();
        }
    }
    fallback.to_owned()
}

/// Module 148700: resolves `(title, body, primary_cta)` for each of the 11 official coachmarks.
pub(crate) fn powerup_coachmark_copy(kind: &PowerupCoachmarkKind) -> (String, String, String) {
    use dc_intl::Arg;
    use std::borrow::Cow;
    match kind {
        PowerupCoachmarkKind::LevelUp { perk_name } => (
            if dc_intl::has_key("Zg/m9K") {
                dc_intl::format(
                    "Zg/m9K",
                    &[("perkName", Arg::Str(Cow::Borrowed(perk_name)))],
                )
            } else {
                format!("You've reached {perk_name}")
            },
            if dc_intl::has_key("1EGXSK") {
                dc_intl::format(
                    "1EGXSK",
                    &[("perkName", Arg::Str(Cow::Borrowed(perk_name)))],
                )
            } else {
                format!(
                    "Congrats! Your server now has full access to all of the {perk_name} perks."
                )
            },
            pc_copy("RzWDqY", "Check it out"),
        ),
        PowerupCoachmarkKind::NewPerks => (
            pc_copy("QpQBPQ", "New Perks. New Possibilities."),
            pc_copy(
                "6hn0xF",
                "Use Boosts to unlock a Server Tag and Enhanced Role Styles!",
            ),
            pc_copy("RzWDqY", "Check it out"),
        ),
        PowerupCoachmarkKind::PurchasablePerks {
            guild_name,
            perk_names,
            single_sku_id,
        } => {
            let title = if dc_intl::has_key("LmpChE") {
                dc_intl::format(
                    "LmpChE",
                    &[("guildName", Arg::Str(Cow::Borrowed(guild_name)))],
                )
            } else {
                format!("{guild_name} has available Boosts!")
            };
            let body = match perk_names.as_slice() {
                [p] => {
                    if dc_intl::has_key("ZF8NT6") {
                        dc_intl::format("ZF8NT6", &[("perk", Arg::Str(Cow::Borrowed(p)))])
                    } else {
                        format!("Use them to unlock {p}!")
                    }
                }
                [p1, p2, p3, ..] => {
                    if dc_intl::has_key("6Sv+3M") {
                        dc_intl::format(
                            "6Sv+3M",
                            &[
                                ("perk", Arg::Str(Cow::Borrowed(p1))),
                                ("perk2", Arg::Str(Cow::Borrowed(p2))),
                                ("perk3", Arg::Str(Cow::Borrowed(p3))),
                            ],
                        )
                    } else {
                        format!("Use them to unlock perks like {p1}, {p2}, and {p3}!")
                    }
                }
                _ => {
                    let joined = perk_names.join(", ");
                    if dc_intl::has_key("wcQOqC") {
                        dc_intl::format("wcQOqC", &[("perks", Arg::Str(Cow::Borrowed(&joined)))])
                    } else {
                        format!("Use them to unlock perks like {joined}!")
                    }
                }
            };
            let cta = if single_sku_id.is_some() {
                pc_copy("gSxlHf", "Enable")
            } else {
                pc_copy("RzWDqY", "Check it out")
            };
            (title, body, cta)
        }
        PowerupCoachmarkKind::NewPerkAvailable { sku_id } => match sku_id.as_str() {
            SKU_GUILD_THEME => (
                pc_copy("RK6NbY", "This server could have its own look"),
                pc_copy(
                    "xlAqGk",
                    "Admins can now use Boosts to give this server a custom theme.",
                ),
                pc_copy("RzWDqY", "Check it out"),
            ),
            SKU_VANITY_URL => (
                pc_copy("Ygpx4Q", "Customize This Server's URL!"),
                pc_copy(
                    "mmNkUA",
                    "Get a Custom Invite Link with Boosts - no level required! Perfect for growing your server.",
                ),
                pc_copy("RzWDqY", "Check it out"),
            ),
            SKU_UPLOADS_250MB => (
                pc_copy("rp0Ff1", "Unlock 250MB uploads"),
                pc_copy(
                    "3L/DZq",
                    "Use Boosts to unlock bigger file sharing for the whole server.",
                ),
                pc_copy("RzWDqY", "Check it out"),
            ),
            _ => (
                pc_copy("kA2c+n", "New Badge Packs!"),
                pc_copy(
                    "TUilLj",
                    "Use Boosts to unlock special, themed badge packs for your Server Tag!",
                ),
                pc_copy("RzWDqY", "Check it out"),
            ),
        },
        PowerupCoachmarkKind::GameServerHosting { available_boosts } => (
            pc_copy("wXLChx", "Host your own game server"),
            if *available_boosts > 0 && dc_intl::has_key("IQ1E+d") {
                dc_intl::format(
                    "IQ1E+d",
                    &[("boostCount", Arg::Num(f64::from(*available_boosts)))],
                )
            } else if dc_intl::has_key("7KXp9J") {
                dc_intl::format(
                    "7KXp9J",
                    &[
                        ("gameName", Arg::Str(Cow::Borrowed(FEATURED_GAME_1))),
                        ("gameName2", Arg::Str(Cow::Borrowed(FEATURED_GAME_2))),
                    ],
                )
            } else {
                format!(
                    "Spin up a server and start playing in seconds using Boosts! Pick from titles like {FEATURED_GAME_1}, {FEATURED_GAME_2}, and more."
                )
            },
            pc_copy("RzWDqY", "Check it out"),
        ),
        PowerupCoachmarkKind::GameServersEnabled => (
            pc_copy("eX64+z", "Game servers enabled"),
            pc_copy(
                "NpgfEB",
                "Visit the Game Servers tab to jump in and start playing together!",
            ),
            pc_copy("RzWDqY", "Check it out"),
        ),
        PowerupCoachmarkKind::BoostForGameServers => (
            pc_copy("t3LNW1", "Boost for Game Servers"),
            if dc_intl::has_key("V9qFAU") {
                dc_intl::format(
                    "V9qFAU",
                    &[
                        ("gameName", Arg::Str(Cow::Borrowed(FEATURED_GAME_1))),
                        ("gameName2", Arg::Str(Cow::Borrowed(FEATURED_GAME_2))),
                    ],
                )
            } else {
                format!(
                    "Pitch in to help unlock a game server for {FEATURED_GAME_1}, {FEATURED_GAME_2}, or other supported games."
                )
            },
            pc_copy("oPAx73", "Boost This Server"),
        ),
        PowerupCoachmarkKind::GuildThemeMember { needed_boosts } => (
            pc_copy("RK6NbY", "This server could have its own look"),
            format!(
                "Add {} more Boost(s) to help unlock Server Theme.",
                needed_boosts
            ),
            pc_copy("oPAx73", "Boost This Server"),
        ),
        PowerupCoachmarkKind::NewTitlesGameServers { is_admin } => (
            pc_copy("wy+j5s", "New titles for Game Servers"),
            if dc_intl::has_key("7OETrT") {
                dc_intl::format(
                    "7OETrT",
                    &[
                        ("gameName", Arg::Str(Cow::Borrowed(FEATURED_GAME_1))),
                        ("gameName2", Arg::Str(Cow::Borrowed(FEATURED_GAME_2))),
                        ("gameName3", Arg::Str(Cow::Borrowed(FEATURED_GAME_3))),
                    ],
                )
            } else {
                format!(
                    "Spin up game servers for {FEATURED_GAME_1}, {FEATURED_GAME_2}, {FEATURED_GAME_3} and other titles using Boosts."
                )
            },
            if *is_admin {
                pc_copy("k0Y0BE", "Check it out")
            } else {
                pc_copy("oPAx73", "Boost This Server")
            },
        ),
        PowerupCoachmarkKind::BoostToUnlock {
            perk_name,
            needed_boosts,
        } => (
            pc_copy("n37JhA", "You're almost there!"),
            if dc_intl::has_key("Yr1ogl") {
                dc_intl::format(
                    "Yr1ogl",
                    &[
                        ("boostCount", Arg::Num(f64::from(*needed_boosts))),
                        ("perkName", Arg::Str(Cow::Borrowed(perk_name))),
                    ],
                )
            } else {
                format!("Just {needed_boosts} more Boost(s) to unlock {perk_name} for everyone.")
            },
            pc_copy("oPAx73", "Boost This Server"),
        ),
        PowerupCoachmarkKind::ExpiringPerk {
            perk_name,
            is_game_server,
            expiring_boosts,
            days_until_expiry,
        } => {
            let title = if *is_game_server {
                pc_copy("9L0pAN", "A game server is expiring soon")
            } else {
                let name = perk_name.as_deref().unwrap_or("Perk");
                if dc_intl::has_key("gG8bI8") {
                    dc_intl::format("gG8bI8", &[("perkName", Arg::Str(Cow::Borrowed(name)))])
                } else {
                    format!("{name} expiring soon")
                }
            };
            let body = if *days_until_expiry == 0 && dc_intl::has_key("BNS5zl") {
                dc_intl::format(
                    "BNS5zl",
                    &[("boostCount", Arg::Num(f64::from(*expiring_boosts)))],
                )
            } else if dc_intl::has_key("Xla/TL") {
                dc_intl::format(
                    "Xla/TL",
                    &[
                        ("boostCount", Arg::Num(f64::from(*expiring_boosts))),
                        ("days", Arg::Num(f64::from(*days_until_expiry))),
                    ],
                )
            } else {
                format!(
                    "Add {expiring_boosts} Boost(s) before it expires in {days_until_expiry} day(s) to keep it active."
                )
            };
            (title, body, pc_copy("oPAx73", "Boost This Server"))
        }
    }
}

impl Root {
    /// Module 148700: renders the active pink-gradient Guild Powerups / Game Servers coachmark popover.
    pub(super) fn powerup_coachmark_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.powerup_coachmark.as_ref()?;
        let (title, body, cta) = powerup_coachmark_copy(&state.kind);
        let guild_id = state.guild_id;
        let single_sku = match &state.kind {
            PowerupCoachmarkKind::PurchasablePerks { single_sku_id, .. } => single_sku_id.clone(),
            _ => None,
        };
        Some(
            gpui::deferred(
                div()
                    .id("powerup-coachmark-popover")
                    .occlude()
                    .absolute()
                    .left(px(320.))
                    .top(px(140.))
                    .w(px(320.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(gpui::linear_gradient(
                        135.,
                        gpui::linear_color_stop(rgba(0x4a2252ff), 0.),
                        gpui::linear_color_stop(rgba(0x232428ff), 1.),
                    ))
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(108.))
                            .w_full()
                            .bg(gpui::linear_gradient(
                                135.,
                                gpui::linear_color_stop(rgba(0xeb459e66), 0.),
                                gpui::linear_color_stop(rgba(0x5865f255), 1.),
                            ))
                            .relative()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("boost_gem", 40., "--white"))
                            .child(
                                div()
                                    .id("powerup-coachmark-close")
                                    .absolute()
                                    .top(px(8.))
                                    .right(px(8.))
                                    .size(px(26.))
                                    .rounded(px(6.))
                                    .bg(rgba(0x00000055))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.powerup_coachmark = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 14., "--white")),
                            ),
                    )
                    .child(
                        div()
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(21.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(body),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .id("powerup-coachmark-cta")
                                    .w_full()
                                    .h(px(36.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .child(cta)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if let Some(sku) = single_sku.as_deref() {
                                            let _ = this.requests.send(
                                                toggle_guild_powerup_request(guild_id, sku, true),
                                            );
                                        } else {
                                            this.open_boost_page(cx);
                                        }
                                        this.powerup_coachmark = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .with_priority(98)
            .into_any_element(),
        )
    }

    /// Module 235780 ("How Boosting Works"): opens the XXL marketing video modal with WebVTT caption support.
    pub(crate) fn open_how_boosting_works(&mut self, guild_id: Option<Id>, cx: &mut Context<Self>) {
        self.request_image(
            "boost:thumbnail".to_owned(),
            BOOST_MARKETING_POSTER_URL.to_owned(),
        );
        self.how_boosting_works = Some(HowBoostingWorksModal {
            guild_id,
            playing: true,
            captions_enabled: boost_marketing_captions_url(dc_intl::locale()).is_some(),
            position_ms: 1_500,
            cues: Vec::new(),
            boost_purchasing_unavailable: false,
        });
        cx.notify();
    }

    /// Module 235780 (`size="xxl"` max 1280px, CSS `401146` `.video_U5duvb`, `0f1VCH` / `gKmQ1G` / `BMx1iy`):
    /// Renders the "How Boosting Works" marketing video modal with poster, play/pause, WebVTT caption track
    /// overlay & CC toggle, and "Boost This Server" / "Boost any server" footer CTA.
    pub(super) fn how_boosting_works_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.how_boosting_works.as_ref()?;
        let has_guild = modal
            .guild_id
            .and_then(|gid| self.stores.guild(gid))
            .is_some();
        let guild_id = modal.guild_id;
        let playing = modal.playing;
        let captions_on = modal.captions_enabled;
        let active_caption = captions_on
            .then(|| active_webvtt_cue(&modal.cues, modal.position_ms))
            .flatten()
            .map(str::to_owned);
        let modal_w = (self.viewport.0 * 0.8).clamp(520., 1280.);
        let video_h = ((modal_w - 48.) * 9. / 16.).min((self.viewport.1 - 210.).max(220.));
        let poster = self.images.get("boost:thumbnail").cloned();

        Some(
            deferred(
                div()
                    .id("how-boosting-works-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000c8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.how_boosting_works = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("how-boosting-works-modal")
                            .aria_label(SharedString::from(dc_intl::t("0f1VCH")))
                            .occlude()
                            .w(px(modal_w))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .shadow_xl()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .child(
                                div()
                                    .px(px(24.))
                                    .pt(px(18.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        text(20., 24., 700., "--text-strong")
                                            .child(dc_intl::t("0f1VCH")),
                                    )
                                    .child(
                                        div()
                                            .id("how-boosting-works-close")
                                            .size(px(30.))
                                            .rounded(px(6.))
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "modal_close",
                                                18.,
                                                "--interactive-text-default",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.how_boosting_works = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // `.video_U5duvb`: 16:9 video stage w/ poster, WebVTT caption bar, and controls
                            .child(
                                div().px(px(24.)).pb(px(16.)).child(
                                    div()
                                        .id("how-boosting-works-player")
                                        .relative()
                                        .w_full()
                                        .h(px(video_h))
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .bg(gpui::rgb(0x111214))
                                        .children(poster.map(|im| {
                                            img(im)
                                                .absolute()
                                                .inset_0()
                                                .size_full()
                                                .object_fit(ObjectFit::Cover)
                                        }))
                                        .child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .bg(rgba(0x00000044))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(
                                                    div()
                                                        .id("hbw-play-toggle")
                                                        .size(px(64.))
                                                        .rounded_full()
                                                        .bg(rgba(0x00000099))
                                                        .hover(|s| s.bg(rgba(0x000000cc)))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(
                                                            gpui::svg()
                                                                .path("icons/boost_circle_play.svg")
                                                                .size(px(40.))
                                                                .text_color(gpui::rgb(0xffffff)),
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            if let Some(m) =
                                                                this.how_boosting_works.as_mut()
                                                            {
                                                                m.playing = !m.playing;
                                                            }
                                                            cx.notify();
                                                        })),
                                                ),
                                        )
                                        .children(active_caption.map(|caption| {
                                            div()
                                                .absolute()
                                                .left(px(32.))
                                                .right(px(32.))
                                                .bottom(px(46.))
                                                .flex()
                                                .justify_center()
                                                .child(
                                                    div()
                                                        .px(px(12.))
                                                        .py(px(6.))
                                                        .rounded(px(6.))
                                                        .bg(rgba(0x000000cc))
                                                        .text_center()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--white"))
                                                        .child(caption),
                                                )
                                        }))
                                        .child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .right_0()
                                                .bottom_0()
                                                .h(px(36.))
                                                .px(px(12.))
                                                .bg(rgba(0x000000b3))
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color("--white"))
                                                        .child(if playing {
                                                            "Playing · Server Boosting"
                                                        } else {
                                                            "Paused · Server Boosting"
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(8.))
                                                        .child(
                                                            div()
                                                                .id("hbw-captions-toggle")
                                                                .px(px(8.))
                                                                .py(px(3.))
                                                                .rounded(px(4.))
                                                                .bg(if captions_on {
                                                                    color(
                                                                        "--control-primary-background-default",
                                                                    )
                                                                } else {
                                                                    rgba(0xffffff22)
                                                                })
                                                                .cursor_pointer()
                                                                .text_size(px(11.))
                                                                .font_weight(FontWeight(700.))
                                                                .text_color(color("--white"))
                                                                .child("CC")
                                                                .on_click(cx.listener(
                                                                    |this, _, _, cx| {
                                                                        if let Some(m) = this
                                                                            .how_boosting_works
                                                                            .as_mut()
                                                                        {
                                                                            m.captions_enabled =
                                                                                !m.captions_enabled;
                                                                        }
                                                                        cx.notify();
                                                                    },
                                                                )),
                                                        )
                                                        .child(
                                                            div()
                                                                .id("hbw-open-video-url")
                                                                .px(px(8.))
                                                                .py(px(3.))
                                                                .rounded(px(4.))
                                                                .bg(rgba(0xffffff22))
                                                                .cursor_pointer()
                                                                .text_size(px(11.))
                                                                .text_color(color("--white"))
                                                                .child("MP4")
                                                                .on_click(cx.listener(
                                                                    |_, _, _, cx| {
                                                                        cx.open_url(
                                                                            BOOST_MARKETING_VIDEO_URL,
                                                                        );
                                                                    },
                                                                )),
                                                        ),
                                                ),
                                        ),
                                ),
                            )
                            // Footer (`696208.H`): Boost CTA or module 338548 Unavailable pill
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .bg(color("--background-base-lower"))
                                    .rounded_b(px(12.))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .child(if modal.boost_purchasing_unavailable {
                                        render_boost_unavailable_pill(self, true, false, cx)
                                    } else {
                                        div()
                                            .id("hbw-boost-cta")
                                            .h(px(38.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(if has_guild {
                                                gpui::rgba(0xe84894ff)
                                            } else {
                                                color("--control-primary-background-default")
                                            })
                                            .hover(|s| s.opacity(0.92))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(6.))
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .when(has_guild, |d| {
                                                d.child(icon("boost_gem", 16., "--white"))
                                            })
                                            .child(dc_intl::t(if has_guild {
                                                "gKmQ1G"
                                            } else {
                                                "BMx1iy"
                                            }))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.how_boosting_works = None;
                                                if let Some(gid) = guild_id
                                                    && this.stores.guild(gid).is_some()
                                                {
                                                    this.boost_this_server(gid, cx);
                                                } else {
                                                    this.open_settings_section("Server Boost", cx);
                                                }
                                                cx.notify();
                                            }))
                                            .into_any_element()
                                    }),
                            ),
                    ),
            )
            .with_priority(98)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_how_boosting_works(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.how_boosting_works = None;
            cx.notify();
            return "how-boosting-works: closed".to_owned();
        }
        let guild_id = if arg.trim() == "any" {
            None
        } else {
            self.guild
                .or_else(|| self.stores.guilds.first().map(|g| g.id))
        };
        self.open_how_boosting_works(guild_id, cx);
        if let Some(m) = self.how_boosting_works.as_mut() {
            m.captions_enabled = true;
            m.boost_purchasing_unavailable = arg.trim() == "unavailable";
            m.cues = parse_webvtt_cues(
                "WEBVTT\n\n00:00:00.000 --> 00:00:05.000\nBoost servers to unlock perks for everyone.\n",
            );
        }
        cx.notify();
        format!("how-boosting-works: opened ({})", arg.trim())
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_powerup_coachmark(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        if arg.trim() == "close" {
            self.powerup_coachmark = None;
            cx.notify();
            return "powerup-coachmark: closed".to_owned();
        }
        let kind = match arg.trim() {
            "level-up" => PowerupCoachmarkKind::LevelUp {
                perk_name: "Level 2".to_owned(),
            },
            "purchasable" => PowerupCoachmarkKind::PurchasablePerks {
                guild_name: "gravecord fixture".to_owned(),
                perk_names: vec!["Server Theme".to_owned()],
                single_sku_id: Some(SKU_GUILD_THEME.to_owned()),
            },
            "expiring" => PowerupCoachmarkKind::ExpiringPerk {
                perk_name: Some("Server Tag".to_owned()),
                is_game_server: false,
                expiring_boosts: 2,
                days_until_expiry: 3,
            },
            _ => PowerupCoachmarkKind::GameServerHosting {
                available_boosts: 4,
            },
        };
        self.powerup_coachmark = Some(PowerupCoachmarkState {
            guild_id: self.guild.unwrap_or(1),
            kind,
        });
        cx.notify();
        format!("powerup-coachmark: opened ({})", arg.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nitro_button_is_a_gift_only_for_nitro() {
        use dc_state::premium::PremiumType;
        assert!(!offers_gift(PremiumType::None));
        assert!(!offers_gift(PremiumType::Basic));
        assert!(!offers_gift(PremiumType::Classic));
        assert!(offers_gift(PremiumType::Nitro));
    }

    #[test]
    fn the_table_has_a_value_for_every_tier() {
        assert_eq!(TABLE.len(), 12);
        assert_eq!(TABLE[0].0, "Emoji Slots");
        assert!(matches!(TABLE[11].1[3], Cell::Yes));
        assert!(matches!(TABLE[11].1[2], Cell::No));
    }

    #[test]
    fn one_faq_row_is_open_at_a_time_and_a_second_press_closes_it() {
        assert_eq!(BoostPage::new().faq_open, None);
        let open = toggled_faq(None, 2);
        assert_eq!(open, Some(2));
        assert_eq!(toggled_faq(open, 4), Some(4));
        assert_eq!(toggled_faq(Some(4), 4), None);
        assert!(FAQ.iter().all(|(_, answer, _)| !answer.is_empty()));
    }

    #[test]
    fn powerup_coachmarks_148700_priority_requests_and_intl() {
        for key in [
            "RzWDqY", "oPAx73", "Zg/m9K", "QpQBPQ", "LmpChE", "gSxlHf", "wXLChx", "eX64+z",
            "t3LNW1", "wy+j5s", "n37JhA", "9L0pAN", "gG8bI8",
        ] {
            assert!(dc_intl::has_key(key), "missing 148700 key {key}");
        }
        assert_eq!(
            select_new_perk_coachmark_sku(&[SKU_UPLOADS_250MB, SKU_VANITY_URL, SKU_GUILD_THEME]),
            Some(SKU_GUILD_THEME)
        );
        assert_eq!(
            select_new_perk_coachmark_sku(&[SKU_UPLOADS_250MB, SKU_BADGE_WAVE1_A]),
            Some(SKU_BADGE_WAVE1_A)
        );
        assert!(matches!(
            toggle_guild_powerup_request(77, SKU_GUILD_THEME, true),
            crate::net::Request::Rest { route, .. } if route == format!("/guilds/77/skus/{SKU_GUILD_THEME}")
        ));
        let (t, b, c) = powerup_coachmark_copy(&PowerupCoachmarkKind::GameServerHosting {
            available_boosts: 3,
        });
        assert!(!t.is_empty() && !b.is_empty() && !c.is_empty());
    }

    #[test]
    fn gradient_stops_interpolate_premultiplied_and_extend_past_the_end() {
        let stops: [Stop; 3] = [
            (0., [156., 89., 209., 0.6]),
            (0.5, [88., 47., 140., 0.3]),
            (0.8, [0., 0., 0., 0.]),
        ];
        let first = stop_color(&stops, 0.);
        assert!((first[3] - 0.6).abs() < 1e-6 && (first[0] - 156. * 0.6).abs() < 1e-3);
        let middle = stop_color(&stops, 0.65);
        // Halfway from the second stop to transparent: premultiplied values and alpha halve.
        assert!((middle[3] - 0.15).abs() < 1e-6 && (middle[0] - 88. * 0.3 / 2.).abs() < 1e-3);
        assert_eq!(stop_color(&stops, 3.), [0.; 4]);
    }

    #[test]
    fn a_radial_image_is_clear_beyond_its_last_stop_and_straight_alpha_at_the_centre() {
        let stops: [Stop; 2] = [(0., [86., 22., 138., 0.5]), (1., [86., 22., 138., 0.])];
        let image = radial_image((4, 4), (2., 0.), (4., 4.), &stops);
        let frame = image.as_bytes(0).unwrap();
        // The top row's third pixel is 0.177 of the way out: it keeps its colour (BGRA) and 0.41 of alpha.
        let near = 2 * 4;
        assert!((i32::from(frame[near + 3]) - 105).abs() <= 3);
        assert!(
            (i32::from(frame[near]) - 138).abs() <= 1
                && (i32::from(frame[near + 2]) - 86).abs() <= 1
        );
        // The far bottom-right pixel is almost clear.
        assert!(frame[(3 * 4 + 3) * 4 + 3] < 40);
    }

    #[test]
    fn powerup_card_footer_722523_actions_and_routing() {
        assert_eq!(
            powerup_card_action(true, true, true, true, false),
            PowerupCardAction::Configure
        );
        assert_eq!(
            powerup_card_action(true, true, false, true, false),
            PowerupCardAction::ViewPerks
        );
        assert_eq!(
            powerup_card_action(true, false, true, true, false),
            PowerupCardAction::ToggleBoost
        );
        assert_eq!(
            powerup_card_action(false, false, true, true, true),
            PowerupCardAction::ViewPerks
        );
        assert_eq!(
            powerup_card_action(false, false, true, true, false),
            PowerupCardAction::ViewPerk
        );
        assert_eq!(dc_intl::t("g5Ds69"), "Configure");
        assert_eq!(dc_intl::t("0Q61kF"), "View Perks");
        assert_eq!(dc_intl::t("Xa11Ep"), "View Perk");
        assert_eq!(
            powerup_settings_page_for_sku(SKU_SERVER_TAG),
            crate::ui::server_settings::Page::Tag
        );
        assert_eq!(
            powerup_settings_page_for_sku(SKU_ROLE_STYLES),
            crate::ui::server_settings::Page::Roles
        );
    }

    #[test]
    fn powerup_level_784140_and_multi_perk_789267_helpers() {
        assert_eq!(powerup_level_boost_count(1), 2);
        assert_eq!(powerup_level_boost_count(2), 7);
        assert_eq!(powerup_level_boost_count(3), 14);
        assert!(powerup_level_subtitle(1).contains('2'));
        assert!(powerup_level_subtitle(2).contains('7'));
        assert!(powerup_level_subtitle(3).contains("14"));
        let tip = powerup_level_disable_tooltip(7);
        assert!(tip.contains('7'), "got {tip}");
        let mut m = PowerupLevelModal::for_tier(42, 2, false, true);
        assert!(m.can_show_action());
        m.is_tier_override = true;
        assert!(!m.can_show_action());
        assert_eq!(dc_intl::t("AOQgki"), "Enable");
        assert_eq!(dc_intl::t("PYPdl4"), "Disable");
        let cost_lbl = multi_perk_cost_label(3);
        assert!(cost_lbl.contains('3'), "got {cost_lbl}");
        let packs = PowerupMultiPerkModal::badge_packs(42, true);
        assert_eq!(packs.perks.len(), 3);
        assert!(packs.perks.iter().any(|p| p.is_new));
    }

    #[test]
    fn powerup_deactivate_179413_warnings_and_intl() {
        for key in [
            "iEBw1M", "7o0K+2", "ETE/oC", "PYPdl4", "OVt5CC", "4jSvr1", "cavtEo", "hN75yb",
            "Du91Rb", "Vf2ZcR", "M4XL5n", "NXvV0+", "ZEvvPz", "s9u/E7",
        ] {
            assert!(dc_intl::has_key(key), "missing 179413 key {key}");
        }
        let role_with = powerup_deactivate_warnings(SKU_ROLE_STYLES, "Role Styles", 50, 4, false);
        assert_eq!(role_with.len(), 1);
        assert!(role_with[0].0.contains('4'));
        let role_none = powerup_deactivate_warnings(SKU_ROLE_STYLES, "Role Styles", 50, 0, false);
        assert!(role_none[0].0.contains("all members"));
        let vanity_set = powerup_deactivate_warnings(SKU_VANITY_URL, "Vanity", 50, 0, true);
        assert!(vanity_set[0].1);
        assert_eq!(vanity_set[0].0, dc_intl::t("hN75yb"));
        let vanity_unset = powerup_deactivate_warnings(SKU_VANITY_URL, "Vanity", 50, 0, false);
        assert_eq!(vanity_unset[0].0, dc_intl::t("Du91Rb"));
        let pack = powerup_deactivate_warnings(SKU_TAG_BADGE_PACKS[0], "Badges", 50, 0, false);
        assert_eq!(pack[0].0, dc_intl::t("Vf2ZcR"));
        let lvl3 = powerup_deactivate_warnings(SKU_BOOST_LEVEL_3, "Level 3", 50, 0, true);
        assert_eq!(lvl3.len(), 2);
        assert!(lvl3[1].1);
        assert_eq!(lvl3[1].0, dc_intl::t("M4XL5n"));
    }

    #[test]
    fn how_boosting_works_235780_vtt_and_intl() {
        for key in ["0f1VCH", "gKmQ1G", "BMx1iy"] {
            assert!(dc_intl::has_key(key), "missing 235780 key {key}");
        }
        assert_eq!(boost_marketing_captions_url("en-US"), None);
        assert_eq!(boost_marketing_captions_url("en-GB"), None);
        let tr = boost_marketing_captions_url("tr").expect("non-en captions url");
        assert!(tr.ends_with("boosting_marketing_video_captions_tr.vtt"));
        let cues = parse_webvtt_cues(
            "WEBVTT\n\n00:00:01.000 --> 00:00:04.500\nUnlock custom perks\nfor your server!\n\n00:01:00.000 --> 00:01:03.000\nBoost today\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(
            active_webvtt_cue(&cues, 2_000),
            Some("Unlock custom perks\nfor your server!")
        );
        assert_eq!(active_webvtt_cue(&cues, 61_000), Some("Boost today"));
        assert_eq!(active_webvtt_cue(&cues, 10_000), None);
    }

    #[test]
    fn boost_unavailable_338548_tooltip_and_intl() {
        for key in ["5nrJDO", "5xN/C1", "aFBQ3d"] {
            assert!(dc_intl::has_key(key), "missing 338548 key {key}");
        }
        let tip = boost_unavailable_tooltip_text();
        assert!(tip.contains(dc_intl::t("aFBQ3d")));
    }
}
