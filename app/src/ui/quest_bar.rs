//! Quest Bar docked above the bottom-left user panel (`DESKTOP_ACCOUNT_PANEL_AREA`, module 66246).
//!
//! Renders the active/enrolled quest (when one is present in `QuestStore`):
//! - Unenrolled: partner branding, `"Get Reward!"` / `"Promoted"` header (`ew`), and expanded hero with
//!   `"{questName} Quest"`, task instructions (`870273`), and enroll CTA (`tR`).
//! - Enrolled / Completed: `"Available until {expirationDate}"` header (`eU`), progress row (`eR`: 42px reward
//!   badge + progress title & subtitle from `870273`), and when expanded:
//!   - Platform picker (`t_`: `"Quest on Desktop"` / `"Quest on Console"`, with `"Back"` button `13/7kX`),
//!   - Desktop warning cards (`tp`: `"Switch to Desktop"` / `"Switch to Windows"`),
//!   - Console Requirements checklist (`tC`: `"Console Requirements"`, 3 steps + `"Show Connections"` link +
//!     game-detection refresh unit `tx`: `"Launched {gameTitle}?"` / `"{gameTitle} not detected"`),
//!   - Footer CTA (`eS`: `"Claim Reward"`, `"Connect Console"`, `"View Eligible Games"`, or `452411` CTA).

use super::discover::text;
use super::quest_copy::{
    CtaAction, CtaContext, CtaSurface, CtaVariant, InstructionsContext, bar_subtitle, bar_title,
    format_short_date, primary_cta_label, resolve_cta, task_instructions,
};
use super::{Root, color, icon};
use dc_intl::Arg;
use dc_state::quests::{ActivePlatformScreen, Quest, QuestPlatform};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, div, img, linear_color_stop, linear_gradient,
    prelude::*, px, rgba,
};

/// Platform eligibility warning card variant (`tp` of module 66246, `aC` of module 61387).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformWarning {
    UseDesktopApp,
    NotSupportedOnMacOs,
}

impl PlatformWarning {
    /// `aC` of module 61387: determines whether a desktop quest needs a platform warning on the current OS/client.
    pub fn for_quest(
        quest: &Quest,
        active_screen: ActivePlatformScreen,
        is_web: bool,
        is_macos: bool,
        now_ms: i64,
    ) -> Option<Self> {
        let details = quest.task_details();
        let active_desktop = quest.is_enrolled()
            && !quest.is_completed()
            && !quest.is_expired(now_ms)
            && quest.third_party_task_details().is_none()
            && (details.percent_complete > 0. || active_screen == ActivePlatformScreen::Desktop);
        if !active_desktop {
            return None;
        }
        if is_macos && details.task_type == dc_state::quests::task::STREAM_ON_DESKTOP {
            return Some(PlatformWarning::NotSupportedOnMacOs);
        }
        if is_web && !quest.is_watch_video() {
            return Some(PlatformWarning::UseDesktopApp);
        }
        None
    }

    /// Warning text string (`BV6xDm` / `MFGxFM` of module 66246).
    pub fn hint_text(self) -> &'static str {
        match self {
            PlatformWarning::UseDesktopApp => dc_intl::t("BV6xDm"),
            PlatformWarning::NotSupportedOnMacOs => dc_intl::t("MFGxFM"),
        }
    }

    /// Warning card heading + body (`onh6ct`/`arVHRA`, `JPihZA`/`MFGxFM` of module 66246).
    pub fn card_copy(self) -> (&'static str, &'static str) {
        match self {
            PlatformWarning::UseDesktopApp => (dc_intl::t("onh6ct"), dc_intl::t("arVHRA")),
            PlatformWarning::NotSupportedOnMacOs => (dc_intl::t("JPihZA"), dc_intl::t("MFGxFM")),
        }
    }
}

/// Interactive state for the bottom-left Quest Bar (module 66246).
#[derive(Clone, Debug, Default)]
pub struct QuestBarState {
    pub expanded: bool,
    pub menu_open: bool,
    pub dismissed_ids: Vec<String>,
    /// Optional platform warning override (exercised by capture/tests or non-Windows streams).
    pub warning_override: Option<PlatformWarning>,
    pub refreshing_console: bool,
}

impl Root {
    /// Active quest to show in the bottom-left Quest Bar (`wo` / `tH` of module 66246).
    /// Returns `None` whenever there are no eligible quests so default empty accounts pay zero cost.
    fn active_bar_quest(&self) -> Option<&Quest> {
        if self.quests.store.quests.is_empty() {
            return None;
        }
        let now = chrono::Utc::now().timestamp_millis();
        self.quests
            .store
            .quests
            .iter()
            .filter(|q| !self.quests.bar.dismissed_ids.contains(&q.id))
            .filter(|q| !q.is_claimed() && (!q.is_expired(now) || q.awaits_claim()))
            // Prefer enrolled/completed quests first, then the first active quest.
            .max_by_key(|q| (q.is_completed(), q.is_enrolled()))
    }

