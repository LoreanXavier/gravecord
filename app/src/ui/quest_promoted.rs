//! Quest Promoted Card (Chat Embed Card, module 107543 `M` / `ef`, CSS `bvFqCe`, `mCoJa1`,
//! `CXA5dw`, `clpvxq`, `DLRZY4`) and Enrollment Blocked ("You've been busy!") countdown modal
//! (module 124232).
//!
//! Mounted inside message embeds (`embed.rs`) when a message contains a `https://discord.com/quests/{id}`
//! URL matching a quest in `QuestStore` (or via capture harness `quest-promoted` for verification).

use super::quest_copy::{self, CtaAction};
use super::quest_menu::{QuestProgressState, QuestSurface, resolve_hook_cta};
use super::quests::quest_asset_url;
use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::quests::Quest;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use std::borrow::Cow;

/// Extracts a quest id from a message containing `https://discord.com/quests/{id}` (or `ptb.` / `canary.`).
pub fn extract_quest_link_id(content: &str) -> Option<&str> {
    for token in content.split_whitespace() {
        let trimmed = token.trim_matches(|c| matches!(c, '<' | '>' | '(' | ')' | '"' | '\''));
        let rest = trimmed
            .strip_prefix("https://discord.com/quests/")
            .or_else(|| trimmed.strip_prefix("https://ptb.discord.com/quests/"))
            .or_else(|| trimmed.strip_prefix("https://canary.discord.com/quests/"))
            .or_else(|| trimmed.strip_prefix("https://discordapp.com/quests/"));
        if let Some(rest) = rest {
            let id = rest.split(['/', '?', '#']).next().unwrap_or("");
            if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                return Some(id);
            }
        }
    }
    None
}

/// Formats `124232.js` countdown string `HH:MM:SS` until `blocked_until` ISO-8601 timestamp.
pub fn format_blocked_countdown(blocked_until: &str, now_ms: i64) -> String {
    let target_ms = chrono::DateTime::parse_from_rfc3339(blocked_until)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(now_ms);
    let secs = ((target_ms - now_ms) / 1000).max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Headline copy for the promoted card (`107543.js`: `nLXlh+` or `0IUT4Y`).
pub fn promoted_reward_headline(quest: &Quest, is_nitro: bool) -> (bool, String) {
    let is_orbs = quest.config.has_virtual_currency_reward();
    if quest.is_claimed() {
        let label = if is_orbs {
            let orbs = quest.config.orb_quantity(is_nitro).unwrap_or(0) as f64;
            dc_intl::format("nLXlh+", &[("orbAmount", Arg::Num(orbs))])
        } else {
            quest.config.primary_reward_name(false)
        };
        (is_orbs, label)
    } else {
        let inner = if is_orbs {
            let orbs = quest.config.orb_quantity(is_nitro).unwrap_or(0) as f64;
            dc_intl::format("nLXlh+", &[("orbAmount", Arg::Num(orbs))])
        } else {
            quest.config.primary_reward_name(true)
        };
        // `0IUT4Y`: "Claim {rewardWithArticleHook}"
        let headline = dc_intl::format_runs(
            "0IUT4Y",
            &[("rewardWithArticle", Arg::Str(Cow::Borrowed(&inner)))],
        )
        .into_iter()
        .map(|(t, _)| t)
        .collect::<String>();
        let headline = if headline.contains(&inner) {
            headline
        } else {
            format!("Claim {inner}")
        };
        (is_orbs, headline)
    }
}

/// Fallback reason for `QUESTS_EMBED` when a linked quest cannot be shown (module 562293 / 632623 / 127219).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(super) enum QuestFallbackReason {
    NotFound,
    Excluded,
    NotShareable,
    MobileOnly,
}

impl QuestFallbackReason {
    pub(super) fn body_key(self) -> &'static str {
        match self {
            Self::Excluded => "NIliWX",
            Self::NotShareable => "Rw9rwW",
            Self::MobileOnly => "Pag1gS",
            Self::NotFound => "c9275Z",
        }
    }
}

/// Enrollment-blocked ("You've been busy!", module 124232) modal state.
#[derive(Clone, Debug)]
pub(super) struct EnrollmentBlockedModal {
    pub blocked_until: String,
}

