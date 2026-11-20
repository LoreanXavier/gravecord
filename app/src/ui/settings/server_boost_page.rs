//! The Server Boost page of user settings (`premium_guild_subscriptions_setting`): the header, the Nitro banner, the
//! servers that could use a Boost (real servers and their Boost counts), the level perks table, the recognition cards
//! and the FAQ. Geometry and copy are from `design/dom/settings/server-boost-darker-cozy.json`; positions are relative
//! to the page's top (154 in the capture). The level table is the guild limits table of module 682901.
//!
//! Not drawn: the "How do Boosts work?" video (a gradient card stands in) and the recognition card illustrations.
//! The open FAQ answer's height is estimated. The three "Learn more" links (header, Nitro banner, FAQ answer) open the `GUILD_BOOSTING_FAQ` article.

use super::Root;
use crate::ui::nitro::{oklab, rgb};
use crate::ui::{color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveText, IntoElement, SharedString,
    StyledText, div, img, linear_color_stop, linear_gradient, prelude::*, px, rgba,
};

/// `GUILD_BOOSTING_FAQ` of the help centre (module 652215): the article the "Learn more" link opens.
const BOOSTING_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/360028038352";

/// The answers of the Boosting FAQ, in the order of `FAQ` (strings `nhkk6k`, `8zlqlD`, `Vz/SCQ`, `+OURPp`, `3TeauK`,
/// `bTRacj`, `2T5iPo`, `NZax1u`, `Aje8Pb`; module 232122). The second one ends with a link to the help centre.
const ANSWERS: [&str; 9] = [
    "Boosts allow you to support your favorite Servers, by unlocking perks for everyone in them. The more Boosts a server has, the more epic the perks that can be unlocked. See the chart above for more details.",
    "You can Boost this server directly on this page! If you want to Boost a different server, you can find step-by-step instructions in our Help Center.",
    "Yes, you can Boost a server as many times as you want! The more Boosts the server gets, the better the perks that will be unlocked.",
    "Absolutely! Server Boosts are a great way to support your favorite servers. When you Boost a server, you'll be recognized with a special Member List Icon, a Boosting role, and a Profile Badge.",
    "Additional Perks are new features you can unlock directly with Boosts, separate from your server's Boost Level. Currently, there are two available: Enhanced Role Styles (which add gradient and holographic effects to roles) and Server Tags (4-character identifiers members can display on their profiles across Discord). Unlike traditional server levels, these perks can be unlocked individually without reaching specific level thresholds.",
    "Enhanced Role Styles and Server Tags each cost 3 Boosts. If you want both perks, you'll need 6 Boosts total (3 for each perk).",
    "No, this supplements the existing level system. All current server level perks (emoji slots, file upload limits, etc.) remain exactly the same. This new system simply gives you additional ways to use your Boosts for targeted perks without affecting your server's level progression.",
    "Yes, Boosts used for additional perks are dedicated specifically to those perks and don't count toward your server's level progression. Similarly, Boosts allocated to your server level can't be used for additional perks. You'll need separate Boosts for each - so if you want Enhanced Role Styles, you need 3 Boosts just for that perk, plus whatever Boosts you want to maintain your current server level.",
    "Each additional perk requires an ongoing allocation of its required Boosts to stay active, just like levels. If your server drops below the required number of Boosts, the perk will be disabled until the necessary Boosts are reallocated.",
];

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

/// The Nitro banner's second line (`6UAu+f` with a count of 2).
const BANNER_LINE: &str = "Nitro users get 2 Boosts included with their membership. Learn more";

/// The byte range of the banner's "Learn more".
fn banner_link() -> std::ops::Range<usize> {
    BANNER_LINE.len() - "Learn more".len()..BANNER_LINE.len()
}

fn at(x: f32, y: f32) -> gpui::Div {
    div().absolute().left(px(x)).top(px(y))
}

