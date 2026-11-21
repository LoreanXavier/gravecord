//! Quest Connection Helper Modal (module 988436, console start REST 343559 / 61387).
//!
//! Opened when clicking "Check Connection" / "Check for Launches" on console-eligible Quests
//! (`quest_bar.rs` & `quests.rs`).
//! Renders:
//! - Header `W5lmKt` ("Connection Helper")
//! - Connected Accounts header (`+/hZM/`) + Refresh button (`wzzjk9`) calling
//!   `POST /quests/{questId}/console/start`
//! - Row 1: `br3uIi` ("This PC") with positive connection indicator
//! - Row 2: `XF4wuA` ("Connected Consoles") with `STpNEC` ("Game detected") badge when
//!   progressing on console, OR red error hints (`Whhv4w` / `xSCvBf` / server hints)
//! - Footer: Secondary `qiS+xj` ("Show Connections" -> Settings -> Connections) + Primary
//!   `cpT0Cq` ("Close").

use super::{Root, color, icon};
use crate::net::Request;
use gpui::{AnyElement, Context, FontWeight, IntoElement, deferred, div, prelude::*, px, rgba};
use serde_json::{Value, json};

pub(crate) const CONSOLE_START_KEY: &str = "quests:console-start";

#[derive(Clone, Debug)]
pub(crate) struct QuestConnectionHelperModal {
    pub quest_id: String,
    pub game_title: String,
    pub refreshing: bool,
    pub game_detected_on_console: bool,
    pub error_hints: Vec<String>,
}

/// `POST /quests/{questId}/console/start` (module 988436 / 343559).
pub(crate) fn console_start_request(quest_id: &str) -> Request {
    Request::Rest {
        key: Some(CONSOLE_START_KEY.to_owned()),
        method: reqwest::Method::POST,
        route: format!("/quests/{quest_id}/console/start"),
        body: Some(json!({})),
    }
}

