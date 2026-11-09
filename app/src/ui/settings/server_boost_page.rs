//! The Server Boost page of user settings (`premium_guild_subscriptions_setting`): the header, the Nitro banner, the
//! servers that could use a Boost (real servers and their Boost counts), the level perks table, the recognition cards
//! and the FAQ. Geometry and copy are from `design/dom/settings/server-boost-darker-cozy.json`; positions are relative
//! to the page's top (154 in the capture). The level table is the guild limits table of module 682901.
//!
//! Not drawn: the "How do Boosts work?" video (a gradient card stands in), the recognition card illustrations, and the
//! FAQ answers (the rows are drawn but do not open: the answers are not in the capture). "Boost this server" and
//! "Learn more" are drawn but do nothing: boosting spends a Boost slot, and the help article is not named in the
//! snapshot.

use super::Root;
use crate::ui::nitro::{oklab, rgb};
use crate::ui::{color, icon};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, img, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};

/// The perks table: label, then the three levels' values. A level with `""` shows a check; `"-"` shows nothing.
const TABLE: [(&str, [&str; 3], f32); 12] = [
    ("Emoji Slots", ["100", "150", "250"], 53.),
    ("Sticker Slots", ["15", "30", "60"], 53.),
    ("Soundboard Slots", ["24", "36", "48"], 53.),
    (
        "Stream Quality",
        ["720p & 60fps", "1440p & 60fps", "1440p & 60fps"],
        53.,
    ),
    ("Audio Quality", ["128kbps", "256kbps", "384kbps"], 53.),
    ("Upload Size Limit", ["10MB", "50MB", "100MB"], 53.),
    ("Video Stage Seats", ["50", "150", "300"], 53.),
    ("Animated Server Icon", ["", "", ""], 73.),
    ("Server Invite Background", ["", "", ""], 75.),
    ("Server Banner", ["-", "Static", "Animated"], 57.),
    ("Custom Role Icons", ["-", "", ""], 57.),
    ("Custom Invite Link", ["-", "-", ""], 57.),
];

/// The level columns' centres, from the capture.
const CENTRES: [f32; 3] = [260., 436., 609.];

const CARDS: [&str; 4] = [
    "Display your support with a Member List icon",
    "Unlock a profile badge that evolves over time",
    "Get an exclusive Booster role",
    "Show support for your community",
];

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

/// "gravecord fixture" as "gf": the first letter of each word.
fn acronym(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .collect()
}

