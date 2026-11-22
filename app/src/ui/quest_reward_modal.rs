//! Quest Completion / Reward Split Modal (module 670092), In-Game Reward Claimed Panel
//! (module 529398), and Members List Quest Hover/Expand Popout Card (module 198525).
//!
//! - `670092`: Opened when claiming/viewing a completed Quest reward (`quest_menu.rs`). Displays
//!   Loading (`ZTNur7`), Reward Error (`Hd3D8W`, `Ij97SL`, `cpT0Cq`), or the 2-column split card
//!   with `{questName} Quest` (`EAYZAr`), `{name} Logo` (`rtm15P`), reward instructions, right-panel
//!   game/CTA copy (`y8Xf3k`, `wirwN+`, reusing `quest_split_modal::resolve_right_panel_copy` when
//!   backed by a live `Quest`), and "Copy Quest Link" (`WmfZHZ` -> `share_quest_link`).
//! - `198525`: Members List Quest Hover/Expand Popout Card for a member playing a promoted Quest
//!   game: Completed (`gHerLS` "You did it!", `cfY4PE` "Claim Reward"), InProgress (`uH2sf2`
//!   "Finish your Quest!", `VN1Ajl` "Check Progress"), Available (`Bz6SkH` "Quest Available!",
//!   `BXFP30` "Join Me" / `BSXPZ+` "Unlock Rewards", contextual action CTA `+qoymD`/`Ie9++s`/
//!   `E4kW5O`/`l7E81v`/`hRIVy+`/`CkUzLd`), plus `Lm8/mH` ("Promoted by {gamePublisher}") and
//!   accessible label `IskzPg`.

use super::{Root, color, icon, quest_split_modal};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, StyledText, deferred, div,
    linear_color_stop, linear_gradient, prelude::*, px, rgba,
};
use serde_json::json;
use std::borrow::Cow;

/// `POST /quests/{questId}/claim-reward` (module 639852 / 343559).
pub(crate) fn claim_quest_reward_request(quest_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("quest:claim-reward:{quest_id}")),
        method: reqwest::Method::POST,
        route: format!("/quests/{quest_id}/claim-reward"),
        body: Some(json!({ "platform": 0, "location": 4 })),
    }
}

/// `GET /quests/{questId}/reward-code` (module 639852 / 343559).
pub(crate) fn fetch_quest_reward_code_request(quest_id: &str) -> Request {
    Request::Rest {
        key: Some(format!("quest:reward-code:{quest_id}")),
        method: reqwest::Method::GET,
        route: format!("/quests/{quest_id}/reward-code"),
        body: None,
    }
}

fn claim_instructions_link_styled() -> StyledText {
    let runs = dc_intl::format_runs("ESmp2+", &[]);
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (chunk, hook) in runs {
        let start = text.len();
        text.push_str(&chunk);
        if hook.as_deref() == Some("$link") && start < text.len() {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

/// Code Claim Instructions sub-modal (module 639852 `U`: `NkZ7OU`, `srzsU2`, `+zx47d`, `/g10LC`).
#[derive(Clone, Debug)]
pub(crate) struct QuestCodeInstructionsModal {
    pub reward_code: String,
    pub instructions: String,
    pub redemption_link: Option<String>,
}

/// Active state of the Quest Reward Split Modal (`670092`).
#[derive(Clone, Debug)]
#[allow(dead_code, clippy::large_enum_variant)]
pub(crate) enum QuestRewardModalState {
    Loading {
        quest_id: String,
    },
    Error {
        quest_id: String,
    },
    Claimed {
        quest_id: String,
        quest_name: String,
        game_title: String,
        publisher: String,
        reward_name: String,
        reward_code: Option<String>,
        is_game_quest: bool,
        cta_label: String,
        cta_url: String,
        /// Module 459624 Orbs Balance Celebration (`WPNjGe`, `YMor7k`, `g4t9aw`, `WYchde`, `Lmysvd`, `loyTil`).
        orbs_earned: Option<u64>,
        orbs_balance: Option<u64>,
        /// Module 529398 In-Game Reward Claimed panel (`0/Yz+Y`, `v1u/zq`, `OD6Tvf`, `loyTil`).
        is_in_game_reward: bool,
        reward_asset_url: Option<String>,
        /// Module 651663 Quest Collectible Avatar Decoration Reward Claim Modal (`0/Yz+Y`, `MAS7uK`, `hjaYYn`).
        collectible_sku_id: Option<String>,
        collectible_applied: bool,
        collectible_expiry_copy: Option<String>,
    },
}

/// Primary CTA label variant on the Members List Quest popout (`198525`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MemberQuestCta {
    TryInDiscord,
    PlayNowInDiscord,
    PlayNow,
    AcceptQuest,
    LaunchQuestInDiscord,
    LaunchQuest,
}

impl MemberQuestCta {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TryInDiscord => dc_intl::t("+qoymD"),
            Self::PlayNowInDiscord => dc_intl::t("Ie9++s"),
            Self::PlayNow => dc_intl::t("E4kW5O"),
            Self::AcceptQuest => dc_intl::t("l7E81v"),
            Self::LaunchQuestInDiscord => dc_intl::t("hRIVy+"),
            Self::LaunchQuest => dc_intl::t("CkUzLd"),
        }
    }
}

