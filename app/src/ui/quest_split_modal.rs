//! `SplitQuestModal` account connect flow (module 968779 `el` / `W` / `es` / `y`, CSS `FmNv9Q`,
//! `83l3AO`, `IBNVLQ`, `e3bXMB`), Play Quest Requirements 2-step checklist (module 976019 `A` / `j`,
//! CSS `g2YoxE`, `zV869I`), and right-panel Nitro / Orbs / Xbox CTA overrides (module 491145 `A`).
//!
//! Opened from `handle_quest_cta_action` (`quest_bar.rs`) on `CtaAction::ConnectAccount` /
//! `CtaAction::ViewConnection` (and via capture harness `quest-split`).

use super::quest_copy;
use super::quests::quest_asset_url;
use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::premium::PremiumType;
use dc_state::quests::{ORBS_INTRO_QUEST_ID, Quest, feature, task};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, div, img, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};
use std::borrow::Cow;

/// Account connection method selectable in `968779.js` `es`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectMethod {
    InGame,
    Web,
}

impl ConnectMethod {
    /// Localized dropdown label (`4PWzD7` "In-Game Account", `CM8LUl` "Web").
    pub fn label(self) -> &'static str {
        dc_intl::t(match self {
            Self::InGame => "4PWzD7",
            Self::Web => "CM8LUl",
        })
    }

    /// Leading icon in the method dropdown.
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::InGame => "bot_game_controller",
            Self::Web => "oauth_entry_link",
        }
    }
}

/// Active state of `SplitQuestModal` (`968779.js`).
#[derive(Clone, Debug)]
pub(super) struct SplitQuestModalState {
    pub quest_id: String,
    pub fetched: bool,
    pub has_already_linked: bool,
    pub can_start_authorization: bool,
    pub selected_method: Option<ConnectMethod>,
    pub method_dropdown_open: bool,
    pub content_error: Option<(Option<String>, Option<String>)>,
}

impl SplitQuestModalState {
    pub fn new(quest_id: String, has_already_linked: bool, can_start_authorization: bool) -> Self {
        Self {
            quest_id,
            fetched: true,
            has_already_linked,
            can_start_authorization,
            selected_method: if can_start_authorization {
                Some(ConnectMethod::Web)
            } else {
                Some(ConnectMethod::InGame)
            },
            method_dropdown_open: false,
            content_error: None,
        }
    }

    /// `showMethodSelect = fetched && !hasAlreadyLinked && canStartAuthorization` (`968779.js`).
    pub fn show_method_select(&self) -> bool {
        self.fetched && !self.has_already_linked && self.can_start_authorization
    }

    /// `effectiveMethod = showMethodSelect ? selectedMethod : InGame` (`968779.js`).
    pub fn effective_method(&self) -> Option<ConnectMethod> {
        if self.show_method_select() {
            self.selected_method
        } else {
            Some(ConnectMethod::InGame)
        }
    }

    /// `showChecklist = fetched && (effectiveMethod.is_some() || hasAlreadyLinked)` (`968779.js`).
    pub fn show_checklist(&self) -> bool {
        self.fetched && (self.effective_method().is_some() || self.has_already_linked)
    }
}

/// Right-panel copy & CTA resolved from module 491145 (`NITRO_2_POINT_0_CTA`, `ORBS_MULTIPLIER_QUEST`,
/// `XBOX_GAME_PASS_QUEST`) or falling back to `870273.Jf` (`game_cta_subtitle`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RightPanelCopy {
    pub title: String,
    pub subtitle: String,
    pub cta_label: String,
    pub opens_nitro: bool,
}

