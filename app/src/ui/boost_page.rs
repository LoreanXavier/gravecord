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

/// Guild powerup SKU IDs (`512750` / `945942.js`).
pub(super) const SKU_SERVER_TAG: &str = "1351706802684952639";
pub(super) const SKU_ROLE_STYLES: &str = "1354906318279807056";
pub(super) const SKU_VANITY_URL: &str = "1387197800336330924";
pub(super) const SKU_GUILD_THEME: &str = "1493634428604252161";

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

/// Builds `POST` or `DELETE /guilds/{guildId}/skus/{skuId}` (`GUILD_POWERUP_TOGGLE`, module 945942 / 533993).
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
                    .child(self.boost_tier_cards()),
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
    fn boost_tier_cards(&self) -> AnyElement {
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
                            .overflow_hidden()
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

        // `933337` / `179413`: Deactivate Perk Confirmation modal (`size="sm"` 400px).
        if modal.confirm_disable {
            let warning_copy = if sku_id == SKU_VANITY_URL {
                dc_intl::t("M4XL5n").to_owned()
            } else {
                dc_intl::format("cavtEo", &[("perk", modal.title.as_str().into())])
            };
            let confirm_card = div()
                .id("powerup-disable-confirm")
                .occlude()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .w(px(400.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-base-low"))
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(text(18., 22., 700., "--text-strong").child(dc_intl::t("TZsu1U")))
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
                                .child(icon("close", 16., "--interactive-text-default")),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(18.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .p(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--status-warning"))
                                .bg(rgba(0xf0b2321a))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    text(13., 18., 700., "--status-warning")
                                        .child(dc_intl::t("OVt5CC")),
                                )
                                .child(text(13., 18., 400., "--text-default").child(warning_copy)),
                        )
                        .child(text(13., 18., 500., "--text-muted").child(dc_intl::format(
                            "5HQUzD",
                            &[("boostCount", dc_intl::Arg::Num(f64::from(cost)))],
                        ))),
                )
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
                        .gap(px(8.))
                        .child(
                            div()
                                .id("powerup-disable-cancel")
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
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
                                .h(px(36.))
                                .px(px(16.))
                                .rounded(px(8.))
                                .bg(color("--status-danger"))
                                .hover(|s| s.opacity(0.9))
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
                                        this.open_server_settings(guild_id, cx);
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
            .w(px(850.))
            .h(px(520.))
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
}
