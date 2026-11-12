//! The Nitro page of user settings (`nitro_setting`): the hero card, the two plan cards, the favourite perks and the
//! comparison table, with the real plans and prices. Geometry and copy are from
//! `design/dom/settings/nitro-darker-cozy.json` (positions relative to the page's top, 154 in the capture).
//!
//! Not drawn: the perk illustrations, the plan card mascots and the hero sparkles (artwork of the official page).
//! A subscriber sees "Manage" in the official client; that state needs a real subscription and is not drawn.

use super::Root;
use crate::ui::nitro::{oklab, rgb};
use crate::ui::{color, icon};
use dc_state::premium::sku;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};

/// A cell of the comparison table.
enum Cell {
    Included,
    No,
    Text(&'static str),
}
use Cell::{Included, No, Text};

const ROWS: [(&str, Cell, Cell); 20] = [
    (
        "Custom emoji anywhere and make them animated",
        Included,
        Included,
    ),
    ("Custom stickers anywhere", Included, Included),
    ("Custom App Icons", Included, Included),
    ("Super Reactions", Included, Included),
    ("Larger File Uploads", Text("50 MB"), Text("1 GB")),
    ("HD streaming", No, Text("Up to 4K and 60fps")),
    ("2 Boosts + 30% off extra Boosts", No, Included),
    ("Animated avatar, banner, and profile theme", No, Included),
    ("Display Name Styles", No, Included),
    ("Custom per-server profiles", No, Included),
    ("Shop member pricing, and exclusive items", No, Included),
    ("Colors for your Discord theme", No, Included),
    ("Nitro badge on your profile", Included, Included),
    ("Custom video backgrounds", Included, Included),
    ("Custom sounds anywhere", No, Included),
    ("Custom entrance sounds", No, Included),
    ("Join up to 200 servers", No, Included),
    ("Longer messages up to 4000 characters", No, Included),
    ("3 Nitro friend passes", No, Included),
    ("Keep Quest avatar decorations rewards longer", No, Included),
];

/// The perk cards: title, text, and the card's top and height.
const PERKS: [(&str, &str, f32, f32, f32); 9] = [
    (
        "More Emoji Power",
        "Hype, roast, and meme with custom emoji anywhere.",
        0.,
        897.,
        266.,
    ),
    (
        "HD Video",
        "Better video resolutions for all your streams. Stream apps and games in up to 4K and 60fps.",
        1.,
        897.,
        266.,
    ),
    (
        "1GB Uploads",
        "Upload what you want with increased 1GB upload size.",
        0.,
        1179.,
        265.,
    ),
    (
        "Custom App Icons",
        "Choose a mobile and in-app desktop icon that fits your vibe.",
        1.,
        1179.,
        265.,
    ),
    (
        "Use Custom Sounds",
        "Use custom sounds and personalized entrance sounds across voice channels.",
        0.,
        1460.,
        282.,
    ),
    (
        "Video Backgrounds",
        "Make video calls unique with your own backgrounds.",
        1.,
        1460.,
        282.,
    ),
    (
        "Unlimited Super Reactions",
        "We made Super Reactions unlimited so you can unleash the chaos.",
        0.,
        1758.,
        282.,
    ),
    (
        "Special Sticker Access",
        "Use custom stickers anywhere.",
        1.,
        1758.,
        282.,
    ),
    (
        "Subscriber Badge",
        "Get this cool badge for being a Nitro subscriber.",
        0.,
        2056.,
        260.,
    ),
];

const BASIC_FEATURES: [(&str, &str); 4] = [
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
const NITRO_FEATURES: [(&str, &str); 6] = [
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

fn text(
    value: impl Into<gpui::SharedString>,
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
        .child(value.into())
}

/// The Nitro display face, bold and upright, upper case (the captured hero and plan card titles; the page's section
/// headings are the sans at 600).
fn display(value: &str, size: f32, line: f32) -> gpui::Div {
    div()
        .font_family(crate::ui::nitro::NITRO_FONT)
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(FontWeight(700.))
        .text_color(color("--text-strong"))
        .child(value.to_uppercase())
}

fn at(x: f32, y: f32) -> gpui::Div {
    div().absolute().left(px(x)).top(px(y))
}

impl Root {
    #[allow(clippy::too_many_arguments)]
    fn settings_outline_button(
        &self,
        id: &'static str,
        label: &'static str,
        glyph: &'static str,
        filled: bool,
        width: f32,
        cx: &mut Context<Self>,
        gift: bool,
    ) -> AnyElement {
        div()
            .id(id)
            .w(px(width))
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if filled {
                rgba(0xffffff14)
            } else {
                rgba(0xffffffff)
            })
            .when(filled, |this| this.bg(rgba(0xffffffff)))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .text_size(px(16.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(if filled {
                rgb(88, 101, 242)
            } else {
                rgba(0xffffffff).into()
            })
            .children(
                (!glyph.is_empty())
                    .then(|| icon(glyph, 20., if filled { "--brand-500" } else { "--white" })),
            )
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.open_purchase(gift, cx)))
            .into_any_element()
    }

    fn settings_plan_card(&self, nitro: bool, cx: &mut Context<Self>) -> AnyElement {
        let plan_sku = if nitro { sku::TIER_2 } else { sku::TIER_0 };
        let price = self
            .plan_price(plan_sku, false)
            .map(|price| price.display());
        let features: &[(&str, &str)] = if nitro {
            &NITRO_FEATURES
        } else {
            &BASIC_FEATURES
        };
        // Sampled from settings-full/nitro-01: the cards run left to right, 10px in and 10px from the edge
        // (85,101,240) to (2,123,195) and (134,71,196) to (170,92,139), constant down the card.
        let (from, to) = if nitro {
            (rgb(134, 71, 196), rgb(170, 92, 139))
        } else {
            (rgb(85, 101, 240), rgb(2, 123, 195))
        };
        div()
            .relative()
            .w(px(336.))
            .h(px(406.))
            .rounded(px(16.))
            .bg(linear_gradient(
                90.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ))
            .child(at(24., 12.).child(display(
                if nitro { "Nitro" } else { "Nitro Basic" },
                28.,
                30.,
            )))
            .child(
                at(24., if nitro { 52. } else { 70. })
                    .flex()
                    .items_baseline()
                    .children(price.map(|price| {
                        div()
                            .flex()
                            .items_baseline()
                            .child(text(price, 16., 20., 700., "--white"))
                            .child(text(" / month", 16., 20., 200., "--white"))
                    })),
            )
            .child(
                at(24., if nitro { 93. } else { 112. })
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(features.iter().map(|(glyph, line)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon(glyph, 24., "--white"))
                            .child(text(*line, 16., 20., 400., "--white"))
                    })),
            )
            .child(at(24., 317.).child(self.settings_outline_button(
                if nitro {
                    "settings-nitro-subscribe"
                } else {
                    "settings-basic-subscribe"
                },
                "Subscribe",
                // The captured plan card buttons carry no wheel (the hero's do).
                "",
                nitro,
                288.,
                cx,
                false,
            )))
            .into_any_element()
    }