/// Progress state of the Members List Quest Popout Card (`198525`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum MemberQuestStatus {
    Completed,
    InProgress { percent: u8 },
    Available { joinable: bool, cta: MemberQuestCta },
}

/// Data backing the Members List Quest Popout Card (`198525`).
#[derive(Clone, Debug)]
pub(crate) struct MemberQuestPopout {
    pub quest_id: String,
    pub game_title: String,
    pub game_publisher: String,
    pub reward_summary: String,
    pub status: MemberQuestStatus,
}

impl MemberQuestPopout {
    pub(crate) fn headline(&self) -> &'static str {
        match self.status {
            MemberQuestStatus::Completed => dc_intl::t("gHerLS"),
            MemberQuestStatus::InProgress { .. } => dc_intl::t("uH2sf2"),
            MemberQuestStatus::Available { .. } => dc_intl::t("Bz6SkH"),
        }
    }

    pub(crate) fn primary_cta(&self) -> &'static str {
        match self.status {
            MemberQuestStatus::Completed => dc_intl::t("cfY4PE"),
            MemberQuestStatus::InProgress { .. } => dc_intl::t("VN1Ajl"),
            MemberQuestStatus::Available { cta, .. } => cta.label(),
        }
    }

    pub(crate) fn promoted_by(&self) -> String {
        dc_intl::format(
            "Lm8/mH",
            &[(
                "gamePublisher",
                Arg::Str(Cow::Borrowed(&self.game_publisher)),
            )],
        )
    }

    pub(crate) fn tile_aria_label(&self) -> String {
        dc_intl::format(
            "IskzPg",
            &[
                ("gameTitle", Arg::Str(Cow::Borrowed(&self.game_title))),
                (
                    "gamePublisher",
                    Arg::Str(Cow::Borrowed(&self.game_publisher)),
                ),
            ],
        )
    }
}

impl Root {
    /// Opens the Quest Completion / Reward modal (`670092` / `639852`) for a quest in `self.quests.store`.
    pub(super) fn open_quest_reward_modal(&mut self, quest_id: &str, cx: &mut Context<Self>) {
        let premium = self.premium();
        if let Some(quest) = self.quests.store.quests.iter().find(|q| q.id == quest_id) {
            let right = quest_split_modal::resolve_right_panel_copy(quest, premium);
            let reward_name = quest
                .config
                .rewards
                .first()
                .map(|r| r.name.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Quest Reward".to_owned());
            let _ = self.requests.send(claim_quest_reward_request(&quest.id));
            let is_in_game_reward = quest.config.has_reward(dc_state::quests::reward::IN_GAME);
            let collectible_reward = quest
                .config
                .rewards
                .iter()
                .find(|r| r.kind == dc_state::quests::reward::COLLECTIBLE);
            let collectible_sku_id = collectible_reward.and_then(|r| {
                r.sku_id
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| Some(quest.id.clone()))
            });
            if let Some(sku_id) = collectible_sku_id.as_deref() {
                let _ = self.requests.send(Request::Fetch {
                    key: format!("collectible-product:{sku_id}"),
                    route: format!("/collectibles-products/{sku_id}"),
                });
            }
            let collectible_expiry_copy = collectible_sku_id.as_ref().map(|_| {
                collectible_reward_expiry_copy(
                    &reward_name,
                    premium == dc_state::premium::PremiumType::Nitro,
                    None,
                )
            });
            let needs_code = quest
                .config
                .has_reward(dc_state::quests::reward::REWARD_CODE)
                || (!is_in_game_reward
                    && collectible_sku_id.is_none()
                    && !quest.config.has_virtual_currency_reward());
            if needs_code {
                let _ = self
                    .requests
                    .send(fetch_quest_reward_code_request(&quest.id));
            }
            let orbs_earned = quest.config.has_virtual_currency_reward().then(|| {
                quest
                    .config
                    .orb_quantity(premium == dc_state::premium::PremiumType::Nitro)
                    .unwrap_or(0)
                    .max(1)
            });
            let orbs_balance =
                orbs_earned.map(|earned| self.quests.currency.balance.unwrap_or(0).max(earned));
            let reward_asset_url = quest
                .config
                .assets_reward_tile
                .clone()
                .filter(|s| !s.is_empty());
            self.quest_reward_modal = Some(QuestRewardModalState::Claimed {
                quest_id: quest.id.clone(),
                quest_name: quest.config.quest_name.clone(),
                game_title: quest.config.game_title.clone(),
                publisher: quest.config.game_publisher.clone(),
                reward_name,
                reward_code: None,
                is_game_quest: !quest.config.game_title.is_empty(),
                cta_label: right.cta_label,
                cta_url: quest.config.cta_link.clone(),
                orbs_earned,
                orbs_balance,
                is_in_game_reward,
                reward_asset_url,
                collectible_sku_id,
                collectible_applied: false,
                collectible_expiry_copy,
            });
        } else {
            self.quest_reward_modal = Some(QuestRewardModalState::Error {
                quest_id: quest_id.to_owned(),
            });
        }
        cx.notify();
    }

