//! Server Settings > Server Insights (module 989433 `cz`, stat card `cF`, its numbers `ck`, store `cM`, button `cV`).
//! Measured in `design/dom/settings/guild-community-analytics-darker-cozy.json`; the cards in their stylesheet only, as a
//! server needs 500 members for them and the fixture has one.
//!
//! Go to Server Insights asks `POST /auth/handoff` for a token and opens the developer portal with it; a failed request
//! opens the portal without one. The token goes only into that link, never to a log.
//!
//! Not run: the three analytics reads, the cards and the button, which would open a browser on the visible desktop.
//! Not done: streamer mode's stand-in for the cards (`er.A`), as the app has no streamer mode yet.

use super::tooltip::{Side, Tip};
use super::{Root, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

pub(super) const INSIGHTS_PREFIX: &str = "guild-insights:";
const COUNTS_PREFIX: &str = "guild-insights:counts:";
const STATS_PREFIX: &str = "guild-insights:stats:";
const HANDOFF_PREFIX: &str = "guild-insights:handoff:";
const NOT_ENOUGH_MEMBERS: i64 = 30029;
const MIN_MEMBERS: u64 = 500;
const NOT_READY: &str = "Looks like your server isn\u{2019}t quite ready to see full insights. You will only have access to Announcement Channels and Welcome Screen analytics.";
const BROKE: &str =
    "Uh oh! Looks like something broke. Please refresh the application and try again.";
/// `DEVELOPERS_ENDPOINT` in the build's `GLOBAL_ENV`.
const PORTAL: &str = "https://discord.com";

/// `cy`.
const STAT_FIELDS: [&str; 4] = ["pct_retained", "new_members", "visitors", "communicators"];

/// `cR`'s entry for one number: the value and its change.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Stat {
    value: Option<f64>,
    change: Option<f64>,
}

#[derive(Default)]
pub(crate) struct InsightsState {
    /// `eD.A.getMemberCount`; the state store does not keep it.
    members: Option<u64>,
    stats: std::collections::HashMap<&'static str, Stat>,
    /// `cG`.
    error: Option<i64>,
    /// `e` in `cV`: the button spins until the handoff answers.
    busy: bool,
    key: Option<String>,
    requested: bool,
}

/// `cL`: this period against the last, keeping any number already known.
fn merge_stats(stats: &mut std::collections::HashMap<&'static str, Stat>, rows: &[Value]) {
    let Some(now) = rows.first() else {
        return;
    };
    let before = rows.get(1);
    for field in STAT_FIELDS {
        let Some(value) = now[field].as_f64() else {
            continue;
        };
        let change = before
            .and_then(|row| row[field].as_f64())
            .filter(|old| *old != 0.)
            .map(|old| (value - old) * 100. / old);
        stats.entry(field).or_insert(Stat {
            value: Some(value),
            change,
        });
    }
}

/// `s_`, in en-US: one decimal only when the number has one, millions abbreviated.
fn format_stat(value: f64) -> String {
    if value >= 1_000_000. {
        return format!("{}M", format_stat(value / 1_000_000.));
    }
    let rounded = (value * 10.).round() / 10.;
    let text = if rounded.fract() != 0. {
        format!("{rounded:.1}")
    } else {
        format!("{rounded:.0}")
    };
    let (whole, part) = text
        .split_once('.')
        .map_or((text.as_str(), None), |(whole, part)| (whole, Some(part)));
    let digits: Vec<char> = whole.chars().collect();
    let mut grouped = String::new();
    for (index, digit) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*digit);
    }
    match part {
        Some(part) => format!("{grouped}.{part}"),
        None => grouped,
    }
}

/// `ck`: the big number and the line under it.
fn stat_lines(stat: Stat, percent: bool) -> (Option<String>, Option<String>) {
    let number = stat
        .value
        .map(|value| format!("{}{}", format_stat(value), if percent { "%" } else { "" }));
    let sub = match (stat.change, stat.value) {
        (Some(change), _) if !change.is_nan() => {
            Some(format!("{}% since last week", format_stat(change.abs())))
        }
        (_, Some(_)) => Some("From the most recent week".to_owned()),
        _ => None,
    };
    (number, sub)
}