    /// Whether the signed-in user has any connected Xbox or PlayStation account (`Du` of module 61387).
    pub(super) fn quest_console_connections(&self) -> (bool, bool) {
        let mut xbox = false;
        let mut psn = false;
        for conn in self.settings_extra.connections.as_deref().unwrap_or(&[]) {
            if conn["revoked"].as_bool().unwrap_or(false) {
                continue;
            }
            match conn["type"].as_str().unwrap_or_default() {
                "xbox" => xbox = true,
                "playstation" => psn = true,
                _ => {}
            }
        }
        (xbox, psn)
    }

    /// Renders the Quest Bar above `activity_panel` inside the user-panel dock (module 66246).
    pub(super) fn quest_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let quest = self.active_bar_quest()?.clone();
        let now = chrono::Utc::now().timestamp_millis();
        let enrolled = quest.is_enrolled();
        let completed = quest.is_completed();
        let expanded = self.quests.bar.expanded && !completed;
        let is_nitro = self.stores.account.premium_type == Some(2);
        let ctx = InstructionsContext {
            has_nitro: is_nitro,
            without_markdown: false,
            has_game_profile_link: false,
            needs_to_connect: false,
        };
        let (has_xbox, has_psn) = self.quest_console_connections();
        let has_console_conn = has_xbox || has_psn;
        let selected = self.quests.store.selected_platform(&quest.id);
        let active_screen = quest.active_platform_screen(selected, now);
        let is_progressing = quest.is_progressing_on_console(now);
        let details = quest.task_details();
        let pct = quest
            .third_party_task_details()
            .map_or(details.percent_complete, |d| d.percent_complete);
        let pct_int = (pct * 100.).floor().clamp(0., 100.) as u64;

        let card = if enrolled {
            self.quest_bar_enrolled(
                &quest,
                now,
                expanded,
                completed,
                active_screen,
                is_progressing,
                has_console_conn,
                pct_int,
                ctx,
                cx,
            )
        } else {
            self.quest_bar_unenrolled(&quest, expanded, ctx, cx)
        };