impl Root {
    fn boost_server_row(
        &self,
        id: impl std::fmt::Display,
        name: &str,
        boosts: u64,
        tier: u64,
        top: f32,
    ) -> AnyElement {
        let icon_image = self.images.get(&format!("guild:{id}")).cloned();
        let count = if boosts == 0 {
            "No Boosts".to_owned()
        } else if boosts == 1 {
            "1 Boost".to_owned()
        } else {
            format!("{boosts} Boosts")
        };
        at(0., top)
            .w(px(696.))
            .h(px(66.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .child(
                at(13., 12.)
                    .size(px(40.))
                    .rounded(px(12.))
                    .overflow_hidden()
                    .bg(color("--background-surface-higher"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(icon_image.clone(), |this, picture| {
                        this.child(img(picture).size(px(40.)))
                    })
                    .when(icon_image.is_none(), |this| {
                        this.child(text(acronym(name), 14., 40., 400., "--text-default"))
                    }),
            )
            .child(at(65., 12.).child(text(name.to_owned(), 16., 20., 600., "--text-default")))
            .child(
                at(65., 35.)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(icon("boost_gem", 12., "--text-muted"))
                    .child(text(count, 12., 16., 600., "--text-muted"))
                    .when(tier > 0, |this| {
                        this.child(div().size(px(4.)).rounded_full().bg(color("--text-muted")))
                            .child(text(
                                format!("Level {tier}"),
                                12.,
                                16.,
                                600.,
                                "--text-muted",
                            ))
                    }),
            )
            .child(
                at(537., 16.)
                    .w(px(146.))
                    .h(px(32.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(rgba(0x97979f1f))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(icon("boost_badge", 16., "--text-strong"))
                    .child(text("Boost this server", 14., 18., 500., "--text-strong")),
            )
            .into_any_element()
    }

    pub(super) fn server_boost_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let subscriber = self.premium().is_subscriber();
        let mut page = div().relative().w_full().h(px(2095.6));
        page = page
            .child(at(0., 49.).child(icon("boost_gem", 24., "--text-strong")))
            .child(at(26., 49.).child(text("Server Boosts", 24., 32., 500., "--text-strong")))
            .child(
                at(0., 85.).w(px(349.)).child(
                    text(
                        "Boosting a server unlocks perks for everyone in that server. You can manage your Boosts here (add, subtract, cancel \u{2014} just beware the 7 day cooldown). Learn more",
                        14.,
                        18.,
                        400.,
                        "--text-muted",
                    ),
                ),
            )
            .child(
                at(396., 9.)
                    .w(px(300.))
                    .h(px(170.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgb(46, 30, 110), 0.),
                        linear_color_stop(rgb(24, 20, 60), 1.),
                    ))
                    .child(at(13., 12.).child(text("How do Boosts work?", 14., 18., 600., "--white")))
                    .child(
                        at(125., 60.)
                            .size(px(48.))
                            .rounded_full()
                            .bg(rgba(0x00000099))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("boosts", 20., "--white")),
                    ),
            );
        // The Nitro banner is for accounts without Nitro; the subscriber's version is not captured.
        if !subscriber {
            page = page.child(
                at(0., 211.)
                    .w(px(696.))
                    .h(px(66.))
                    .rounded(px(8.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(oklab(0.35, 0.12, -0.1, 0.9), 0.),
                        linear_color_stop(oklab(0.26239, 0.0025, -0.0089, 1.), 0.6),
                    ))
                    .child(at(14., 13.).size(px(40.)).rounded(px(8.)).bg(rgb(88, 40, 150)).flex().items_center().justify_center().child(icon("boost_gem", 24., "--white")))
                    .child(at(66., 13.).child(text("Get 2 Boosts with Nitro!", 16., 20., 600., "--text-default")))
                    .child(
                        at(66., 36.)
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("nitro_wheel", 14., "--text-muted"))
                            .child(text(
                                "Nitro users get 2 Boosts included with their membership. Learn more",
                                12.,
                                16.,
                                600.,
                                "--text-muted",
                            )),
                    )
                    .child(
                        at(584., 17.)
                            .id("boost-get-nitro")
                            .w(px(100.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .bg(rgba(0xffffffff))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .child(icon("nitro_wheel", 16., "--black"))
                            .child(text("Get Nitro", 14., 18., 500., "--black"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_settings(cx);
                                this.open_nitro(cx);
                            })),
                    ),
            );
        }
        page = page.child(at(0., 308.).child(text(
            "These servers could use a Boost",
            16.,
            20.,
            600.,
            "--text-strong",
        )));
        let mut top = 345.;
        for guild in &self.stores.guilds {
            page = page.child(self.boost_server_row(
                guild.id,
                &guild.name,
                guild.premium_subscription_count,
                guild.premium_tier,
                top,
            ));
            top += 74.;
        }
        // The perks table, below the list.
        let head = top + 39.;
        page = page.child(at(16., head).child(text("Perks", 20., 24., 600., "--text-default")));
        for (level, boosts) in ["2 Boosts", "7 Boosts", "14 Boosts"].iter().enumerate() {
            let centre = CENTRES[level];
            page = page.child(
                at(centre - 60., head)
                    .w(px(120.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(text(
                        format!("Level {}", level + 1),
                        20.,
                        24.,
                        600.,
                        "--text-default",
                    ))
                    .child(text(*boosts, 14., 18., 500., "--text-muted")),
            );
        }
        let mut row = head + 75.;
        for (label, values, step) in TABLE {
            page = page.child(at(16., row).w(px(135.)).child(text(
                label,
                16.,
                20.,
                500.,
                "--text-muted",
            )));
            for (level, value) in values.iter().enumerate() {
                let centre = CENTRES[level];
                let cell = match *value {
                    "-" => div().into_any_element(),
                    "" => icon("nitrocmp_check", 18., "--text-strong").into_any_element(),
                    value => text(value, 16., 20., 600., "--text-strong").into_any_element(),
                };
                page = page.child(
                    at(centre - 70., row)
                        .w(px(140.))
                        .flex()
                        .justify_center()
                        .child(cell),
                );
            }
            row += step;
        }
        // Recognition cards and the FAQ follow the table.
        let cards_head = row + 14.;
        page = page.child(at(0., cards_head).child(text(
            "Give your community a Boost & be recognized",
            24.,
            32.,
            500.,
            "--text-strong",
        )));
        for (index, label) in CARDS.iter().enumerate() {
            let x = (index % 2) as f32 * 356.;
            let y = cards_head + 48. + (index / 2) as f32 * 110.;
            page = page.child(
                at(x, y)
                    .w(px(340.))
                    .h(px(94.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .child(at(0., 62.).w(px(340.)).flex().justify_center().child(text(
                        *label,
                        14.,
                        18.,
                        500.,
                        "--text-muted",
                    ))),
            );
        }
        let faq_head = cards_head + 48. + 220. + 16.;
        page = page.child(at(0., faq_head).child(text(
            "Boosting FAQ",
            24.,
            32.,
            500.,
            "--text-strong",
        )));
        for (index, question) in FAQ.iter().enumerate() {
            page = page.child(
                at(0., faq_head + 48. + index as f32 * 50.)
                    .w(px(696.))
                    .h(px(46.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .child(at(13., 11.).child(text(*question, 16., 20., 600., "--text-muted")))
                    .child(at(660., 12.).child(icon("chevron_down", 20., "--text-muted"))),
            );
        }
        page.into_any_element()
    }
}
