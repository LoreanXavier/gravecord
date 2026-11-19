//! The 100 server limit (module 398884 `Sn`: 100 servers or more without `canUseIncreasedGuildCap`, Nitro's
//! `INCREASED_GUILD_LIMIT`, or staff) and its upsell, "Wanna join more servers?" (module 716480, opened by module
//! 772366 with `getGuildCount()`).
//!
//! The upsell is the premium upsell modal of module 945240 (classes of module 777133, the same box as the per-server
//! profile upsell of module 63473): the art of module 716480 (225 wide, 8px corners, lifted 48px over the box, 24px over
//! the text), the title (24px under it), the body of two paragraphs (`5qLH7B` under 100 servers, `mk9CS6` at the
//! limit) whose "and more with Discord Nitro!" (`onAndMore`) closes it and opens User Settings on Nitro (module 87719
//! `e`), then the tier 2 subscribe button and Back.
//!
//! Openers: Add a Server at the limit (`tX` of module 396655 opens the upsell instead of the Create a Server modal) and
//! an invite card's Join at the limit (`N` of module 963055, `GUILD_CAP_SHOWN`).

use super::{Root, color};
use dc_state::premium::{Feature, PremiumType};
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, img, prelude::*, px};

/// The art of module 716480.
const ART: &str = "https://cdn.discordapp.com/assets/content/e79d4cfdd618a7a39abcd4d48afd7b216dab54ca862f39d293270af910e517e7.png";
/// The server limit without Nitro.
pub(super) const GUILD_CAP: usize = 100;

/// `Sn` of module 398884: at the limit with neither Nitro's larger cap nor staff.
pub(super) fn at_guild_cap(guilds: usize, premium: PremiumType, staff: bool) -> bool {
    guilds >= GUILD_CAP && !(premium.has(Feature::IncreasedGuildLimit) || staff)
}

/// One run of the upsell's body: its text, whether it is the `onAndMore` link, whether it is the NEW badge, and
/// whether a new paragraph starts before it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Run {
    pub text: String,
    pub link: bool,
    pub badge: bool,
    pub paragraph: bool,
}

/// The body of module 716480 for this many servers: `5qLH7B` (with `guildCount`) under 100, `mk9CS6` at 100 or more.
/// Its first `$p` is the `context` line of the same module (`C+Hqzs` / `m0xavd`); the runs merge the two paragraphs,
/// so the second starts where that line ends.
pub(super) fn body_runs(guilds: usize) -> Vec<Run> {
    let count = guilds as i64;
    let (key, first) = if guilds < GUILD_CAP {
        (
            "5qLH7B",
            dc_intl::format("C+Hqzs", &[("guildCount", count.into())]),
        )
    } else {
        ("mk9CS6", dc_intl::t("m0xavd").to_owned())
    };
    let mut out: Vec<Run> = Vec::new();
    for (text, tag) in dc_intl::format_runs(key, &[("guildCount", count.into())]) {
        let tag = tag.as_deref().unwrap_or("");
        let run = |text: String, paragraph: bool| Run {
            text,
            link: tag == "onAndMore",
            badge: tag == "newBadgeHook",
            paragraph,
        };
        if out.is_empty()
            && let Some(rest) = text.strip_prefix(first.as_str())
        {
            out.push(run(first.clone(), true));
            if !rest.is_empty() {
                out.push(run(rest.to_owned(), true));
            }
            continue;
        }
        let paragraph = out.is_empty();
        out.push(run(text, paragraph));
    }
    out
}

/// One paragraph of the body: its text and the ranges of the link (`false`) and the badge (`true`).
type Paragraph = (String, Vec<(std::ops::Range<usize>, bool)>);

impl Root {
    /// `Sn` of module 398884 for this account.
    pub(super) fn at_guild_cap(&self) -> bool {
        let staff = self
            .stores
            .current_user
            .as_ref()
            .is_some_and(|user| user.has_flag(dc_state::USER_FLAG_STAFF));
        at_guild_cap(self.stores.guilds.len(), self.premium(), staff)
    }

    /// Module 772366: the upsell with the current server count.
    pub(super) fn open_guild_cap_upsell(&mut self, cx: &mut Context<Self>) {
        self.request_image(ART.to_owned(), ART.to_owned());
        self.guild_cap_upsell = true;
        cx.notify();
    }