impl Root {
    /// Opens the Quest Connection Helper modal (`988436`) for `quest_id`.
    pub(super) fn open_quest_connection_helper(
        &mut self,
        quest_id: &str,
        refresh: bool,
        cx: &mut Context<Self>,
    ) {
        let game_title = self
            .quests
            .store
            .quests
            .iter()
            .find(|q| q.id == quest_id)
            .map(|q| q.config.game_title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Quest Game".to_owned());
        if refresh {
            let _ = self.requests.send(console_start_request(quest_id));
        }
        self.quest_connection_helper = Some(QuestConnectionHelperModal {
            quest_id: quest_id.to_owned(),
            game_title,
            refreshing: refresh,
            game_detected_on_console: false,
            error_hints: Vec::new(),
        });
        cx.notify();
    }

    /// Handles the `POST /quests/{id}/console/start` REST response.
    pub(super) fn handle_quest_console_start_response(
        &mut self,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.quest_connection_helper.as_mut() else {
            return;
        };
        modal.refreshing = false;
        let status = body["status"].as_u64().unwrap_or(200);
        if status == 429 {
            modal.error_hints = vec![dc_intl::t("Whhv4w").to_owned()];
        } else if body["error"].is_string() || status >= 400 {
            modal.error_hints = vec![dc_intl::t("xSCvBf").to_owned()];
        } else if let Some(hints) = body["body"]["error_hints_v2"].as_array() {
            modal.error_hints = hints
                .iter()
                .take(5)
                .filter_map(|h| h["message"].as_str().map(str::to_owned))
                .collect();
            modal.game_detected_on_console = modal.error_hints.is_empty();
        } else {
            modal.error_hints.clear();
            modal.game_detected_on_console = true;
        }
        cx.notify();
    }

    /// Modal layer for Quest Connection Helper (`988436`).
    pub(super) fn quest_connection_helper_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.quest_connection_helper.as_ref()?;
        let quest_id = modal.quest_id.clone();
        let refreshing = modal.refreshing;
        let has_errors = !modal.error_hints.is_empty();

        Some(
            deferred(
                div()
                    .id("quest-conn-helper-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.quest_connection_helper = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("quest-conn-helper-modal")
                            .w(px(460.))
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
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(20.))
                                                    .line_height(px(24.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("W5lmKt")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .line_height(px(18.))
                                                    .text_color(color("--text-muted"))
                                                    .child(format!(
                                                        "Verify how {} is connected to track your Quest progress.",
                                                        modal.game_title
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("quest-conn-helper-close")
                                            .flex_none()
                                            .size(px(32.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(icon("modal_close", 18., "--icon-subtle"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_connection_helper = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Body
                            .child(
                                div()
                                    .px(px(24.))
                                    .pb(px(18.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    // Content header: Connected Accounts + Refresh
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-default"))
                                                    .child(dc_intl::t("+/hZM/")),
                                            )
                                            .child(
                                                div()
                                                    .id("quest-conn-refresh")
                                                    .when(!refreshing, |d| d.cursor_pointer())
                                                    .when(refreshing, |d| d.opacity(0.5))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(4.))
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-link"))
                                                    .child(icon("mod_refresh", 14., "--text-link"))
                                                    .child(dc_intl::t("wzzjk9"))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if refreshing {
                                                            return;
                                                        }
                                                        if let Some(m) =
                                                            this.quest_connection_helper.as_mut()
                                                        {
                                                            m.refreshing = true;
                                                        }
                                                        let _ = this.requests.send(
                                                            console_start_request(&quest_id),
                                                        );
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    // Row 1: This PC
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lower"))
                                            .px(px(12.))
                                            .py(px(10.))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(icon("clips_screen", 18., "--icon-strong"))
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .font_weight(FontWeight(600.))
                                                            .text_color(color("--text-strong"))
                                                            .child(dc_intl::t("br3uIi")),
                                                    ),
                                            )
                                            .child(icon("circle_check", 18., "--status-positive")),
                                    )
                                    // Row 2: Connected Consoles
                                    .child(
                                        div()
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(if has_errors {
                                                color("--status-danger")
                                            } else {
                                                color("--border-subtle")
                                            })
                                            .bg(color("--background-base-lower"))
                                            .overflow_hidden()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .px(px(12.))
                                                    .py(px(10.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(10.))
                                                            .child(icon(
                                                                "bot_game_controller",
                                                                18.,
                                                                "--icon-strong",
                                                            ))
                                                            .child(
                                                                div()
                                                                    .text_size(px(14.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--text-strong",
                                                                    ))
                                                                    .child(dc_intl::t("XF4wuA")),
                                                            ),
                                                    )
                                                    .child(if modal.game_detected_on_console {
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(6.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(12.))
                                                                    .font_weight(FontWeight(600.))
                                                                    .text_color(color(
                                                                        "--status-positive",
                                                                    ))
                                                                    .child(dc_intl::t("STpNEC")),
                                                            )
                                                            .child(icon(
                                                                "circle_check",
                                                                18.,
                                                                "--status-positive",
                                                            ))
                                                            .into_any_element()
                                                    } else if has_errors {
                                                        icon("circle_info", 18., "--status-danger")
                                                            .into_any_element()
                                                    } else {
                                                        icon("circle_info", 18., "--icon-subtle")
                                                            .into_any_element()
                                                    }),
                                            )
                                            .when(has_errors, |box_div| {
                                                box_div.child(
                                                    div()
                                                        .bg(rgba(0xda373c18))
                                                        .border_t_1()
                                                        .border_color(color("--border-subtle"))
                                                        .flex()
                                                        .flex_col()
                                                        .children(modal.error_hints.iter().map(
                                                            |hint| {
                                                                div()
                                                                    .px(px(12.))
                                                                    .py(px(8.))
                                                                    .flex()
                                                                    .items_start()
                                                                    .gap(px(8.))
                                                                    .child(icon(
                                                                        "circle_info",
                                                                        14.,
                                                                        "--status-danger",
                                                                    ))
                                                                    .child(
                                                                        div()
                                                                            .flex_1()
                                                                            .min_w_0()
                                                                            .text_size(px(12.))
                                                                            .line_height(px(16.))
                                                                            .text_color(color(
                                                                                "--text-default",
                                                                            ))
                                                                            .child(hint.clone()),
                                                                    )
                                                            },
                                                        )),
                                                )
                                            }),
                                    ),
                            )
                            // Footer: Show Connections + Close
                            .child(
                                div()
                                    .px(px(24.))
                                    .py(px(14.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .justify_end()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("quest-conn-show-connections")
                                            .cursor_pointer()
                                            .px(px(14.))
                                            .h(px(36.))
                                            .rounded(px(6.))
                                            .bg(color("--control-secondary-background-default"))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-secondary-text-default"))
                                            .child(dc_intl::t("qiS+xj"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.quest_connection_helper = None;
                                                this.open_settings_section("Connections", cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("quest-conn-done")
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
                                                this.quest_connection_helper = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(96)
            .into_any_element(),
        )
    }

    /// Capture harness: `quest-conn-helper [detected|error|close]` (module 988436).
    #[cfg(feature = "capture")]
    pub(crate) fn script_quest_connection_helper(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        match arg.trim() {
            "close" => {
                self.quest_connection_helper = None;
                cx.notify();
                "quest-conn-helper: closed".to_owned()
            }
            "error" => {
                self.quest_connection_helper = Some(QuestConnectionHelperModal {
                    quest_id: "quest_demo_988".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    refreshing: false,
                    game_detected_on_console: false,
                    error_hints: vec![
                        "Xbox privacy settings are currently hiding game activity.".to_owned(),
                        dc_intl::t("Whhv4w").to_owned(),
                    ],
                });
                cx.notify();
                "quest-conn-helper: error".to_owned()
            }
            _ => {
                self.quest_connection_helper = Some(QuestConnectionHelperModal {
                    quest_id: "quest_demo_988".to_owned(),
                    game_title: "Starfall Odyssey".to_owned(),
                    refreshing: false,
                    game_detected_on_console: true,
                    error_hints: Vec::new(),
                });
                cx.notify();
                "quest-conn-helper: detected".to_owned()
            }
        }
    }
}