pub fn resolve_right_panel_copy(quest: &Quest, premium: PremiumType) -> RightPanelCopy {
    let tier2 = premium == PremiumType::Nitro;
    let tier01 = matches!(premium, PremiumType::Basic | PremiumType::Classic);
    if quest.config.has_feature(feature::NITRO_2_POINT_0_CTA) {
        let (t, s, c) = if tier2 {
            ("LIc2xy", "gzazAe", "HlVJm0")
        } else if tier01 {
            ("HTsycG", "3JKWU6", "2iX+XV")
        } else {
            ("bOf5IH", "SzOYGc", "620Iwi")
        };
        return RightPanelCopy {
            title: format_plain(t, &[]),
            subtitle: format_plain(s, &[]),
            cta_label: format_plain(c, &[]),
            opens_nitro: true,
        };
    }
    if quest.config.has_feature(feature::ORBS_MULTIPLIER_QUEST) {
        let (t, s, c) = if tier2 {
            ("QP/6xR", "zPnFYf", "TNS24d")
        } else if tier01 {
            ("CFfUkp", "02smVH", "2iX+XV")
        } else {
            ("CFfUkp", "02smVH", "620Iwi")
        };
        return RightPanelCopy {
            title: format_plain(t, &[]),
            subtitle: format_plain(s, &[]),
            cta_label: format_plain(c, &[]),
            opens_nitro: true,
        };
    }
    if quest.config.has_feature(feature::XBOX_GAME_PASS_QUEST) {
        let (t, s, c) = if tier2 {
            ("4zROKG", "v9AaV4", "/AUOfc")
        } else if tier01 {
            ("Q15RXC", "zUjL+R", "+ADjaQ")
        } else {
            ("Q15RXC", "dHQQvk", "qI5XnY")
        };
        return RightPanelCopy {
            title: format_plain(t, &[]),
            subtitle: format_plain(s, &[]),
            cta_label: format_plain(c, &[]),
            opens_nitro: true,
        };
    }
    let cta = if quest.config.cta_button_label.is_empty() {
        "Play Game".to_owned()
    } else {
        quest.config.cta_button_label.clone()
    };
    RightPanelCopy {
        title: quest.config.game_title.clone(),
        subtitle: quest_copy::game_cta_subtitle(quest),
        cta_label: cta,
        opens_nitro: false,
    }
}

