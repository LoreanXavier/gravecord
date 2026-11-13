//! The data behind the Nitro home's "Reward" cards: the tiered tenure badge (module 682901 `VD`/`sp`, 105909 `$F`,
//! 439174 `UO`/`Xr`, 655752 `P`, 627380 `t`, 208223 `N`, 764231 `T`, the card itself in `sL` of module 886268) and
//! the Nitro Orbs program reward (`/users/@me/program-rewards`, module 961250; store 895944; card `so`, footer `sl`).

use chrono::{DateTime, Datelike, Duration, Months, Utc};
use serde_json::Value;

/// One tier of the tenure badge (`VD` of module 682901): its id, the months it takes, the message key of its Nitro
/// name (`nameUnformattedNitro`), and its pictures (module 208223): `standard` (a bundled asset), `ambientLarge`.
pub(super) struct Tier {
    pub id: &'static str,
    pub months: u32,
    pub name_key: &'static str,
    pub standard: &'static str,
    pub ambient_large: &'static str,
}

const CDN: &str = "https://cdn.discordapp.com/assets/content/";
const ASSETS: &str = "https://discord.com/assets/";

/// `sp`, in order, with `VD` and the pictures of module 208223 (the 1-month `standard` is module 714206).
pub(super) static TIERS: [Tier; 8] = [
    Tier {
        id: "premium_tenure_1_month_v2",
        months: 1,
        name_key: "tx9Fvw",
        standard: "242f4d0069d97414.svg",
        ambient_large: "d8df65e0c94fd9d1f5028b4b252710fede2f4059d419d32b6f70f253f0c27999.png",
    },
    Tier {
        id: "premium_tenure_3_month_v2",
        months: 3,
        name_key: "xJNb5C",
        standard: "a28ccbd2a55e8413.svg",
        ambient_large: "d0e033cda164abbfc02c39e70afcccb410954ceb63544178608eff0323092ccd.png",
    },
    Tier {
        id: "premium_tenure_6_month_v2",
        months: 6,
        name_key: "4scBFY",
        standard: "45bb84c5fd852085.svg",
        ambient_large: "a67846bf44a97eeaf00209a31f30146f2c3685abb207ec89f1aa97404efb7883.png",
    },
    Tier {
        id: "premium_tenure_12_month_v2",
        months: 12,
        name_key: "83Jw2B",
        standard: "d343a0b3439e81d1.svg",
        ambient_large: "10b3c17fdd7d4f454e9c2335a94e48f05a1ed9c23e5f48101e5b9895f8055bb7.png",
    },
    Tier {
        id: "premium_tenure_24_month_v2",
        months: 24,
        name_key: "vRSCKv",
        standard: "bb018f1613e8d528.svg",
        ambient_large: "1468c2db0e265bed25bdcae7f6fe586a66493cd7c8cf0ca9228dc8b899e22d1b.png",
    },
    Tier {
        id: "premium_tenure_36_month_v2",
        months: 36,
        name_key: "mV86tk",
        standard: "11a893d07ab86fe4.svg",
        ambient_large: "5d4f18789f059f49d6bb2519692c96e36479ddac46af90030e6bb0473199f0ed.png",
    },
    Tier {
        id: "premium_tenure_60_month_v2",
        months: 60,
        name_key: "WcsLxW",
        standard: "2801bd9ef48c1d87.svg",
        ambient_large: "ecfc2da5e1ee0563dc487d35e1aad74f083dd2f18e5798028ffbc96f255d6840.png",
    },
    Tier {
        id: "premium_tenure_72_month_v2",
        months: 72,
        name_key: "Z/qgMK",
        standard: "c20bd44dcc3b5ecb.svg",
        ambient_large: "7fbfa17e5011425252d9ff6c1f286f41560c8acc58cb543b3c895031ffda5289.png",
    },
];

impl Tier {
    pub(super) fn standard_url(&self) -> String {
        format!("{ASSETS}{}", self.standard)
    }

