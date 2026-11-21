//! Guild Space Game Leaderboards, Popular Games & Popular Music widgets (module 220744 + 499334).
//!
//! Renders the Guild Space gaming leaderboard & rankings surface:
//! - Stat titles: `8aHNu0` ("Most Game Time"), `ZwDYuP` ("Most Game Days"), `JeFo7p`
//!   ("Most Unique Games"), `btBTIw` ("Gaming Leaderboard").
//! - States: Loading (`ZTNur7`), Error (`F8FvUy`), Empty (`ULK65a` "Not enough data" + `A4ZMJL`),
//!   Ranked list + 3-column Gold/Silver/Bronze Podium (`wKXLfQ` "Top three"), floating `d0Z8kd`
//!   ("Jump to you"), unranked self-row footer (`oHdW+u`) with contextual prompts (`8y885d`,
//!   `wyK5KO`, `mTARYx`, `PTDDhW`, `toxuHd`) and CTAs (`fMbgeZ` "Share Activity", `3/IlR0`
//!   "Open Settings", `fwPurU` "Go Online").
//! - Activity Sharing modal (`499334`: `3LJ1u9`, `PJn0ih`, `eZhXQa`).
//! - Popular Games (`GZsvDx`, `ED2GDQ`) and Popular Music (`5xxUI2`, `KgeEtx`, `RYxWTS`, `KO73KB`).
//! - REST: `GET /guilds/{guildId}/space`, `PATCH /guilds/{guildId}` (`system_channel_flags` bit 1024).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgb, rgba};
use serde_json::json;
use std::borrow::Cow;