        Some(
            div()
                .id("quest-bar-container")
                .w_full()
                .border_b_1()
                .border_color(color("--border-subtle"))
                .overflow_hidden()
                .child(card)
                .into_any_element(),
        )
    }

    /// Unenrolled Quest Bar (`ew` header + `tR` expanded body, module 66246).
    fn quest_bar_unenrolled(
        &self,
        quest: &Quest,
        expanded: bool,
        ctx: InstructionsContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let quest_id = quest.id.clone();
        let cta_link = quest.config.cta_link.clone();
        let reward_url = quest
            .config
            .assets_reward_tile
            .as_deref()
            .or(quest.config.assets_game_tile.as_deref())
            .map(|a| super::quests::quest_asset_url(&quest.id, a));
        let reward_img = reward_url.and_then(|u| self.images.get(&u).cloned());
        let title = dc_intl::format(
            "EQa7os",
            &[(
                "questName",
                Arg::Str(std::borrow::Cow::Borrowed(&quest.config.quest_name)),
            )],
        );
        let instructions = task_instructions(quest, ctx);
        let cta_label = primary_cta_label(quest, false);

        div()
            .w_full()
            .p(px(10.))
            .bg(linear_gradient(
                135.,
                linear_color_stop(rgba(0x5865f233), 0.),
                linear_color_stop(color("--background-base-low"), 1.),
            ))
            .flex()
            .flex_col()
            .gap(px(8.))
            // Header row (`ew`): reward thumb + quest name / "Get Reward!" + "Promoted" + Actions
            .child(
                div()
                    .id("quest-bar-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quests.bar.expanded = !this.quests.bar.expanded;
                        this.quests.bar.menu_open = false;
                        cx.notify();
                    }))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(40.))
                            .rounded(px(6.))
                            .overflow_hidden()
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .children(
                                reward_img.map(|im| {
                                    img(im).size_full().object_fit(gpui::ObjectFit::Cover)
                                }),
                            )
                            .when(
                                quest.config.assets_reward_tile.is_none()
                                    && quest.config.assets_game_tile.is_none(),
                                |this| this.child(icon("home_quests", 20., "--text-brand")),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                text(13., 16., 600., "--text-strong")
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(quest.config.quest_name.clone()),
                            )
                            .child(
                                text(12., 16., 500., "--text-muted")
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(dc_intl::t("3mgEQf")),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(text(11., 14., 400., "--text-muted").child(dc_intl::t("o6FLcF")))
                            .child(icon("standing_circle_question", 12., "--text-muted"))
                            .child(self.quest_bar_actions_button(&quest_id, &cta_link, cx)),
                    ),
            )
            .when(expanded, |this| {
                let enroll_id = quest.id.clone();
                this.child(
                    div()
                        .pt(px(4.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(text(14., 18., 600., "--text-strong").child(title))
                        .child(text(12., 16., 400., "--text-subtle").child(instructions))
                        .child(
                            div()
                                .id("quest-bar-enroll-cta")
                                .mt(px(4.))
                                .w_full()
                                .h(px(32.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.quest_enroll_local(&enroll_id, cx);
                                }))
                                .child(
                                    text(13., 16., 600., "--control-primary-text-default")
                                        .child(cta_label),
                                ),
                        ),
                )
            })
            .into_any_element()
    }

    /// Enrolled / Completed Quest Bar (`eb` / `tF` / `eR` / `tQ` / `tC` / `tp`, module 66246).
    #[allow(clippy::too_many_arguments)]
    fn quest_bar_enrolled(
        &self,
        quest: &Quest,
        now_ms: i64,
        expanded: bool,
        completed: bool,
        active_screen: ActivePlatformScreen,
        is_progressing: bool,
        has_console_conn: bool,
        pct_int: u64,
        ctx: InstructionsContext,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let quest_id = quest.id.clone();
        let cta_link = quest.config.cta_link.clone();
        let exp_date = format_short_date(&quest.config.expires_at);
        let available_until = dc_intl::format(
            "pX+fmn",
            &[(
                "expirationDate",
                Arg::Str(std::borrow::Cow::Borrowed(&exp_date)),
            )],
        );
        let title = bar_title(quest, active_screen, None);
        let subtitle = bar_subtitle(quest, expanded, is_progressing, None, false, ctx);
        let show_header = expanded || completed;
        let platforms = quest.supported_platforms();
        let can_go_back = platforms.len() > 1 && pct_int == 0 && !is_progressing;
        let awaiting_console = quest.is_play_on_console() && !has_console_conn && !is_progressing;

        div()
            .w_full()
            .p(px(10.))
            .bg(if completed {
                linear_gradient(
                    90.,
                    linear_color_stop(rgba(0x5865f240), 0.),
                    linear_color_stop(rgba(0xeb459e30), 1.),
                )
            } else {
                linear_gradient(
                    90.,
                    linear_color_stop(color("--background-surface-high"), 0.),
                    linear_color_stop(color("--background-base-low"), 1.),
                )
            })
            .flex()
            .flex_col()
            .gap(px(8.))
            // `eU`: "Available until {expirationDate}" + Actions `...`
            .when(show_header, |this| {
                this.child(
                    div()
                        .pb(px(6.))
                        .border_b_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .child(
                            text(11., 14., 500., "--text-muted")
                                .flex_1()
                                .min_w(px(0.))
                                .child(available_until),
                        )
                        .child(self.quest_bar_actions_button(&quest_id, &cta_link, cx)),
                )
            })
            // `eR` (`TTqPW3`): 42px progress ring + title & subtitle
            .child(
                div()
                    .id("quest-bar-progress-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !completed {
                            this.quests.bar.expanded = !this.quests.bar.expanded;
                            this.quests.bar.menu_open = false;
                            cx.notify();
                        }
                    }))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(progress_badge(42., pct_int, completed))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                text(13., 16., 600., "--text-strong")
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                            .child(text(12., 16., 400., "--text-subtle").child(subtitle)),
                    ),
            )
            // Expanded body (`tB` -> `tQ` -> `tv`)
            .when(expanded, |this| {
                this.child(match active_screen {
                    ActivePlatformScreen::Select => self.quest_bar_platform_select(&quest.id, cx),
                    ActivePlatformScreen::Desktop => {
                        self.quest_bar_desktop_warning(quest, active_screen, now_ms)
                    }
                    ActivePlatformScreen::Console => self.quest_bar_console_requirements(
                        quest,
                        has_console_conn,
                        is_progressing,
                        cx,
                    ),
                })
            })
            // Footer CTA (`eS` / `tQ`)
            .when(
                completed || (expanded && active_screen != ActivePlatformScreen::Select),
                |this| {
                    let back_id = quest.id.clone();
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(expanded && can_go_back, |row| {
                                row.child(
                                    div()
                                        .id("quest-bar-back")
                                        .flex_none()
                                        .size(px(32.))
                                        .rounded(px(6.))
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|s| {
                                            s.bg(color("--control-secondary-background-hover"))
                                        })
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.quests.store.set_selected_platform(&back_id, None);
                                            cx.notify();
                                        }))
                                        .child(icon(
                                            "chevron_small_left",
                                            16.,
                                            "--control-secondary-icon-default",
                                        ))
                                        // Accessible tooltip/title parity (`13/7kX`: "Back")
                                        .when(false, |el| el.child(dc_intl::t("13/7kX"))),
                                )
                            })
                            .child(self.quest_bar_footer_cta(
                                quest,
                                now_ms,
                                active_screen,
                                awaiting_console,
                                pct_int > 0,
                                is_progressing,
                                cx,
                            )),
                    )
                },
            )
            .into_any_element()
    }

    /// `t_` of module 66246: "Quest on Desktop" + "Quest on Console" stacked selector.
    fn quest_bar_platform_select(&self, quest_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let id_desktop = quest_id.to_owned();
        let id_console = quest_id.to_owned();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .id("quest-bar-select-desktop")
                    .w_full()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests
                            .store
                            .set_selected_platform(&id_desktop, Some(QuestPlatform::Desktop));
                        cx.notify();
                    }))
                    .child(
                        text(13., 16., 500., "--control-secondary-text-default")
                            .child(dc_intl::t("QXc01+")),
                    )
                    .child(icon(
                        "platform_desktop",
                        16.,
                        "--control-secondary-icon-default",
                    )),
            )
            .child(
                div()
                    .id("quest-bar-select-console")
                    .w_full()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(6.))
                    .bg(color("--control-secondary-background-default"))
                    .hover(|s| s.bg(color("--control-secondary-background-hover")))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.quests
                            .store
                            .set_selected_platform(&id_console, Some(QuestPlatform::Console));
                        cx.notify();
                    }))
                    .child(
                        text(13., 16., 500., "--control-secondary-text-default")
                            .child(dc_intl::t("8lAfuB")),
                    )
                    .child(icon(
                        "bot_game_controller",
                        16.,
                        "--control-secondary-icon-default",
                    )),
            )
            .into_any_element()
    }

    /// `tp` / `tE` of module 66246 (`card_RJPPHr`): desktop/macOS eligibility warning card.
    fn quest_bar_desktop_warning(
        &self,
        quest: &Quest,
        active_screen: ActivePlatformScreen,
        now_ms: i64,
    ) -> AnyElement {
        let warning = self.quests.bar.warning_override.or_else(|| {
            PlatformWarning::for_quest(
                quest,
                active_screen,
                false,
                cfg!(target_os = "macos"),
                now_ms,
            )
        });
        let Some(warning) = warning else {
            return div().into_any_element();
        };
        let (header, body) = warning.card_copy();
        div()
            .w_full()
            .mt(px(4.))
            .p(px(10.))
            .rounded(px(8.))
            .bg(color("--message-background-hover"))
            .border_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(icon(
                        "standing_circle_error",
                        16.,
                        "--text-feedback-warning",
                    ))
                    .child(text(12., 16., 600., "--text-default").child(header)),
            )
            .child(text(12., 16., 400., "--text-muted").child(body))
            .into_any_element()
    }

    /// `tC` + `tx` of module 66246: Console Requirements 3-step checklist + game launch refresh unit.
    fn quest_bar_console_requirements(
        &self,
        quest: &Quest,
        has_console_conn: bool,
        is_progressing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let multi = quest.is_multi_game_play_quest();
        let game = &quest.config.game_title;
        let minutes = quest.task_details().target_minutes as i64;
        let completed = quest.is_completed();
        let step1_done = has_console_conn || is_progressing || completed;
        let step2_done = is_progressing || completed;
        let step3_done = completed;

        let step2_label = if multi {
            dc_intl::t("5tXqFe").to_owned()
        } else {
            dc_intl::format(
                "+8JB6Y",
                &[("gameTitle", Arg::Str(std::borrow::Cow::Borrowed(game)))],
            )
        };
        let step3_label = dc_intl::format("HhfrYS", &[("numMinutes", Arg::Num((minutes) as f64))]);
        let error_hints = self.quests.store.error_hints(&quest.id);
        let has_errors = !error_hints.is_empty();
        let quest_id_refresh = quest.id.clone();

        let step_row = |num: usize, done: bool, label: String, extra: Option<AnyElement>| {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .mt(px(1.))
                        .size(px(16.))
                        .rounded_full()
                        .bg(color(if done {
                            "--status-positive"
                        } else {
                            "--background-mod-strong"
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if done {
                            icon("checkbox_check", 10., "--text-overlay-light").into_any_element()
                        } else {
                            text(10., 12., 600., "--text-strong")
                                .child(num.to_string())
                                .into_any_element()
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(text(12., 16., 400., "--text-default").child(label))
                        .children(extra),
                )
        };

        let show_connections = has_console_conn.then(|| {
            div()
                .id("quest-bar-show-connections")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_settings_page("connections", cx);
                }))
                .child(
                    text(11., 14., 500., "--text-link")
                        .hover(|s| s.underline())
                        .child(dc_intl::t("qiS+xj")),
                )
                .into_any_element()
        });

        div()
            .w_full()
            .pt(px(6.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(text(11., 14., 700., "--text-muted").child(dc_intl::t("UPWlJu")))
            .child(step_row(
                1,
                step1_done,
                dc_intl::t("EJFSvD").to_owned(),
                show_connections,
            ))
            .child(step_row(2, step2_done, step2_label, None))
            .child(step_row(3, step3_done, step3_label, None))
            // `tx` (`microphoneUnit_Ggksyu`): shown when console connected, not progressing, not complete
            .when(has_console_conn && !is_progressing && !completed, |this| {
                let header = if has_errors {
                    if multi {
                        dc_intl::t("N33EuL").to_owned()
                    } else {
                        dc_intl::format(
                            "28Ql27",
                            &[("gameTitle", Arg::Str(std::borrow::Cow::Borrowed(game)))],
                        )
                    }
                } else if multi {
                    dc_intl::t("YstzGO").to_owned()
                } else {
                    dc_intl::format(
                        "gX0Qcx",
                        &[("gameTitle", Arg::Str(std::borrow::Cow::Borrowed(game)))],
                    )
                };
                let body = if has_errors {
                    error_hints.join(" ")
                } else if multi {
                    dc_intl::t("bUyEZZ").to_owned()
                } else {
                    dc_intl::format(
                        "GXqvC1",
                        &[("gameTitle", Arg::Str(std::borrow::Cow::Borrowed(game)))],
                    )
                };
                this.child(
                    div()
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(color("--border-subtle"))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon(
                                    "standing_circle_error",
                                    14.,
                                    if has_errors {
                                        "--text-feedback-critical"
                                    } else {
                                        "--text-feedback-warning"
                                    },
                                ))
                                .child(
                                    text(12., 16., 500., "--text-default")
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(header),
                                )
                                .child(
                                    div()
                                        .id("quest-bar-console-refresh")
                                        .flex_none()
                                        .size(px(20.))
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(color("--background-mod-subtle")))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.quests.bar.refreshing_console = false;
                                            this.quests.store.clear_error_hints(&quest_id_refresh);
                                            cx.notify();
                                        }))
                                        .child(icon(
                                            "system_refresh",
                                            14.,
                                            "--interactive-text-default",
                                        )),
                                ),
                        )
                        .child(text(11., 15., 400., "--text-muted").child(body)),
                )
            })
            .into_any_element()
    }

    /// `eS` of module 66246: Quest Bar footer CTA button.
    #[allow(clippy::too_many_arguments)]
    fn quest_bar_footer_cta(
        &self,
        quest: &Quest,
        now_ms: i64,
        active_screen: ActivePlatformScreen,
        awaiting_console_connections: bool,
        has_made_progress: bool,
        is_progressing: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let completed = quest.is_completed();
        let show_idle_cta =
            active_screen != ActivePlatformScreen::Select && !has_made_progress && !is_progressing;
        let (label, variant, action) = if completed {
            (
                dc_intl::t("cfY4PE").to_owned(),
                CtaVariant::Primary,
                CtaAction::ClaimReward,
            )
        } else if active_screen == ActivePlatformScreen::Console && awaiting_console_connections {
            (
                dc_intl::t("csptqV").to_owned(),
                CtaVariant::Primary,
                CtaAction::ConnectConsole,
            )
        } else if show_idle_cta && quest.is_multi_game_play_quest() {
            (
                dc_intl::t("93PTEs").to_owned(),
                CtaVariant::Secondary,
                CtaAction::ViewEligibleGames,
            )
        } else {
            let spec = resolve_cta(
                quest,
                CtaContext {
                    surface: CtaSurface::QuestBar,
                    now_ms,
                    is_progressing,
                    awaiting_console_connections,
                    active_screen,
                    has_linked_game_account: false,
                    animate_refresh_icon: true,
                },
            );
            (spec.label, spec.variant, spec.action)
        };

        let quest_id = quest.id.clone();
        let cta_link = quest.config.cta_link.clone();
        let primary_bg = matches!(variant, CtaVariant::Primary | CtaVariant::OverlayPrimary);

        div()
            .id("quest-bar-footer-cta")
            .flex_1()
            .min_w(px(0.))
            .h(px(32.))
            .px(px(10.))
            .rounded(px(6.))
            .bg(color(if primary_bg {
                "--control-primary-background-default"
            } else {
                "--control-secondary-background-default"
            }))
            .hover(|s| {
                s.bg(color(if primary_bg {
                    "--control-primary-background-hover"
                } else {
                    "--control-secondary-background-hover"
                }))
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.handle_quest_cta_action(&quest_id, &cta_link, action, cx);
            }))
            .child(
                text(
                    13.,
                    16.,
                    600.,
                    if primary_bg {
                        "--control-primary-text-default"
                    } else {
                        "--control-secondary-text-default"
                    },
                )
                .child(label),
            )
            .into_any_element()
    }

    /// Actions `...` trigger button (`DEoVWZ` of module 66246 -> opens `270045` context menu).
    fn quest_bar_actions_button(
        &self,
        quest_id: &str,
        _cta_link: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let qid = quest_id.to_owned();
        div()
            .id("quest-bar-actions")
            .flex_none()
            .size(px(20.))
            .rounded(px(4.))
            .hover(|s| s.bg(color("--background-mod-subtle")))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                cx.stop_propagation();
                let pos = ev.position();
                this.open_quest_menu(
                    qid.clone(),
                    super::quest_menu::QuestSurface::QuestBar,
                    false,
                    true,
                    true,
                    f32::from(pos.x),
                    (f32::from(pos.y) - 180.).max(8.),
                    cx,
                );
            }))
            .child(icon("more_horizontal", 14., "--interactive-text-default"))
            .when(false, |el| el.child(dc_intl::t("DEoVWZ")))
            .into_any_element()
    }

    /// Quest CTA click dispatcher (`201805` / `968779` / `452411`).
    pub(super) fn handle_quest_cta_action(
        &mut self,
        quest_id: &str,
        cta_link: &str,
        action: CtaAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            CtaAction::Enroll => {
                self.send_quest_enroll(quest_id, super::quest_menu::QuestSurface::QuestHome, cx);
            }
            CtaAction::ConnectConsole => {
                self.open_settings_page("connections", cx);
            }
            CtaAction::ConnectAccount => {
                self.open_quest_split_modal(quest_id.to_owned(), false, true, cx);
            }
            CtaAction::ViewConnection => {
                self.open_quest_split_modal(quest_id.to_owned(), true, true, cx);
            }
            CtaAction::OpenGameLink
            | CtaAction::WatchVideo
            | CtaAction::WatchOnMobile
            | CtaAction::LaunchActivity => {
                if !cta_link.is_empty() {
                    cx.open_url(cta_link);
                } else {
                    self.open_quests(cx);
                }
            }
            CtaAction::ViewEligibleGames => {
                // Module 194267: open SPONSORED_QUEST_SHEET for package quests.
                let (pkg, games) = self
                    .quests
                    .store
                    .quests
                    .iter()
                    .find(|q| q.id == quest_id)
                    .map(|q| {
                        let pkg = q.config.features.iter().find_map(|f| match f {
                            12 => Some("PACKAGE_ACTION_ADVENTURE".to_owned()),
                            13 => Some("PACKAGE_RPG_MMO".to_owned()),
                            14 => Some("PACKAGE_RACING_SPORTS".to_owned()),
                            15 => Some("PACKAGE_SANDBOX_CREATIVE".to_owned()),
                            16 => Some("PACKAGE_FAMILY_FRIENDLY".to_owned()),
                            17 => Some("PACKAGE_HOLIDAY_SEASON".to_owned()),
                            18 => Some("PACKAGE_NEW_YEARS".to_owned()),
                            _ => None,
                        });
                        let list = vec![super::quest_call_and_sheet::EligibleGameTile {
                            app_id: q.id.clone(),
                            name: if q.config.game_title.is_empty() {
                                q.config.quest_name.clone()
                            } else {
                                q.config.game_title.clone()
                            },
                            store_url: (!cta_link.is_empty()).then(|| cta_link.to_owned()),
                        }];
                        (pkg, list)
                    })
                    .unwrap_or_else(|| (None, Vec::new()));
                self.open_sponsored_quest_sheet(pkg, games, cx);
            }
            CtaAction::ClaimReward => {
                self.trigger_quest_claim(quest_id, cx);
            }
            CtaAction::ViewReward | CtaAction::SeeCode => {
                self.open_quests(cx);
            }
            CtaAction::CheckForLaunches | CtaAction::CheckConnection => {
                self.quests.store.clear_error_hints(quest_id);
                self.open_quest_connection_helper(quest_id, true, cx);
            }
            CtaAction::OpenPlatformMenu => {
                if self.quests.open_platform_menu.as_deref() == Some(quest_id) {
                    self.quests.open_platform_menu = None;
                } else {
                    self.quests.open_platform_menu = Some(quest_id.to_owned());
                }
                cx.notify();
            }
            CtaAction::Disabled => {}
        }
    }

    pub(super) fn quest_enroll_local(&mut self, quest_id: &str, cx: &mut Context<Self>) {
        if let Some(quest) = self
            .quests
            .store
            .quests
            .iter_mut()
            .find(|q| q.id == quest_id)
        {
            let status = quest
                .user_status
                .get_or_insert_with(|| dc_state::quests::UserStatus {
                    quest_id: quest_id.to_owned(),
                    ..Default::default()
                });
            if status.enrolled_at.is_none() {
                status.enrolled_at = Some(chrono::Utc::now().to_rfc3339());
            }
        }
        cx.notify();
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_bar(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "expand" => {
                self.quests.bar.expanded = true;
            }
            "collapse" => {
                self.quests.bar.expanded = false;
            }
            "console" => {
                self.quests.bar.expanded = true;
                if let Some(q) = self.active_bar_quest().cloned() {
                    self.quests
                        .store
                        .set_selected_platform(&q.id, Some(QuestPlatform::Console));
                }
            }
            "console-connected" => {
                self.quests.bar.expanded = true;
                let list = self.settings_extra.connections.get_or_insert_with(Vec::new);
                if !list.iter().any(|c| c["type"].as_str() == Some("xbox")) {
                    list.push(serde_json::json!({
                        "id": "capture-xbox-1",
                        "type": "xbox",
                        "name": "GravePlayer",
                        "verified": true,
                        "revoked": false
                    }));
                }
                if let Some(q) = self.active_bar_quest().cloned() {
                    self.quests
                        .store
                        .set_selected_platform(&q.id, Some(QuestPlatform::Console));
                }
            }
            "warning" => {
                self.quests.bar.expanded = true;
                self.quests.bar.warning_override = Some(PlatformWarning::NotSupportedOnMacOs);
                if let Some(q) = self.active_bar_quest().cloned() {
                    self.quests
                        .store
                        .set_selected_platform(&q.id, Some(QuestPlatform::Desktop));
                }
            }
            "clear" => {
                self.quests.bar.expanded = false;
                self.quests.bar.warning_override = None;
            }
            _ => {}
        }
        cx.notify();
        format!(
            "quest-bar: mode={mode} expanded={} quests={}",
            self.quests.bar.expanded,
            self.quests.store.quests.len()
        )
    }
}

