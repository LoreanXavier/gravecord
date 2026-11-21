//! Quest Call Header Card (`10394`) and Sponsored Quest Sheet (`194267` + `659309`).
//!
//! - Module `10394` (`QuestChannelCallHeader`): compact voice/stream call header card for
//!   `QUEST_LIVE_STREAM` with all 4 progress states:
//!   1. Unenrolled: `o6FLcF` ("Promoted"), `EQa7os` ("{questName} Quest"), `pX+fmn`
//!      ("Available until {expirationDate}"), `LLLLPD` ("Learn More") + enroll CTA.
//!   2. Enrolled & participating: task progress bar + `pX+fmn`.
//!   3. Enrolled & not participating: task instructions + `VN1Ajl` ("Check Progress").
//!   4. Completed (unclaimed): `APddvF` ("Claim by {expirationDate}") + `cfY4PE` ("Claim Reward").
//! - Module `194267` (`SPONSORED_QUEST_SHEET`): eligible-games popout/modal for `PACKAGE_*`
//!   quests mapping all 7 package titles (`PYFVdf`, `rFQo2F`, `zDRa6g`, `vz9U2Q`, `IOiAE0`,
//!   `cJPqvD`, `S0w2mv`) + fallback `D+DkEH` ("Eligible Games"), `VgN1Bn` ("Your reward awaits."),
//!   padded 10-slot grid of eligible games, and error modal `659309` (`iE/uSw` / `G6SnvT` / `cpT0Cq`).

use super::{Root, color, icon};
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgb, rgba};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum QuestCallPhase {
    Unenrolled,
    EnrolledParticipating { percent: u8 },
    EnrolledIdle,
    Completed,
}

#[derive(Clone, Debug)]
pub struct EligibleGameTile {
    #[allow(dead_code)]
    pub app_id: String,
    pub name: String,
    pub store_url: Option<String>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(super) enum QuestCallOrSheetOverlay {
    CallHeaderCard {
        quest_id: String,
        quest_name: String,
        task_description: String,
        expiration_date: String,
        phase: QuestCallPhase,
    },
    SponsoredSheet {
        package_feature: Option<String>,
        games: Vec<EligibleGameTile>,
        loading: bool,
        error: bool,
    },
}

/// Maps `quest.config.features` `PACKAGE_*` feature to module `194267` title.
pub fn package_title(feature: Option<&str>) -> &'static str {
    match feature {
        Some("PACKAGE_ACTION_ADVENTURE") => dc_intl::t("PYFVdf"),
        Some("PACKAGE_RPG_MMO") => dc_intl::t("rFQo2F"),
        Some("PACKAGE_RACING_SPORTS") => dc_intl::t("zDRa6g"),
        Some("PACKAGE_SANDBOX_CREATIVE") => dc_intl::t("vz9U2Q"),
        Some("PACKAGE_FAMILY_FRIENDLY") => dc_intl::t("IOiAE0"),
        Some("PACKAGE_HOLIDAY_SEASON") => dc_intl::t("cJPqvD"),
        Some("PACKAGE_NEW_YEARS") => dc_intl::t("S0w2mv"),
        _ => dc_intl::t("D+DkEH"),
    }
}

/// Pads eligible game count to 4, 5, 8, or 10 slots matching `194267`.
pub fn sheet_grid_slots(count: usize) -> usize {
    if count <= 4 {
        4
    } else if count <= 5 {
        5
    } else if count <= 8 {
        8
    } else {
        10
    }
}

impl Root {
    /// Opens the `194267` Sponsored Quest Sheet for a `PACKAGE_*` quest.
    pub(super) fn open_sponsored_quest_sheet(
        &mut self,
        package_feature: Option<String>,
        games: Vec<EligibleGameTile>,
        cx: &mut Context<Self>,
    ) {
        let error = games.is_empty();
        self.quest_call_sheet_overlay = Some(QuestCallOrSheetOverlay::SponsoredSheet {
            package_feature,
            games,
            loading: false,
            error,
        });
        cx.notify();
    }