    fn perk_card(
        &self,
        title: &'static str,
        body: &'static str,
        column: f32,
        top: f32,
        height: f32,
    ) -> AnyElement {
        div()
            .absolute()
            .left(px(column * 356.))
            .top(px(top))
            .w(px(340.))
            .h(px(height))
            .rounded(px(16.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(24.))
                    .px(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    .child(text(title, 20., 24., 600., "--text-strong"))
                    .child(
                        div()
                            .mt(px(7.))
                            .child(text(body, 14., 18., 400., "--text-default")),
                    ),
            )
            .into_any_element()
    }

    fn compare_cell(value: &Cell, centre: f32, top: f32) -> AnyElement {
        let (label, weight): (&str, f32) = match value {
            Included => ("Included", 400.),
            No => ("Not included", 400.),
            Text(value) => (value, 600.),
        };
        div()
            .absolute()
            .left(px(centre - 90.))
            .top(px(top))
            .w(px(180.))
            .flex()
            .justify_center()
            .child(text(
                label,
                16.,
                if weight > 500. { 20. } else { 16. },
                weight,
                if weight > 500. {
                    "--text-strong"
                } else {
                    "--text-default"
                },
            ))
            .into_any_element()
    }

    pub(super) fn nitro_settings_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let basic = self
            .plan_price(sku::TIER_0, false)
            .map(|price| price.display());
        let nitro = self
            .plan_price(sku::TIER_2, false)
            .map(|price| price.display());
        let starting = basic
            .as_ref()
            .map(|price| format!("Plans start at only {price}/month. Cancel anytime"))
            .unwrap_or_default();
        let mut page = div().relative().w_full().h(px(4202.3));
        // The hero card, 696 x 330.
        page = page
            .child(
                at(1., 9.)
                    .w(px(696.))
                    .h(px(330.))
                    .rounded(px(16.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgb(52, 51, 190), 0.),
                        linear_color_stop(rgb(70, 112, 228), 0.78),
                    ))
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(70.))
                            .rounded_b(px(16.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(oklab(0.6, 0.1, -0.16, 0.), 0.),
                                linear_color_stop(rgb(214, 128, 238), 1.),
                            )),
                    ),
            )
            .child(
                at(640., 25.)
                    .size(px(40.))
                    .rounded(px(8.))
                    .bg(rgba(0x0000008f))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("golive_heart", 20., "--white")),
            )
            .child(
                at(0., 81.).w(px(696.)).flex().justify_center().child(
                    display("Unlock a World of Perks with Nitro", 32., 34.)
                        .w(px(480.))
                        .text_center()
                        .text_color(color("--white")),
                ),
            )
            .child(
                at(0., 171.)
                    .w(px(696.))
                    .flex()
                    .justify_center()
                    .child(text(starting, 20., 24., 400., "--white")),
            )
            .child(at(180., 228.).child(self.settings_outline_button(
                "settings-nitro-hero-subscribe",
                "Subscribe",
                "nitro_wheel",
                true,
                160.,
                cx,
                false,
            )))
            .child(at(356., 228.).child(self.settings_outline_button(
                "settings-nitro-hero-gift",
                "Gift Nitro",
                "gift",
                false,
                160.,
                cx,
                true,
            )))
            .child(at(1., 363.).child(self.settings_plan_card(false, cx)))
            .child(at(361., 363.).child(self.settings_plan_card(true, cx)));
        // Favourite perks.
        page = page.child(at(0., 823.).w(px(696.)).flex().justify_center().child(text(
            "Favorite Nitro Perks",
            32.,
            40.,
            600.,
            "--text-strong",
        )));
        for (title, body, column, top, height) in PERKS {
            page = page.child(self.perk_card(title, body, column, top, height));
        }
        // The comparison table.
        let table_top = 2492.;
        page = page
            .child(
                at(0., 2406.)
                    .w(px(696.))
                    .flex()
                    .justify_center()
                    .child(text(
                        "Pick the plan that works best for you",
                        32.,
                        40.,
                        600.,
                        "--text-strong",
                    )),
            )
            .child(
                at(484., table_top)
                    .w(px(212.))
                    .h(px(1532.))
                    .rounded(px(12.))
                    .border_2()
                    .border_color(rgb(184, 69, 193))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgb(61, 38, 99), 0.),
                        linear_color_stop(rgb(24, 22, 33), 0.1),
                    )),
            )
            .child(
                at(546., table_top - 6.)
                    .h(px(24.))
                    .px(px(12.))
                    .rounded(px(12.))
                    .bg(rgb(89, 31, 114))
                    .flex()
                    .items_center()
                    .child(text("Most Popular", 12., 16., 600., "--white")),
            )
            .child(at(2., table_top + 25.).child(text(
                "Pricing and Features",
                20.,
                24.,
                600.,
                "--text-strong",
            )))
            .child(at(2., table_top + 107.).child(text("Price", 14., 18., 400., "--text-default")))
            .child(
                at(323., table_top + 105.).child(text(
                    basic
                        .map(|price| format!("{price}/month"))
                        .unwrap_or_default(),
                    16.,
                    20.,
                    600.,
                    "--text-strong",
                )),
            )
            .child(
                at(526., table_top + 105.).child(text(
                    nitro
                        .map(|price| format!("{price}/month"))
                        .unwrap_or_default(),
                    16.,
                    20.,
                    600.,
                    "--text-strong",
                )),
            );
        let mut row_top = table_top + 166.;
        for (index, (label, basic, nitro)) in ROWS.iter().enumerate() {
            let label_height = if index == 0 { 36. } else { 18. };
            let middle = row_top + label_height / 2. - 8.;
            page = page
                .child(at(2., row_top).w(px(237.)).child(text(
                    *label,
                    14.,
                    18.,
                    400.,
                    "--text-default",
                )))
                .child(Self::compare_cell(basic, 412., middle))
                .child(Self::compare_cell(nitro, 620., middle));
            row_top += label_height + 50.;
        }
        page.into_any_element()
    }
}