/// A sentence whose `link` part opens the Boosting article (the help centre link of the official strings).
fn article_line(
    id: &'static str,
    shown: &str,
    link: std::ops::Range<usize>,
    cx: &mut Context<Root>,
) -> AnyElement {
    let styled = StyledText::new(shown.to_owned()).with_highlights([(
        link.clone(),
        HighlightStyle {
            color: Some(color("--text-link").into()),
            ..HighlightStyle::default()
        },
    )]);
    let root = cx.weak_entity();
    InteractiveText::new(id, styled)
        .on_click(vec![link], move |_, _, cx| {
            let _ = root.update(cx, |this, cx| {
                this.settings_open_url(BOOSTING_ARTICLE, cx);
                cx.notify();
            });
        })
        .into_any_element()
}

/// The open FAQ item's answer; the second one ends in a link.
fn faq_answer(index: usize, cx: &mut Context<Root>) -> AnyElement {
    let answer = ANSWERS[index];
    if index == 1 {
        let start = answer.find("in our Help Center").unwrap_or(0);
        return article_line(
            "boost-faq-help-center",
            answer,
            start..start + "in our Help Center".len(),
            cx,
        );
    }
    div().child(dc_intl::tr(answer)).into_any_element()
}

/// The lines a text of `chars` characters takes in the 670px answer column at 14px (about 100 characters a line).
fn answer_lines(chars: usize) -> f32 {
    chars.div_ceil(100).max(1) as f32
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
        id: Id,
        name: &str,
        boosts: u64,
        tier: u64,
        top: f32,
        cx: &mut Context<Self>,
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
                    .id(SharedString::from(format!("boost-this-server-{id}")))
                    .cursor_pointer()
                    // The same flow as the server's own button (module 987144 `g` with this server).
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.boost_server_from(id, true, cx);
                    }))
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
                    .child(text(
                        dc_intl::tr("Boost this server"),
                        14.,
                        18.,
                        500.,
                        "--text-strong",
                    )),
            )
            .into_any_element()
    }

    pub(super) fn server_boost_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let subscriber = self.premium().is_subscriber();
        // The open FAQ answer lengthens the page: a 4px gap, 8px of padding and its lines.
        let open_extra = self
            .settings_extra
            .boost_faq
            .map_or(0., |index| 12. + 18. * answer_lines(ANSWERS[index].len()));
        let mut page = div().relative().w_full().h(px(2095.6 + open_extra));
        page = page
            .child(at(0., 49.).child(icon("boost_gem", 24., "--text-strong")))
            .child(at(26., 49.).child(text(dc_intl::tr("Server Boosts"), 24., 32., 500., "--text-strong")))
            .child({
                // "Learn more" is the link to the Boosting article (`helpDeskArticle`, module 440074).
                let line = "Boosting a server unlocks perks for everyone in that server. You can manage your Boosts here (add, subtract, cancel \u{2014} just beware the 7 day cooldown). Learn more";
                let start = line.len() - "Learn more".len();
                at(0., 85.)
                    .w(px(349.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(400.))
                    .text_color(color("--text-muted"))
                    .child(article_line(
                        "boost-header-learn-more",
                        line,
                        start..line.len(),
                        cx,
                    ))
            })
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
                    .child(at(13., 12.).child(text(dc_intl::tr("How do Boosts work?"), 14., 18., 600., "--white")))
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
                    .child(
                        at(14., 13.)
                            .size(px(40.))
                            .rounded(px(8.))
                            .bg(rgb(88, 40, 150))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("boost_gem", 24., "--white")),
                    )
                    .child(at(66., 13.).child(text(
                        "Get 2 Boosts with Nitro!",
                        16.,
                        20.,
                        600.,
                        "--text-default",
                    )))
                    .child(
                        at(66., 36.)
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("nitro_wheel", 14., "--text-muted"))
                            // "Learn more" is `helpdeskArticle`, `GUILD_BOOSTING_FAQ` (module 158935 `mB`).
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-muted"))
                                    .child(article_line(
                                        "boost-banner-learn-more",
                                        BANNER_LINE,
                                        banner_link(),
                                        cx,
                                    )),
                            ),
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
                            .child(text(dc_intl::tr("Get Nitro"), 14., 18., 500., "--black"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_settings(cx);
                                this.open_nitro(cx);
                            })),
                    ),
            );
        } else {
            page = page.child(
                at(0., 211.)
                    .w(px(696.))
                    .h(px(66.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .child(
                        at(14., 13.)
                            .size(px(40.))
                            .rounded(px(8.))
                            .bg(rgb(88, 40, 150))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("boost_gem", 24., "--white")),
                    )
                    .child(at(66., 13.).child(text(
                        dc_intl::tr("Server Boosts"),
                        16.,
                        20.,
                        600.,
                        "--text-strong",
                    )))
                    .child(at(66., 36.).child(text(
                        dc_intl::t("iqhIp4"),
                        12.,
                        16.,
                        400.,
                        "--text-muted",
                    )))
                    .child(
                        at(516., 17.)
                            .id("boost-cancel-manage")
                            .w(px(166.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-critical-secondary-border-default"))
                            .bg(color("--control-critical-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(text(
                                dc_intl::t("CRwE7n"),
                                13.,
                                18.,
                                500.,
                                "--control-critical-secondary-text-default",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let guild_id = this.stores.guilds.first().map(|g| g.id);
                                this.open_boost_cancel_modal(
                                    "slot_primary".to_owned(),
                                    guild_id,
                                    cx,
                                );
                            })),
                    ),
            );
        }
        page = page.child(at(0., 308.).child(text(
            dc_intl::tr("These servers could use a Boost"),
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
                cx,
            ));
            top += 74.;
        }
        // The perks table, below the list.
        let head = top + 39.;
        page = page.child(at(16., head).child(text(
            dc_intl::tr("Perks"),
            20.,
            24.,
            600.,
            "--text-default",
        )));
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
                dc_intl::tr(label),
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
            dc_intl::tr("Give your community a Boost & be recognized"),
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
                        dc_intl::tr(label),
                        14.,
                        18.,
                        500.,
                        "--text-muted",
                    ))),
            );
        }
        let faq_head = cards_head + 48. + 220. + 16.;
        page = page.child(at(0., faq_head).child(text(
            dc_intl::tr("Boosting FAQ"),
            24.,
            32.,
            500.,
            "--text-strong",
        )));
        // `listItem_EVXXpI` (module 232122): a column with a 4px gap; one answer open at a time, the open item on
        // `--background-surface-highest` with a strong border, its answer 8px under the question (`answer_EVXXpI`).
        let open = self.settings_extra.boost_faq;
        let mut list = at(0., faq_head + 48.)
            .w(px(696.))
            .flex()
            .flex_col()
            .gap(px(4.));
        for (index, question) in FAQ.iter().enumerate() {
            let expanded = open == Some(index);
            let group = SharedString::from(format!("boost-faq-{index}"));
            list = list.child(
                div()
                    .id(group.clone())
                    .group(group.clone())
                    .w(px(696.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(if expanded {
                        "--border-strong"
                    } else {
                        "--border-subtle"
                    }))
                    .bg(color(if expanded {
                        "--background-surface-highest"
                    } else {
                        "--background-surface-high"
                    }))
                    .p(px(12.))
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let faq = &mut this.settings_extra.boost_faq;
                        *faq = if *faq == Some(index) {
                            None
                        } else {
                            Some(index)
                        };
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
                                    .text_color(color(if expanded {
                                        "--text-default"
                                    } else {
                                        "--text-muted"
                                    }))
                                    .group_hover(group, |style| {
                                        style.text_color(color("--text-default"))
                                    })
                                    .child(dc_intl::tr(question)),
                            )
                            .child({
                                let chevron = icon("chevron_down", 20., "--text-muted");
                                if expanded {
                                    chevron
                                        .with_transformation(gpui::Transformation::rotate(
                                            gpui::percentage(0.5),
                                        ))
                                        .into_any_element()
                                } else {
                                    chevron.into_any_element()
                                }
                            }),
                    )
                    .when(expanded, |this| {
                        this.child(
                            div()
                                .pt(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-muted"))
                                .child(faq_answer(index, cx)),
                        )
                    }),
            );
        }
        page = page.child(list);
        page.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_link_is_its_learn_more() {
        assert_eq!(&BANNER_LINE[banner_link()], "Learn more");
        assert!(BOOSTING_ARTICLE.ends_with("/articles/360028038352"));
    }
}
