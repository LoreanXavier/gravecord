//! The bodies of the Gift Inventory page: the redeem field, the Gifting Badge progress and the gifts the account
//! purchased. Measured in `design/dom/settings/gift-inventory-darker-cozy.json`; the category headings come from the
//! generated page.
//!
//! The redeem field, its lookup and accept dialog are in `gift_redeem.rs`. The list of purchased gifts draws only its empty state: the record layout of a gift code is not in the
//! captured traffic.

use super::Root;
use crate::ui::color;
use crate::ui::nitro::art_key;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};

const TIERS: [(&str, &str, &str); 6] = [
    ("Patron", "Gifted 1x", "gift-box"),
    ("Champion", "Gifted 2x", "gift-champion"),
    ("Luminary", "Gifted 3x", "gift-luminary"),
    ("Icon", "Gifted 6x", "gift-icon"),
    ("Hero", "Gifted 10x", "gift-hero"),
    ("Legend", "Gifted 20x", "gift-legend"),
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

fn at(x: f32, y: f32) -> gpui::Div {
    div().absolute().left(px(x)).top(px(y))
}

impl Root {
    pub(super) fn gift_inventory_custom(
        &self,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(match key {
            "redeem_code_input" => self.gift_redeem_row(cx),
            "gifting_badge_content" => self.gifting_badge_body(cx),
            "purchased_gifts_content" => self.purchased_gifts_body(cx),
            _ => return None,
        })
    }

    fn gifting_badge_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div().relative().w_full().h(px(304.));
        let mut card = at(0., 8.)
            .w(px(696.))
            .h(px(200.))
            .bg(gpui::rgba(0x97979f14))
            .rounded_t(px(8.));
        for (index, (name, count, art)) in TIERS.iter().enumerate() {
            let centre = 88. + index as f32 * 104.;
            card = card
                .child(
                    at(centre - 22., 40.).size(px(44.)).children(
                        self.images
                            .get(&art_key(art))
                            .cloned()
                            .map(|picture| img(picture).size(px(44.))),
                    ),
                )
                .child(
                    at(centre - 52., 92.)
                        .w(px(104.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(text(*name, 14., 18., 600., "--text-muted"))
                        .child(text(*count, 12., 16., 400., "--text-muted")),
                );
        }
        card = card.child(
            at(0., 152.).w(px(696.)).flex().justify_center().child(text(
                "Your badge progresses as you gift. Exceptions include when a gift is refunded or when fraud is detected.",
                12.,
                16.,
                400.,
                "--text-muted",
            )),
        );
        page = page.child(card).child(
            at(0., 208.)
                .w(px(696.))
                .h(px(88.))
                .rounded_b(px(8.))
                .bg(color("--background-surface-high"))
                .child(
                    at(540., 24.)
                        .id("gift-badge-send")
                        .w(px(132.))
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(gpui::hsla(0., 0., 1., 0.08))
                        .bg(color("--control-primary-background-default"))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(text("Send a Gift", 16., 20., 500., "--white"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.close_settings(cx);
                            this.open_nitro(cx);
                            this.open_purchase(true, cx);
                        })),
                ),
        );
        page.into_any_element()
    }

    fn purchased_gifts_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let none = self
            .nitro
            .gift_codes
            .as_ref()
            .is_none_or(|body| body.as_array().is_some_and(|rows| rows.is_empty()));
        if !none {
            return div().h(px(152.1)).into_any_element();
        }
        div()
            .h(px(152.1))
            .pt(px(47.))
            .flex()
            .flex_col()
            .items_center()
            .child(text(
                "There are no gifts.",
                24.,
                28.,
                400.,
                "--text-default",
            ))
            .child(
                div()
                    .mt(px(9.))
                    .flex()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child("Feeling generous? Give someone the gift of ")
                    .child(
                        div()
                            .id("gifts-nitro-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child("Nitro")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_settings(cx);
                                this.open_nitro(cx);
                            })),
                    )
                    .child(" and make their day!"),
            )
            .into_any_element()
    }
}
