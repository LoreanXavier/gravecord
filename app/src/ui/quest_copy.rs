//! Quest task instructions, status copy, and CTA state machine (modules 870273, 452411, 500180, 651892, 417404).
//!
//! `task_instructions` (`mU` / `U` of module 870273), `card_description` (`A9` / `x`), `bar_subtitle` (`tH` / `b`),
//! `bar_title` (`VX` / `q`), `primary_cta_label` (`NA` / `V`), `redemption_instructions` (`XR` / `Y`), and
//! `game_cta_subtitle` (`Jf` / `Q`) format every quest task variant through `dc_intl`.
//!
//! `resolve_cta` and `platform_menu_options` implement the Quest CTA matrix of module 452411 across every
//! `QuestCtaState` (`UA` of module 79545) and `CtaSurface` (`V3`).

use chrono::{DateTime, Datelike, Local};
use dc_intl::Arg;
use dc_state::quests::{
    ActivePlatformScreen, ConsolePlatform, ORBS_INTRO_QUEST_ID, Quest, QuestCtaState,
    QuestPlatform, TaskDetails, ThirdPartyTaskDetails, feature, reward, task,
};

/// `V3` of module 79545: surfaces that mount the Quest CTA button (module 452411).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtaSurface {
    QuestBar,
    QuestHomeTile,
    QuestHomeTileV2,
}

/// Visual emphasis for a resolved CTA button (`wX` of module 79545, module 452411).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtaVariant {
    Primary,
    Secondary,
    OverlayPrimary,
}

impl CtaSurface {
    pub fn primary_variant(self) -> CtaVariant {
        if self == CtaSurface::QuestHomeTileV2 {
            CtaVariant::OverlayPrimary
        } else {
            CtaVariant::Primary
        }
    }
}

/// Action kind produced by `resolve_cta` (module 452411).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtaAction {
    Enroll,
    WatchVideo,
    WatchOnMobile,
    LaunchActivity,
    OpenGameLink,
    OpenPlatformMenu,
    ConnectConsole,
    CheckForLaunches,
    CheckConnection,
    ConnectAccount,
    ViewConnection,
    ViewEligibleGames,
    ClaimReward,
    ViewReward,
    SeeCode,
    Disabled,
}

/// Resolved CTA button specification (module 452411).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtaSpec {
    pub label: String,
    pub variant: CtaVariant,
    pub action: CtaAction,
    pub disabled: bool,
    pub trailing_external: bool,
    pub trailing_chevron: bool,
}

/// One row of the `Play` platform dropdown (`tc` of module 452411).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformMenuOption {
    pub id: String,
    pub label: String,
    pub target: PlatformMenuTarget,
    pub selected: bool,
    pub connected: bool,
    pub has_error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformMenuTarget {
    Desktop,
    Console(ConsolePlatform),
    CheckConnection,
}

/// Formats an ISO-8601 date string as `M/D/YYYY` (`S5` default `dateStyle: "short"`, module 61387).
pub fn format_short_date(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| {
            let local = dt.with_timezone(&Local);
            format!("{}/{}/{}", local.month(), local.day(), local.year())
        })
        .unwrap_or_default()
}

/// Formats an ISO-8601 date string as `M/D` (`S5` with `{month: "numeric", day: "numeric"}`, modules 452411/793217).
pub fn format_month_day(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| {
            let local = dt.with_timezone(&Local);
            format!("{}/{}", local.month(), local.day())
        })
        .unwrap_or_default()
}

/// `k5` of module 417404: human duration for a temporary collectible reward (`years` / `months` / `weeks` / `days`).
pub fn collectible_reward_duration(quest: &Quest) -> Option<String> {
    let reward = quest.config.collectible_reward()?;
    let reward_exp = DateTime::parse_from_rfc3339(reward.expires_at.as_deref()?).ok()?;
    let quest_exp = DateTime::parse_from_rfc3339(&quest.config.expires_at).ok()?;
    let days = (reward_exp.signed_duration_since(quest_exp).num_days()).max(0);
    let months = days / 30 + i64::from((days % 30) >= 25);
    if months >= 12 {
        let years = months / 12;
        return Some(dc_intl::format(
            "PClsrw",
            &[("years", Arg::Num((years) as f64))],
        ));
    }
    if months > 0 {
        return Some(dc_intl::format(
            "kridzK",
            &[("months", Arg::Num((months) as f64))],
        ));
    }
    if days >= 7 {
        let weeks = (days + 6) / 7;
        return Some(dc_intl::format(
            "EmoBD2",
            &[("weeks", Arg::Num((weeks) as f64))],
        ));
    }
    Some(dc_intl::format(
        "k2UNz+",
        &[("days", Arg::Num((days) as f64))],
    ))
}

/// `mH` / `mq` of module 417404: display name of the quest's primary reward (with or without article).
pub fn reward_name(quest: &Quest, is_nitro: bool, with_article: bool) -> String {
    if quest.config.has_virtual_currency_reward() {
        let orbs = quest.config.orb_quantity(is_nitro).unwrap_or(0) as i64;
        return dc_intl::format("nLXlh+", &[("orbAmount", Arg::Num((orbs) as f64))]);
    }
    quest.config.primary_reward_name(with_article)
}