/// Resolves the 2 checklist step strings (`976019.A`: `C45G4B`, `j3155Y`/`4+3zP/`, `tNmoIp`/`vDLUD3`, `nP1hFd`/`GLZnbq`).
pub fn checklist_step_labels(
    quest: &Quest,
    has_already_linked: bool,
    method: ConnectMethod,
    is_nitro: bool,
    clickable_game_title: bool,
) -> (String, String) {
    let game = quest.config.game_title.as_str();
    let step1_key = if has_already_linked {
        "C45G4B"
    } else if method == ConnectMethod::Web {
        if clickable_game_title {
            "4+3zP/"
        } else {
            "j3155Y"
        }
    } else if clickable_game_title {
        "vDLUD3"
    } else {
        "tNmoIp"
    };
    let step1 = format_plain(step1_key, &[("gameTitle", Arg::Str(Cow::Borrowed(game)))]);

    let objective = quest
        .config
        .task_details
        .get(task::ACHIEVEMENT_IN_GAME)
        .and_then(|t| t.task_title.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("Complete the quest objective");
    let reward_name = quest_copy::reward_name(quest, is_nitro, true);
    let step2_key = if has_already_linked && !quest.is_completed() && clickable_game_title {
        "GLZnbq"
    } else {
        "nP1hFd"
    };
    let step2 = format_plain(
        step2_key,
        &[
            ("objective", Arg::Str(Cow::Borrowed(objective))),
            ("gameTitle", Arg::Str(Cow::Borrowed(game))),
            ("rewardName", Arg::Str(Cow::Borrowed(&reward_name))),
        ],
    );
    (step1, step2)
}

fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

impl Root {
    /// Opens `SplitQuestModal` (`968779`) for `quest_id`.
    pub(super) fn open_quest_split_modal(
        &mut self,
        quest_id: String,
        has_already_linked: bool,
        can_start_authorization: bool,
        cx: &mut Context<Self>,
    ) {
        self.quests.split_modal = Some(SplitQuestModalState::new(
            quest_id,
            has_already_linked,
            can_start_authorization,
        ));
        cx.notify();
    }

    /// Renders `SplitQuestModal` (`968779.js` `W` + `es` + `y` + `976019` + `491145`).
    pub(super) fn quest_split_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let state = self.quests.split_modal.as_ref()?;
        let quest = self
            .quests
            .store
            .quests
            .iter()
            .find(|q| q.id == state.quest_id)
            .cloned()?;

        // Error dialog fallback (`D` of 968779.js: `9cfXek` / `nYQyEg` / `cpT0Cq`).
        if let Some((err_title, err_desc)) = state.content_error.as_ref() {
            let title = err_title
                .clone()
                .unwrap_or_else(|| dc_intl::t("9cfXek").to_owned());
            let body = err_desc
                .clone()
                .unwrap_or_else(|| dc_intl::t("nYQyEg").to_owned());
            return Some(
                gpui::deferred(
                    div()
                        .id("quest-split-error-backdrop")
                        .occlude()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(rgba(0x000000b3))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quests.split_modal = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("quest-split-error-modal")
                                .w(px(440.))
                                .rounded(px(12.))
                                .bg(color("--background-surface-high"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .p(px(24.))
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(body),
                                )
                                .child(
                                    div().mt(px(8.)).flex().justify_end().child(
                                        div()
                                            .id("quest-split-error-close")
                                            .h(px(36.))
                                            .px(px(18.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-primary-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quests.split_modal = None;
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("cpT0Cq")),
                                    ),
                                ),
                        ),
                )
                .with_priority(6)
                .into_any_element(),
            );
        }

        let is_nitro = self.premium().is_subscriber();
        let completed = quest.is_completed();
        let linked = state.has_already_linked;
        let show_method_select = state.show_method_select();
        let effective_method = state.effective_method();
        let show_checklist = state.show_checklist();
        let shareable = quest.id != ORBS_INTRO_QUEST_ID;
        let qid = quest.id.clone();

        // Left panel copy (`es` of 968779.js)
        let heading = dc_intl::t(if linked { "2+opCy" } else { "dp0CUb" });
        let subtitle = if !linked {
            dc_intl::t("Z1T4zl").to_owned()
        } else if !quest.config.cta_link.is_empty() {
            format_plain(
                "X8hBDz",
                &[(
                    "gameTitle",
                    Arg::Str(Cow::Borrowed(&quest.config.game_title)),
                )],
            )
        } else {
            format_plain(
                "u3mdpP",
                &[(
                    "gameTitle",
                    Arg::Str(Cow::Borrowed(&quest.config.game_title)),
                )],
            )
        };
        let (step1_text, step2_text) = checklist_step_labels(
            &quest,
            linked,
            effective_method.unwrap_or(ConnectMethod::InGame),
            is_nitro,
            !quest.config.cta_link.is_empty(),
        );
        let info_data_sharing = format_plain(
            "VaiGaz",
            &[(
                "gameTitle",
                Arg::Str(Cow::Borrowed(&quest.config.game_title)),
            )],
        );
        let info_36h = format_plain("GBjQwf", &[]);

        // Right panel copy (`491145` or `870273.Jf`)
        let right = resolve_right_panel_copy(&quest, self.premium());
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
        let game_tile_img = quest
            .config
            .assets_game_tile
            .as_deref()
            .or(quest.config.assets_reward_tile.as_deref())
            .and_then(|a| self.images.get(&quest_asset_url(&quest.id, a)).cloned());
        let logo_alt = dc_intl::format(
            "rtm15P",
            &[("name", Arg::Str(Cow::Borrowed(&quest.config.game_title)))],
        );

        let step_indicator = |done: bool, current: bool| {
            div()
                .flex_none()
                .size(px(20.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .when(done, |s| {
                    s.bg(color("--brand-500"))
                        .child(icon("checkmark_large", 12., "--white"))
                })
                .when(!done, |s| {
                    s.border_2().border_color(if current {
                        color("--icon-overlay-light")
                    } else {
                        color("--interactive-background-selected")
                    })
                })
        };

        // Left Panel (300px, top-left expressive purple gradient, padding 24px)
        let instructions_url = quest
            .config
            .task_details
            .get(task::ACHIEVEMENT_IN_GAME)
            .and_then(|t| t.account_link_instructions.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| quest.config.cta_link.clone());

        let left_panel = div()
            .w(px(300.))
            .h_full()
            .flex_none()
            .p(px(24.))
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgba(0x5865f238), 0.),
                linear_color_stop(color("--background-surface-high"), 0.7),
            ))
            .border_r_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(heading),
            )
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-subtle"))
                    .child(subtitle),
            )
            // Method dropdown (`okJIPY`: "Choose how to connect")
            .when(show_method_select, |this| {
                let current_label = state
                    .selected_method
                    .map(ConnectMethod::label)
                    .unwrap_or_else(|| dc_intl::t("okJIPY"));
                let current_icon = state.selected_method.map(ConnectMethod::icon_name);
                this.child(
                    div()
                        .relative()
                        .mb(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-subtle"))
                                .child(dc_intl::t("okJIPY")),
                        )
                        .child(
                            div()
                                .id("quest-split-method-trigger")
                                .h(px(38.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .bg(color("--background-base-lowest"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(m) = this.quests.split_modal.as_mut() {
                                        m.method_dropdown_open = !m.method_dropdown_open;
                                        cx.notify();
                                    }
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .children(
                                            current_icon.map(|ic| icon(ic, 16., "--icon-subtle")),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .text_color(color("--text-default"))
                                                .child(current_label),
                                        ),
                                )
                                .child(icon("chevron_down", 14., "--icon-subtle")),
                        )
                        .when(state.method_dropdown_open, |dd| {
                            dd.child(
                                div()
                                    .p(px(4.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .border_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .children([ConnectMethod::InGame, ConnectMethod::Web].into_iter().map(
                                        |opt| {
                                            let selected = state.selected_method == Some(opt);
                                            div()
                                                .id(match opt {
                                                    ConnectMethod::InGame => "quest-split-opt-ingame",
                                                    ConnectMethod::Web => "quest-split-opt-web",
                                                })
                                                .px(px(8.))
                                                .py(px(6.))
                                                .rounded(px(6.))
                                                .cursor_pointer()
                                                .when(selected, |s| s.bg(color("--background-mod-strong")))
                                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                                .flex()
                                                .items_center()
                                                .gap(px(8.))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(m) = this.quests.split_modal.as_mut() {
                                                        m.selected_method = Some(opt);
                                                        m.method_dropdown_open = false;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(icon(opt.icon_name(), 15., "--icon-subtle"))
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(color("--text-default"))
                                                        .child(opt.label()),
                                                )
                                        },
                                    )),
                            )
                        }),
                )
            })
            // Checklist (`976019.A`, CSS `g2YoxE` + `zV869I`)
            .when(show_checklist, |this| {
                let step1_done = linked || completed;
                let step2_done = completed;
                this.child(
                    div()
                        .w_full()
                        .p(px(12.))
                        .rounded(px(8.))
                        .bg(color("--message-background-hover"))
                        .border_1()
                        .border_color(color("--interactive-background-hover"))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(12.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                // `dc9w4N`: "Play Quest Requirements"
                                .child(dc_intl::t("dc9w4N")),
                        )
                        // Step 1
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .child(step_indicator(step1_done, !step1_done))
                                        .child(
                                            div()
                                                .w(px(2.))
                                                .h(px(16.))
                                                .my(px(2.))
                                                .bg(if step1_done {
                                                    color("--brand-500")
                                                } else {
                                                    color("--interactive-background-selected")
                                                }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(step1_text),
                                ),
                        )
                        // Step 2
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(px(10.))
                                .child(step_indicator(step2_done, step1_done && !step2_done))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(step2_text),
                                ),
                        )
                        // Bottom Info Notice (`976019.j`, shown if `!completed`)
                        .when(!completed, |card| {
                            card.child(
                                div()
                                    .mt(px(12.))
                                    .pt(px(12.))
                                    .border_t_1()
                                    .border_color(color("--interactive-background-hover"))
                                    .flex()
                                    .items_start()
                                    .gap(px(8.))
                                    .child(icon("circle_info", 14., "--text-link"))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .line_height(px(15.))
                                                    .text_color(color("--text-muted"))
                                                    .child(info_data_sharing),
                                            )
                                            .when(linked, |n| {
                                                n.child(
                                                    div()
                                                        .id("quest-split-36h-learn")
                                                        .text_size(px(11.))
                                                        .line_height(px(15.))
                                                        .text_color(color("--text-link"))
                                                        .cursor_pointer()
                                                        .hover(|s| s.underline())
                                                        .on_click(cx.listener(|_, _, _, cx| {
                                                            cx.open_url("https://support.discord.com/hc/en-us/articles/22225719947543");
                                                        }))
                                                        .child(info_36h),
                                                )
                                            }),
                                    ),
                            )
                        }),
                )
            })
            .child(div().flex_1())
            // Bottom Primary CTA (`.leftCtaSlot_83l3AO`: `T0zC77` "Connect Now" | `KgYvrZ` "How to Connect")
            .when(state.fetched && !linked && effective_method.is_some(), |this| {
                let method = effective_method.unwrap_or(ConnectMethod::InGame);
                let btn_label = dc_intl::t(match method {
                    ConnectMethod::Web => "T0zC77",
                    ConnectMethod::InGame => "KgYvrZ",
                });
                let url = instructions_url.clone();
                this.child(
                    div()
                        .id("quest-split-left-cta")
                        .w_full()
                        .h(px(38.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-primary-text-default"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if method == ConnectMethod::Web {
                                this.quests.split_modal = None;
                                this.open_settings_page("connections", cx);
                            } else if !url.is_empty() {
                                cx.open_url(&url);
                            }
                        }))
                        .child(btn_label),
                )
            });

        // Right Panel (454px, hero + logotype + game tile + 491145/870273 copy + CTA)
        let right_link = quest.config.cta_link.clone();
        let right_opens_nitro = right.opens_nitro;
        let right_panel =
            div()
                .relative()
                .w(px(454.))
                .h_full()
                .flex_none()
                .bg(color("--background-base-lowest"))
                .flex()
                .flex_col()
                .justify_between()
                .children(hero_img.map(|im| {
                    div()
                        .absolute()
                        .inset_0()
                        .child(img(im).size_full().object_fit(ObjectFit::Cover))
                }))
                .child(div().absolute().inset_0().bg(linear_gradient(
                    0.,
                    linear_color_stop(color("--background-base-lowest"), 0.25),
                    linear_color_stop(rgba(0x00000033), 1.),
                )))
                // Top-right floating buttons (`Q`, `.modalButtons_e3bXMB`: Copy Link `WqhZss` + Close)
                .child(
                    div()
                        .relative()
                        .w_full()
                        .p(px(12.))
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .when(shareable, |row| {
                            let share_id = qid.clone();
                            row.child(
                                div()
                                    .id("quest-split-copy-link")
                                    .size(px(32.))
                                    .rounded_full()
                                    .bg(rgba(0x11121499))
                                    .hover(|s| s.bg(rgba(0x111214cc)))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.share_quest_link(&share_id, cx);
                                    }))
                                    .child(icon("share_link", 16., "--white"))
                                    .when(false, |el| el.child(dc_intl::t("WqhZss"))),
                            )
                        })
                        .child(
                            div()
                                .id("quest-split-close")
                                .size(px(32.))
                                .rounded_full()
                                .bg(rgba(0x11121499))
                                .hover(|s| s.bg(rgba(0x111214cc)))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.quests.split_modal = None;
                                    cx.notify();
                                }))
                                .child(icon("modal_close", 16., "--white")),
                        ),
                )
                // Centered partner logotype (`.partnerLogotype_IBNVLQ`, height 69px)
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(logo_img.map(|im| img(im).max_h(px(69.)).max_w(px(220.))))
                        .when(
                            quest.config.assets_logotype_dark.is_none()
                                && quest.config.assets_logotype.is_none(),
                            |el| {
                                el.child(
                                    div()
                                        .px(px(16.))
                                        .py(px(8.))
                                        .rounded(px(8.))
                                        .bg(rgba(0x11121480))
                                        .text_size(px(22.))
                                        .font_weight(FontWeight(800.))
                                        .text_color(color("--white"))
                                        .child(quest.config.game_title.clone()),
                                )
                            },
                        )
                        .when(false, |el| el.child(logo_alt)),
                )
                // Partner content row + bottom CTA (`.partnerContentContainer_IBNVLQ`, `.rightCtaSlot_IBNVLQ`)
                .child(
                    div()
                        .relative()
                        .w_full()
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(14.))
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(42.))
                                        .rounded(px(8.))
                                        .overflow_hidden()
                                        .bg(color("--background-surface-high"))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .children(game_tile_img.map(|im| {
                                            img(im).size_full().object_fit(ObjectFit::Cover)
                                        }))
                                        .when(
                                            quest.config.assets_game_tile.is_none()
                                                && quest.config.assets_reward_tile.is_none(),
                                            |el| {
                                                el.child(icon(
                                                    "bot_game_controller",
                                                    20.,
                                                    "--text-brand",
                                                ))
                                            },
                                        ),
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
                                                .text_size(px(15.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight(600.))
                                                .text_color(color("--text-strong"))
                                                .child(right.title),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-subtle"))
                                                .child(right.subtitle),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("quest-split-right-cta")
                                .w_full()
                                .h(px(38.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--control-secondary-text-default"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if right_opens_nitro {
                                        this.quests.split_modal = None;
                                        this.open_nitro(cx);
                                    } else if !right_link.is_empty() {
                                        cx.open_url(&right_link);
                                    }
                                }))
                                .child(right.cta_label),
                        ),
                );

        Some(
            gpui::deferred(
                div()
                    .id("quest-split-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quests.split_modal = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("quest-split-modal")
                            .w(px(754.))
                            .h(px(524.))
                            .rounded(px(12.))
                            .bg(color("--background-surface-high"))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .overflow_hidden()
                            .flex()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(left_panel)
                            .child(right_panel),
                    ),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture-only helper: `quest-split <connect|connected|error|close>`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_split(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        let qid = self
            .quests
            .store
            .quests
            .first()
            .map(|q| q.id.clone())
            .unwrap_or_default();
        match mode.trim() {
            "connect" => {
                if qid.is_empty() {
                    return "quest-split: no quest in store".into();
                }
                self.open_quest_split_modal(qid.clone(), false, true, cx);
                format!("quest-split: connect ({qid})")
            }
            "connected" => {
                if qid.is_empty() {
                    return "quest-split: no quest in store".into();
                }
                self.open_quest_split_modal(qid.clone(), true, true, cx);
                format!("quest-split: connected ({qid})")
            }
            "error" => {
                if qid.is_empty() {
                    return "quest-split: no quest in store".into();
                }
                let mut st = SplitQuestModalState::new(qid.clone(), false, false);
                st.content_error = Some((None, None));
                self.quests.split_modal = Some(st);
                cx.notify();
                format!("quest-split: error ({qid})")
            }
            "close" => {
                self.quests.split_modal = None;
                cx.notify();
                "quest-split: closed".into()
            }
            other => format!("quest-split: unknown mode {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_achievement_quest(features: Vec<u64>) -> Quest {
        Quest::from_value(&json!({
            "id": "777",
            "config": {
                "config_version": 2,
                "id": "777",
                "starts_at": "2026-01-01T00:00:00Z",
                "expires_at": "2027-01-01T00:00:00Z",
                "features": features,
                "messages": {
                    "quest_name": "Valorant Champions",
                    "game_title": "VALORANT",
                    "game_publisher": "Riot Games"
                },
                "cta_config": {
                    "link": "https://playvalorant.com",
                    "button_label": "Play VALORANT"
                },
                "task_config_v2": {
                    "tasks": {
                        "ACHIEVEMENT_IN_GAME": {
                            "type": "ACHIEVEMENT_IN_GAME",
                            "target": 1,
                            "messages": { "task_title": "Win 1 match" }
                        }
                    }
                },
                "rewards_config": {
                    "rewards": [{
                        "type": 3,
                        "messages": { "name": "Radiant Frame", "name_with_article": "the Radiant Frame" }
                    }],
                    "platforms": [0]
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn checklist_and_right_panel_overrides_resolve_all_variants() {
        let q = sample_achievement_quest(vec![]);
        let (s1_web, s2_unlinked) =
            checklist_step_labels(&q, false, ConnectMethod::Web, false, false);
        assert!(s1_web.contains("VALORANT"));
        assert!(s2_unlinked.contains("Win 1 match"));
        let (s1_linked, _) = checklist_step_labels(&q, true, ConnectMethod::InGame, false, true);
        assert!(s1_linked.contains("VALORANT"));

        let base = resolve_right_panel_copy(&q, PremiumType::None);
        assert_eq!(base.title, "VALORANT");
        assert!(!base.opens_nitro);

        let nitro_q = sample_achievement_quest(vec![feature::NITRO_2_POINT_0_CTA]);
        assert!(resolve_right_panel_copy(&nitro_q, PremiumType::None).opens_nitro);
        assert!(resolve_right_panel_copy(&nitro_q, PremiumType::Nitro).opens_nitro);

        let orbs_q = sample_achievement_quest(vec![feature::ORBS_MULTIPLIER_QUEST]);
        assert!(resolve_right_panel_copy(&orbs_q, PremiumType::Basic).opens_nitro);

        let xbox_q = sample_achievement_quest(vec![feature::XBOX_GAME_PASS_QUEST]);
        assert!(resolve_right_panel_copy(&xbox_q, PremiumType::Nitro).opens_nitro);
    }
}