    /// Renders `10394` Quest Call Header Card or `194267` Sponsored Quest Sheet overlay.
    pub(super) fn quest_call_sheet_overlay_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ov = self.quest_call_sheet_overlay.as_ref()?;
        let card: AnyElement = match ov {
            QuestCallOrSheetOverlay::CallHeaderCard {
                quest_id,
                quest_name,
                task_description,
                expiration_date,
                phase,
            } => {
                let heading = dc_intl::format(
                    "EQa7os",
                    &[("questName", Arg::Str(Cow::Borrowed(quest_name)))],
                );
                let subtitle_key = if matches!(phase, QuestCallPhase::Completed) {
                    "APddvF"
                } else {
                    "pX+fmn"
                };
                let subtitle = dc_intl::format(
                    subtitle_key,
                    &[("expirationDate", Arg::Str(Cow::Borrowed(expiration_date)))],
                );
                let qid = quest_id.clone();

                div()
                    .id("quest-call-header-card")
                    .w(px(380.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    // Top banner when unenrolled (`o6FLcF` "Promoted")
                    .when(matches!(phase, QuestCallPhase::Unenrolled), |card| {
                        card.child(
                            div()
                                .h(px(64.))
                                .px(px(14.))
                                .bg(rgba(0x5865f233))
                                .border_b_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .px(px(8.))
                                        .py(px(3.))
                                        .rounded(px(10.))
                                        .bg(rgba(0x00000088))
                                        .text_size(px(11.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child(dc_intl::t("o6FLcF")),
                                )
                                .child(icon("discover_nav_quests", 24., "--icon-strong")),
                        )
                    })
                    .child(
                        div()
                            .p(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            // Title row (`EQa7os`, `pX+fmn`/`APddvF`, `DEoVWZ` Actions)
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(8.))
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
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(SharedString::from(heading)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .child(SharedString::from(subtitle)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("quest-call-actions")
                                            .aria_label(dc_intl::t("DEoVWZ"))
                                            .size(px(26.))
                                            .flex_none()
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_call_sheet_overlay = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 14., "--icon-subtle")),
                                    ),
                            )
                            // Task description / Progress bar
                            .child(match phase {
                                QuestCallPhase::EnrolledParticipating { percent } => div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .flex()
                                            .justify_between()
                                            .text_size(px(12.))
                                            .text_color(color("--text-default"))
                                            .child(SharedString::from(task_description.clone()))
                                            .child(SharedString::from(format!("{percent}%"))),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(6.))
                                            .rounded(px(3.))
                                            .bg(color("--background-base-lowest"))
                                            .overflow_hidden()
                                            .child(
                                                div()
                                                    .w(px(348. * (*percent as f32 / 100.)))
                                                    .h_full()
                                                    .bg(rgb(0x5865f2)),
                                            ),
                                    )
                                    .into_any_element(),
                                _ => div()
                                    .p(px(10.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(task_description.clone()))
                                    .into_any_element(),
                            })
                            // Footer CTAs
                            .child(match phase {
                                QuestCallPhase::Unenrolled => div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id("quest-call-learn-more")
                                            .flex_1()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-secondary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_call_sheet_overlay = None;
                                                this.open_quests(cx);
                                            }))
                                            .child(dc_intl::t("LLLLPD")),
                                    )
                                    .child(
                                        div()
                                            .id("quest-call-accept")
                                            .flex_1()
                                            .h(px(36.))
                                            .rounded(px(8.))
                                            .bg(color("--control-primary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let _ = &qid;
                                                if let Some(
                                                    QuestCallOrSheetOverlay::CallHeaderCard {
                                                        phase,
                                                        ..
                                                    },
                                                ) = this.quest_call_sheet_overlay.as_mut()
                                                {
                                                    *phase =
                                                        QuestCallPhase::EnrolledParticipating {
                                                            percent: 25,
                                                        };
                                                }
                                                cx.notify();
                                            }))
                                            .child(dc_intl::t("cfY4PE")),
                                    )
                                    .into_any_element(),
                                QuestCallPhase::EnrolledIdle => div()
                                    .id("quest-call-check-progress")
                                    .w_full()
                                    .h(px(36.))
                                    .rounded(px(8.))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.quest_call_sheet_overlay = None;
                                        this.open_quests(cx);
                                    }))
                                    .child(dc_intl::t("VN1Ajl"))
                                    .into_any_element(),
                                QuestCallPhase::Completed => div()
                                    .id("quest-call-claim")
                                    .w_full()
                                    .h(px(36.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.quest_call_sheet_overlay = None;
                                        this.open_quests(cx);
                                    }))
                                    .child(dc_intl::t("cfY4PE"))
                                    .into_any_element(),
                                QuestCallPhase::EnrolledParticipating { .. } => {
                                    div().into_any_element()
                                }
                            }),
                    )
                    .into_any_element()
            }
            QuestCallOrSheetOverlay::SponsoredSheet {
                package_feature,
                games,
                loading,
                error,
            } => {
                if *error {
                    // Module 659309: "Game Sheet Error"
                    div()
                        .id("sponsored-quest-sheet-error")
                        .w(px(420.))
                        .rounded(px(12.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("iE/uSw")),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("G6SnvT")),
                        )
                        .child(
                            div().flex().justify_end().child(
                                div()
                                    .id("sponsored-sheet-err-close")
                                    .h(px(36.))
                                    .px(px(18.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.quest_call_sheet_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(dc_intl::t("cpT0Cq")),
                            ),
                        )
                        .into_any_element()
                } else {
                    let title = package_title(package_feature.as_deref());
                    let total_slots = sheet_grid_slots(games.len());
                    let pad = total_slots.saturating_sub(games.len());
                    div()
                        .id("sponsored-quest-sheet")
                        .w(px(460.))
                        .rounded(px(12.))
                        .bg(color("--background-surface-high"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .p(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(16.))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        // Header: game tile + package title + `VgN1Bn` "Your reward awaits." + close
                        .child(
                            div()
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
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .size(px(40.))
                                                .flex_none()
                                                .rounded(px(8.))
                                                .bg(rgba(0x5865f233))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(icon(
                                                    "discover_nav_quests",
                                                    22.,
                                                    "--icon-strong",
                                                )),
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
                                                        .text_size(px(16.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--text-strong"))
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("VgN1Bn")),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("sponsored-sheet-close")
                                        .aria_label(dc_intl::t("cpT0Cq"))
                                        .size(px(28.))
                                        .flex_none()
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.quest_call_sheet_overlay = None;
                                            cx.notify();
                                        }))
                                        .child(icon("modal_close", 16., "--icon-subtle")),
                                ),
                        )
                        // Grid of eligible games (up to 10) + placeholder slots
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(10.))
                                .when(*loading, |grid| {
                                    grid.children((0..4).map(|_| {
                                        div()
                                            .w(px(96.))
                                            .h(px(116.))
                                            .rounded(px(8.))
                                            .bg(color("--background-base-lower"))
                                    }))
                                })
                                .when(!*loading, |grid| {
                                    grid.children(games.iter().take(10).enumerate().map(
                                        |(i, game)| {
                                            let url = game.store_url.clone();
                                            div()
                                                .id(SharedString::from(format!(
                                                    "sponsored-game-{i}"
                                                )))
                                                .w(px(96.))
                                                .h(px(116.))
                                                .p(px(10.))
                                                .rounded(px(8.))
                                                .bg(color("--background-base-lower"))
                                                .border_1()
                                                .border_color(color("--border-subtle"))
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(color("--interactive-background-hover"))
                                                })
                                                .flex()
                                                .flex_col()
                                                .justify_between()
                                                .on_click(cx.listener(move |_, _, _, cx| {
                                                    if let Some(u) = url.as_deref() {
                                                        cx.open_url(u);
                                                    }
                                                }))
                                                .child(
                                                    div()
                                                        .size(px(36.))
                                                        .rounded(px(8.))
                                                        .bg(rgb(0x5865f2))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .child(icon(
                                                            "bot_game_controller",
                                                            20.,
                                                            "--white",
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .line_height(px(15.))
                                                        .font_weight(FontWeight(600.))
                                                        .text_color(color("--text-strong"))
                                                        .line_clamp(2)
                                                        .child(SharedString::from(
                                                            game.name.clone(),
                                                        )),
                                                )
                                        },
                                    ))
                                    .children((0..pad).map(|_| {
                                        div()
                                            .w(px(96.))
                                            .h(px(116.))
                                            .rounded(px(8.))
                                            .bg(color("--background-base-lowest"))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .opacity(0.4)
                                    }))
                                }),
                        )
                        .into_any_element()
                }
            }
        };

        Some(
            gpui::deferred(
                div()
                    .id("quest-call-sheet-backdrop")
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
                        this.quest_call_sheet_overlay = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `quest-call-sheet [call-unenrolled|call-progress|call-completed|sheet|sheet-error|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_call_sheet(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "call-progress" => {
                self.quest_call_sheet_overlay = Some(QuestCallOrSheetOverlay::CallHeaderCard {
                    quest_id: "q1".into(),
                    quest_name: "Valorant".into(),
                    task_description: "Stream Valorant for 15 minutes".into(),
                    expiration_date: "Dec 15, 2026".into(),
                    phase: QuestCallPhase::EnrolledParticipating { percent: 60 },
                });
                cx.notify();
                "quest-call-sheet: call-progress".into()
            }
            "call-completed" => {
                self.quest_call_sheet_overlay = Some(QuestCallOrSheetOverlay::CallHeaderCard {
                    quest_id: "q1".into(),
                    quest_name: "Valorant".into(),
                    task_description: "Stream Valorant for 15 minutes".into(),
                    expiration_date: "Dec 31, 2026".into(),
                    phase: QuestCallPhase::Completed,
                });
                cx.notify();
                "quest-call-sheet: call-completed".into()
            }
            "sheet" => {
                self.open_sponsored_quest_sheet(
                    Some("PACKAGE_ACTION_ADVENTURE".into()),
                    vec![
                        EligibleGameTile {
                            app_id: "1".into(),
                            name: "Hades II".into(),
                            store_url: Some("https://store.steampowered.com".into()),
                        },
                        EligibleGameTile {
                            app_id: "2".into(),
                            name: "Elden Ring".into(),
                            store_url: Some("https://store.steampowered.com".into()),
                        },
                        EligibleGameTile {
                            app_id: "3".into(),
                            name: "Hollow Knight".into(),
                            store_url: Some("https://store.steampowered.com".into()),
                        },
                    ],
                    cx,
                );
                "quest-call-sheet: sheet".into()
            }
            "sheet-error" => {
                self.open_sponsored_quest_sheet(None, Vec::new(), cx);
                "quest-call-sheet: sheet-error".into()
            }
            "close" => {
                self.quest_call_sheet_overlay = None;
                cx.notify();
                "quest-call-sheet: closed".into()
            }
            _ => {
                self.quest_call_sheet_overlay = Some(QuestCallOrSheetOverlay::CallHeaderCard {
                    quest_id: "q1".into(),
                    quest_name: "Valorant".into(),
                    task_description:
                        "Stream Valorant in voice for 15 minutes to earn an Avatar Decoration."
                            .into(),
                    expiration_date: "Dec 15, 2026".into(),
                    phase: QuestCallPhase::Unenrolled,
                });
                cx.notify();
                "quest-call-sheet: call-unenrolled".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_package_titles_and_grid_slots() {
        assert_eq!(
            package_title(Some("PACKAGE_ACTION_ADVENTURE")),
            dc_intl::t("PYFVdf")
        );
        assert_eq!(
            package_title(Some("PACKAGE_NEW_YEARS")),
            dc_intl::t("S0w2mv")
        );
        assert_eq!(package_title(None), dc_intl::t("D+DkEH"));
        assert_eq!(sheet_grid_slots(3), 4);
        assert_eq!(sheet_grid_slots(5), 5);
        assert_eq!(sheet_grid_slots(7), 8);
        assert_eq!(sheet_grid_slots(9), 10);
    }
}