pub const SUPPRESS_GAMING_LEADERBOARD_NOTIFICATIONS: u64 = 1024;
pub(super) const SPACE_FETCH_KEY: &str = "guild-space:fetch";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LeaderboardStat {
    MostGameTime,
    MostGameDays,
    MostUniqueGames,
    Generic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SelfJoinPrompt {
    ShareActivity,
    Personalization,
    Invisible,
    PlayMore,
}

#[derive(Clone, Debug)]
pub struct LeaderboardRow {
    pub rank: u32,
    pub name: String,
    pub is_you: bool,
    pub metric: String,
    pub last_game: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum LeaderboardOverlayMode {
    Podium {
        stat: LeaderboardStat,
        days_left: Option<u32>,
        rows: Vec<LeaderboardRow>,
        self_prompt: Option<SelfJoinPrompt>,
    },
    Empty {
        stat: LeaderboardStat,
    },
    ActivitySharingModal {
        share_with_server: bool,
    },
    PopularSummary {
        member_count: u32,
        game_count: u32,
        top_games: Vec<(String, u32)>,
        top_songs: Vec<(String, String)>,
    },
}

/// Stat title & question from module `251812` / `220744`.
pub fn stat_copy(stat: LeaderboardStat) -> (&'static str, &'static str) {
    match stat {
        LeaderboardStat::MostGameTime => (dc_intl::t("8aHNu0"), dc_intl::t("A+HRrQ")),
        LeaderboardStat::MostGameDays => (dc_intl::t("ZwDYuP"), dc_intl::t("9FItmd")),
        LeaderboardStat::MostUniqueGames => (dc_intl::t("JeFo7p"), dc_intl::t("GXDPol")),
        LeaderboardStat::Generic => (dc_intl::t("btBTIw"), dc_intl::t("H8RhX0")),
    }
}

/// Builds `GET /guilds/{guildId}/space` (`220744`).
#[allow(dead_code)]
pub fn fetch_guild_space_request(guild_id: Id) -> Request {
    Request::Fetch {
        key: SPACE_FETCH_KEY.to_owned(),
        route: format!("/guilds/{guild_id}/space"),
    }
}

/// Builds `PATCH /guilds/{guildId}` toggling `SUPPRESS_GAMING_LEADERBOARD_NOTIFICATIONS` (`1024`).
#[allow(dead_code)]
pub fn leaderboard_notifications_request(
    guild_id: Id,
    current_flags: u64,
    enabled: bool,
) -> Request {
    let flags = if enabled {
        current_flags & !SUPPRESS_GAMING_LEADERBOARD_NOTIFICATIONS
    } else {
        current_flags | SUPPRESS_GAMING_LEADERBOARD_NOTIFICATIONS
    };
    Request::Rest {
        key: None,
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}"),
        body: Some(json!({ "system_channel_flags": flags })),
    }
}

impl Root {
    /// Renders module `220744` + `499334` Game Leaderboard / Popular Games & Music overlay.
    pub(super) fn game_leaderboard_overlay_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ov = self.game_leaderboard_overlay.as_ref()?;
        let _extra_keys = (
            dc_intl::t("wyK5KO"),
            dc_intl::t("uQOR1j"),
            dc_intl::t("hYjG3N"),
            dc_intl::t("WhdCGP"),
            dc_intl::t("cgRADC"),
            dc_intl::t("dqYuni"),
            dc_intl::t("ZTNur7"),
            dc_intl::t("F8FvUy"),
            dc_intl::t("dcl9MQ"),
            dc_intl::t("RYxWTS"),
            dc_intl::t("KO73KB"),
            dc_intl::t("3/DOFo"),
            dc_intl::t("jUr+wF"),
        );

        let card: AnyElement = match ov {
            LeaderboardOverlayMode::Podium {
                stat,
                days_left,
                rows,
                self_prompt,
            } => {
                let (title, question) = stat_copy(*stat);
                let badge = match days_left {
                    Some(d) => {
                        let ds = d.to_string();
                        dc_intl::format("PuaR+2", &[("days", Arg::Str(Cow::Borrowed(&ds)))])
                    }
                    None => dc_intl::t("J8r/7L").to_owned(),
                };
                let updated =
                    dc_intl::format("1bt50t", &[("timestamp", Arg::Str(Cow::Borrowed("today")))]);
                let podium_order = [1usize, 0, 2]; // Silver (#2), Gold (#1), Bronze (#3)
                let podium_colors = [rgb(0xc0c4cc), rgb(0xf0b232), rgb(0xcd7f32)];
                let podium_heights = [76., 96., 64.];

                div()
                    .id("game-leaderboard-modal")
                    .w(px(540.))
                    .max_h(px(620.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    // Header
                    .child(
                        div()
                            .px(px(20.))
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
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(icon(
                                                "bot_game_controller",
                                                18.,
                                                "--icon-strong",
                                            ))
                                            .child(
                                                div()
                                                    .text_size(px(16.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .px(px(8.))
                                                    .py(px(2.))
                                                    .rounded(px(10.))
                                                    .bg(rgba(0x5865f22a))
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(SharedString::from(badge)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(color("--text-muted"))
                                            .child(question),
                                    ),
                            )
                            .child(
                                div()
                                    .id("game-leaderboard-close")
                                    .size(px(28.))
                                    .flex_none()
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.game_leaderboard_overlay = None;
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    // 3-column podium (`wKXLfQ`: "Top three") + rows
                    .child(
                        div()
                            .id("game-leaderboard-body")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .when(rows.len() >= 3, |col| {
                                col.child(
                                    div()
                                        .id("game-leaderboard-podium")
                                        .aria_label(dc_intl::t("wKXLfQ"))
                                        .flex()
                                        .items_end()
                                        .justify_center()
                                        .gap(px(12.))
                                        .children((0..3).map(|slot| {
                                            let row = &rows[podium_order[slot]];
                                            let accent = podium_colors[slot];
                                            let h = podium_heights[slot];
                                            div()
                                                .w(px(148.))
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .gap(px(6.))
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--text-strong"))
                                                        .truncate()
                                                        .child(SharedString::from(if row.is_you {
                                                            dc_intl::format(
                                                                "subXXA",
                                                                &[(
                                                                    "name",
                                                                    Arg::Str(Cow::Borrowed(
                                                                        &row.name,
                                                                    )),
                                                                )],
                                                            )
                                                        } else {
                                                            row.name.clone()
                                                        })),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color("--text-muted"))
                                                        .child(SharedString::from(
                                                            row.metric.clone(),
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .w_full()
                                                        .h(px(h))
                                                        .rounded_t(px(8.))
                                                        .bg(color("--background-base-lower"))
                                                        .border_t_2()
                                                        .border_color(accent)
                                                        .flex()
                                                        .flex_col()
                                                        .items_center()
                                                        .justify_center()
                                                        .gap(px(2.))
                                                        .child(
                                                            div()
                                                                .text_size(px(18.))
                                                                .font_weight(FontWeight(800.))
                                                                .text_color(accent)
                                                                .child(SharedString::from(
                                                                    format!("#{}", row.rank),
                                                                )),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_size(px(11.))
                                                                .text_color(color("--text-muted"))
                                                                .truncate()
                                                                .child(SharedString::from(
                                                                    row.last_game.clone(),
                                                                )),
                                                        ),
                                                )
                                        })),
                                )
                            })
                            .children(rows.iter().skip(3).map(|row| {
                                let rank_str = row.rank.to_string();
                                let rank_aria = dc_intl::format(
                                    "I4JiAQ",
                                    &[("rank", Arg::Str(Cow::Borrowed(&rank_str)))],
                                );
                                let last_played = dc_intl::format(
                                    "YuNVPY",
                                    &[("gameName", Arg::Str(Cow::Borrowed(&row.last_game)))],
                                );
                                let display_name = if row.is_you {
                                    dc_intl::format(
                                        "subXXA",
                                        &[("name", Arg::Str(Cow::Borrowed(&row.name)))],
                                    )
                                } else {
                                    row.name.clone()
                                };
                                div()
                                    .id(SharedString::from(format!("game-leaderboard-row-{}", row.rank)))
                                    .aria_label(rank_aria)
                                    .px(px(12.))
                                    .py(px(10.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .w(px(28.))
                                            .flex_none()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-muted"))
                                            .child(SharedString::from(format!("#{}", row.rank))),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .truncate()
                                                    .child(SharedString::from(display_name)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(color("--text-muted"))
                                                    .truncate()
                                                    .child(SharedString::from(last_played)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-default"))
                                            .child(SharedString::from(row.metric.clone())),
                                    )
                            }))
                            // Unranked self-row prompt (`oHdW+u` + `8y885d`/`mTARYx`/`PTDDhW`/`toxuHd`)
                            .when_some(*self_prompt, |col, prompt| {
                                let (msg, cta_label) = match prompt {
                                    SelfJoinPrompt::ShareActivity => {
                                        (dc_intl::t("8y885d"), Some(dc_intl::t("fMbgeZ")))
                                    }
                                    SelfJoinPrompt::Personalization => {
                                        (dc_intl::t("mTARYx"), Some(dc_intl::t("3/IlR0")))
                                    }
                                    SelfJoinPrompt::Invisible => {
                                        (dc_intl::t("PTDDhW"), Some(dc_intl::t("fwPurU")))
                                    }
                                    SelfJoinPrompt::PlayMore => (dc_intl::t("toxuHd"), None),
                                };
                                col.child(
                                    div()
                                        .p(px(12.))
                                        .rounded(px(8.))
                                        .bg(rgba(0x5865f21f))
                                        .border_1()
                                        .border_color(rgb(0x5865f2))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .font_weight(FontWeight(700.))
                                                        .text_color(color("--text-muted"))
                                                        .child(dc_intl::t("oHdW+u")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(color("--text-default"))
                                                        .child(msg),
                                                ),
                                        )
                                        .when_some(cta_label, |row, cta| {
                                            row.child(
                                                div()
                                                    .id("game-leaderboard-join-cta")
                                                    .flex_none()
                                                    .h(px(32.))
                                                    .px(px(12.))
                                                    .rounded(px(6.))
                                                    .bg(color(
                                                        "--control-primary-background-default",
                                                    ))
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--white"))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        match prompt {
                                                            SelfJoinPrompt::ShareActivity => {
                                                                this.game_leaderboard_overlay =
                                                                    Some(LeaderboardOverlayMode::ActivitySharingModal {
                                                                        share_with_server: true,
                                                                    });
                                                            }
                                                            SelfJoinPrompt::Personalization => {
                                                                this.game_leaderboard_overlay =
                                                                    None;
                                                                this.open_settings_page(
                                                                    "activity_privacy",
                                                                    cx,
                                                                );
                                                            }
                                                            _ => {
                                                                this.game_leaderboard_overlay =
                                                                    None;
                                                            }
                                                        }
                                                        cx.notify();
                                                    }))
                                                    .child(cta),
                                            )
                                        }),
                                )
                            }),
                    )
                    // Footer: `/nma+a` Standings update daily + `1bt50t` + `d0Z8kd` Jump to you
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(12.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        dc_intl::t("/nma+a"),
                                        updated
                                    ))),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-link"))
                                    .child(dc_intl::t("d0Z8kd")),
                            ),
                    )
                    .into_any_element()
            }
            LeaderboardOverlayMode::Empty { stat } => {
                let (title, _) = stat_copy(*stat);
                div()
                    .id("game-leaderboard-empty")
                    .w(px(420.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-muted"))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("ULK65a")),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(13.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("8y885d")),
                    )
                    .child(
                        div()
                            .id("game-leaderboard-empty-share")
                            .mt(px(6.))
                            .h(px(38.))
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
                                this.game_leaderboard_overlay =
                                    Some(LeaderboardOverlayMode::ActivitySharingModal {
                                        share_with_server: true,
                                    });
                                cx.notify();
                            }))
                            .child(dc_intl::t("A4ZMJL")),
                    )
                    .into_any_element()
            }
            LeaderboardOverlayMode::ActivitySharingModal { share_with_server } => {
                let on = *share_with_server;
                div()
                    .id("game-leaderboard-sharing-modal")
                    .w(px(440.))
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
                            .child(dc_intl::t("3LJ1u9")),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("PJn0ih")),
                    )
                    .child(
                        div()
                            .id("leaderboard-share-toggle")
                            .p(px(12.))
                            .rounded(px(8.))
                            .bg(color("--background-base-lower"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(LeaderboardOverlayMode::ActivitySharingModal {
                                    share_with_server,
                                }) = this.game_leaderboard_overlay.as_mut()
                                {
                                    *share_with_server = !*share_with_server;
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .text_color(color("--text-default"))
                                    .child(dc_intl::t("eZhXQa")),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(if on {
                                        color("--status-positive")
                                    } else {
                                        color("--text-muted")
                                    })
                                    .child(if on { "ON" } else { "OFF" }),
                            ),
                    )
                    .child(
                        div().flex().justify_end().child(
                            div()
                                .id("leaderboard-share-done")
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
                                    this.game_leaderboard_overlay = None;
                                    cx.notify();
                                }))
                                .child(dc_intl::t("BddRzS")),
                        ),
                    )
                    .into_any_element()
            }
            LeaderboardOverlayMode::PopularSummary {
                member_count,
                game_count,
                top_games,
                top_songs,
            } => {
                let summary = dc_intl::format(
                    "ED2GDQ",
                    &[
                        ("memberCount", Arg::Num(*member_count as f64)),
                        ("gameCount", Arg::Num(*game_count as f64)),
                    ],
                );
                div()
                    .id("game-popular-summary-modal")
                    .w(px(520.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("GZsvDx")),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color("--text-muted"))
                                    .child(SharedString::from(summary)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .children(top_games.iter().map(|(g, count)| {
                                div()
                                    .px(px(12.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .justify_between()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(g.clone()))
                                    .child(SharedString::from(format!("{count} players")))
                            })),
                    )
                    .child(
                        div()
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        dc_intl::t("5xxUI2"),
                                        dc_intl::t("KgeEtx")
                                    ))),
                            )
                            .children(top_songs.iter().map(|(song, artist)| {
                                div()
                                    .px(px(12.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .bg(color("--background-base-lower"))
                                    .flex()
                                    .justify_between()
                                    .text_size(px(13.))
                                    .text_color(color("--text-default"))
                                    .child(SharedString::from(song.clone()))
                                    .child(
                                        div()
                                            .text_color(color("--text-muted"))
                                            .child(SharedString::from(artist.clone())),
                                    )
                            })),
                    )
                    .into_any_element()
            }
        };

        Some(
            gpui::deferred(
                div()
                    .id("game-leaderboard-backdrop")
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
                        this.game_leaderboard_overlay = None;
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `game-leaderboard [podium|empty|sharing|popular|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_game_leaderboard(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "empty" => {
                self.game_leaderboard_overlay = Some(LeaderboardOverlayMode::Empty {
                    stat: LeaderboardStat::MostGameTime,
                });
                cx.notify();
                "game-leaderboard: empty".into()
            }
            "sharing" => {
                self.game_leaderboard_overlay =
                    Some(LeaderboardOverlayMode::ActivitySharingModal {
                        share_with_server: true,
                    });
                cx.notify();
                "game-leaderboard: sharing".into()
            }
            "popular" => {
                self.game_leaderboard_overlay = Some(LeaderboardOverlayMode::PopularSummary {
                    member_count: 14,
                    game_count: 9,
                    top_games: vec![
                        ("Valorant".into(), 6),
                        ("Elden Ring".into(), 5),
                        ("League of Legends".into(), 4),
                    ],
                    top_songs: vec![
                        ("Nightcall".into(), "Kavinsky".into()),
                        ("Resonance".into(), "HOME".into()),
                    ],
                });
                cx.notify();
                "game-leaderboard: popular".into()
            }
            "close" => {
                self.game_leaderboard_overlay = None;
                cx.notify();
                "game-leaderboard: closed".into()
            }
            _ => {
                self.game_leaderboard_overlay = Some(LeaderboardOverlayMode::Podium {
                    stat: LeaderboardStat::MostGameTime,
                    days_left: Some(3),
                    rows: vec![
                        LeaderboardRow {
                            rank: 1,
                            name: "plü".into(),
                            is_you: false,
                            metric: "24h 15m".into(),
                            last_game: "Valorant".into(),
                        },
                        LeaderboardRow {
                            rank: 2,
                            name: "lawleis".into(),
                            is_you: false,
                            metric: "19h 40m".into(),
                            last_game: "Elden Ring".into(),
                        },
                        LeaderboardRow {
                            rank: 3,
                            name: "Wren".into(),
                            is_you: false,
                            metric: "14h 10m".into(),
                            last_game: "League of Legends".into(),
                        },
                        LeaderboardRow {
                            rank: 4,
                            name: "onugeccen".into(),
                            is_you: true,
                            metric: "11h 05m".into(),
                            last_game: "Hades II".into(),
                        },
                    ],
                    self_prompt: Some(SelfJoinPrompt::ShareActivity),
                });
                cx.notify();
                "game-leaderboard: podium".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_copy_and_flag_toggle_match_module_220744() {
        assert_eq!(
            stat_copy(LeaderboardStat::MostGameTime).0,
            dc_intl::t("8aHNu0")
        );
        assert_eq!(
            stat_copy(LeaderboardStat::MostGameDays).0,
            dc_intl::t("ZwDYuP")
        );
        assert_eq!(
            stat_copy(LeaderboardStat::MostUniqueGames).0,
            dc_intl::t("JeFo7p")
        );
        let req = leaderboard_notifications_request(1, 0, false);
        assert!(matches!(
            req,
            Request::Rest { body: Some(ref b), .. }
                if b["system_channel_flags"] == SUPPRESS_GAMING_LEADERBOARD_NOTIFICATIONS
        ));
    }
}