/// `c_`: from the start of the week before last until now.
fn stats_window(now: chrono::DateTime<chrono::Local>) -> (String, String) {
    use chrono::Datelike;
    let days = i64::from(now.weekday().num_days_from_sunday()) + 1;
    let start = now - chrono::Duration::milliseconds(days * 86_400_000 + 3_628_800_000);
    let iso = |at: chrono::DateTime<chrono::Local>| {
        at.with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    };
    (iso(start), iso(now))
}

/// `DEVELOPER_PORTAL_LOGIN_HANDOFF`.
fn handoff_url(key: &str, token: &str, guild: Id) -> String {
    format!(
        "{PORTAL}/developers/handoff#handoff_key={key}&handoff_token={token}&redirect_to=%2Fdevelopers%2Fservers%2F{guild}"
    )
}

impl Root {
    fn insights_mut(&mut self) -> Option<&mut InsightsState> {
        self.server_settings.as_mut().map(|open| &mut open.insights)
    }

    pub(super) fn request_insights(&mut self, guild: Id) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{COUNTS_PREFIX}{guild}")),
            method: reqwest::Method::GET,
            route: format!("/guilds/{guild}?with_counts=true"),
            body: None,
        });
    }

    /// `i` in `cz`: a small server that is neither partnered nor verified has no numbers to read.
    fn insights_too_small(&self, guild: Id, members: Option<u64>) -> bool {
        let special = self.stores.guild(guild).is_some_and(|known| {
            known
                .features
                .iter()
                .any(|feature| feature == "PARTNERED" || feature == "VERIFIED")
        });
        members.is_none_or(|members| !special && members < MIN_MEMBERS)
    }

    fn request_insight_stats(&mut self, guild: Id) {
        let (start, end) = stats_window(chrono::Local::now());
        for route in [
            "engagement/overview",
            "growth-activation/overview",
            "growth-activation/retention",
        ] {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{STATS_PREFIX}{guild}")),
                method: reqwest::Method::GET,
                route: format!(
                    "/guilds/{guild}/analytics/{route}?start={start}&end={end}&interval=2"
                ),
                body: None,
            });
        }
    }

    pub(super) fn insights_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        let report = self.fetched.remove(key).unwrap_or_default();
        let ok = report["ok"] == true;
        let body = &report["body"];
        if let Some(guild) = key
            .strip_prefix(COUNTS_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            let members = body["approximate_member_count"].as_u64();
            let too_small = self.insights_too_small(guild, members);
            let mut read = false;
            if let Some(state) = self.insights_mut() {
                state.members = members;
                read = !too_small && !state.requested;
                state.requested |= read;
            }
            if read {
                self.request_insight_stats(guild);
            }
        } else if key.starts_with(STATS_PREFIX) {
            if let Some(state) = self.insights_mut() {
                if ok {
                    state.error = None;
                    // The retention list also has rows with no number; `pct_retained` filters them out.
                    let rows: Vec<Value> = body
                        .as_array()
                        .map(|rows| {
                            let retained = rows.iter().any(|row| row.get("pct_retained").is_some());
                            rows.iter()
                                .filter(|row| !retained || row.get("pct_retained").is_some())
                                .take(2)
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    merge_stats(&mut state.stats, &rows);
                } else {
                    state.error = Some(body["code"].as_i64().unwrap_or(0));
                }
            }
        } else if let Some(guild) = key
            .strip_prefix(HANDOFF_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            let token = body["handoff_token"].as_str().map(str::to_owned);
            let saved = self.insights_mut().and_then(|state| {
                state.busy = false;
                state.key.take()
            });
            match (ok, token, saved) {
                (true, Some(token), Some(handoff)) => {
                    cx.open_url(&handoff_url(&handoff, &token, guild));
                }
                _ => cx.open_url(&format!("{PORTAL}/developers/servers/{guild}")),
            }
        }
        cx.notify();
    }

    /// `cz`.
    pub(super) fn server_insights_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(open) = self.server_settings.as_ref() else {
            return div().into_any_element();
        };
        let state = &open.insights;
        let too_small = self.insights_too_small(guild, state.members);
        let notice = if too_small || state.error == Some(NOT_ENOUGH_MEMBERS) {
            Some((NOT_READY, false))
        } else if state.error.is_some() {
            Some((BROKE, true))
        } else {
            None
        };
        let has_stats = !state.stats.is_empty();
        let stat = |field: &str| state.stats.get(field).copied().unwrap_or_default();
        let cards = has_stats.then(|| {
            div()
                .mt(px(14.))
                .flex()
                .flex_wrap()
                .child(self.insight_card(
                    0,
                    "Weekly Visitors",
                    Some("A member who has clicked into the server and viewed a channel."),
                    stat_lines(stat("visitors"), false),
                    cx,
                ))
                .child(self.insight_card(
                    1,
                    "Weekly Communicators",
                    Some("A visitor who talked (voice or text)."),
                    stat_lines(stat("communicators"), false),
                    cx,
                ))
                .child(self.insight_card(
                    2,
                    "Weekly New Members",
                    None,
                    stat_lines(stat("new_members"), false),
                    cx,
                ))
                .child(self.insight_card(
                    3,
                    "Weekly New Member Retention",
                    Some("Percentage of new members that still came back to the server the week after joining."),
                    stat_lines(stat("pct_retained"), true),
                    cx,
                ))
        });
        let busy = state.busy;
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Server Insights"),
            )
            .child(
                div()
                    .mt(px(8.))
                    .mb(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("We\u{2019}ve put together a bunch of helpful data to help you better run your community. Learn how active your community is, where new members are coming from, and much more. Use what you learn to make informed decisions to improve your server\u{2019}s engagement!")
                    .child(
                        div()
                            .mt(px(14.))
                            .child("Analytics about Announcement Channels, Server Discovery, and Welcome Screen also live here."),
                    ),
            )
            .children(notice.map(|(text, critical)| {
                if critical {
                    Self::insights_critical(text)
                } else {
                    Self::safety_notice(div().child(text).into_any_element())
                }
            }))
            .child(
                div()
                    .mt(px(24.))
                    .mx_auto()
                    .w(px(458.))
                    .py(px(32.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w(px(350.))
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child("Server Insights is hosted on a separate website so that it\u{2019}s easier to share links and browse cross platform."),
                    )
                    .child(
                        div()
                            .id("insights-go")
                            .mt(px(20.))
                            .h(px(40.))
                            .px(px(15.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .when(!busy, |this| this.cursor_pointer())
                            .when(busy, |this| this.opacity(0.5))
                            .flex()
                            .items_center()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child("Go to Server Insights")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.press_insights(guild);
                                cx.notify();
                            })),
                    ),
            )
            .children(cards)
            .into_any_element()
    }

    /// `cV`.
    fn press_insights(&mut self, guild: Id) {
        let key = uuid::Uuid::new_v4().to_string();
        let Some(state) = self.insights_mut() else {
            return;
        };
        if state.busy {
            return;
        }
        state.busy = true;
        state.key = Some(key.clone());
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{HANDOFF_PREFIX}{guild}")),
            method: reqwest::Method::POST,
            route: "/auth/handoff".to_owned(),
            body: Some(json!({ "key": key })),
        });
    }

    fn insights_critical(text: &'static str) -> AnyElement {
        div()
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--inlinenotice-border-critical"))
            .bg(color("--background-feedback-mod-critical"))
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-strong"))
            .child(text)
            .into_any_element()
    }

    /// `cF`.
    fn insight_card(
        &self,
        index: usize,
        title: &'static str,
        tip: Option<&'static str>,
        (number, sub): (Option<String>, Option<String>),
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let change = ["visitors", "communicators", "new_members", "pct_retained"]
            .get(index)
            .and_then(|field| {
                self.server_settings
                    .as_ref()?
                    .insights
                    .stats
                    .get(field)?
                    .change
            });
        let (up, down) = (
            change.is_some_and(|change| change > 0.),
            change.is_some_and(|change| change < 0.),
        );
        let available = number.is_some();
        div()
            .mt(px(16.))
            .when(index % 2 == 1, |this| this.ml(px(16.)))
            .w(px(322.))
            .p(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .child(
                div()
                    .mb(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(title)
                    .children(tip.map(|tip| {
                        div().ml(px(4.)).child(self.with_tip(
                            SharedString::from(format!("insights-tip-{index}")),
                            Tip::text(tip, Side::Top),
                            icon("circle_info", 16., "--interactive-text-default"),
                            cx,
                        ))
                    })),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .when(available, |this| {
                        this.text_color(color("--interactive-text-active"))
                    })
                    .when(!available, |this| {
                        this.text_color(color("--interactive-text-default"))
                    })
                    .child(number.unwrap_or_else(|| "N/A".to_owned())),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-default"))
                    .when(up, |this| {
                        this.child(
                            gpui::svg()
                                .path("icons/insights_trend.svg")
                                .mr(px(5.))
                                .w(px(14.))
                                .h(px(12.))
                                .text_color(color("--green-360")),
                        )
                    })
                    .when(down, |this| {
                        this.child(
                            gpui::svg()
                                .path("icons/insights_trend.svg")
                                .mr(px(5.))
                                .mt(px(2.))
                                .w(px(14.))
                                .h(px(12.))
                                .text_color(color("--red-400"))
                                .with_transformation(gpui::Transformation::rotate(
                                    gpui::percentage(0.5),
                                )),
                        )
                    })
                    .children(sub),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn stats_compare_the_last_two_rows_and_keep_what_is_known() {
        let mut stats = HashMap::new();
        merge_stats(
            &mut stats,
            &[
                json!({"visitors": 150, "new_members": 5}),
                json!({"visitors": 100, "new_members": 0}),
            ],
        );
        assert_eq!(stats["visitors"].change, Some(50.));
        // The old value was 0: no change to show.
        assert_eq!(stats["new_members"].change, None);
        merge_stats(&mut stats, &[json!({"visitors": 999})]);
        assert_eq!(stats["visitors"].value, Some(150.));
        assert!(!stats.contains_key("communicators"));
    }

    #[test]
    fn numbers_read_like_s_and_ck() {
        assert_eq!(format_stat(1234.), "1,234");
        assert_eq!(format_stat(12.34), "12.3");
        assert_eq!(format_stat(999.), "999");
        assert_eq!(format_stat(2_500_000.), "2.5M");
        assert_eq!(format_stat(3_000_000.), "3M");
        let stat = Stat {
            value: Some(1200.),
            change: Some(-12.5),
        };
        assert_eq!(
            stat_lines(stat, false),
            (
                Some("1,200".to_owned()),
                Some("12.5% since last week".to_owned())
            )
        );
        let first_week = Stat {
            value: Some(40.),
            change: None,
        };
        assert_eq!(
            stat_lines(first_week, true),
            (
                Some("40%".to_owned()),
                Some("From the most recent week".to_owned())
            )
        );
        assert_eq!(stat_lines(Stat::default(), false), (None, None));
    }

    #[test]
    fn the_window_starts_six_weeks_and_the_days_of_this_week_ago() {
        use chrono::TimeZone;
        // A Sunday: one day, then the 42 days of `36288e5`.
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 8, 12, 0, 0)
            .single()
            .unwrap();
        let (start, end) = stats_window(now);
        let parse = |text: &str| chrono::DateTime::parse_from_rfc3339(text).unwrap();
        assert_eq!((parse(&end) - parse(&start)).num_days(), 43);
        assert!(end.ends_with(".000Z"));
    }

    #[test]
    fn the_handoff_link_carries_its_key_and_way_back() {
        assert_eq!(
            handoff_url("k", "t", 5),
            "https://discord.com/developers/handoff#handoff_key=k&handoff_token=t&redirect_to=%2Fdevelopers%2Fservers%2F5"
        );
    }
}