/// Formats an intl string that may contain inline hook/link tags into plain text.
fn format_plain(key: &str, args: &[(&str, Arg<'_>)]) -> String {
    dc_intl::format_runs(key, args)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// `WM` of module 500180: remaining video watch duration button label (`Watch 1m` / `Resume 30s`).
pub fn video_cta_label(details: &TaskDetails) -> String {
    let remaining = (details.target_seconds - details.progress_seconds).max(0.);
    let total_secs = remaining.floor() as i64;
    let duration_short = if total_secs >= 60 {
        let mins = (total_secs as f64 / 60.).round() as i64;
        dc_intl::format("PHhTXX", &[("count", Arg::Num((mins) as f64))])
    } else {
        dc_intl::format("rUfeQx", &[("count", Arg::Num(total_secs.max(1) as f64))])
    };
    if details.percent_complete > 0. {
        dc_intl::format(
            "pF/deA",
            &[(
                "durationShort",
                Arg::Str(std::borrow::Cow::Borrowed(&duration_short)),
            )],
        )
    } else {
        dc_intl::format(
            "CHrvqg",
            &[(
                "durationShort",
                Arg::Str(std::borrow::Cow::Borrowed(&duration_short)),
            )],
        )
    }
}

/// `YT` of module 651892: short encouragement line based on completion percentage.
pub fn progress_encouragement(
    quest: &Quest,
    details: &TaskDetails,
    third_party: Option<&ThirdPartyTaskDetails>,
) -> String {
    if quest.is_completed() {
        return dc_intl::t("BzFeTF").to_owned();
    }
    if quest.is_enrolled() {
        let pct = third_party.map_or(details.percent_complete, |d| d.percent_complete);
        if pct >= 0.75 {
            return dc_intl::t("gvCR4H").to_owned();
        }
        if (0.45..=0.55).contains(&pct) {
            return dc_intl::t("JNx8sG").to_owned();
        }
        if pct > 0. {
            return dc_intl::t("JMbfnc").to_owned();
        }
        return dc_intl::t("7e5k7L").to_owned();
    }
    dc_intl::format(
        "EQa7os",
        &[(
            "questName",
            Arg::Str(std::borrow::Cow::Borrowed(&quest.config.quest_name)),
        )],
    )
}

// ---- Module 870273: Quest task descriptions & bar copy ----

/// `y` of module 870273: Nitro permanent collectible play-quest key selector.
fn play_nitro_permanent_key(has_reward_duration: bool, game_profile_link: bool) -> &'static str {
    if has_reward_duration {
        if game_profile_link {
            "/4XT0b"
        } else {
            "0hwcvM"
        }
    } else if game_profile_link {
        "tQoqXX"
    } else {
        "eb/Sna"
    }
}

/// `k` of module 870273: Nitro extension collectible play-quest key selector.
fn play_nitro_extension_key(game_profile_link: bool) -> &'static str {
    if game_profile_link {
        "klYWbT"
    } else {
        "ziB0HF"
    }
}

/// `X` of module 870273: standard timed collectible play-quest key selector.
fn play_timed_collectible_key(game_profile_link: bool) -> &'static str {
    if game_profile_link {
        "4JS2QJ"
    } else {
        "AwuMRS"
    }
}

/// `M` of module 870273: standard win-reward play-quest key selector.
fn play_win_reward_key(game_profile_link: bool) -> &'static str {
    if game_profile_link {
        "GFdaUK"
    } else {
        "NIimTt"
    }
}

/// Options for `task_instructions` (`mU` / `U` of module 870273).
#[derive(Clone, Copy, Debug, Default)]
pub struct InstructionsContext {
    pub has_nitro: bool,
    pub without_markdown: bool,
    pub has_game_profile_link: bool,
    pub needs_to_connect: bool,
}