    /// Renders the Quest Reward Split Modal (`670092`), Code Claim Instructions sub-modal
    /// (`639852`), and/or Members List Quest Popout (`198525`).
    pub(super) fn quest_reward_and_member_popout_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(instr) = self.quest_code_instructions_modal.as_ref() {
            return Some(self.render_quest_code_instructions_modal(instr, cx));
        }
        if let Some(state) = self.quest_reward_modal.as_ref() {
            return Some(self.render_quest_reward_modal(state, cx));
        }
        if let Some(popout) = self.member_quest_popout.as_ref() {
            return Some(self.render_member_quest_popout(popout, cx));
        }
        None
    }

    /// Renders the Code Claim Instructions sub-modal (`U` of module 639852: `NkZ7OU`, `srzsU2`,
    /// `0n2u0k`, `+zx47d`, `/g10LC`).
    fn render_quest_code_instructions_modal(
        &self,
        instr: &QuestCodeInstructionsModal,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let masked = self.streamer_hides_personal();
        let shown_code = if masked {
            dc_intl::t("0n2u0k").to_owned()
        } else {
            instr.reward_code.clone()
        };
        let code_copy = instr.reward_code.clone();
        let redeem_url = instr
            .redemption_link
            .as_ref()
            .map(|tpl| tpl.replace("{code}", &instr.reward_code));

        deferred(
            div()
                .id("quest-code-instructions-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.quest_code_instructions_modal = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("quest-code-instructions-modal")
                        .w(px(480.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(24.))
                                .pt(px(20.))
                                .pb(px(12.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("NkZ7OU")),
                                )
                                .child(
                                    div()
                                        .id("quest-code-instructions-close")
                                        .size(px(30.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("modal_close", 18., "--icon-subtle"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quest_code_instructions_modal = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .pb(px(20.))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(instr.instructions.clone()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight(700.))
                                                .text_color(color("--text-muted"))
                                                .child(dc_intl::t("srzsU2").to_uppercase()),
                                        )
                                        .child(
                                            div()
                                                .h(px(44.))
                                                .px(px(12.))
                                                .rounded(px(8.))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .bg(color("--background-base-lowest"))
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(10.))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_size(px(14.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .truncate()
                                                        .child(shown_code),
                                                )
                                                .child(
                                                    div()
                                                        .id("quest-code-instructions-copy")
                                                        .cursor_pointer()
                                                        .px(px(12.))
                                                        .h(px(28.))
                                                        .rounded(px(6.))
                                                        .bg(color(
                                                            "--control-primary-background-default",
                                                        ))
                                                        .flex()
                                                        .items_center()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--white"))
                                                        .child(dc_intl::tr("Copy"))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                cx.write_to_clipboard(
                                                                    gpui::ClipboardItem::new_string(
                                                                        code_copy.clone(),
                                                                    ),
                                                                );
                                                                this.put_app_toast(
                                                                    dc_intl::t("+5kSoW").to_owned(),
                                                                    true,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .px(px(24.))
                                .py(px(14.))
                                .border_t_1()
                                .border_color(color("--border-subtle"))
                                .bg(color("--background-base-lower"))
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(10.))
                                .child({
                                    let has_redeem = redeem_url.is_some();
                                    div()
                                        .id("quest-code-instructions-primary")
                                        .cursor_pointer()
                                        .px(px(18.))
                                        .h(px(36.))
                                        .rounded(px(6.))
                                        .bg(color("--control-primary-background-default"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--white"))
                                        .child(if has_redeem {
                                            dc_intl::t("+zx47d")
                                        } else {
                                            dc_intl::t("/g10LC")
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(url) = redeem_url.as_ref() {
                                                cx.open_url(url);
                                            }
                                            this.quest_code_instructions_modal = None;
                                            cx.notify();
                                        }))
                                }),
                        ),
                ),
        )
        .with_priority(97)
        .into_any_element()
    }

    fn render_quest_reward_modal(
        &self,
        state: &QuestRewardModalState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let card = match state {
            QuestRewardModalState::Loading { .. } => div()
                .id("quest-reward-loading")
                .w(px(360.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .p(px(32.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(icon("home_quests", 32., "--icon-strong"))
                .child(
                    div()
                        .text_size(px(16.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("ZTNur7")),
                ),
            QuestRewardModalState::Error { .. } => div()
                .id("quest-reward-error")
                .w(px(440.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .overflow_hidden()
                .flex()
                .flex_col()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(24.))
                        .pt(px(20.))
                        .pb(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("Hd3D8W")),
                        )
                        .child(
                            div()
                                .id("quest-reward-err-close")
                                .flex_none()
                                .size(px(32.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("modal_close", 18., "--icon-subtle"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quest_reward_modal = None;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .px(px(24.))
                        .pb(px(20.))
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("Ij97SL")),
                )
                .child(
                    div()
                        .px(px(24.))
                        .py(px(14.))
                        .bg(color("--background-base-lower"))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .id("quest-reward-err-btn")
                                .cursor_pointer()
                                .px(px(18.))
                                .h(px(36.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("cpT0Cq"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quest_reward_modal = None;
                                    cx.notify();
                                })),
                        ),
                ),
            QuestRewardModalState::Claimed {
                quest_id,
                quest_name,
                game_title,
                publisher,
                reward_name,
                reward_code,
                is_game_quest,
                cta_label,
                cta_url,
                orbs_earned,
                orbs_balance,
                is_in_game_reward,
                reward_asset_url,
                collectible_sku_id,
                collectible_applied,
                collectible_expiry_copy,
            } => {
                let heading = dc_intl::format(
                    "EAYZAr",
                    &[("questName", Arg::Str(Cow::Borrowed(quest_name)))],
                );
                let logo_aria =
                    dc_intl::format("rtm15P", &[("name", Arg::Str(Cow::Borrowed(game_title)))]);
                let right_subtitle = if *is_game_quest {
                    dc_intl::t("wirwN+")
                } else {
                    dc_intl::t("y8Xf3k")
                };
                let share_id = quest_id.clone();
                let open_url = cta_url.clone();
                let copy_code = reward_code.clone();

                div()
                    .id("quest-reward-split-modal")
                    .w(px(720.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-high"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    // Header
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(16.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("quest-reward-logo")
                                            .aria_label(logo_aria)
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(8.))
                                            .bg(rgba(0x5865f233))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("home_quests", 18., "--white")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(18.))
                                            .line_height(px(22.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(heading),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("quest-reward-copy-link")
                                            .cursor_pointer()
                                            .px(px(12.))
                                            .h(px(30.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("WmfZHZ"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.share_quest_link(&share_id, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("quest-reward-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_reward_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    // 2-column split body
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            // Left panel: Orbs Balance Celebration (459624) or Reward details & code
                            .child(if let Some(earned) = *orbs_earned {
                                let earned_str = earned.to_string();
                                let _earned_aria = dc_intl::format(
                                    "WPNjGe",
                                    &[("count", Arg::Num(earned as f64))],
                                );
                                let earned_num = dc_intl::format(
                                    "YMor7k",
                                    &[("count", Arg::Str(Cow::Borrowed(&earned_str)))],
                                );
                                let reward_header = format!(
                                    "{} | {}",
                                    dc_intl::t("loyTil").trim(),
                                    dc_intl::t("Lmysvd")
                                );
                                let bal = orbs_balance.unwrap_or(earned);
                                let bal_str = bal.to_string();
                                let bal_runs = dc_intl::format_runs(
                                    "g4t9aw",
                                    &[("orbsBalance", Arg::Str(Cow::Borrowed(&bal_str)))],
                                );
                                let mut bal_text = String::new();
                                let mut bal_hl = Vec::new();
                                for (chunk, hook) in bal_runs {
                                    let s = bal_text.len();
                                    bal_text.push_str(&chunk);
                                    if matches!(
                                        hook.as_deref(),
                                        Some("$b") | Some("orbsBalanceHook")
                                    ) && s < bal_text.len()
                                    {
                                        bal_hl.push((
                                            s..bal_text.len(),
                                            HighlightStyle {
                                                font_weight: Some(FontWeight(700.)),
                                                color: Some(color("--text-strong").into()),
                                                ..HighlightStyle::default()
                                            },
                                        ));
                                    }
                                }
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .child(reward_header),
                                    )
                                    .child(
                                        div()
                                            .rounded(px(12.))
                                            .p(px(24.))
                                            .bg(linear_gradient(
                                                135.,
                                                linear_color_stop(rgba(0x5865f244), 0.),
                                                linear_color_stop(rgba(0xeb459e33), 1.),
                                            ))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .size(px(64.))
                                                    .rounded_full()
                                                    .bg(rgba(0x5865f255))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon("shop_orb_18", 34., "--white")),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(icon("shop_orb_18", 24., "--text-strong"))
                                                    .child(
                                                        div()
                                                            .text_size(px(28.))
                                                            .line_height(px(32.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .child(earned_num),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_center()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .text_color(color("--text-muted"))
                                                    .child(
                                                        StyledText::new(bal_text)
                                                            .with_highlights(bal_hl),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("quest-orbs-explore-shop")
                                            .w_full()
                                            .h(px(38.))
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
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("WYchde"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_reward_modal = None;
                                                this.quests_open_orbs_shop(cx);
                                            })),
                                    )
                                    .into_any_element()
                            } else if let Some(sku_id) = collectible_sku_id.clone() {
                                // Module 651663: Quest Collectible Avatar Decoration Reward Claim Modal
                                // (`loyTil`, 152px avatar decoration preview, `0/Yz+Y` "Congratulations!",
                                // duration/Nitro retention copy `l9uXL8`/`o97tNn`/`PkyRZo`, and `MAS7uK`
                                // "Use Now" / disabled `hjaYYn` "Decoration Applied").
                                let reward_header = format!(
                                    "{} | {}",
                                    dc_intl::t("loyTil").trim(),
                                    reward_name
                                );
                                let applied = *collectible_applied;
                                let expiry_text = collectible_expiry_copy
                                    .clone()
                                    .unwrap_or_else(|| {
                                        collectible_reward_expiry_copy(
                                            reward_name,
                                            self.premium()
                                                == dc_state::premium::PremiumType::Nitro,
                                            None,
                                        )
                                    });
                                let me_avatar = self
                                    .stores
                                    .current_user
                                    .as_ref()
                                    .and_then(|u| self.images.get(&super::avatar_source(u).0))
                                    .cloned();
                                let deco_img = reward_asset_url
                                    .as_deref()
                                    .and_then(|u| self.images.get(u).cloned());
                                div()
                                    .id("quest-collectible-reward-panel")
                                    .flex_1()
                                    .min_w_0()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .truncate()
                                            .child(reward_header),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(16.))
                                            .child(
                                                // 152px avatar + avatar decoration ring preview (`SIZE_152`)
                                                div()
                                                    .relative()
                                                    .size(px(152.))
                                                    .rounded_full()
                                                    .border_2()
                                                    .border_color(color("--brand-500"))
                                                    .bg(linear_gradient(
                                                        135.,
                                                        linear_color_stop(rgba(0x5865f244), 0.),
                                                        linear_color_stop(rgba(0xeb459e44), 1.),
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(match me_avatar {
                                                        Some(pic) => gpui::img(pic)
                                                            .size(px(124.))
                                                            .rounded_full()
                                                            .object_fit(gpui::ObjectFit::Cover)
                                                            .into_any_element(),
                                                        None => icon("nitro_wheel", 52., "--white")
                                                            .into_any_element(),
                                                    })
                                                    .children(deco_img.map(|pic| {
                                                        gpui::img(pic)
                                                            .absolute()
                                                            .inset_0()
                                                            .size(px(152.))
                                                            .object_fit(gpui::ObjectFit::Contain)
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .mb(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(px(22.))
                                                            .line_height(px(28.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .text_center()
                                                            .child(dc_intl::t("0/Yz+Y")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .line_height(px(20.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(color("--text-subtle"))
                                                            .text_center()
                                                            .child(expiry_text),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("quest-collectible-apply-btn")
                                            .w_full()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .when(applied, |d| d.opacity(0.6))
                                            .when(!applied, |d| {
                                                d.hover(|s| {
                                                    s.bg(color(
                                                        "--control-secondary-background-hover",
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.apply_quest_collectible_decoration(
                                                        &sku_id, cx,
                                                    );
                                                }))
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(if applied {
                                                dc_intl::t("hjaYYn")
                                            } else {
                                                dc_intl::t("MAS7uK")
                                            }),
                                    )
                                    .into_any_element()
                            } else if *is_in_game_reward {
                                // Module 529398: In-Game Reward Claimed panel (`loyTil`, `0/Yz+Y`, `v1u/zq`, `OD6Tvf`).
                                let reward_header = format!(
                                    "{} | {}",
                                    dc_intl::t("loyTil").trim(),
                                    reward_name
                                );
                                let body_copy = dc_intl::format(
                                    "v1u/zq",
                                    &[("rewardName", Arg::Str(Cow::Borrowed(reward_name)))],
                                );
                                let tile_img = reward_asset_url
                                    .as_deref()
                                    .and_then(|u| self.images.get(u).cloned());
                                div()
                                    .id("quest-in-game-reward-panel")
                                    .flex_1()
                                    .min_w_0()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-muted"))
                                            .truncate()
                                            .child(reward_header),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .gap(px(14.))
                                            .child(
                                                div()
                                                    .size(px(148.))
                                                    .rounded(px(12.))
                                                    .overflow_hidden()
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .bg(linear_gradient(
                                                        135.,
                                                        linear_color_stop(rgba(0x5865f244), 0.),
                                                        linear_color_stop(rgba(0xeb459e33), 1.),
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(match tile_img {
                                                        Some(image) => gpui::img(image)
                                                            .size_full()
                                                            .object_fit(gpui::ObjectFit::Cover)
                                                            .into_any_element(),
                                                        None => icon("gift", 48., "--white")
                                                            .into_any_element(),
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .text_center()
                                                    .child(dc_intl::t("0/Yz+Y")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(20.))
                                                    .text_color(color("--text-muted"))
                                                    .text_center()
                                                    .child(body_copy),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("quest-in-game-claimed-btn")
                                            .w_full()
                                            .h(px(34.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .opacity(0.6)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("OD6Tvf")),
                                    )
                                    .into_any_element()
                            } else {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .p(px(24.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.))
                                    .child(
                                        div()
                                            .rounded(px(10.))
                                            .p(px(20.))
                                            .bg(linear_gradient(
                                                135.,
                                                linear_color_stop(rgba(0x5865f244), 0.),
                                                linear_color_stop(rgba(0xeb459e33), 1.),
                                            ))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap(px(10.))
                                            .child(
                                                div()
                                                    .size(px(56.))
                                                    .rounded_full()
                                                    .bg(rgba(0x5865f255))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon("gift", 28., "--white")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .text_center()
                                                    .child(reward_name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::format(
                                                        "Lm8/mH",
                                                        &[(
                                                            "gamePublisher",
                                                            Arg::Str(Cow::Borrowed(publisher)),
                                                        )],
                                                    )),
                                            ),
                                    )
                                    .children(copy_code.map(|code| {
                                        let code_copy = code.clone();
                                        let code_for_instr = code.clone();
                                        let reward_title_copy = reward_name.clone();
                                        let cta_for_instr = cta_url.clone();
                                        let shown = if self.streamer_hides_personal() {
                                            dc_intl::t("0n2u0k").to_owned()
                                        } else {
                                            code
                                        };
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("5j/Zym")),
                                            )
                                            .child(
                                                div()
                                                    .id("quest-reward-claim-instructions-link")
                                                    .cursor_pointer()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(claim_instructions_link_styled())
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.quest_code_instructions_modal =
                                                            Some(QuestCodeInstructionsModal {
                                                                reward_code: code_for_instr.clone(),
                                                                instructions: format!(
                                                                    "Open the redemption portal and paste your code to claim {reward_title_copy}."
                                                                ),
                                                                redemption_link: (!cta_for_instr
                                                                    .is_empty())
                                                                .then(|| cta_for_instr.clone()),
                                                            });
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .h(px(48.))
                                                    .rounded(px(8.))
                                                    .border_1()
                                                    .border_color(color("--border-subtle"))
                                                    .bg(color("--background-base-lowest"))
                                                    .px(px(14.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--text-strong"))
                                                            .truncate()
                                                            .child(shown),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("quest-reward-copy-code")
                                                            .flex_none()
                                                            .cursor_pointer()
                                                            .px(px(12.))
                                                            .h(px(28.))
                                                            .rounded(px(6.))
                                                            .bg(color(
                                                                "--control-primary-background-default",
                                                            ))
                                                            .flex()
                                                            .items_center()
                                                            .text_size(px(12.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--white"))
                                                            .child(dc_intl::tr("Copy"))
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    cx.write_to_clipboard(
                                                                        gpui::ClipboardItem::new_string(
                                                                            code_copy.clone(),
                                                                        ),
                                                                    );
                                                                    this.put_app_toast(
                                                                        dc_intl::t("+5kSoW")
                                                                            .to_owned(),
                                                                        true,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                    ),
                                            )
                                    }))
                                    .into_any_element()
                            })
                            // Right panel: Game / Discovery CTA
                            .child(
                                div()
                                    .w(px(280.))
                                    .flex_none()
                                    .p(px(24.))
                                    .bg(color("--background-base-lower"))
                                    .border_l_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .flex_col()
                                    .justify_between()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .line_height(px(22.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(game_title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-default"))
                                                    .child(right_subtitle),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .id("quest-reward-cta")
                                                    .cursor_pointer()
                                                    .w_full()
                                                    .h(px(38.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-primary-background-default",
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .child(cta_label.clone())
                                                    .on_click(cx.listener(
                                                        move |_this, _, _, cx| {
                                                            if !open_url.is_empty() {
                                                                cx.open_url(&open_url);
                                                            }
                                                        },
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .id("quest-reward-done")
                                                    .cursor_pointer()
                                                    .w_full()
                                                    .h(px(34.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-secondary-background-default",
                                                    ))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color(
                                                        "--control-secondary-text-default",
                                                    ))
                                                    .child(dc_intl::t("cpT0Cq"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.quest_reward_modal = None;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            ),
                    )
            }
        };

        deferred(
            div()
                .id("quest-reward-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.quest_reward_modal = None;
                    cx.notify();
                }))
                .child(card),
        )
        .with_priority(96)
        .into_any_element()
    }

    /// Renders the Members List Quest Hover/Expand Popout Card (`198525`).
    fn render_member_quest_popout(
        &self,
        popout: &MemberQuestPopout,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let quest_id = popout.quest_id.clone();
        let status = popout.status;

        let status_badge = match status {
            MemberQuestStatus::Completed | MemberQuestStatus::InProgress { .. } => None,
            MemberQuestStatus::Available { joinable: true, .. } => Some(dc_intl::t("BXFP30")),
            MemberQuestStatus::Available {
                joinable: false, ..
            } => Some(dc_intl::t("BSXPZ+")),
        };

        let card = div()
            .id("member-quest-popout-card")
            .aria_label(popout.tile_aria_label())
            .w(px(320.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            // Hero strip
            .child(
                div()
                    .px(px(16.))
                    .py(px(14.))
                    .bg(linear_gradient(
                        135.,
                        linear_color_stop(rgba(0x5865f266), 0.),
                        linear_color_stop(rgba(0x2b2d31ff), 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .children(status_badge.map(|badge| {
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(badge.to_uppercase())
                            }))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .truncate()
                                    .child(popout.headline()),
                            ),
                    )
                    .child(
                        div()
                            .id("member-quest-popout-close")
                            .flex_none()
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--icon-subtle"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.member_quest_popout = None;
                                cx.notify();
                            })),
                    ),
            )
            // Body
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .bg(rgba(0x5865f233))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("home_quests", 22., "--white")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .truncate()
                                            .child(popout.game_title.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .truncate()
                                            .child(popout.promoted_by()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(popout.reward_summary.clone()),
                    )
                    .children(match status {
                        MemberQuestStatus::InProgress { percent } => {
                            let fill = (f32::from(percent.min(100)) / 100.).clamp(0., 1.);
                            Some(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(6.))
                                            .rounded_full()
                                            .bg(color("--background-base-lowest"))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(px(286. * fill))
                                                    .rounded_full()
                                                    .bg(color(
                                                        "--control-primary-background-default",
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(format!("{percent}% complete")),
                                    ),
                            )
                        }
                        _ => None,
                    })
                    .child(
                        div()
                            .id("member-quest-popout-cta")
                            .cursor_pointer()
                            .w_full()
                            .h(px(36.))
                            .rounded(px(6.))
                            .bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(13.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(popout.primary_cta())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.member_quest_popout = None;
                                match status {
                                    MemberQuestStatus::Completed => {
                                        this.open_quest_reward_modal(&quest_id, cx);
                                    }
                                    _ => {
                                        this.open_quests(cx);
                                    }
                                }
                            })),
                    ),
            );

        deferred(
            div()
                .id("member-quest-popout-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x00000080))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.member_quest_popout = None;
                    cx.notify();
                }))
                .child(card),
        )
        .with_priority(95)
        .into_any_element()
    }

    /// Capture harness:
    /// `quest-reward-modal [claimed|loading|error|member-completed|member-progress|member-available|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_reward_modal(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        match arg.trim() {
            "collectible" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Claimed {
                    quest_id: "quest_collectible_651663".to_owned(),
                    quest_name: "Starfall Odyssey".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    publisher: "Nebula Studios".to_owned(),
                    reward_name: "Starfall Halo".to_owned(),
                    reward_code: None,
                    is_game_quest: true,
                    cta_label: "Launch Game".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    orbs_earned: None,
                    orbs_balance: None,
                    is_in_game_reward: false,
                    reward_asset_url: None,
                    collectible_sku_id: Some("123456789012345678".to_owned()),
                    collectible_applied: false,
                    collectible_expiry_copy: Some(collectible_reward_expiry_copy(
                        "Starfall Halo",
                        true,
                        None,
                    )),
                });
                cx.notify();
                "quest-reward-modal: collectible".to_owned()
            }
            "in-game" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Claimed {
                    quest_id: "quest_ingame_42".to_owned(),
                    quest_name: "Starfall Odyssey".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    publisher: "Nebula Studios".to_owned(),
                    reward_name: "Starfall Wing Glider".to_owned(),
                    reward_code: None,
                    is_game_quest: true,
                    cta_label: "Launch Game".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    orbs_earned: None,
                    orbs_balance: None,
                    is_in_game_reward: true,
                    reward_asset_url: None,
                    collectible_sku_id: None,
                    collectible_applied: false,
                    collectible_expiry_copy: None,
                });
                cx.notify();
                "quest-reward-modal: in-game".to_owned()
            }
            "orbs" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Claimed {
                    quest_id: "quest_orbs_42".to_owned(),
                    quest_name: "Starfall Odyssey".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    publisher: "Nebula Studios".to_owned(),
                    reward_name: "700 Orbs".to_owned(),
                    reward_code: None,
                    is_game_quest: true,
                    cta_label: "Launch Game".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    orbs_earned: Some(700),
                    orbs_balance: Some(1450),
                    is_in_game_reward: false,
                    reward_asset_url: None,
                    collectible_sku_id: None,
                    collectible_applied: false,
                    collectible_expiry_copy: None,
                });
                cx.notify();
                "quest-reward-modal: orbs".to_owned()
            }
            "" | "claimed" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Claimed {
                    quest_id: "quest_demo_42".to_owned(),
                    quest_name: "Starfall Odyssey".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    publisher: "Nebula Studios".to_owned(),
                    reward_name: "Starfall Avatar Decoration".to_owned(),
                    reward_code: Some("STAR-FALL-2026-DISC".to_owned()),
                    is_game_quest: true,
                    cta_label: "Launch Game".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    orbs_earned: None,
                    orbs_balance: None,
                    is_in_game_reward: false,
                    reward_asset_url: None,
                    collectible_sku_id: None,
                    collectible_applied: false,
                    collectible_expiry_copy: None,
                });
                cx.notify();
                "quest-reward-modal: claimed".to_owned()
            }
            "loading" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Loading {
                    quest_id: "quest_demo_42".to_owned(),
                });
                cx.notify();
                "quest-reward-modal: loading".to_owned()
            }
            "error" => {
                self.member_quest_popout = None;
                self.quest_reward_modal = Some(QuestRewardModalState::Error {
                    quest_id: "quest_demo_42".to_owned(),
                });
                cx.notify();
                "quest-reward-modal: error".to_owned()
            }
            "member-completed" => {
                self.quest_reward_modal = None;
                self.member_quest_popout = Some(MemberQuestPopout {
                    quest_id: "quest_demo_42".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    game_publisher: "Nebula Studios".to_owned(),
                    reward_summary: "Earn the exclusive Starfall Avatar Decoration.".to_owned(),
                    status: MemberQuestStatus::Completed,
                });
                cx.notify();
                "quest-reward-modal: member-completed".to_owned()
            }
            "member-progress" => {
                self.quest_reward_modal = None;
                self.member_quest_popout = Some(MemberQuestPopout {
                    quest_id: "quest_demo_42".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    game_publisher: "Nebula Studios".to_owned(),
                    reward_summary: "Play 15 minutes to unlock the Starfall reward.".to_owned(),
                    status: MemberQuestStatus::InProgress { percent: 65 },
                });
                cx.notify();
                "quest-reward-modal: member-progress".to_owned()
            }
            "member-available" => {
                self.quest_reward_modal = None;
                self.member_quest_popout = Some(MemberQuestPopout {
                    quest_id: "quest_demo_42".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    game_publisher: "Nebula Studios".to_owned(),
                    reward_summary:
                        "Join your friend in-game and accept the Quest to earn rewards.".to_owned(),
                    status: MemberQuestStatus::Available {
                        joinable: true,
                        cta: MemberQuestCta::AcceptQuest,
                    },
                });
                cx.notify();
                "quest-reward-modal: member-available".to_owned()
            }
            "instructions" => {
                self.quest_reward_modal = None;
                self.member_quest_popout = None;
                self.quest_code_instructions_modal = Some(QuestCodeInstructionsModal {
                    reward_code: "STAR-FALL-2026-ORBS".to_owned(),
                    instructions: "Open the publisher redemption portal, sign in with your linked account, and paste your reward code below.".to_owned(),
                    redemption_link: Some("https://discord.com/quests?code={code}".to_owned()),
                });
                cx.notify();
                "quest-reward-modal: instructions".to_owned()
            }
            "close" => {
                self.quest_reward_modal = None;
                self.quest_code_instructions_modal = None;
                self.member_quest_popout = None;
                cx.notify();
                "quest-reward-modal: closed".to_owned()
            }
            other => format!("quest-reward-modal: unknown {other}"),
        }
    }

    /// Module 651663: applies the claimed Quest avatar decoration (`PATCH /users/@me` with
    /// `{ avatar_decoration_sku_id }`) and flips the button state to `"Decoration Applied"` (`hjaYYn`).
    fn apply_quest_collectible_decoration(&mut self, sku_id: &str, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Rest {
            key: Some("quest-apply-avatar-decoration".to_owned()),
            method: reqwest::Method::PATCH,
            route: "/users/@me".to_owned(),
            body: Some(serde_json::json!({ "avatar_decoration_sku_id": sku_id })),
        });
        if let Some(QuestRewardModalState::Claimed {
            collectible_applied,
            ..
        }) = self.quest_reward_modal.as_mut()
        {
            *collectible_applied = true;
        }
        cx.notify();
    }
}

/// Module 651663 / 61387.FA: resolves retention/expiry copy for a claimed avatar decoration reward.
pub(super) fn collectible_reward_expiry_copy(
    reward_name: &str,
    has_nitro: bool,
    expiration_date: Option<&str>,
) -> String {
    match (has_nitro, expiration_date) {
        (true, Some(exp)) => dc_intl::format(
            "PkyRZo",
            &[
                ("rewardName", Arg::Str(Cow::Borrowed(reward_name))),
                ("expirationDate", Arg::Str(Cow::Borrowed(exp))),
            ],
        ),
        (true, None) => dc_intl::format(
            "o97tNn",
            &[("rewardName", Arg::Str(Cow::Borrowed(reward_name)))],
        ),
        (false, _) => dc_intl::format(
            "l9uXL8",
            &[("decorationName", Arg::Str(Cow::Borrowed(reward_name)))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quest_collectible_651663_copy_and_keys() {
        for k in ["0/Yz+Y", "hjaYYn", "MAS7uK", "l9uXL8", "o97tNn", "PkyRZo"] {
            assert!(dc_intl::has_key(k), "missing 651663 key {k}");
        }
        let nitro_perm = collectible_reward_expiry_copy("Starfall Halo", true, None);
        assert!(nitro_perm.contains("Starfall Halo") && nitro_perm.contains("Nitro"));
        let nitro_exp = collectible_reward_expiry_copy("Starfall Halo", true, Some("Dec 31"));
        assert!(nitro_exp.contains("Dec 31"));
        let forever = collectible_reward_expiry_copy("Starfall Halo", false, None);
        assert!(forever.contains("Starfall Halo"));
    }

    #[test]
    fn member_quest_popout_formats_intl_labels() {
        let p = MemberQuestPopout {
            quest_id: "1".to_owned(),
            game_title: "Starfall".to_owned(),
            game_publisher: "Nebula".to_owned(),
            reward_summary: "Reward".to_owned(),
            status: MemberQuestStatus::Available {
                joinable: true,
                cta: MemberQuestCta::LaunchQuestInDiscord,
            },
        };
        assert_eq!(p.headline(), "Quest Available!");
        assert_eq!(p.primary_cta(), "Launch Quest in Discord");
        assert_eq!(p.promoted_by(), "Promoted by Nebula");
        assert!(p.tile_aria_label().contains("Starfall"));
        assert!(p.tile_aria_label().contains("Nebula"));
    }
}