    pub(super) fn ambient_large_url(&self) -> String {
        format!("{CDN}{}", self.ambient_large)
    }
}

pub(super) fn tier(id: &str) -> Option<&'static Tier> {
    TIERS.iter().find(|tier| tier.id == id)
}

/// `Wo` of module 105909.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Upcoming,
    Withheld,
    Earned,
}

/// `$F`: the badge the card is about, and its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Badge {
    pub tier: &'static Tier,
    pub status: Status,
}

impl PartialEq for Tier {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Tier {}

impl std::fmt::Debug for Tier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.id)
    }
}

pub(super) fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

/// moment's `add(n, "months")`: the same day of a later month, the month's last day when it has fewer.
fn add_months(time: DateTime<Utc>, months: u32) -> DateTime<Utc> {
    time.checked_add_months(Months::new(months)).unwrap_or(time)
}

/// `Xr` of module 439174: the day a tier is earned, one day after its months have passed since `since`.
pub(super) fn earned_on(tier: &Tier, since: DateTime<Utc>) -> DateTime<Utc> {
    add_months(since, tier.months) + Duration::days(1)
}

/// `UO` of module 439174: the highest tier whose day has come.
pub(super) fn tier_for(since: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<&'static Tier> {
    let since = since?;
    TIERS
        .iter()
        .rev()
        .find(|tier| now >= earned_on(tier, since))
}

/// What the profile and the subscription say about the account (`d`, `h` and `hasPaidTier2Subscription` of module
/// 105909).
pub(super) struct Facts<'a> {
    /// `/users/{me}/profile`: its `badges` and `premium_since`.
    pub profile: Option<&'a Value>,
    /// `premiumSince` of the Nitro subscription, when the account's tier is Nitro.
    pub subscription_since: Option<DateTime<Utc>>,
    /// `hasPaidTier2Subscription()`.
    pub paid_nitro: bool,
}

impl Facts<'_> {
    fn profile_since(&self) -> Option<DateTime<Utc>> {
        self.profile?["premium_since"].as_str().and_then(parse_time)
    }

    /// `d`: the tier the profile shows (the last tenure badge of its list), else the one the date gives a paid Nitro
    /// account. Nothing without a profile or a `premium_since`.
    fn profile_tier(&self, now: DateTime<Utc>) -> Option<&'static Tier> {
        let profile = self.profile?;
        let since = self.profile_since()?;
        let listed = profile["badges"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .find_map(|badge| tier(badge["id"].as_str()?));
        listed.or_else(|| {
            self.paid_nitro
                .then(|| tier_for(Some(since), now))
                .flatten()
        })
    }

    /// `Xb`: when the run of Nitro began, from the subscription, else from the profile.
    pub(super) fn since(&self) -> Option<DateTime<Utc>> {
        self.subscription_since.or_else(|| self.profile_since())
    }

    /// `$F`.
    pub(super) fn badge(&self, now: DateTime<Utc>) -> Option<Badge> {
        if let Some(tier) = self.profile_tier(now) {
            return Some(Badge {
                tier,
                status: Status::Earned,
            });
        }
        let since = self.since()?;
        if !self.paid_nitro {
            return tier_for(Some(since), now).map(|tier| Badge {
                tier,
                status: Status::Withheld,
            });
        }
        Some(Badge {
            tier: &TIERS[0],
            status: Status::Upcoming,
        })
    }
}

/// `P` of module 655752: the tier that comes next (the upcoming one itself).
pub(super) fn next_tier(badge: Option<Badge>) -> Option<&'static Tier> {
    let badge = badge?;
    if badge.status == Status::Upcoming {
        return Some(badge.tier);
    }
    let index = TIERS.iter().position(|tier| tier.id == badge.tier.id)?;
    TIERS.get(index + 1)
}