impl Root {
    /// Module 562293 (`i`): `"This was a Quest but..."` (`vnP31W`) fallback card for `QUESTS_EMBED`
    /// (`632623` / `127219`). Dark: `--background-base-lowest` content + `--background-base-lower` art;
    /// Light: `--background-base-lower` content + `--background-mod-normal` art; `"View Quests"` (`GURBQl`) CTA.
    pub(super) fn quest_embed_fallback_card(
        &self,
        quest_id: &str,
        reason: QuestFallbackReason,
        id_prefix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let light = super::theme() == dc_tokens::Theme::Light;
        let content_bg = if light {
            color("--background-base-lower")
        } else {
            color("--background-base-lowest")
        };
        let art_bg = if light {
            color("--background-mod-normal")
        } else {
            color("--background-base-lower")
        };
        let art_url = "https://discord.com/assets/2431801debe6535e.svg";
        let art = self.images.get(art_url).cloned();
        div()
            .id(SharedString::from(format!(
                "{id_prefix}-quest-fallback-{quest_id}"
            )))
            .min_w(px(280.))
            .max_w(px(660.))
            .w_full()
            .h(px(250.))
            .mb(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_row()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(content_bg)
                    .pt(px(32.))
                    .px(px(16.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("vnP31W")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t(reason.body_key())),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "{id_prefix}-quest-fallback-cta-{quest_id}"
                            )))
                            .self_start()
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
                            .child(dc_intl::t("GURBQl"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_quests(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(art_bg)
                    .p(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match art {
                        Some(image) => gpui::img(image)
                            .h(px(180.))
                            .object_fit(gpui::ObjectFit::Contain)
                            .into_any_element(),
                        None => icon("home_quests", 64., "--icon-muted").into_any_element(),
                    }),
            )
            .into_any_element()
    }

    /// Renders a `107543` Quest Promoted Card (`min-w: 336px, max-w: 425px, rounded: 10px`).
    pub(super) fn quest_promoted_card(
        &self,
        quest: &Quest,
        id_prefix: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let expired = quest.is_expired(now_ms);
        let claimed = quest.is_claimed();
        let completed = quest.is_completed();
        let enrolled = quest.is_enrolled();
        let is_nitro = self.premium().is_subscriber();
        let qid = quest.id.clone();

        let hero_img = quest
            .config
            .assets_hero
            .as_deref()
            .and_then(|a| self.images.get(&quest_asset_url(&quest.id, a)).cloned());
        let logo_img = quest
            .config
            .assets_logotype_dark
            .as_deref()
            .or(quest.config.assets_logotype.as_deref())
            .and_then(|a| self.images.get(&quest_asset_url(&quest.id, a)).cloned());
        let reward_img = quest
            .config
            .assets_reward_tile
            .as_deref()
            .or(quest.config.assets_game_tile.as_deref())
            .and_then(|a| self.images.get(&quest_asset_url(&quest.id, a)).cloned());

        let publisher = quest
            .config
            .cosponsor_name
            .as_deref()
            .unwrap_or(quest.config.game_publisher.as_str())
            .to_owned();
        let expiry_md = quest_copy::format_month_day(&quest.config.expires_at);
        let ends_text = dc_intl::format(
            "7D8r4F",
            &[("expiryDate", Arg::Str(Cow::Borrowed(&expiry_md)))],
        );
        let quest_name_line = dc_intl::format(
            "EAYZAr",
            &[(
                "questName",
                Arg::Str(Cow::Borrowed(&quest.config.quest_name)),
            )],
        )
        .to_uppercase();
        let (is_orbs, headline) = promoted_reward_headline(quest, is_nitro);
        let description = quest_copy::card_description(
            quest,
            quest_copy::InstructionsContext {
                has_nitro: is_nitro,
                without_markdown: true,
                ..Default::default()
            },
        );
        let task_details = quest.task_details();
        let pct_label = format!(
            "{}%",
            (task_details.percent_complete * 100.0).round() as u32
        );
        let blocked_until = self.quests.store.enrollment_blocked_until.clone();

        // Header (`eE`, CSS `CXA5dw`: 150px height, 12px padding, hero + scrim)
        let menu_qid = qid.clone();
        let header = div()
            .relative()
            .w_full()
            .h(px(150.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"))
            .children(hero_img.map(|im| {
                div()
                    .absolute()
                    .inset_0()
                    .child(img(im).size_full().object_fit(ObjectFit::Cover))
            }))
            // `.overlay_CXA5dw` bottom scrim
            .child(div().absolute().inset_0().bg(linear_gradient(
                0.,
                linear_color_stop(color("--background-surface-high"), 0.),
                linear_color_stop(color("--background-surface-high").opacity(0.), 1.),
            )))
            .child(
                div()
                    .relative()
                    .size_full()
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .justify_between()
                    // `.topRow_CXA5dw`: optional Preview pill (`SKNnqq`) + `270045` context menu trigger (`DEoVWZ`)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().when(quest.preview, |this| {
                                this.px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(10.))
                                    .bg(color("--brand-500"))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("SKNnqq"))
                            }))
                            .child(
                                div()
                                    .id(SharedString::from(format!("{id_prefix}-more-{qid}")))
                                    .size(px(24.))
                                    .rounded_full()
                                    .bg(rgba(0x1112147a))
                                    .hover(|s| s.bg(rgba(0x111214b3)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(
                                        move |this, ev: &gpui::ClickEvent, _, cx| {
                                            cx.stop_propagation();
                                            let pos = ev.position();
                                            this.open_quest_menu(
                                                menu_qid.clone(),
                                                QuestSurface::QuestEmbed,
                                                true,
                                                true,
                                                true,
                                                f32::from(pos.x),
                                                f32::from(pos.y) + 6.,
                                                cx,
                                            );
                                        },
                                    ))
                                    .child(icon(
                                        "more_horizontal",
                                        16.,
                                        "--interactive-text-active",
                                    ))
                                    .when(false, |el| el.child(dc_intl::t("DEoVWZ"))),
                            ),
                    )
                    // Partner logotype (`max-h: 34px; max-w: 121px`)
                    .children(logo_img.map(|im| {
                        div()
                            .h(px(34.))
                            .flex()
                            .items_center()
                            .child(img(im).max_h(px(34.)).max_w(px(121.)))
                    }))
                    // `.bottomRow_CXA5dw`: Promoted by (`VAbKhK`) + verified badge (`OfMjx9`) + publisher | Ends M/D (`7D8r4F`)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-muted"))
                                            .child(dc_intl::t("VAbKhK")),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon(
                                                "discover_flower_check",
                                                16.,
                                                "--green-360",
                                            ))
                                            .when(false, |el| el.child(dc_intl::t("OfMjx9"))),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-overlay-light"))
                                            .child(publisher),
                                    ),
                            )
                            .when(!expired && !claimed && !expiry_md.is_empty(), |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-muted"))
                                        .child(ends_text),
                                )
                            }),
                    ),
            );

        // Body (`Z`, CSS `clpvxq`: 80x80 reward tile/progress + text column)
        let body = div()
            .relative()
            .w_full()
            .min_h(px(104.))
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .when(completed && !claimed, |this| {
                this.bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x2dc7710d), 0.),
                    linear_color_stop(rgba(0x2dc7711a), 1.),
                ))
            })
            // 80x80 reward tile / enrolled progress badge
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(80.))
                    .rounded(px(9.4))
                    .overflow_hidden()
                    .bg(color("--background-base-lower"))
                    .border_2()
                    .border_color(if completed {
                        color("--green-360")
                    } else if enrolled {
                        color("--brand-500")
                    } else {
                        color("--border-subtle")
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(reward_img.map(|im| img(im).size_full().object_fit(ObjectFit::Cover)))
                    .when(
                        quest.config.assets_reward_tile.is_none()
                            && quest.config.assets_game_tile.is_none(),
                        |this| {
                            this.child(icon(
                                if is_orbs {
                                    "shop_orb_18"
                                } else {
                                    "home_quests"
                                },
                                32.,
                                "--text-brand",
                            ))
                        },
                    )
                    .when(enrolled && !expired, |this| {
                        this.child(
                            div()
                                .absolute()
                                .bottom(px(2.))
                                .right(px(2.))
                                .px(px(5.))
                                .py(px(1.))
                                .rounded(px(6.))
                                .bg(rgba(0x111214cc))
                                .text_size(px(10.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(pct_label),
                        )
                    }),
            )
            // Right text column
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-brand"))
                            .child(quest_name_line),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .when(is_orbs, |this| {
                                this.child(icon("shop_orb_12", 14., "--text-strong"))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(15.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(headline),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(description),
                    ),
            );

        // Footer (`.container_DLRZY4`, min-h 62px, padding 12px, gap 8px, border-top)
        let footer: AnyElement = if let Some(until) = blocked_until {
            div()
                .w_full()
                .min_h(px(62.))
                .p(px(12.))
                .border_t_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .opacity(0.6)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-secondary-text-default"))
                        // `V293qn`: "Temporarily Unavailable"
                        .child(dc_intl::t("V293qn")),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("{id_prefix}-why-{qid}")))
                        .h(px(36.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-primary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.quests.enrollment_blocked_modal = Some(EnrollmentBlockedModal {
                                blocked_until: until.clone(),
                            });
                            cx.notify();
                        }))
                        // `vY9GgG`: "Why?"
                        .child(dc_intl::t("vY9GgG")),
                )
                .into_any_element()
        } else {
            let awaiting_claim = completed && !claimed;
            let left_label =
                if expired && !awaiting_claim && !quest.config.cta_button_label.is_empty() {
                    quest.config.cta_button_label.clone()
                } else {
                    // `LLLLPD`: "Learn More"
                    dc_intl::t("LLLLPD").to_owned()
                };
            let left_link = quest.config.cta_link.clone();
            let left_external = expired && !awaiting_claim;

            let right_btn: AnyElement = if completed && !expired && !claimed {
                let claim_id = qid.clone();
                div()
                    .id(SharedString::from(format!("{id_prefix}-claim-{qid}")))
                    .flex_1()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .hover(|s| s.bg(color("--control-primary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-primary-text-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.trigger_quest_claim(&claim_id, cx);
                    }))
                    // `cfY4PE`: "Claim Reward"
                    .child(dc_intl::t("cfY4PE"))
                    .into_any_element()
            } else if expired {
                let ended = dc_intl::format(
                    "6p8BZx",
                    &[("expiryDate", Arg::Str(Cow::Borrowed(&expiry_md)))],
                );
                div()
                    .flex_1()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-secondary-background-default"))
                    .opacity(0.6)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-secondary-text-default"))
                    .child(ended)
                    .into_any_element()
            } else {
                let progress_state = QuestProgressState::from_quest(quest);
                let hook = resolve_hook_cta(quest, progress_state, false, false);
                let action_opt = hook.action;
                let act_qid = qid.clone();
                let act_link = quest.config.cta_link.clone();
                div()
                    .id(SharedString::from(format!("{id_prefix}-cta-{qid}")))
                    .flex_1()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .bg(color("--control-primary-background-default"))
                    .when(action_opt.is_some(), |s| {
                        s.cursor_pointer()
                            .hover(|h| h.bg(color("--control-primary-background-hover")))
                    })
                    .when(action_opt.is_none(), |s| s.opacity(0.6))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--control-primary-text-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(action) = action_opt {
                            match action {
                                CtaAction::Enroll => {
                                    this.send_quest_enroll(&act_qid, QuestSurface::QuestEmbed, cx);
                                }
                                CtaAction::ClaimReward => {
                                    this.trigger_quest_claim(&act_qid, cx);
                                }
                                other => {
                                    this.handle_quest_cta_action(&act_qid, &act_link, other, cx);
                                }
                            }
                        }
                    }))
                    .child(hook.text)
                    .into_any_element()
            };

            div()
                .w_full()
                .min_h(px(62.))
                .p(px(12.))
                .border_t_1()
                .border_color(color("--border-subtle"))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .id(SharedString::from(format!("{id_prefix}-left-{qid}")))
                        .flex_1()
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-secondary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if left_external && !left_link.is_empty() {
                                cx.open_url(&left_link);
                            } else {
                                this.open_quests(cx);
                            }
                        }))
                        .child(left_label),
                )
                .child(right_btn)
                .into_any_element()
        };

        div()
            .id(SharedString::from(format!("{id_prefix}-card-{qid}")))
            .min_w(px(336.))
            .max_w(px(425.))
            .w(px(400.))
            .rounded(px(10.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
            .child(footer)
            .into_any_element()
    }

    /// Renders the `124232` Enrollment Blocked ("You've been busy!") countdown modal or the
    /// capture-only promoted card preview overlay.
    pub(super) fn quest_promoted_overlay_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(blocked) = self.quests.enrollment_blocked_modal.as_ref() {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let countdown = format_blocked_countdown(&blocked.blocked_until, now_ms);
            let comeback = dc_intl::format(
                "+5XVH+",
                &[("countdownString", Arg::Str(Cow::Borrowed(&countdown)))],
            );
            return Some(
                gpui::deferred(
                    div()
                        .id("quest-blocked-backdrop")
                        .occlude()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(gpui::rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quests.enrollment_blocked_modal = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("quest-blocked-modal")
                                .w(px(400.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-high"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .p(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        // `g+02d5`: "You've been busy!"
                                        .child(dc_intl::t("g+02d5")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        // `XEHDT/`: "You've done a lot of Quests today!"
                                        .child(dc_intl::t("XEHDT/")),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(comeback),
                                )
                                .child(
                                    div().mt(px(8.)).flex().justify_end().child(
                                        div()
                                            .id("quest-blocked-close")
                                            .h(px(36.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .hover(|s| {
                                                s.bg(color("--control-secondary-background-hover"))
                                            })
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quests.enrollment_blocked_modal = None;
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("cpT0Cq")),
                                    ),
                                ),
                        ),
                )
                .with_priority(7)
                .into_any_element(),
            );
        }

        // Capture-only promoted card showcase overlay
        let qid = self.quests.promoted_preview_quest.as_ref()?;
        let quest = self.quests.store.quests.iter().find(|q| &q.id == qid)?;
        Some(
            gpui::deferred(
                div()
                    .id("quest-promoted-preview-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(gpui::rgba(0x00000099))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quests.promoted_preview_quest = None;
                        cx.notify();
                    }))
                    .child(self.quest_promoted_card(quest, "promoted-preview", cx)),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture-only helper: `quest-promoted <open|blocked|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_promoted(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "open" => {
                if let Some(q) = self.quests.store.quests.first() {
                    self.quests.promoted_preview_quest = Some(q.id.clone());
                    self.quests.enrollment_blocked_modal = None;
                    cx.notify();
                    format!("quest-promoted: open {}", q.id)
                } else {
                    "quest-promoted: no quest in store".into()
                }
            }
            "blocked" => {
                let until = (chrono::Utc::now()
                    + chrono::Duration::hours(2)
                    + chrono::Duration::minutes(15))
                .to_rfc3339();
                self.quests.enrollment_blocked_modal = Some(EnrollmentBlockedModal {
                    blocked_until: until,
                });
                cx.notify();
                "quest-promoted: blocked modal".into()
            }
            "close" => {
                self.quests.promoted_preview_quest = None;
                self.quests.enrollment_blocked_modal = None;
                cx.notify();
                "quest-promoted: closed".into()
            }
            other => format!("quest-promoted: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quest_link_extraction_handles_discord_urls() {
        assert_eq!(
            extract_quest_link_id("check out https://discord.com/quests/123456789012345678 now!"),
            Some("123456789012345678")
        );
        assert_eq!(
            extract_quest_link_id("<https://canary.discord.com/quests/999888?foo=1>"),
            Some("999888")
        );
        assert_eq!(
            extract_quest_link_id("https://example.com/quests/123"),
            None
        );
    }

    #[test]
    fn promoted_headline_and_countdown_format_cleanly() {
        let q = Quest::from_value(&json!({
            "id": "55",
            "config": {
                "config_version": 2,
                "id": "55",
                "starts_at": "2026-01-01T00:00:00Z",
                "expires_at": "2027-01-01T00:00:00Z",
                "messages": {"quest_name": "Orbital", "game_title": "Halo", "game_publisher": "Xbox"},
                "task_config_v2": {"tasks": {"PLAY_ON_DESKTOP": {"type": "PLAY_ON_DESKTOP", "target": 900}}},
                "rewards_config": {
                    "rewards": [{"type": 4, "orb_quantity": 700}],
                    "platforms": [0]
                }
            }
        }))
        .unwrap();
        let (is_orbs, headline) = promoted_reward_headline(&q, false);
        assert!(is_orbs);
        assert!(headline.contains("700"));
        let cd = format_blocked_countdown("2026-01-01T03:05:09Z", 1_767_225_600_000);
        assert_eq!(cd, "03:05:09");
    }
}