    pub(super) fn guild_cap_upsell_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.guild_cap_upsell {
            return None;
        }
        let runs = body_runs(self.stores.guilds.len());
        // `root_PwiJiV` centres the text; each paragraph is a line of its own, the link in the link colour and the NEW
        // badge (module 805251, `type: "new"`) as a brand pill.
        // GAP: gpui highlights cannot round the badge's corners, so the NEW pill is square.
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        for run in runs {
            if run.paragraph || paragraphs.is_empty() {
                paragraphs.push((String::new(), Vec::new()));
            }
            let (text, marks) = paragraphs.last_mut().expect("a paragraph");
            let start = text.len();
            text.push_str(&run.text);
            if run.link || run.badge {
                marks.push((start..text.len(), run.badge));
            }
        }
        let entity = cx.entity().downgrade();
        let body = div()
            .w_full()
            .px(px(32.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .text_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .children(
                paragraphs
                    .into_iter()
                    .enumerate()
                    .map(|(index, (text, marks))| {
                        let highlights: Vec<_> = marks
                            .iter()
                            .map(|(range, badge)| {
                                let style = if *badge {
                                    gpui::HighlightStyle {
                                        color: Some(color("--white").into()),
                                        background_color: Some(color("--background-brand").into()),
                                        font_weight: Some(FontWeight(700.)),
                                        ..Default::default()
                                    }
                                } else {
                                    gpui::HighlightStyle {
                                        color: Some(color("--text-link").into()),
                                        ..Default::default()
                                    }
                                };
                                (range.clone(), style)
                            })
                            .collect();
                        let links: Vec<_> = marks
                            .iter()
                            .filter(|(_, badge)| !badge)
                            .map(|(range, _)| range.clone())
                            .collect();
                        let entity = entity.clone();
                        gpui::InteractiveText::new(
                            ("guild-cap-upsell-body", index),
                            gpui::StyledText::new(text).with_highlights(highlights),
                        )
                        .on_click(links, move |_, _, cx| {
                            // `onAndMore`: the modal closes and settings open on the Nitro panel.
                            let _ = entity.update(cx, |this, cx| {
                                this.guild_cap_upsell = false;
                                this.open_settings_page("nitro", cx);
                            });
                        })
                    }),
            );
        let cta = self.subscribe_cta(Some(dc_state::premium::sku::TIER_2));
        let disabled = cta.disabled;
        let subscribe = div()
            .id("guild-cap-upsell-subscribe")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--control-connected-background-default"))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-connected-text-default"))
            .when(!disabled, |this| this.cursor_pointer())
            .when(disabled, |this| this.opacity(0.5))
            .child(crate::ui::settings::mask_icon(
                "nitro_wheel",
                16.,
                "--control-connected-text-default",
            ))
            .child(dc_intl::tr(cta.text))
            .on_click(cx.listener(move |this, _, _, cx| {
                if disabled {
                    return;
                }
                this.guild_cap_upsell = false;
                this.open_purchase(false, cx);
            }));
        let back = div()
            .id("guild-cap-upsell-back")
            .h(px(32.))
            .px(px(16.))
            .rounded(px(8.))
            .bg(color("--control-secondary-background-default"))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--control-secondary-text-default"))
            .child(dc_intl::t("13/7kX"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.guild_cap_upsell = false;
                cx.notify();
            }));
        let art_loaded = self.images.contains_key(ART);
        let art = self
            .images
            .get(ART)
            .cloned()
            .map(|image| img(image).w(px(225.)).into_any_element());
        Some(
            deferred(
                div()
                    .id("guild-cap-upsell-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.guild_cap_upsell = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("guild-cap-upsell")
                            .occlude()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .relative()
                            .w(px(440.))
                            .mt(px(48.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .flex()
                            .flex_col()
                            .items_center()
                            // The art box is lifted only once the art has loaded (an `img` without its file has
                            // no height); until then the text starts 24px down.
                            .children(art.map(|art| {
                                div()
                                    .mt(px(-48.))
                                    .mb(px(24.))
                                    .w(px(225.))
                                    .rounded(px(8.))
                                    // `--elevation-medium` (`artContainerBoxShadow_PwiJiV`).
                                    .shadow(vec![gpui::BoxShadow {
                                        color: gpui::hsla(0., 0., 0., 0.16),
                                        offset: gpui::point(px(0.), px(4.)),
                                        blur_radius: px(4.),
                                        spread_radius: px(0.),
                                        inset: false,
                                    }])
                                    .child(art)
                            }))
                            .when(!art_loaded, |this| this.pt(px(24.)))
                            .child(
                                div()
                                    .px(px(16.))
                                    .mb(px(32.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(
                                        div()
                                            .mb(px(24.))
                                            .text_center()
                                            .text_size(px(20.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("CoNXB+")),
                                    )
                                    .child(body),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p(px(16.))
                                    .flex()
                                    .flex_row_reverse()
                                    .items_center()
                                    .justify_between()
                                    .child(subscribe)
                                    .child(back),
                            ),
                    ),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_is_a_hundred_servers_without_nitro_or_staff() {
        assert!(!at_guild_cap(99, PremiumType::None, false));
        assert!(at_guild_cap(100, PremiumType::None, false));
        assert!(at_guild_cap(100, PremiumType::Basic, false));
        assert!(!at_guild_cap(100, PremiumType::Nitro, false));
        assert!(!at_guild_cap(150, PremiumType::None, true));
    }

    #[test]
    fn the_body_has_two_paragraphs_a_badge_and_the_link() {
        let near = body_runs(95);
        let text: String = near.iter().map(|run| run.text.as_str()).collect();
        assert!(text.contains("You're in 95 servers"));
        assert_eq!(near.iter().filter(|run| run.paragraph).count(), 2);
        assert!(near.iter().any(|run| run.badge && run.text == "NEW"));
        assert!(
            near.iter()
                .any(|run| run.link && run.text == "and more with Discord Nitro!")
        );
        let at = body_runs(100);
        let text: String = at.iter().map(|run| run.text.as_str()).collect();
        assert!(text.contains("you've hit the 100 server limit"));
    }
}