/// Circular progress ring badge shared by Quest Bar (`42px`) and Quest Home tiles (`76px`).
pub(super) fn progress_badge(size: f32, pct_int: u64, completed: bool) -> AnyElement {
    let ring_color = if completed {
        color("--status-positive")
    } else if pct_int > 0 {
        color("--text-brand")
    } else {
        color("--border-strong")
    };
    let inner = (size - 6.).max(24.);
    div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .border_2()
        .border_color(ring_color)
        .bg(color("--background-base-lowest"))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(inner))
                .rounded_full()
                .bg(color("--background-surface-high"))
                .flex()
                .items_center()
                .justify_center()
                .child(if completed {
                    icon("checkbox_check", (size * 0.4).min(24.), "--status-positive")
                        .into_any_element()
                } else if pct_int > 0 {
                    text(
                        if size >= 60. { 15. } else { 11. },
                        if size >= 60. { 18. } else { 14. },
                        700.,
                        "--text-strong",
                    )
                    .child(format!("{pct_int}%"))
                    .into_any_element()
                } else {
                    icon("home_quests", (size * 0.42).min(24.), "--text-brand").into_any_element()
                }),
        )
        .into_any_element()
}

/// Active Quest Bar Bounty Card state (module 682908: `ogxXGq`, `GcsZKJ`, `NN79E9`, `o6FLcF`,
/// `DEoVWZ`, `kfks9Y`, `UMclVN`, `1uzE2S`).
#[derive(Clone, Debug)]
pub(crate) struct QuestBarBountyState {
    pub id: String,
    pub product_name: String,
    pub cta_label: String,
    pub cta_url: String,
    pub video_duration_seconds: u32,
    pub menu_open: bool,
}

