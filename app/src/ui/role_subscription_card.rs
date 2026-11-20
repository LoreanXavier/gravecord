//! Role Subscription Tier Card, benefit lists, trial pill, and purchase confirmation Welcome modal
//! (module 31637, period formatter 500345).

use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img, prelude::*, px,
    rgba,
};
use std::borrow::Cow;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct TierBenefit {
    pub name: String,
    pub description: String,
    pub emoji: Option<String>,
    pub channel_id: Option<Id>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct TierEmoji {
    pub id: String,
    pub name: String,
    pub animated: bool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct TierTrial {
    pub interval: TrialInterval,
    pub interval_count: u32,
    pub active_trial_user_limit: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TrialInterval {
    Day,
    Week,
    Month,
    Year,
}

/// Formats a subscription/trial interval (`500345` `Gg`/`cV`: `jzH70Z`, `iVZYyl`, `erUSmA`, `IfYQVC`).
pub(crate) fn format_trial_duration(interval: TrialInterval, count: u32) -> String {
    match interval {
        TrialInterval::Day => dc_intl::format("jzH70Z", &[("days", Arg::Num(count as f64))]),
        TrialInterval::Week => dc_intl::format("iVZYyl", &[("weeks", Arg::Num(count as f64))]),
        TrialInterval::Month => dc_intl::format("erUSmA", &[("months", Arg::Num(count as f64))]),
        TrialInterval::Year => dc_intl::format("IfYQVC", &[("years", Arg::Num(count as f64))]),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TierDisabledReason {
    ApprovalRequired,
    PendingChangeToThisTier(String),
    PendingChangeElsewhere,
    ActiveTrial,
    AppleMobileOnly,
}

impl TierDisabledReason {
    /// Official `cannotOpenReason` copy (`31637` `tE`).
    pub(crate) fn message(&self) -> String {
        match self {
            Self::ApprovalRequired => dc_intl::t("pQK5ho").to_owned(),
            Self::PendingChangeToThisTier(date) => {
                dc_intl::format("UlBRTl", &[("changeDate", Arg::Str(Cow::Borrowed(date)))])
            }
            Self::PendingChangeElsewhere => dc_intl::t("ePFYOS").to_owned(),
            Self::ActiveTrial => dc_intl::t("0lPoT2").to_owned(),
            Self::AppleMobileOnly => dc_intl::t("cEMaCt").to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TierSubscriptionState {
    Available,
    Subscribed,
    Canceled,
    Disabled(TierDisabledReason),
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct RoleSubscriptionTier {
    pub id: String,
    pub name: String,
    pub price_label: String,
    pub period_label: String,
    pub description: String,
    pub image_url: Option<String>,
    pub published: bool,
    pub role_name: Option<String>,
    pub role_color: Option<u32>,
    pub exclusive_channels: Vec<TierBenefit>,
    pub additional_benefits: Vec<TierBenefit>,
    pub emojis: Vec<TierEmoji>,
    pub trial: Option<TierTrial>,
    pub state: TierSubscriptionState,
    pub trial_ineligible_warning: bool,
    pub desc_expanded: bool,
    pub channels_expanded: bool,
    pub benefits_expanded: bool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum RoleSubView {
    TierCards,
    Welcome {
        tier_name: String,
        include_flair: bool,
        channel_count: usize,
        benefit_count: usize,
        emoji_count: usize,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct RoleSubscriptionModal {
    pub guild_id: Id,
    pub server_name: String,
    pub tiers: Vec<RoleSubscriptionTier>,
    pub view: RoleSubView,
    pub show_trial_tooltip_for: Option<usize>,
}

/// Official `tb` natural-language benefits joiner (`31637` `tN`).
pub(crate) fn format_welcome_benefits(
    include_flair: bool,
    channel_count: usize,
    benefit_count: usize,
    emoji_count: usize,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if include_flair {
        parts.push(dc_intl::t("EhdV2x").to_owned());
    }
    if channel_count > 0 {
        parts.push(dc_intl::format(
            "p/l+BI",
            &[("channelCount", Arg::Num(channel_count as f64))],
        ));
    }
    if benefit_count > 0 {
        parts.push(dc_intl::format(
            "eXP5vt",
            &[("benefitCount", Arg::Num(benefit_count as f64))],
        ));
    }
    if emoji_count > 0 {
        parts.push(dc_intl::format(
            "YjmQwW",
            &[("emojiCount", Arg::Num(emoji_count as f64))],
        ));
    }
    let joined = match parts.len() {
        0 => return None,
        1 => parts.remove(0),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => {
            let last = parts.pop().unwrap_or_default();
            format!("{}, and {last}", parts.join(", "))
        }
    };
    Some(dc_intl::format(
        "TnhFHv",
        &[("benefits", Arg::Str(Cow::Owned(joined)))],
    ))
}

impl Root {
    pub(crate) fn close_role_subscription_modal(&mut self) -> bool {
        self.role_subscription_modal.take().is_some()
    }

    fn render_benefit_section(
        &self,
        tier_idx: usize,
        is_channels: bool,
        header: &'static str,
        benefits: &[TierBenefit],
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if benefits.is_empty() {
            return None;
        }
        let shown = if expanded {
            benefits
        } else {
            &benefits[..benefits.len().min(5)]
        };
        let truncated = benefits.len().saturating_sub(5);
        let toggle_id = SharedString::from(format!(
            "role-sub-toggle-{tier_idx}-{}",
            if is_channels { "ch" } else { "ben" }
        ));
        let toggle_text = if expanded {
            dc_intl::t("r/xxPU").to_owned()
        } else {
            dc_intl::format("XSdy72", &[("numTruncated", Arg::Num(truncated as f64))])
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t(header).to_uppercase()),
                )
                .children(shown.iter().map(|b| {
                    div()
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
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-strong"))
                                        .child(b.name.clone()),
                                )
                                .when(!b.description.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_size(px(13.))
                                            .line_height(px(18.))
                                            .text_color(color("--text-muted"))
                                            .child(b.description.clone()),
                                    )
                                }),
                        )
                        .when_some(b.emoji.clone(), |row, em| {
                            row.child(div().text_size(px(18.)).child(em))
                        })
                }))
                .when(truncated > 0, |col| {
                    col.child(
                        div()
                            .id(toggle_id)
                            .self_start()
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .cursor_pointer()
                            .text_size(px(12.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(toggle_text)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.role_subscription_modal.as_mut()
                                    && let Some(t) = m.tiers.get_mut(tier_idx)
                                {
                                    if is_channels {
                                        t.channels_expanded = !t.channels_expanded;
                                    } else {
                                        t.benefits_expanded = !t.benefits_expanded;
                                    }
                                }
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }

    fn render_tier_card(
        &self,
        idx: usize,
        tier: &RoleSubscriptionTier,
        show_trial_tip: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let per_period = dc_intl::format(
            "isLGyX",
            &[("period", Arg::Str(Cow::Borrowed(&tier.period_label)))],
        );
        let trial_banner = tier.trial.as_ref().map(|trial| {
            let dur = format_trial_duration(trial.interval, trial.interval_count);
            let pill = dc_intl::format(
                "L2ouip",
                &[("trialPeriodDuration", Arg::Str(Cow::Owned(dur)))],
            );
            let tip = dc_intl::format(
                "FYyR4o",
                &[(
                    "activeTrialUserLimit",
                    Arg::Num(trial.active_trial_user_limit as f64),
                )],
            );
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .px(px(12.))
                        .py(px(6.))
                        .rounded_t(px(12.))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child(pill)
                        .child(
                            div()
                                .id(SharedString::from(format!("role-sub-trial-info-{idx}")))
                                .cursor_pointer()
                                .child(icon("circle_info", 14., "--white"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(m) = this.role_subscription_modal.as_mut() {
                                        m.show_trial_tooltip_for =
                                            if m.show_trial_tooltip_for == Some(idx) {
                                                None
                                            } else {
                                                Some(idx)
                                            };
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .when(show_trial_tip, |col| {
                    col.child(
                        div()
                            .mx(px(12.))
                            .p(px(8.))
                            .rounded(px(6.))
                            .bg(color("--background-base-lowest"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(tip),
                    )
                })
        });

        let cta_block: AnyElement = match &tier.state {
            TierSubscriptionState::Subscribed => div()
                .h(px(38.))
                .w_full()
                .rounded(px(8.))
                .bg(color("--control-primary-background-default"))
                .opacity(0.6)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(dc_intl::t("XvAuMo"))
                .into_any_element(),
            TierSubscriptionState::Canceled => div()
                .h(px(38.))
                .w_full()
                .rounded(px(8.))
                .bg(color("--control-secondary-background-default"))
                .opacity(0.6)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(dc_intl::t("iKHmu0"))
                .into_any_element(),
            TierSubscriptionState::Disabled(reason) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .p(px(10.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-default"))
                        .child(reason.message()),
                )
                .child(
                    div()
                        .h(px(38.))
                        .w_full()
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .opacity(0.5)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("fM5ZsX")),
                )
                .into_any_element(),
            TierSubscriptionState::Available => {
                let tier_name = tier.name.clone();
                let include_flair = tier.role_name.is_some();
                let ch_count = tier.exclusive_channels.len();
                let ben_count = tier.additional_benefits.len();
                let em_count = tier.emojis.len();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(tier.trial_ineligible_warning, |col| {
                        col.child(
                            div()
                                .p(px(8.))
                                .rounded(px(6.))
                                .bg(color("--background-surface-higher"))
                                .text_size(px(12.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("vuvsK5")),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("role-sub-subscribe-{idx}")))
                            .h(px(38.))
                            .w_full()
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("fM5ZsX"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.role_subscription_modal.as_mut() {
                                    m.view = RoleSubView::Welcome {
                                        tier_name: tier_name.clone(),
                                        include_flair,
                                        channel_count: ch_count,
                                        benefit_count: ben_count,
                                        emoji_count: em_count,
                                    };
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
        };

        let desc_long = tier.description.chars().count() > 96;
        let desc_shown = if desc_long && !tier.desc_expanded {
            let cut: String = tier.description.chars().take(96).collect();
            format!("{cut}…")
        } else {
            tier.description.clone()
        };

        let role_flair = tier.role_name.as_ref().map(|rname| {
            let dot = tier.role_color.unwrap_or(0x5865f2);
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("FJZmYx").to_uppercase()),
                )
                .child(
                    div()
                        .self_start()
                        .px(px(10.))
                        .py(px(4.))
                        .rounded_full()
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().size(px(10.)).rounded_full().bg(gpui::rgb(dot)))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(rname.clone()),
                        ),
                )
        });

        let emoji_section = (!tier.emojis.is_empty()).then(|| {
            let header =
                dc_intl::format("NAnXEo", &[("count", Arg::Num(tier.emojis.len() as f64))]);
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-muted"))
                        .child(header.to_uppercase()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(tier.emojis.iter().map(|em| {
                            let url = format!(
                                "https://cdn.discordapp.com/emojis/{}.{}?size=48",
                                em.id,
                                if em.animated { "gif" } else { "webp" }
                            );
                            let pic = self.images.get(&url).cloned();
                            div()
                                .size(px(28.))
                                .rounded(px(6.))
                                .bg(color("--background-surface-higher"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .when_some(pic, |b, p| b.child(img(p).size(px(24.))))
                                .when(!self.images.contains_key(&url), |b| {
                                    b.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(em.name.chars().take(2).collect::<String>()),
                                    )
                                })
                        })),
                )
        });

        div()
            .w(px(300.))
            .flex_none()
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-lower"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(trial_banner)
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(tier.name.clone()),
                    )
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(16.))
                            .bg(color("--background-surface-higher"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                "boost_gem",
                                32.,
                                "--control-primary-background-default",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(26.))
                                    .line_height(px(30.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(tier.price_label.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(per_period),
                            ),
                    )
                    .child(cta_block)
                    .when(!desc_shown.is_empty(), |col| {
                        col.child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .text_center()
                                .child(desc_shown),
                        )
                        .when(desc_long, |col| {
                            col.child(
                                div()
                                    .id(SharedString::from(format!("role-sub-desc-{idx}")))
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(if tier.desc_expanded {
                                        dc_intl::t("r/xxPU")
                                    } else {
                                        dc_intl::t("KzmEA9")
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(m) = this.role_subscription_modal.as_mut()
                                            && let Some(t) = m.tiers.get_mut(idx)
                                        {
                                            t.desc_expanded = !t.desc_expanded;
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                    }),
            )
            .child(div().h(px(1.)).w_full().bg(color("--border-subtle")))
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .children(role_flair)
                    .children(self.render_benefit_section(
                        idx,
                        true,
                        "LtfhAj",
                        &tier.exclusive_channels,
                        tier.channels_expanded,
                        cx,
                    ))
                    .children(self.render_benefit_section(
                        idx,
                        false,
                        "8oxWpO",
                        &tier.additional_benefits,
                        tier.benefits_expanded,
                        cx,
                    ))
                    .children(emoji_section),
            )
            .into_any_element()
    }

    pub(in crate::ui) fn role_subscription_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.role_subscription_modal.as_ref()?;
        let body: AnyElement = match &modal.view {
            RoleSubView::TierCards => {
                let cards: Vec<AnyElement> = modal
                    .tiers
                    .iter()
                    .enumerate()
                    .map(|(idx, tier)| {
                        self.render_tier_card(
                            idx,
                            tier,
                            modal.show_trial_tooltip_for == Some(idx),
                            cx,
                        )
                    })
                    .collect();
                div()
                    .id("role-sub-cards-modal")
                    .occlude()
                    .max_w(px(980.))
                    .max_h(px(640.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.3),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(28.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(18.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(modal.server_name.clone()),
                            )
                            .child(
                                div()
                                    .id("role-sub-close")
                                    .size(px(32.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 18., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_role_subscription_modal();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("role-sub-cards-scroll")
                            .p(px(20.))
                            .overflow_y_scroll()
                            .flex()
                            .items_start()
                            .gap(px(16.))
                            .children(cards),
                    )
                    .into_any_element()
            }
            RoleSubView::Welcome {
                tier_name,
                include_flair,
                channel_count,
                benefit_count,
                emoji_count,
            } => {
                let heading = dc_intl::format(
                    "I5dYFs",
                    &[
                        ("serverName", Arg::Str(Cow::Borrowed(&modal.server_name))),
                        ("tierName", Arg::Str(Cow::Borrowed(tier_name))),
                    ],
                );
                let benefits = format_welcome_benefits(
                    *include_flair,
                    *channel_count,
                    *benefit_count,
                    *emoji_count,
                );
                let gid = modal.guild_id;
                div()
                    .id("role-sub-welcome-modal")
                    .occlude()
                    .w(px(420.))
                    .p(px(24.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.3),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(28.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(icon(
                        "boost_gem",
                        44.,
                        "--control-primary-background-default",
                    ))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .text_center()
                            .child(heading),
                    )
                    .when_some(benefits, |col, summary| {
                        col.child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .text_center()
                                .child(summary),
                        )
                    })
                    .child(
                        div()
                            .mt(px(8.))
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .id("role-sub-welcome-open")
                                    .h(px(38.))
                                    .w_full()
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child(dc_intl::t("WWvswD"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.close_role_subscription_modal();
                                        if this.stores.guild(gid).is_some() {
                                            this.select(gid, None);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("role-sub-welcome-done")
                                    .h(px(38.))
                                    .w_full()
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("y84Dhr"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_role_subscription_modal();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        };

        Some(
            deferred(
                div()
                    .id("role-sub-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_role_subscription_modal();
                        cx.notify();
                    }))
                    .child(body),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_role_sub(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1568692564637524109);
        let server_name = self
            .stores
            .guild(guild_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "gravecord fixture".to_owned());
        let tiers = vec![
            RoleSubscriptionTier {
                id: "tier_1".to_owned(),
                name: "Supporter".to_owned(),
                price_label: "$4.99".to_owned(),
                period_label: "month".to_owned(),
                description: "Support the community and unlock subscriber channels, early dev notes, and custom badges across every channel.".to_owned(),
                image_url: None,
                published: true,
                role_name: Some("Supporter".to_owned()),
                role_color: Some(0x5865f2),
                exclusive_channels: vec![
                    TierBenefit {
                        name: "#supporter-lounge".to_owned(),
                        description: "Hang out with fellow supporters and devs.".to_owned(),
                        emoji: Some("💬".to_owned()),
                        channel_id: None,
                    },
                    TierBenefit {
                        name: "#behind-the-scenes".to_owned(),
                        description: "Sneak peeks and work-in-progress builds.".to_owned(),
                        emoji: Some("🛠️".to_owned()),
                        channel_id: None,
                    },
                ],
                additional_benefits: vec![TierBenefit {
                    name: "Priority Q&A".to_owned(),
                    description: "Submit questions for monthly livestreams.".to_owned(),
                    emoji: Some("⭐".to_owned()),
                    channel_id: None,
                }],
                emojis: vec![
                    TierEmoji {
                        id: "1".to_owned(),
                        name: "gg".to_owned(),
                        animated: false,
                    },
                    TierEmoji {
                        id: "2".to_owned(),
                        name: "hype".to_owned(),
                        animated: false,
                    },
                ],
                trial: Some(TierTrial {
                    interval: TrialInterval::Day,
                    interval_count: 7,
                    active_trial_user_limit: 50,
                }),
                state: TierSubscriptionState::Available,
                trial_ineligible_warning: false,
                desc_expanded: false,
                channels_expanded: false,
                benefits_expanded: false,
            },
            RoleSubscriptionTier {
                id: "tier_2".to_owned(),
                name: "VIP".to_owned(),
                price_label: "$9.99".to_owned(),
                period_label: "month".to_owned(),
                description: "Everything in Supporter plus voice hangouts and custom role flair.".to_owned(),
                image_url: None,
                published: true,
                role_name: Some("VIP Member".to_owned()),
                role_color: Some(0xeb459e),
                exclusive_channels: vec![TierBenefit {
                    name: "#vip-voice".to_owned(),
                    description: "Weekly community voice sessions.".to_owned(),
                    emoji: Some("🎙️".to_owned()),
                    channel_id: None,
                }],
                additional_benefits: vec![],
                emojis: vec![],
                trial: None,
                state: TierSubscriptionState::Subscribed,
                trial_ineligible_warning: false,
                desc_expanded: false,
                channels_expanded: false,
                benefits_expanded: false,
            },
            RoleSubscriptionTier {
                id: "tier_3".to_owned(),
                name: "Founder".to_owned(),
                price_label: "$19.99".to_owned(),
                period_label: "month".to_owned(),
                description: "Top tier community patronage with dedicated credits.".to_owned(),
                image_url: None,
                published: true,
                role_name: Some("Founder".to_owned()),
                role_color: Some(0xfee75c),
                exclusive_channels: vec![],
                additional_benefits: vec![TierBenefit {
                    name: "Credits Roll".to_owned(),
                    description: "Listed in every release changelog.".to_owned(),
                    emoji: Some("🏆".to_owned()),
                    channel_id: None,
                }],
                emojis: vec![],
                trial: None,
                state: TierSubscriptionState::Disabled(
                    TierDisabledReason::PendingChangeToThisTier("Dec 01, 2026".to_owned()),
                ),
                trial_ineligible_warning: false,
                desc_expanded: false,
                channels_expanded: false,
                benefits_expanded: false,
            },
        ];
        let view = if arg == "welcome" {
            RoleSubView::Welcome {
                tier_name: "Supporter".to_owned(),
                include_flair: true,
                channel_count: 2,
                benefit_count: 1,
                emoji_count: 2,
            }
        } else {
            RoleSubView::TierCards
        };
        self.role_subscription_modal = Some(RoleSubscriptionModal {
            guild_id,
            server_name,
            tiers,
            view,
            show_trial_tooltip_for: if arg == "tip" { Some(0) } else { None },
        });
        cx.notify();
        format!("role-sub: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_subscription_copy_and_welcome_summary_match_31637() {
        assert_eq!(format_trial_duration(TrialInterval::Day, 7), "7 Days");
        assert_eq!(format_trial_duration(TrialInterval::Week, 1), "1 Week");
        assert_eq!(format_trial_duration(TrialInterval::Month, 1), "1 Month");
        assert_eq!(format_trial_duration(TrialInterval::Year, 1), "1 Year");

        let summary = format_welcome_benefits(true, 3, 2, 5).unwrap();
        assert!(summary.contains("visual flair"));
        assert!(summary.contains("3 exclusive channels"));
        assert!(summary.contains("2 benefits"));
        assert!(summary.contains("5 emoji"));

        for reason in [
            TierDisabledReason::ApprovalRequired,
            TierDisabledReason::PendingChangeToThisTier("Dec 01, 2026".to_owned()),
            TierDisabledReason::PendingChangeElsewhere,
            TierDisabledReason::ActiveTrial,
            TierDisabledReason::AppleMobileOnly,
        ] {
            assert!(!reason.message().is_empty());
        }
    }
}