/// `mU` / `U` of module 870273: formats the full quest instruction sentence for any task/reward combination.
pub fn task_instructions(quest: &Quest, ctx: InstructionsContext) -> String {
    let details = quest.task_details();
    let third_party = quest.third_party_task_details();
    let duration = collectible_reward_duration(quest);
    let reward_article = reward_name(quest, ctx.has_nitro, true);
    let minutes = details.target_minutes as i64;
    let game_title = &quest.config.game_title;
    let has_premium_benefit = quest.config.has_collectible_premium_benefit();
    let is_permanent = quest.config.is_collectible_premium_permanent();
    let non_gaming = quest.config.has_feature(feature::NON_GAMING_PLAY_QUEST);
    let link = !ctx.without_markdown && ctx.has_game_profile_link;
    let dur_str = duration.as_deref().unwrap_or("");

    // 1. Multi-game play quest (`_` = `ui(quest)`).
    if quest.is_multi_game_play_quest() {
        let key = if ctx.without_markdown {
            "1votF6"
        } else {
            quest
                .config
                .features
                .iter()
                .find_map(|f| match *f {
                    feature::PACKAGE_ACTION_ADVENTURE => Some("H485IA"),
                    feature::PACKAGE_RPG_MMO => Some("3XS8Ni"),
                    feature::PACKAGE_RACING_SPORTS => Some("X+UCju"),
                    feature::PACKAGE_SANDBOX_CREATIVE => Some("6o4n1Q"),
                    feature::PACKAGE_FAMILY_FRIENDLY => Some("DUsNmf"),
                    feature::PACKAGE_HOLIDAY_SEASON => Some("cWP8/Z"),
                    feature::PACKAGE_NEW_YEARS => Some("8+sIJz"),
                    _ => None,
                })
                .unwrap_or("CDeHul")
        };
        return format_plain(
            key,
            &[
                (
                    "rewardNameWithArticle",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                ("targetMinutes", Arg::Num((minutes) as f64)),
            ],
        );
    }

    // 2. Combined Console + Desktop play quest (`v` = `g5 && xZ`).
    if quest.is_play_on_console() && quest.is_play_on_desktop() {
        let key = if ctx.has_nitro && has_premium_benefit {
            if is_permanent {
                if non_gaming {
                    if duration.is_some() {
                        "u5QXpw"
                    } else {
                        "ztXW8V"
                    }
                } else {
                    play_nitro_permanent_key(duration.is_some(), link)
                }
            } else if non_gaming {
                "1AcTqm"
            } else {
                play_nitro_extension_key(link)
            }
        } else if has_premium_benefit || duration.is_some() {
            if non_gaming {
                "enQ3jU"
            } else {
                play_timed_collectible_key(link)
            }
        } else if non_gaming {
            "e+K3xJ"
        } else {
            play_win_reward_key(link)
        };
        return format_plain(
            key,
            &[
                (
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                ("targetMinutes", Arg::Num((minutes) as f64)),
                (
                    "rewardNameWithArticle",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
            ],
        );
    }

    // 3. Console-only play quest (`g5`).
    if quest.is_play_on_console() {
        let key = if ctx.has_nitro && has_premium_benefit {
            if is_permanent {
                play_nitro_permanent_key(duration.is_some(), link)
            } else {
                play_nitro_extension_key(link)
            }
        } else if has_premium_benefit || duration.is_some() {
            play_timed_collectible_key(link)
        } else {
            play_win_reward_key(link)
        };
        return format_plain(
            key,
            &[
                (
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                ("targetMinutes", Arg::Num((minutes) as f64)),
                ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                (
                    "rewardNameWithArticle",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
            ],
        );
    }

    // 4. Desktop-only play quest (`xZ`).
    if quest.is_play_on_desktop() {
        let key = if ctx.has_nitro && has_premium_benefit {
            if is_permanent {
                if non_gaming {
                    if duration.is_some() {
                        "u5QXpw"
                    } else {
                        "ztXW8V"
                    }
                } else {
                    play_nitro_permanent_key(duration.is_some(), link)
                }
            } else if non_gaming {
                "1AcTqm"
            } else {
                play_nitro_extension_key(link)
            }
        } else if has_premium_benefit || duration.is_some() {
            if non_gaming {
                "enQ3jU"
            } else {
                play_timed_collectible_key(link)
            }
        } else if non_gaming {
            "03VJqu"
        } else if link {
            "NrD2h8"
        } else {
            "FZL5Q5"
        };
        return format_plain(
            key,
            &[
                (
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                (
                    "rewardNameWithArticle",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                (
                    "questReward",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
            ],
        );
    }

    // 5. Tiered reward-code stream quest (`kr`).
    if quest.config.is_tiered_reward_code() {
        let key = if link { "HHVg4i" } else { "a/ia7F" };
        return format_plain(
            key,
            &[
                (
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
            ],
        );
    }

    // 6. Watch video quest (`vv`).
    if quest.is_watch_video() {
        let video_title = quest.config.video_title().unwrap_or("video");
        let key = if has_premium_benefit && ctx.has_nitro {
            if is_permanent {
                if duration.is_some() {
                    "tXwfJT"
                } else {
                    "xqX+r5"
                }
            } else {
                "vs/xBu"
            }
        } else if has_premium_benefit || duration.is_some() {
            "W/HkLO"
        } else if quest.id == ORBS_INTRO_QUEST_ID {
            "Rsd5bL"
        } else {
            "g+InPC"
        };
        return format_plain(
            key,
            &[
                (
                    "videoTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(video_title)),
                ),
                (
                    "rewardNameWithArticle",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
                ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
            ],
        );
    }

    // 7. Play Activity quest (`vl`).
    if quest.is_play_activity() {
        if quest.is_any_activity_quest() {
            return format_plain(
                "VYwSSu",
                &[
                    ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                    (
                        "questReward",
                        Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                    ),
                ],
            );
        }
        if quest.config.has_feature(feature::CLOUD_GAMING_ACTIVITY)
            && quest
                .config
                .has_feature(feature::CLOUD_GAMING_PROVIDER_NVIDIA)
        {
            return format_plain(
                "0NNM3l",
                &[
                    (
                        "activityName",
                        Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                    ),
                    (
                        "providerName",
                        Arg::Str(std::borrow::Cow::Borrowed("NVIDIA GeForce NOW")),
                    ),
                    (
                        "providerLink",
                        Arg::Str(std::borrow::Cow::Borrowed(
                            "https://support.discord.com/hc/articles/360000000000",
                        )),
                    ),
                    ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                    (
                        "questReward",
                        Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                    ),
                ],
            );
        }
        return format_plain(
            "UuzHh8",
            &[
                (
                    "activityName",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
                (
                    "questReward",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
            ],
        );
    }

    // 8. Achievement in-game quest (`I6` with third-party details).
    if quest.is_achievement_in_game()
        && let Some(tp) = third_party.as_ref()
    {
        let key = if ctx.needs_to_connect {
            if link { "0SLl/G" } else { "BlfaHK" }
        } else if ctx.has_nitro && has_premium_benefit {
            if is_permanent {
                if duration.is_some() {
                    if link { "uLVYG5" } else { "NdXW5c" }
                } else if link {
                    "2Ctf1d"
                } else {
                    "8066TK"
                }
            } else if duration.is_some() {
                if link { "yMEn77" } else { "6FOKAX" }
            } else if link {
                "bxN0nx"
            } else {
                "thO6iA"
            }
        } else if has_premium_benefit || duration.is_some() {
            if link { "ojhBxZ" } else { "IACEB/" }
        } else if link {
            "bxN0nx"
        } else {
            "thO6iA"
        };
        return format_plain(
            key,
            &[
                (
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(game_title)),
                ),
                (
                    "objective",
                    Arg::Str(std::borrow::Cow::Borrowed(&tp.description)),
                ),
                ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
                (
                    "questReward",
                    Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
                ),
            ],
        );
    }

    // 9. Third-party achievement-in-activity fallback.
    if let Some(tp) = third_party {
        return tp.description;
    }

    // 10. Stream on Desktop fallback.
    let key = if ctx.has_nitro && has_premium_benefit {
        if is_permanent {
            if duration.is_some() {
                if link { "3RwRv8" } else { "TmKqHw" }
            } else if link {
                "l9yxDa"
            } else {
                "X8Yt/1"
            }
        } else if link {
            "eEuma3"
        } else {
            "smG9ql"
        }
    } else if has_premium_benefit || duration.is_some() {
        if link { "Cko4a4" } else { "BLyDvO" }
    } else if link {
        "ER9rII"
    } else {
        "hkJ+Gs"
    };
    format_plain(
        key,
        &[
            (
                "gameTitle",
                Arg::Str(std::borrow::Cow::Borrowed(game_title)),
            ),
            ("streamingDurationRequirement", Arg::Num((minutes) as f64)),
            ("duration", Arg::Str(std::borrow::Cow::Borrowed(dur_str))),
            (
                "questReward",
                Arg::Str(std::borrow::Cow::Borrowed(&reward_article)),
            ),
        ],
    )
}

/// `A9` / `x` of module 870273: card body description (claimed date or task instructions).
pub fn card_description(quest: &Quest, ctx: InstructionsContext) -> String {
    if let Some(claimed_at) = quest
        .user_status
        .as_ref()
        .and_then(|s| s.claimed_at.as_deref())
    {
        let date = format_short_date(claimed_at);
        return dc_intl::format(
            "lOVr0O",
            &[("claimDate", Arg::Str(std::borrow::Cow::Borrowed(&date)))],
        );
    }
    task_instructions(quest, ctx)
}

/// `tH` / `b` of module 870273: Quest Bar subtitle in collapsed and expanded states.
pub fn bar_subtitle(
    quest: &Quest,
    is_expanded: bool,
    is_progressing: bool,
    has_already_linked: Option<bool>,
    has_game_click: bool,
    ctx: InstructionsContext,
) -> String {
    if quest.is_completed() {
        let exp = quest
            .config
            .rewards_expire_at
            .as_deref()
            .map(format_short_date)
            .unwrap_or_default();
        return dc_intl::format(
            "APddvF",
            &[("expirationDate", Arg::Str(std::borrow::Cow::Borrowed(&exp)))],
        );
    }
    if quest.is_achievement_in_game() && has_already_linked == Some(false) {
        return dc_intl::t("mAdqf7").to_owned();
    }
    if is_expanded {
        if quest.is_achievement_in_game() && has_already_linked == Some(true) {
            let key = if has_game_click { "X8hBDz" } else { "u3mdpP" };
            return format_plain(
                key,
                &[(
                    "gameTitle",
                    Arg::Str(std::borrow::Cow::Borrowed(&quest.config.game_title)),
                )],
            );
        }
        return task_instructions(quest, ctx);
    }
    if quest.is_watch_video() {
        return dc_intl::t("o+e9yh").to_owned();
    }
    let details = quest.task_details();
    let third_party = quest.third_party_task_details();
    let pct = third_party
        .as_ref()
        .map_or(details.percent_complete, |d| d.percent_complete);
    if pct > 0. {
        if quest.is_achievement_in_game() && has_already_linked == Some(true) {
            return dc_intl::t("JkyCIO").to_owned();
        }
        if is_progressing {
            return progress_encouragement(quest, &details, third_party.as_ref());
        }
        return dc_intl::t("mOrpXG").to_owned();
    }
    dc_intl::t("S6UUc5").to_owned()
}

/// `VX` / `q` of module 870273: Quest Bar title (`Quest Completed!`, `Quest 42% complete`, `Select Platform`, etc.).
pub fn bar_title(
    quest: &Quest,
    active_screen: ActivePlatformScreen,
    has_already_linked: Option<bool>,
) -> String {
    if quest.is_completed() {
        return dc_intl::t("ij5E/5").to_owned();
    }
    if quest.is_achievement_in_game() && has_already_linked == Some(false) {
        return dc_intl::t("s9r2a1").to_owned();
    }
    let details = quest.task_details();
    let third_party = quest.third_party_task_details();
    let pct = third_party
        .as_ref()
        .map_or(details.percent_complete, |d| d.percent_complete);
    if quest.is_achievement_in_game() && has_already_linked == Some(true) && pct == 0. {
        return dc_intl::t("2+opCy").to_owned();
    }
    if quest.is_enrolled() && pct > 0. {
        let pct_text = format!("{}%", (pct * 100.).floor() as u64);
        return dc_intl::format(
            "lVZaXD",
            &[("percent", Arg::Str(std::borrow::Cow::Borrowed(&pct_text)))],
        );
    }
    if active_screen == ActivePlatformScreen::Select {
        return dc_intl::t("EMrUHQ").to_owned();
    }
    if quest.is_play_on_console() {
        return dc_intl::t("mOrpXG").to_owned();
    }
    dc_intl::t("7e5k7L").to_owned()
}

/// `NA` / `V` of module 870273: primary quest enrollment / launch CTA label.
pub fn primary_cta_label(quest: &Quest, short_text: bool) -> String {
    let bucket = if quest.is_play_on_console() || quest.is_play_activity() {
        0
    } else if quest.is_stream_on_desktop() {
        1
    } else if quest.is_watch_video_desktop() {
        2
    } else if quest.is_achievement_quest() {
        3
    } else {
        0
    };
    let details = quest.task_details();
    match bucket {
        0 => {
            if quest.config.has_feature(feature::CLOUD_GAMING_ACTIVITY) {
                dc_intl::t("+qoymD").to_owned()
            } else if quest.is_activity_quest() {
                dc_intl::t(if short_text { "E4kW5O" } else { "Ie9++s" }).to_owned()
            } else {
                dc_intl::t("l7E81v").to_owned()
            }
        }
        1 => dc_intl::t("l7E81v").to_owned(),
        2 => video_cta_label(&details),
        3 => {
            if quest.is_activity_quest() {
                dc_intl::t(if short_text { "CkUzLd" } else { "hRIVy+" }).to_owned()
            } else {
                dc_intl::t("l7E81v").to_owned()
            }
        }
        _ => dc_intl::t("l7E81v").to_owned(),
    }
}

/// `XR` / `Y` of module 870273: platform-specific redemption instructions for a reward code.
pub fn redemption_instructions(quest: &Quest, platform: u64) -> Option<&str> {
    let reward = if quest.config.is_tiered_reward_code() {
        let idx = quest
            .user_status
            .as_ref()
            .and_then(|s| s.claimed_tier)
            .unwrap_or(0) as usize;
        quest
            .config
            .rewards
            .get(idx)
            .filter(|r| r.kind == reward::REWARD_CODE)
    } else {
        quest.config.rewards.first()
    };
    reward.and_then(|r| {
        r.redemption_instructions_by_platform
            .get(&platform)
            .map(String::as_str)
    })
}

/// `Jf` / `Q` of module 870273: game link card subtitle.
pub fn game_cta_subtitle(quest: &Quest) -> String {
    if let Some(sub) = quest.config.cta_subtitle.as_deref() {
        return sub.to_owned();
    }
    if quest.is_achievement_in_game()
        && let Some(title) = quest
            .config
            .task_details
            .get(task::ACHIEVEMENT_IN_GAME)
            .and_then(|t| t.task_title.as_deref())
    {
        return title.to_owned();
    }
    if quest.is_watch_video()
        || quest.config.has_feature(feature::NON_GAMING_PLAY_QUEST)
        || quest.is_multi_game_play_quest()
    {
        dc_intl::t("y8Xf3k").to_owned()
    } else {
        dc_intl::t("wirwN+").to_owned()
    }
}

// ---- Module 452411: Quest CTA Matrix & Platform Menu ----

/// Context needed to resolve the Quest CTA button (module 452411).
#[derive(Clone, Copy, Debug)]
pub struct CtaContext {
    pub surface: CtaSurface,
    pub now_ms: i64,
    pub is_progressing: bool,
    pub awaiting_console_connections: bool,
    pub active_screen: ActivePlatformScreen,
    pub has_linked_game_account: bool,
    pub animate_refresh_icon: bool,
}

/// `tm` of module 452411: resolves the exact CTA button label, variant, and action across all 8 quest states.
pub fn resolve_cta(quest: &Quest, ctx: CtaContext) -> CtaSpec {
    if quest.config.tasks.is_empty() {
        return CtaSpec {
            label: dc_intl::t("P84bAD").to_owned(),
            variant: CtaVariant::Primary,
            action: CtaAction::Disabled,
            disabled: false,
            trailing_external: false,
            trailing_chevron: false,
        };
    }

    let state = quest.cta_state(ctx.now_ms);
    let task_type = quest.primary_task_type();
    let details = quest.task_details();
    let primary = ctx.surface.primary_variant();

    match state {
        // `tg` (UNENROLLED)
        QuestCtaState::Unenrolled => {
            let label = match task_type {
                task::WATCH_VIDEO | task::WATCH_VIDEO_ON_MOBILE => video_cta_label(&details),
                task::ACHIEVEMENT_IN_ACTIVITY => dc_intl::t("CkUzLd").to_owned(),
                task::PLAY_ACTIVITY => {
                    if quest.config.has_feature(feature::CLOUD_GAMING_ACTIVITY) {
                        dc_intl::t("+qoymD").to_owned()
                    } else {
                        dc_intl::t("E4kW5O").to_owned()
                    }
                }
                _ => dc_intl::t("kUQLMJ").to_owned(),
            };
            let action = if quest.is_watch_video_mobile_only() {
                CtaAction::WatchOnMobile
            } else if task_type == task::WATCH_VIDEO {
                CtaAction::WatchVideo
            } else {
                CtaAction::Enroll
            };
            CtaSpec {
                label,
                variant: primary,
                action,
                disabled: false,
                trailing_external: false,
                trailing_chevron: false,
            }
        }

        // `tA` / `tT` (ENROLLED / INCOMPLETE)
        QuestCtaState::Enrolled | QuestCtaState::Incomplete => {
            if state == QuestCtaState::Incomplete
                && (quest.is_play_on_desktop() || quest.is_play_on_console())
                && ctx.surface == CtaSurface::QuestBar
                && quest.is_multi_game_play_quest()
            {
                return CtaSpec {
                    label: dc_intl::t("9KoPyB").to_owned(),
                    variant: CtaVariant::Secondary,
                    action: CtaAction::Disabled,
                    disabled: true,
                    trailing_external: false,
                    trailing_chevron: false,
                };
            }

            // `tx` (game play / console / in-game achievement quests)
            if quest.is_game_play_quest() {
                if quest.is_achievement_in_game() {
                    if ctx.has_linked_game_account {
                        if ctx.surface == CtaSurface::QuestBar {
                            let label = if quest.config.cta_button_label.is_empty() {
                                dc_intl::t("SHZo2x").to_owned()
                            } else {
                                quest.config.cta_button_label.clone()
                            };
                            return CtaSpec {
                                label,
                                variant: CtaVariant::Secondary,
                                action: CtaAction::OpenGameLink,
                                disabled: false,
                                trailing_external: false,
                                trailing_chevron: false,
                            };
                        }
                        return CtaSpec {
                            label: dc_intl::t("SHZo2x").to_owned(),
                            variant: primary,
                            action: CtaAction::ViewConnection,
                            disabled: false,
                            trailing_external: false,
                            trailing_chevron: false,
                        };
                    }
                    let v2 = ctx.surface == CtaSurface::QuestHomeTileV2;
                    return CtaSpec {
                        label: dc_intl::t(if v2 { "SHZo2x" } else { "sbdnpw" }).to_owned(),
                        variant: primary,
                        action: CtaAction::ConnectAccount,
                        disabled: false,
                        trailing_external: false,
                        trailing_chevron: false,
                    };
                }

                // `tu` (Console quest on Quest Home tile footer)
                if quest.is_play_on_console() && ctx.surface == CtaSurface::QuestHomeTile {
                    if ctx.is_progressing || ctx.active_screen == ActivePlatformScreen::Desktop {
                        return CtaSpec {
                            label: dc_intl::t("9KoPyB").to_owned(),
                            variant: CtaVariant::Secondary,
                            action: CtaAction::Disabled,
                            disabled: true,
                            trailing_external: false,
                            trailing_chevron: false,
                        };
                    }
                    if ctx.awaiting_console_connections {
                        return CtaSpec {
                            label: dc_intl::t("Cfye4v").to_owned(),
                            variant: CtaVariant::Secondary,
                            action: CtaAction::ConnectConsole,
                            disabled: false,
                            trailing_external: false,
                            trailing_chevron: false,
                        };
                    }
                    // `z` (Check For Launches / Check Connection)
                    return CtaSpec {
                        label: dc_intl::t(if ctx.animate_refresh_icon {
                            "nPThNb"
                        } else {
                            "cdd1iy"
                        })
                        .to_owned(),
                        variant: if ctx.animate_refresh_icon {
                            CtaVariant::Primary
                        } else {
                            CtaVariant::Secondary
                        },
                        action: if ctx.animate_refresh_icon {
                            CtaAction::CheckForLaunches
                        } else {
                            CtaAction::CheckConnection
                        },
                        disabled: false,
                        trailing_external: false,
                        trailing_chevron: false,
                    };
                }

                // `td` (Multi-game play quest on Quest Bar footer)
                if quest.is_multi_game_play_quest() && ctx.surface == CtaSurface::QuestBar {
                    return CtaSpec {
                        label: dc_intl::t("93PTEs").to_owned(),
                        variant: CtaVariant::Secondary,
                        action: CtaAction::ViewEligibleGames,
                        disabled: false,
                        trailing_external: false,
                        trailing_chevron: false,
                    };
                }

                // `tE` / `tc` (Quest Home Tile V2 Play / platform menu)
                if ctx.surface == CtaSurface::QuestHomeTileV2 {
                    let has_consoles = !quest.supported_consoles().is_empty();
                    return CtaSpec {
                        label: dc_intl::t("U7cCLU").to_owned(),
                        variant: primary,
                        action: if has_consoles {
                            CtaAction::OpenPlatformMenu
                        } else {
                            CtaAction::OpenGameLink
                        },
                        disabled: false,
                        trailing_external: !has_consoles,
                        trailing_chevron: has_consoles,
                    };
                }

                return CtaSpec {
                    label: dc_intl::t("9KoPyB").to_owned(),
                    variant: CtaVariant::Secondary,
                    action: CtaAction::Disabled,
                    disabled: true,
                    trailing_external: false,
                    trailing_chevron: false,
                };
            }

            // `y` (video / activity enrolled CTA)
            let (label, action) = if quest.is_watch_video_mobile_only() {
                (dc_intl::t("/cXIc6").to_owned(), CtaAction::WatchOnMobile)
            } else if quest.is_watch_video_desktop() {
                (video_cta_label(&details), CtaAction::WatchVideo)
            } else if quest.is_activity_quest() {
                let lbl = if quest.is_achievement_in_activity() {
                    dc_intl::t("CkUzLd").to_owned()
                } else if quest.is_play_activity() {
                    if quest.config.has_feature(feature::CLOUD_GAMING_ACTIVITY) {
                        dc_intl::t("+qoymD").to_owned()
                    } else {
                        dc_intl::t("E4kW5O").to_owned()
                    }
                } else {
                    dc_intl::t("l7E81v").to_owned()
                };
                (lbl, CtaAction::LaunchActivity)
            } else {
                (dc_intl::t("9KoPyB").to_owned(), CtaAction::Disabled)
            };
            let disabled = action == CtaAction::Disabled;
            CtaSpec {
                label,
                variant: if disabled {
                    CtaVariant::Secondary
                } else {
                    primary
                },
                action,
                disabled,
                trailing_external: false,
                trailing_chevron: false,
            }
        }

        // `_` (COMPLETED / EXPIRED_CLAIMABLE)
        QuestCtaState::Completed | QuestCtaState::ExpiredClaimable => CtaSpec {
            label: dc_intl::t("cfY4PE").to_owned(),
            variant: primary,
            action: CtaAction::ClaimReward,
            disabled: false,
            trailing_external: false,
            trailing_chevron: false,
        },

        // `g` (CLAIMED)
        QuestCtaState::Claimed => {
            let (label, action) = if quest.config.has_reward_code() {
                (dc_intl::t("bAGFz3").to_owned(), CtaAction::SeeCode)
            } else {
                (dc_intl::t("vTgCWx").to_owned(), CtaAction::ViewReward)
            };
            CtaSpec {
                label,
                variant: primary,
                action,
                disabled: false,
                trailing_external: false,
                trailing_chevron: false,
            }
        }

        // `tf` (EXPIRED_CLAIMED)
        QuestCtaState::ExpiredClaimed => {
            if ctx.surface == CtaSurface::QuestHomeTile {
                let (label, action) = if quest.config.has_reward_code() {
                    (dc_intl::t("bAGFz3").to_owned(), CtaAction::SeeCode)
                } else {
                    (dc_intl::t("vTgCWx").to_owned(), CtaAction::ViewReward)
                };
                CtaSpec {
                    label,
                    variant: primary,
                    action,
                    disabled: false,
                    trailing_external: false,
                    trailing_chevron: false,
                }
            } else {
                expired_cta(quest)
            }
        }

        // `tI` (EXPIRED)
        QuestCtaState::Expired => expired_cta(quest),
    }
}

fn expired_cta(quest: &Quest) -> CtaSpec {
    let expiry = format_month_day(&quest.config.expires_at);
    CtaSpec {
        label: dc_intl::format(
            "6p8BZx",
            &[("expiryDate", Arg::Str(std::borrow::Cow::Borrowed(&expiry)))],
        ),
        variant: CtaVariant::Secondary,
        action: CtaAction::Disabled,
        disabled: true,
        trailing_external: false,
        trailing_chevron: false,
    }
}

/// Aria-label for the `Play` platform selector menu (`tc` of module 452411).
pub fn platform_menu_aria_label() -> &'static str {
    dc_intl::t("TWSB/5")
}

/// Builds the platform menu items (`tc` of module 452411): Xbox / PlayStation + Desktop + optional Check Connection.
pub fn platform_menu_options(
    quest: &Quest,
    selected_platform: Option<QuestPlatform>,
    selected_console: Option<ConsolePlatform>,
    connected_xbox: bool,
    connected_psn: bool,
    has_console_error: bool,
) -> Vec<PlatformMenuOption> {
    let mut items = Vec::new();
    for console in quest.supported_consoles() {
        let is_connected = match console {
            ConsolePlatform::Xbox => connected_xbox,
            ConsolePlatform::PlayStation => connected_psn,
        };
        let (id, label) = if is_connected {
            let platform_name = match console {
                ConsolePlatform::Xbox => dc_intl::t("CoT7ML"),
                ConsolePlatform::PlayStation => dc_intl::t("fFl4jo"),
            };
            (
                platform_name.to_owned(),
                dc_intl::format(
                    "LEgD7t",
                    &[(
                        "platform",
                        Arg::Str(std::borrow::Cow::Borrowed(platform_name)),
                    )],
                ),
            )
        } else {
            let id_name = match console {
                ConsolePlatform::Xbox => dc_intl::t("CoT7ML"),
                ConsolePlatform::PlayStation => dc_intl::t("fFl4jo"),
            };
            let connect_name = match console {
                ConsolePlatform::Xbox => dc_intl::t("CoT7ML"),
                ConsolePlatform::PlayStation => dc_intl::t("ikuLkv"),
            };
            (
                id_name.to_owned(),
                dc_intl::format(
                    "XWSHTb",
                    &[(
                        "platform",
                        Arg::Str(std::borrow::Cow::Borrowed(connect_name)),
                    )],
                ),
            )
        };
        let selected = selected_platform == Some(QuestPlatform::Console)
            && selected_console == Some(console)
            && is_connected;
        items.push(PlatformMenuOption {
            id,
            label,
            target: PlatformMenuTarget::Console(console),
            selected,
            connected: selected && !has_console_error,
            has_error: selected && has_console_error,
        });
    }

    let desktop_name = dc_intl::t("g6Dr44");
    items.push(PlatformMenuOption {
        id: dc_intl::t("QXc01+").to_owned(),
        label: dc_intl::format(
            "LEgD7t",
            &[(
                "platform",
                Arg::Str(std::borrow::Cow::Borrowed(desktop_name)),
            )],
        ),
        target: PlatformMenuTarget::Desktop,
        selected: selected_platform == Some(QuestPlatform::Desktop),
        connected: false,
        has_error: false,
    });

    if selected_platform == Some(QuestPlatform::Console)
        && let Some(console) = selected_console
    {
        let is_connected = match console {
            ConsolePlatform::Xbox => connected_xbox,
            ConsolePlatform::PlayStation => connected_psn,
        };
        if is_connected {
            items.push(PlatformMenuOption {
                id: "play-quest-platform-select-check-connection".to_owned(),
                label: dc_intl::t("cdd1iy").to_owned(),
                target: PlatformMenuTarget::CheckConnection,
                selected: false,
                connected: false,
                has_error: false,
            });
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_quest(tasks: serde_json::Value, rewards: serde_json::Value) -> Quest {
        Quest::from_value(&json!({
            "id": "1001",
            "preview": false,
            "config": {
                "config_version": 2,
                "starts_at": "2026-01-01T00:00:00Z",
                "expires_at": "2027-06-01T00:00:00Z",
                "features": [],
                "messages": {
                    "quest_name": "Elden Ring",
                    "game_title": "Elden Ring",
                    "game_publisher": "Bandai Namco"
                },
                "cta_config": {
                    "link": "https://example.com",
                    "button_label": "Learn More"
                },
                "task_config_v2": { "tasks": tasks },
                "rewards_config": {
                    "assignment_method": 1,
                    "rewards": rewards,
                    "rewards_expire_at": "2027-07-01T00:00:00Z",
                    "platforms": [0]
                }
            },
            "user_status": null
        }))
        .unwrap()
    }

    #[test]
    fn quest_instructions_cover_play_video_activity_console_and_multi_game() {
        let desktop = sample_quest(
            json!({"PLAY_ON_DESKTOP": {"type": "PLAY_ON_DESKTOP", "target": 900}}),
            json!([{"type": 4, "orb_quantity": 700}]),
        );
        let text = task_instructions(&desktop, InstructionsContext::default());
        assert_eq!(
            text,
            "Play Elden Ring for 15 minutes with your Discord client open and win 700 Orbs."
        );

        let video = sample_quest(
            json!({"WATCH_VIDEO": {"type": "WATCH_VIDEO", "target": 60}}),
            json!([{"type": 4, "orb_quantity": 500}]),
        );
        assert_eq!(
            task_instructions(&video, InstructionsContext::default()),
            "Watch the video to win 500 Orbs!"
        );

        let mut multi = sample_quest(
            json!({"PLAY_ON_DESKTOP": {"type": "PLAY_ON_DESKTOP", "target": 900, "applications": [{"id": "1"}, {"id": "2"}]}}),
            json!([{"type": 4, "orb_quantity": 700}]),
        );
        multi
            .config
            .features
            .push(feature::PACKAGE_ACTION_ADVENTURE);
        assert!(
            task_instructions(&multi, InstructionsContext::default())
                .contains("Action Packed event titles")
        );
    }

    #[test]
    fn quest_bar_title_and_subtitle_follow_progress_states() {
        let mut q = sample_quest(
            json!({"PLAY_ON_DESKTOP": {"type": "PLAY_ON_DESKTOP", "target": 900}}),
            json!([{"type": 4, "orb_quantity": 700}]),
        );
        q.user_status = Some(dc_state::quests::UserStatus {
            quest_id: "1001".to_owned(),
            enrolled_at: Some("2026-06-01T00:00:00Z".to_owned()),
            stream_progress_seconds: 450.,
            ..Default::default()
        });
        assert_eq!(
            bar_title(&q, ActivePlatformScreen::Desktop, None),
            "Quest 50% complete"
        );
        assert_eq!(
            bar_subtitle(&q, false, true, None, false, InstructionsContext::default()),
            "You're halfway there!"
        );
        q.user_status.as_mut().unwrap().completed_at = Some("2026-06-02T00:00:00Z".to_owned());
        assert_eq!(
            bar_title(&q, ActivePlatformScreen::Desktop, None),
            "Quest Completed!"
        );
        assert!(
            bar_subtitle(
                &q,
                false,
                false,
                None,
                false,
                InstructionsContext::default()
            )
            .starts_with("Claim by ")
        );
    }

    #[test]
    fn resolve_cta_handles_every_state_and_platform_menu() {
        let now = 1_780_000_000_000;
        let mut q = sample_quest(
            json!({
                "PLAY_ON_DESKTOP": {"type": "PLAY_ON_DESKTOP", "target": 900},
                "PLAY_ON_XBOX": {"type": "PLAY_ON_XBOX", "target": 900},
                "PLAY_ON_PLAYSTATION": {"type": "PLAY_ON_PLAYSTATION", "target": 900}
            }),
            json!([{"type": 1, "messages": {"name": "Skin", "name_with_article": "a Skin"}}]),
        );
        let ctx = CtaContext {
            surface: CtaSurface::QuestHomeTile,
            now_ms: now,
            is_progressing: false,
            awaiting_console_connections: true,
            active_screen: ActivePlatformScreen::Console,
            has_linked_game_account: false,
            animate_refresh_icon: true,
        };
        assert_eq!(resolve_cta(&q, ctx).label, "Accept Quest");

        q.user_status = Some(dc_state::quests::UserStatus {
            quest_id: "1001".to_owned(),
            enrolled_at: Some("2026-06-01T00:00:00Z".to_owned()),
            ..Default::default()
        });
        assert_eq!(resolve_cta(&q, ctx).label, "Connect Console");

        let opts = platform_menu_options(
            &q,
            Some(QuestPlatform::Console),
            Some(ConsolePlatform::Xbox),
            true,
            false,
            false,
        );
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0].label, "Play on XBOX");
        assert_eq!(opts[1].label, "Connect PSN");
        assert_eq!(opts[2].label, "Play on Desktop");
        assert_eq!(opts[3].label, "Check Connection");

        q.user_status.as_mut().unwrap().completed_at = Some("2026-06-02T00:00:00Z".to_owned());
        assert_eq!(resolve_cta(&q, ctx).label, "Claim Reward");

        q.user_status.as_mut().unwrap().claimed_at = Some("2026-06-03T00:00:00Z".to_owned());
        assert_eq!(resolve_cta(&q, ctx).label, "See Code");
    }
}