impl Root {
    /// Overlay layer for Quest Bar Bounty Card & popout menu (`682908`).
    pub(super) fn quest_bar_bounty_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bounty = self.quest_bar_bounty.as_ref()?;
        let bounty_id = bounty.id.clone();
        let cta_url = bounty.cta_url.clone();
        let watch_label = if bounty.video_duration_seconds > 0 {
            format!(
                "{} ({}s)",
                dc_intl::t("kfks9Y"),
                bounty.video_duration_seconds
            )
        } else {
            dc_intl::t("kfks9Y").to_owned()
        };

        let menu = bounty.menu_open.then(|| {
            div()
                .id("quest-bar-bounty-menu")
                .aria_label(dc_intl::t("ogxXGq"))
                .w(px(220.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("bounty-open-link")
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(bounty.cta_label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !cta_url.is_empty() {
                                cx.open_url(&cta_url);
                            }
                            this.quest_bar_bounty = None;
                            cx.notify();
                        })),
                )
                .child(div().my(px(3.)).h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .id("bounty-why-seeing")
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("GcsZKJ"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.quest_bar_bounty = None;
                            this.put_app_toast(dc_intl::t("GcsZKJ").to_owned(), true, cx);
                        })),
                )
                .child(
                    div()
                        .id("bounty-hide-this")
                        .cursor_pointer()
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-feedback-critical"))
                        .child(dc_intl::t("NN79E9"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this.requests.send(crate::net::Request::Rest {
                                key: Some("quests:bounty-dismiss".to_owned()),
                                method: reqwest::Method::POST,
                                route: format!("/quests/creatives/{bounty_id}/dismiss"),
                                body: Some(serde_json::json!({ "ad_creative_type": "BOUNTY" })),
                            });
                            this.quest_bar_bounty = None;
                            cx.notify();
                        })),
                )
        });

        Some(
            gpui::deferred(
                div()
                    .id("quest-bar-bounty-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000080))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(12.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quest_bar_bounty = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("quest-bar-bounty-card")
                            .w(px(280.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(gpui::linear_gradient(
                                180.,
                                gpui::linear_color_stop(rgba(0x2d3266ff), 0.),
                                gpui::linear_color_stop(rgba(0x16171aff), 1.),
                            ))
                            .p(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            // Header: product icon + Promoted + Actions button
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .size(px(28.))
                                                    .rounded(px(6.))
                                                    .bg(rgba(0x5865f244))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(icon("home_quests", 16., "--white")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(rgba(0xffffffcc))
                                                    .child(dc_intl::t("o6FLcF")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("bounty-more-btn")
                                            .aria_label(dc_intl::t("DEoVWZ"))
                                            .cursor_pointer()
                                            .size(px(26.))
                                            .rounded(px(6.))
                                            .bg(rgba(0xffffff1a))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--white"))
                                            .child("•••")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(b) = this.quest_bar_bounty.as_mut() {
                                                    b.menu_open = !b.menu_open;
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Reward tile + copy
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .id("bounty-reward-tile")
                                            .aria_label(dc_intl::t("UMclVN"))
                                            .flex_none()
                                            .size(px(56.))
                                            .rounded(px(10.))
                                            .border_1()
                                            .border_color(rgba(0xffffff28))
                                            .bg(rgba(0x5865f233))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("shop_orb_18", 28., "--white")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--white"))
                                                    .truncate()
                                                    .child(bounty.product_name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(rgba(0xffffffcc))
                                                    .child(dc_intl::t("1uzE2S")),
                                            ),
                                    ),
                            )
                            // Watch CTA
                            .child(
                                div()
                                    .id("bounty-watch-cta")
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
                                    .child(watch_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.quest_bar_bounty = None;
                                        this.open_quests(cx);
                                    })),
                            ),
                    )
                    .children(menu),
            )
            .with_priority(95)
            .into_any_element(),
        )
    }

    /// Capture harness: `quest-bounty [card|menu|close]` (module 682908).
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_bounty(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        match arg.trim() {
            "close" => {
                self.quest_bar_bounty = None;
                cx.notify();
                "quest-bounty: closed".to_owned()
            }
            "menu" => {
                self.quest_bar_bounty = Some(QuestBarBountyState {
                    id: "bounty_682908".to_owned(),
                    product_name: "Starfall Odyssey".to_owned(),
                    cta_label: "Visit Starfall Odyssey".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    video_duration_seconds: 30,
                    menu_open: true,
                });
                cx.notify();
                "quest-bounty: menu".to_owned()
            }
            _ => {
                self.quest_bar_bounty = Some(QuestBarBountyState {
                    id: "bounty_682908".to_owned(),
                    product_name: "Starfall Odyssey".to_owned(),
                    cta_label: "Visit Starfall Odyssey".to_owned(),
                    cta_url: "https://discord.com/quests".to_owned(),
                    video_duration_seconds: 30,
                    menu_open: false,
                });
                cx.notify();
                "quest-bounty: card".to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_warning_copy_resolves_official_strings() {
        assert_eq!(
            PlatformWarning::UseDesktopApp.hint_text(),
            "Use the desktop app to make progress!"
        );
        assert_eq!(
            PlatformWarning::UseDesktopApp.card_copy(),
            (
                "Switch to Desktop",
                "Quest progress can only be made on the Discord desktop app"
            )
        );
        assert_eq!(
            PlatformWarning::NotSupportedOnMacOs.card_copy(),
            (
                "Switch to Windows",
                "This Quest can't be completed on macOS"
            )
        );
    }
}
