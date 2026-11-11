//! The Server Boost page the server dropdown's "Server Boost" row and the channel list's boost bar open: a full
//! window page (module 355097's `GuildBoostingModal` family; layout classes `*_mQ3T6n`, `*_0jpYct`, `*_xZJtlG`,
//! `*_RL7BaR`, `*_twYJsJ`, `*_c_ash1`, `*_EVXXpI` of `196063.6435217f69e9b808.css`). Measured in
//! `design/dom/boost/dropdown-boost-modal-*-darker-cozy.json`.
//!
//! Not drawn: the header's animated backdrop (a looping `.webm` of floating gems), the video card's picture and the
//! FAQ answers (the frames show every answer collapsed). The tier cards use the existing perk icons in place of the
//! per-perk art. "Boost This Server" goes as far as the existing Server Boost settings page and never past payment.

use super::discover::{gutter_scrollbar, text};
use super::{Root, color, icon};
use crate::parity::Parity;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ScrollHandle, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};

/// The page's open state: where it is scrolled and which FAQ rows are open.
pub(super) struct BoostPage {
    pub scroll: ScrollHandle,
    pub faq_open: Vec<bool>,
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
            ("boost_perk_1", "100 Emoji Slots"),
            ("boost_perk_2", "24 Soundboard Slots"),
            ("boost_perk_gif", "Animated Server Icon"),
            ("boost_perk_3", "Better Quality Audio"),
        ],
    ),
    (
        "Level 2",
        "7 Boosts",
        [
            ("boost_perk_2", "HD Streaming"),
            ("boost_perk_3", "50MB File Uploads"),
            ("boost_perk_0", "Custom Role Icons"),
            ("boost_perk_images", "Server Banner"),
        ],
    ),
    (
        "Level 3",
        "14 Boosts",
        [
            ("boost_perk_v0", "Custom Server Invite Link"),
            ("boost_perk_3", "100MB File Uploads"),
            ("boost_perk_3", "Highest Quality Audio"),
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
    ("boost_gem", "Display your support with a Member List icon"),
    (
        "boost_badge",
        "Unlock a profile badge that evolves over time",
    ),
    ("role_shield", "Get an exclusive Booster role"),
    ("boosts", "Show support for your community"),
];

/// The FAQ's questions, in the frame's order.
const FAQ: [&str; 9] = [
    "What is a Server Boost?",
    "How do I Boost a server?",
    "Can I Boost a Server multiple times?",
    "Can I continue Boosting a server after it reaches Level 3?",
    "What are 'additional perks' and how are they different from Levels?",
    "How much do the new additional perks cost?",
    "Are additional perks replacing the current server level system?",
    "Do additional perks use separate Boosts from my server level?",
    "Are additional perks a one-time purchase, or do they require ongoing Boosts?",
];

impl BoostPage {
    pub(super) fn new() -> Self {
        Self {
            scroll: ScrollHandle::new(),
            faq_open: vec![false; FAQ.len()],
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
        self.boost_page = Some(BoostPage::new());
        cx.notify();
    }

    pub(super) fn close_boost_page(&mut self) -> bool {
        self.boost_page.take().is_some()
    }

    /// "Boost This Server": as far as the existing Server Boost page; nothing here buys anything.
    fn boost_page_boost(&mut self, cx: &mut Context<Self>) {
        self.boost_page = None;
        self.open_settings_section("Server Boost", cx);
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
        let faq_open = page.faq_open.clone();

        let guild_chip = |size: f32, radius: f32| {
            div()
                .size(px(size))
                .flex_none()
                .rounded(px(radius))
                .border_1()
                .border_color(gpui::hsla(0., 0., 1., 0.16))
                .bg(color("--background-base-low"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(if size > 40. { 20. } else { 12. }))
                .line_height(px(if size > 40. { 24. } else { 16. }))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(acronym.clone())
        };

        // The header: the gradient the backdrop video sits on, the server, the headline, the two buttons.
        let header = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .right(px(0.))
                    .h(px(1280.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x9c59d199), 0.),
                        linear_color_stop(rgba(0x582f8c4d), 0.5),
                    )),
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
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(40.))
                                    .child(
                                        // GPUI has no letter spacing, so the -1.26px tracking is not applied and the two
                                        // lines (the wrap the official page makes at 770px) are set explicitly.
                                        text(42., 46.2, 700., "--text-strong")
                                            .font_family(super::nitro::NITRO_FONT)
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .whitespace_nowrap()
                                            .child("BOOST THIS SERVER & UNLOCK")
                                            .child("PERKS FOR EVERYONE"),
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
                                                    .bg(gpui::hsla(0., 0., 1., 0.1))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(16.))
                                                    .line_height(px(20.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(gpui::rgb(0xffffff))
                                                    .child("Get Nitro with 2 Free Boosts")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.boost_page = None;
                                                        this.open_nitro(cx);
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    // `container_hKMjy9`: the video card, 770 wide, 1px border, radius lg; no picture.
                    .child(
                        div()
                            .w(px(770.))
                            .h(px(435.))
                            .mb(px(32.))
                            .rounded(px(16.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(rgba(0x1a1747ff))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(76.))
                                    .rounded(px(38.))
                                    .bg(gpui::rgb(0xffffff))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        "chevron_right_20",
                                        32.,
                                        "--background-base-lowest",
                                    )),
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
                div().flex().gap(px(24.)).children(
                    [
                        (
                            "Enhanced Role Styles",
                            "Make certain roles stand out with animated, vibrant gradients.",
                        ),
                        (
                            "Server Tag",
                            "Let your members represent your server everywhere on Discord.",
                        ),
                    ]
                    .map(|(title, body)| {
                        div()
                            .w(px(588.))
                            .h(px(318.))
                            .flex_none()
                            .rounded(px(16.))
                            .border_1()
                            .border_color(gpui::hsla(240. / 360., 0.04, 0.61, 0.12))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(rgba(0x56168a33), 0.),
                                linear_color_stop(rgba(0x56168a00), 0.5),
                            ))
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_end()
                            .gap(px(24.))
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
                                    .child(icon("boost_gem", 16., "--guild-boosting-pink-refresh"))
                                    .child("3 Boosts"),
                            )
                    }),
                ),
            );

        let table = self.boost_table();
        let recognized =
            div()
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
                .child(div().flex().gap(px(24.)).w_full().children(RECOGNIZED.map(
                    |(art, line)| {
                        div()
                            .flex_1()
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
                            .child(icon(art, 48., "--icon-muted"))
                            .child(
                                text(16., 20., 500., "--text-default")
                                    .text_center()
                                    .child(line),
                            )
                    },
                )));

        let faq = div()
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
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(FAQ.iter().enumerate().map(|(index, question)| {
                        let open = faq_open.get(index).copied().unwrap_or(false);
                        div()
                            .id(("boost-faq", index))
                            .h(px(46.))
                            .px(px(12.))
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
                            .items_center()
                            .gap(px(12.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(page) = this.boost_page.as_mut()
                                    && let Some(row) = page.faq_open.get_mut(index)
                                {
                                    *row = !*row;
                                }
                                cx.notify();
                            }))
                            .child(
                                text(16., 20., 600., "--text-default")
                                    .flex_1()
                                    .text_color(color("--text-muted"))
                                    .child(*question),
                            )
                            .child(icon("chevron_down", 20., "--text-muted"))
                    })),
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
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.1))
                        .bg(gpui::hsla(240. / 360., 0.06, 0.1, 0.8))
                        .flex()
                        .items_center()
                        .gap(px(40.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
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
            .child(icon("modal_close", 20., "--primary-330"));

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
                        let bar = if active {
                            gpui::hsla(326. / 360., 1., 0.65, 1.)
                        } else {
                            rgba(0x474464ff).into()
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
                                            .border_4()
                                            .border_color(rgba(0x202024ff))
                                            .bg(if active {
                                                super::hsla("--guild-boosting-pink-refresh")
                                            } else {
                                                rgba(0x474464ff).into()
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("boost_gem", 20., "--white")),
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
        let cell = |content: AnyElement| {
            div()
                .w(gpui::relative(0.2))
                .flex_none()
                .px(px(4.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(content)
        };
        let head = |title: &'static str, sub: &'static str| {
            cell(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(text(18., 22., 600., "--text-strong").child(title))
                    .child(text(14., 18., 500., "--text-muted").child(sub))
                    .into_any_element(),
            )
        };
        let header_row = div()
            .h(px(75.))
            .py(px(12.))
            .flex()
            .items_start()
            .border_b_1()
            .border_color(color("--border-normal"))
            .child(
                div()
                    .w(gpui::relative(0.2))
                    .flex_none()
                    .px(px(16.))
                    .child(text(18., 22., 600., "--text-strong").child("Perks")),
            )
            .child(head("Unboosted", "No Boosts"))
            .child(head("Level 1", "2 Boosts"))
            .child(head("Level 2", "7 Boosts"))
            .child(head("Level 3", "14 Boosts"));
        let rows = TABLE.iter().enumerate().map(|(index, (label, cells))| {
            let last = index == TABLE.len() - 1;
            let has_icon = cells.iter().any(|cell| !matches!(cell, Cell::Text(_)));
            div()
                .h(px(if has_icon { 57. } else { 53. }))
                .py(px(12.))
                .flex()
                .items_center()
                .when(!last, |row| {
                    row.border_b_1().border_color(color("--border-normal"))
                })
                .child(
                    div()
                        .w(gpui::relative(0.2))
                        .flex_none()
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
                        Cell::No => icon("close", 24., "--interactive-muted").into_any_element(),
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
                    // `.recommendedTierHighlight`: 20% wide, 2px pink border, 20px past the table at both ends.
                    .child(
                        div()
                            .absolute()
                            .top(px(-20.))
                            .bottom(px(-20.))
                            .right(px(COLUMN * 0.2 * (4 - RECOMMENDED - 1) as f32))
                            .w(px(COLUMN * 0.2))
                            .rounded(px(16.))
                            .border_2()
                            .border_color(super::hsla("--guild-boosting-pink-refresh"))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-12.))
                                    .left(px(COLUMN * 0.1 - 53.))
                                    .px(px(8.))
                                    .h(px(24.))
                                    .rounded(px(8.))
                                    .bg(super::hsla("--guild-boosting-pink-refresh"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(gpui::rgb(0xffffff))
                                    .child("RECOMMENDED"),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_a_value_for_every_tier() {
        assert_eq!(TABLE.len(), 12);
        assert_eq!(TABLE[0].0, "Emoji Slots");
        assert!(matches!(TABLE[11].1[3], Cell::Yes));
        assert!(matches!(TABLE[11].1[2], Cell::No));
    }

    #[test]
    fn the_faq_closes_and_toggles_by_row() {
        let mut page = BoostPage::new();
        assert_eq!(page.faq_open.len(), FAQ.len());
        page.faq_open[2] = true;
        assert!(page.faq_open[2] && !page.faq_open[1]);
    }
}