/// moment's `a.diff(b, "months", true)` (`monthDiff(a, b)`): whole months plus the fraction of the month around the
/// end.
fn month_diff(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    if a.day() < b.day() {
        return -month_diff(b, a);
    }
    let whole = (b.year() - a.year()) as i64 * 12 + (b.month() as i64 - a.month() as i64);
    let shift = |months: i64| -> DateTime<Utc> {
        if months >= 0 {
            add_months(a, months as u32)
        } else {
            a.checked_sub_months(Months::new((-months) as u32))
                .unwrap_or(a)
        }
    };
    let anchor = shift(whole);
    let ms = |time: DateTime<Utc>| time.timestamp_millis() as f64;
    let adjust = if ms(b) - ms(anchor) < 0. {
        let before = shift(whole - 1);
        (ms(b) - ms(anchor)) / (ms(anchor) - ms(before))
    } else {
        let after = shift(whole + 1);
        (ms(b) - ms(anchor)) / (ms(after) - ms(anchor))
    };
    let result = -(whole as f64 + adjust);
    if result == 0. { 0. } else { result }
}

/// `t` of module 627380: the days and months until the next tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Until {
    pub days: i64,
    pub months: i64,
}

pub(super) fn until_next(
    next: Option<&Tier>,
    since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<Until> {
    let (next, since) = (next?, since?);
    let at = earned_on(next, since);
    Some(Until {
        days: (at - now).num_days().max(0),
        months: month_diff(at, now).round().max(0.) as i64,
    })
}

/// The progress bar of the card (`g` in `sL`): how far the run is toward the next tier.
pub(super) fn progress(
    badge: Option<Badge>,
    next: Option<&Tier>,
    since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<f32> {
    let (badge, next, since) = (badge?, next?, since?);
    if badge.status == Status::Withheld {
        return None;
    }
    let span = (add_months(since, next.months) - since).num_milliseconds() as f64;
    let done = (now - since).num_milliseconds() as f64 - 86_400_000.;
    Some((done / span).clamp(0., 1.) as f32)
}

/// `T` of module 764231: "N Months" below a year, "N Years" from one.
pub(super) fn tenure_length(tier: &Tier) -> String {
    if tier.months < 12 {
        dc_intl::format("erUSmA", &[("months", f64::from(tier.months).into())])
    } else {
        dc_intl::format("qOdyDe", &[("years", f64::from(tier.months / 12).into())])
    }
}

/// The card's description (`e` in `sL`).
pub(super) fn description(badge: Option<Badge>, until: Option<Until>) -> Option<String> {
    let badge = badge?;
    Some(match badge.status {
        Status::Upcoming => dc_intl::format(
            "a1eKDi",
            &[("days", (until.map_or(0, |until| until.days) as f64).into())],
        ),
        Status::Withheld => tenure_length(badge.tier),
        Status::Earned => {
            let next = next_tier(Some(badge)).map(|tier| dc_intl::t(tier.name_key));
            match (until, next) {
                (Some(until), Some(name)) if until.days <= 30 => dc_intl::format(
                    "NEXoaI",
                    &[
                        ("days", (until.days as f64).into()),
                        ("nextBadgeName", name.into()),
                    ],
                ),
                (Some(until), Some(name)) if until.months <= 3 => dc_intl::format(
                    "KDV8oD",
                    &[
                        ("months", (until.months as f64).into()),
                        ("nextBadgeName", name.into()),
                    ],
                ),
                _ => tenure_length(badge.tier),
            }
        }
    })
}

/// `{date, date, short}` in en-US: `M/D/YY` (`Intl.DateTimeFormat` short date).
pub(super) fn short_date(time: DateTime<Utc>) -> String {
    let local = time.with_timezone(&chrono::Local).date_naive();
    format!(
        "{}/{}/{:02}",
        local.month(),
        local.day(),
        local.year().rem_euclid(100)
    )
}

/// The NITRO program's reward (`getRewardForProgram(NITRO)`) from the body of `GET /users/@me/program-rewards`.
pub(super) fn nitro_reward(body: &Value) -> Option<&Value> {
    body["rewards"]
        .as_array()?
        .iter()
        .find(|reward| reward["reward_program"].as_i64() == Some(0))
}

/// `passesGeneralUIInvariant` of module 622886.
pub(super) fn passes_ui_invariant(reward: &Value, now: DateTime<Utc>) -> bool {
    let Some(state) = reward["program_current_state"].as_str() else {
        return false;
    };
    match reward["next_reward_date"]
        .as_str()
        .filter(|date| !date.is_empty())
    {
        None => matches!(state, "payment_processing" | "payment_error"),
        Some(date) => parse_time(date).is_some_and(|date| date >= now),
    }
}

/// `differenceInDays` of date-fns: whole days, cut toward zero.
pub(super) fn days_until(date: &str, now: DateTime<Utc>) -> Option<i64> {
    let date = parse_time(date)?;
    Some((date - now).num_days())
}

/// A day at midnight UTC, for tests and fixtures.
#[cfg(test)]
pub(super) fn day(year: i32, month: u32, date: u32) -> DateTime<Utc> {
    chrono::NaiveDate::from_ymd_opt(year, month, date)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map(|time| time.and_utc())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tier_is_earned_a_day_after_its_months() {
        let since = day(2025, 1, 31);
        // moment keeps the last day of a shorter month.
        assert_eq!(earned_on(&TIERS[0], since), day(2025, 3, 1));
        assert_eq!(tier_for(Some(since), day(2025, 2, 28)), None);
        assert_eq!(
            tier_for(Some(since), day(2025, 3, 1)).map(|t| t.months),
            Some(1)
        );
        assert_eq!(
            tier_for(Some(since), day(2026, 11, 13)).map(|t| t.months),
            Some(12)
        );
    }

    #[test]
    fn the_profile_badge_wins_then_the_date_decides() {
        let profile = json!({"premium_since": "2024-02-10T09:30:00+00:00",
            "badges": [{"id": "premium"}, {"id": "premium_tenure_12_month_v2"}]});
        let facts = Facts {
            profile: Some(&profile),
            subscription_since: None,
            paid_nitro: true,
        };
        let now = day(2026, 11, 13);
        let badge = facts.badge(now).unwrap();
        assert_eq!((badge.tier.months, badge.status), (12, Status::Earned));
        assert_eq!(next_tier(Some(badge)).map(|t| t.months), Some(24));

        // A paid account whose profile lists no tenure badge has the one its date gives.
        let bare = json!({"premium_since": "2024-02-10T09:30:00+00:00", "badges": []});
        let facts = Facts {
            profile: Some(&bare),
            subscription_since: None,
            paid_nitro: true,
        };
        assert_eq!(facts.badge(now).unwrap().tier.months, 24);

        // Without a profile date: the subscription's run, upcoming for a paid account, withheld otherwise.
        let empty = json!({"badges": []});
        let facts = Facts {
            profile: Some(&empty),
            subscription_since: Some(day(2026, 11, 1)),
            paid_nitro: true,
        };
        let badge = facts.badge(now).unwrap();
        assert_eq!((badge.tier.months, badge.status), (1, Status::Upcoming));
        assert_eq!(next_tier(Some(badge)).map(|t| t.months), Some(1));
        let facts = Facts {
            profile: Some(&empty),
            subscription_since: Some(day(2024, 1, 1)),
            paid_nitro: false,
        };
        let badge = facts.badge(now).unwrap();
        assert_eq!((badge.tier.months, badge.status), (24, Status::Withheld));
        let facts = Facts {
            profile: None,
            subscription_since: None,
            paid_nitro: true,
        };
        assert_eq!(facts.badge(now), None);
    }

    #[test]
    fn the_description_counts_down_to_the_next_tier() {
        let now = day(2026, 11, 13);
        let earned = Badge {
            tier: &TIERS[3],
            status: Status::Earned,
        };
        // 24 months from 2024-12-01 plus a day is 2026-12-02: 19 days away.
        let until = until_next(next_tier(Some(earned)), Some(day(2024, 12, 1)), now).unwrap();
        assert_eq!(
            until,
            Until {
                days: 19,
                months: 1
            }
        );
        assert_eq!(
            description(Some(earned), Some(until)).unwrap(),
            "19 days to Nitro Diamond"
        );
        let later = until_next(next_tier(Some(earned)), Some(day(2025, 1, 20)), now).unwrap();
        assert_eq!(later.months, 2);
        assert_eq!(
            description(Some(earned), Some(later)).unwrap(),
            "2 months to Nitro Diamond"
        );
        let far = until_next(next_tier(Some(earned)), Some(day(2025, 11, 1)), now).unwrap();
        assert_eq!(description(Some(earned), Some(far)).unwrap(), "1 Year");
        let upcoming = Badge {
            tier: &TIERS[0],
            status: Status::Upcoming,
        };
        assert_eq!(
            description(Some(upcoming), Some(Until { days: 1, months: 0 })).unwrap(),
            "Unlocks in 1 day"
        );
        assert_eq!(tenure_length(&TIERS[2]), "6 Months");
        assert_eq!(tenure_length(&TIERS[4]), "2 Years");
        assert_eq!(description(None, None), None);
    }

    #[test]
    fn progress_runs_from_the_start_of_the_run_to_the_next_tier() {
        let earned = Badge {
            tier: &TIERS[0],
            status: Status::Earned,
        };
        let since = day(2026, 1, 1);
        // Three months from 2026-01-01 are 90 days; 46 days in, a day is taken off: 45 / 90.
        let at = progress(Some(earned), Some(&TIERS[1]), Some(since), day(2026, 2, 16)).unwrap();
        assert!((at - 0.5).abs() < 1e-6, "{at}");
        let withheld = Badge {
            tier: &TIERS[0],
            status: Status::Withheld,
        };
        assert_eq!(
            progress(
                Some(withheld),
                Some(&TIERS[1]),
                Some(since),
                day(2026, 2, 16)
            ),
            None
        );
        assert_eq!(
            progress(Some(earned), Some(&TIERS[1]), Some(since), day(2025, 2, 16)),
            Some(0.)
        );
    }

    #[test]
    fn the_nitro_reward_is_the_program_zero_entry_and_must_be_current() {
        let body = json!({"rewards": [
            {"reward_program": 1, "reward_amount": 5},
            {"reward_program": 0, "reward_amount": 200, "next_reward_date": "2026-11-20T00:00:00+00:00",
             "program_current_state": "active", "total_rewarded_from_program": 1400}
        ]});
        let reward = nitro_reward(&body).unwrap();
        assert_eq!(reward["reward_amount"], 200);
        let now = day(2026, 11, 13);
        assert!(passes_ui_invariant(reward, now));
        assert!(!passes_ui_invariant(reward, day(2026, 12, 1)));
        assert_eq!(days_until("2026-11-20T00:00:00+00:00", now), Some(7));
        let processing =
            json!({"program_current_state": "payment_processing", "next_reward_date": ""});
        assert!(passes_ui_invariant(&processing, now));
        assert!(!passes_ui_invariant(&json!({"next_reward_date": ""}), now));
        assert!(nitro_reward(&json!({"rewards": []})).is_none());
    }

    #[test]
    fn month_diff_matches_moment() {
        // moment("2026-03-15").diff("2026-01-15", "months", true) is 2; half of February more is about 2.5.
        assert!((month_diff(day(2026, 3, 15), day(2026, 1, 15)) - 2.).abs() < 1e-9);
        let half = month_diff(day(2026, 3, 1), day(2026, 1, 15));
        assert!((half - 1.5).abs() < 0.05, "{half}");
    }

    #[test]
    fn short_dates_read_month_day_year() {
        assert_eq!(
            short_date(day(2024, 2, 10) + Duration::hours(12)),
            "2/10/24"
        );
    }
}
