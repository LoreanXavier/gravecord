//! The server's verification level in place of the composer (GuildVerificationStore `getCheck`, module 834942, drawn
//! by module 678655 with the notice of module 786051): an account that is not claimed, has no verified email, has no
//! phone, or is too new to the server or to Discord sees why it may not talk yet, with the button that fixes it or
//! the time left.

use super::{Root, color};
use crate::parity::Parity;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};
use std::sync::atomic::{AtomicBool, Ordering};

/// `VerificationLevels` (`PvD`).
const LOW: u64 = 1;
const MEDIUM: u64 = 2;
const HIGH: u64 = 3;
const VERY_HIGH: u64 = 4;
/// `$8o`: minutes an account must exist (MEDIUM) and minutes a member must have been in the server (HIGH).
const ACCOUNT_AGE_MIN: u64 = 5;
const MEMBER_AGE_MIN: u64 = 10;
/// `BYPASSES_VERIFICATION` of the member flags (module 340837).
const BYPASSES_VERIFICATION: u64 = 1 << 2;
/// The user flag `STAFF`.
const STAFF: u64 = 1;
/// `new Date("2022-12-02 00:00:00")`, local time, as milliseconds: members who joined before it keep their roles'
/// exemption even where onboarding was ever enabled.
fn onboarding_cutoff_ms() -> i64 {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(2022, 12, 2, 0, 0, 0)
        .single()
        .map_or(1_669_939_200_000, |at| at.timestamp_millis())
}

/// What module 834942 `I` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GateFacts {
    pub level: u64,
    /// The server has a verification role (`verificationRoleId`), and the account's member object lacks it.
    pub verification_role: bool,
    pub missing_role: bool,
    pub owner: bool,
    /// The account's member object here: its flags, whether it holds a role that is not managed, and when it joined.
    pub member: Option<(u64, bool, Option<i64>)>,
    pub onboarding_ever_enabled: bool,
    pub phone: bool,
    pub claimed: bool,
    pub email_verified: bool,
    pub staff: bool,
    pub account_created_ms: i64,
    /// `guild.joinedAt`.
    pub guild_joined_ms: Option<i64>,
}

/// The first reason of module 678655 that applies, in its order (rules screening comes between the first two).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Gate {
    NotClaimed,
    /// The member lacks the server's verification role (`HbivnU`).
    MissingRole,
    NotPhoneVerified,
    NotEmailVerified,
    /// The member deadline, in milliseconds since the epoch.
    NewMember(i64),
    /// The account deadline.
    NewAccount(i64),
}

/// Module 834942 `I`, then the order module 678655 reads its answer in.
pub(super) fn gate(facts: &GateFacts, now_ms: i64) -> Option<Gate> {
    let leveled = facts.level != 0;
    if !leveled && !facts.verification_role || facts.owner {
        return None;
    }
    if facts
        .member
        .is_some_and(|(flags, _, _)| flags & BYPASSES_VERIFICATION != 0)
    {
        return None;
    }
    let missing_role = facts.verification_role && facts.member.is_some() && facts.missing_role;
    let (mut not_claimed, mut not_email, mut not_phone) = (false, false, false);
    let (mut new_account, mut new_member) = (None, None);
    if leveled && !facts.phone {
        // A member holding a role that is not managed passes, unless the server has a verification role, or
        // onboarding was ever on and it joined after the cut.
        let exempt = facts.member.is_some_and(|(_, has_role, joined)| {
            let early = joined.is_none_or(|at| at < onboarding_cutoff_ms());
            let onboarding = facts.onboarding_ever_enabled && !early;
            !facts.verification_role && !onboarding && has_role
        });
        if !exempt {
            let account_deadline = facts.account_created_ms + 60_000 * ACCOUNT_AGE_MIN as i64;
            let member_deadline =
                facts.guild_joined_ms.unwrap_or(now_ms) + 60_000 * MEMBER_AGE_MIN as i64;
            not_claimed = facts.level >= LOW && !facts.claimed;
            if !facts.staff {
                not_email = facts.level >= LOW && !facts.email_verified;
                not_phone = facts.level >= VERY_HIGH;
                new_account = (facts.level >= MEDIUM && account_deadline > now_ms)
                    .then_some(account_deadline);
                new_member =
                    (facts.level >= HIGH && member_deadline > now_ms).then_some(member_deadline);
            }
        }
    }
    if not_claimed {
        Some(Gate::NotClaimed)
    } else if missing_role {
        Some(Gate::MissingRole)
    } else if not_phone && !facts.staff {
        Some(Gate::NotPhoneVerified)
    } else if not_email {
        Some(Gate::NotEmailVerified)
    } else if let Some(deadline) = new_member {
        Some(Gate::NewMember(deadline))
    } else {
        new_account.map(Gate::NewAccount)
    }
}

/// Module 576470's default render: days, when there are any, then hours, minutes and seconds, two digits each, with
/// colons between them.
pub(super) fn countdown(left_ms: i64) -> String {
    let total = (left_ms.max(0) / 1000) as u64;
    let (days, hours, minutes, seconds) = (
        total / 86_400,
        total / 3_600 % 24,
        total / 60 % 60,
        total % 60,
    );
    let mut parts = vec![hours, minutes, seconds];
    if days > 0 {
        parts.insert(0, days);
    }
    parts
        .iter()
        .map(|part| format!("{part:02}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// A one-second redraw is pending for the countdown.
static TICKING: AtomicBool = AtomicBool::new(false);

fn parse_ms(at: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|at| at.timestamp_millis())
}

impl Root {
    fn gate_facts(&self) -> Option<GateFacts> {
        let guild = self.current_guild()?;
        let me = self.stores.current_user.as_ref()?;
        // The account's own member object as the guild carries it: its roles, flags and join time.
        let has_role = guild
            .my_roles
            .iter()
            .any(|role| guild.role_info.get(role).is_some_and(|info| !info.managed));
        let member = Some((guild.my_flags, has_role, parse_ms(&guild.joined_at)));
        let account = &self.stores.account;
        let role = guild.verification_role_id;
        Some(GateFacts {
            level: guild.verification_level.unwrap_or(0),
            verification_role: role.is_some(),
            // Module 678655 draws the notice only for a role the role store knows (`null!=E`).
            missing_role: role.is_some_and(|role| {
                guild.roles.contains_key(&role) && !guild.my_roles.contains(&role)
            }),
            owner: guild.owner_id == Some(me.id),
            member,
            onboarding_ever_enabled: guild
                .features
                .iter()
                .any(|feature| feature == "GUILD_ONBOARDING_EVER_ENABLED"),
            phone: account.phone.is_some(),
            claimed: account.email.is_some() || account.phone.is_some(),
            email_verified: account.verified,
            staff: me.flags & STAFF != 0,
            account_created_ms: ((me.id >> 22) + 1_420_070_400_000) as i64,
            guild_joined_ms: parse_ms(&guild.joined_at),
        })
    }

    /// The reason this account may not talk in the open server yet, if any.
    pub(super) fn verification_gate(&self) -> Option<Gate> {
        let facts = self.gate_facts()?;
        gate(&facts, chrono::Utc::now().timestamp_millis())
    }

    /// Module 786051 for the gate: the message, then its button (`sm`, `secondary`), or the time left in
    /// `countdown_ST_Y69` (`--text-muted`, 16px from the end).
    pub(super) fn verification_composer(&self, gate: Gate, cx: &mut Context<Self>) -> AnyElement {
        let now = chrono::Utc::now().timestamp_millis();
        let role = self.current_guild().and_then(|guild| {
            let id = guild.verification_role_id?;
            let info = guild.role_info.get(&id);
            Some((
                guild.id,
                id,
                guild.roles.get(&id).cloned().unwrap_or_default(),
                info.map_or(0, |info| info.color),
                info.is_some_and(|info| info.guild_connection),
            ))
        });
        let (message, button, left): (String, Option<&'static str>, Option<i64>) = match gate {
            Gate::NotClaimed => (dc_intl::t("Eg3/c9").to_owned(), Some("fiNVin"), None),
            // `HbivnU` with `@name` in bold, and Get Role (`6Ge2LG`) for a linked role (`guild_connections`).
            Gate::MissingRole => (
                String::new(),
                role.as_ref()
                    .filter(|(_, _, _, _, linked)| *linked)
                    .map(|_| "6Ge2LG"),
                None,
            ),
            Gate::NotPhoneVerified => (dc_intl::t("2dThMM").to_owned(), Some("50gfOv"), None),
            Gate::NotEmailVerified => (dc_intl::t("FkGPS5").to_owned(), Some("lm1UKt"), None),
            Gate::NewMember(deadline) => (
                dc_intl::t!("IH7RMF", min = MEMBER_AGE_MIN),
                None,
                Some(deadline - now),
            ),
            Gate::NewAccount(deadline) => (
                dc_intl::t!("2JA2GH", min = ACCOUNT_AGE_MIN),
                None,
                Some(deadline - now),
            ),
        };
        if left.is_some() && !TICKING.swap(true, Ordering::Relaxed) {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                TICKING.store(false, Ordering::Relaxed);
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        div()
            .relative()
            .flex_none()
            .mt(px(-16.))
            .px(px(8.))
            .pb(px(8.))
            .children(self.typing_bar(cx))
            .child(
                div()
                    .parity_with_border("verification-composer", 1.)
                    .min_h(px(58.))
                    .py(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-low"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .ml(px(16.))
                            .flex_1()
                            .min_w(px(0.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .when(gate != Gate::MissingRole, |title| title.child(message))
                            .when(gate == Gate::MissingRole, |title| {
                                let name = role
                                    .as_ref()
                                    .map(|(_, _, name, _, _)| format!("@{name}"))
                                    .unwrap_or_default();
                                title.flex().flex_wrap().children(
                                    dc_intl::format_runs(
                                        "HbivnU",
                                        &[("roleName", dc_intl::Arg::from(name.as_str()))],
                                    )
                                    .into_iter()
                                    .map(|(text, tag)| {
                                        div()
                                            .when(tag.as_deref() == Some("$b"), |run| {
                                                run.font_weight(FontWeight(700.))
                                            })
                                            .child(text)
                                    }),
                                )
                            }),
                    )
                    .children(button.map(|key| {
                        div()
                            .id("verification-gate-button")
                            .flex_none()
                            .ml(px(12.))
                            .mr(px(12.))
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-secondary-border-default"))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(dc_intl::t(key))
                            .on_click(cx.listener(move |this, _, _, cx| match gate {
                                Gate::NotClaimed => this.open_claim_account_modal(false, cx),
                                Gate::NotPhoneVerified => this.open_account_form("phone", cx),
                                Gate::NotEmailVerified => this.trigger_verify_resend(cx),
                                Gate::MissingRole => {
                                    if let Some((guild, id, name, color, _)) = role.clone() {
                                        this.open_role_connection_verify(
                                            guild, id, name, color, cx,
                                        );
                                    }
                                }
                                _ => {}
                            }))
                    }))
                    .children(left.map(|left| {
                        div()
                            .flex_none()
                            .mr(px(16.))
                            .text_color(color("--text-muted"))
                            .child(countdown(left))
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    fn fresh() -> GateFacts {
        GateFacts {
            level: LOW,
            claimed: true,
            email_verified: true,
            account_created_ms: NOW - 86_400_000,
            guild_joined_ms: Some(NOW - 86_400_000),
            ..GateFacts::default()
        }
    }

    #[test]
    fn no_level_owner_bypass_or_phone_lets_anyone_talk() {
        let mut facts = fresh();
        facts.claimed = false;
        facts.level = 0;
        assert_eq!(gate(&facts, NOW), None);
        facts.level = VERY_HIGH;
        facts.owner = true;
        assert_eq!(gate(&facts, NOW), None);
        facts.owner = false;
        facts.member = Some((BYPASSES_VERIFICATION, false, None));
        assert_eq!(gate(&facts, NOW), None);
        facts.member = None;
        facts.phone = true;
        assert_eq!(gate(&facts, NOW), None);
    }

    #[test]
    fn the_reasons_come_in_the_official_order() {
        let mut facts = fresh();
        facts.level = VERY_HIGH;
        facts.claimed = false;
        assert_eq!(gate(&facts, NOW), Some(Gate::NotClaimed));
        facts.claimed = true;
        assert_eq!(gate(&facts, NOW), Some(Gate::NotPhoneVerified));
        facts.level = HIGH;
        facts.email_verified = false;
        assert_eq!(gate(&facts, NOW), Some(Gate::NotEmailVerified));
        facts.email_verified = true;
        facts.guild_joined_ms = Some(NOW - 60_000);
        assert_eq!(gate(&facts, NOW), Some(Gate::NewMember(NOW + 9 * 60_000)));
        facts.level = MEDIUM;
        facts.account_created_ms = NOW - 60_000;
        assert_eq!(gate(&facts, NOW), Some(Gate::NewAccount(NOW + 4 * 60_000)));
    }

    #[test]
    fn a_role_that_is_not_managed_exempts_unless_onboarding_came_after() {
        let mut facts = fresh();
        facts.email_verified = false;
        facts.member = Some((0, true, Some(NOW)));
        assert_eq!(gate(&facts, NOW), None);
        facts.onboarding_ever_enabled = true;
        assert_eq!(gate(&facts, NOW), Some(Gate::NotEmailVerified));
    }

    #[test]
    fn countdowns_read_as_the_official_timer_does() {
        assert_eq!(countdown(299_000), "00:04:59");
        assert_eq!(countdown(90_061_000), "01:01:01:01");
        assert_eq!(countdown(-5), "00:00:00");
    }
}

impl Root {
    /// Capture only: `verification <level> <joined seconds ago> [unverified] [unclaimed] [role]` puts the open server under that
    /// verification level as a member that is not its owner, holds no role and joined that long ago; `unverified`
    /// clears the account's verified email and `unclaimed` its email; the phone is always cleared (nothing is sent).
    #[cfg(feature = "capture")]
    pub(crate) fn script_verification(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let level = words
            .first()
            .and_then(|word| word.parse::<u64>().ok())
            .unwrap_or(0);
        let ago = words
            .get(1)
            .and_then(|word| word.parse::<i64>().ok())
            .unwrap_or(0);
        let joined = chrono::Utc::now() - chrono::Duration::seconds(ago);
        let Some(id) = self.guild else {
            return "no server open".to_owned();
        };
        if let Some(guild) = self.stores.guilds.iter_mut().find(|guild| guild.id == id) {
            guild.verification_level = Some(level);
            guild.owner_id = None;
            guild.my_roles.clear();
            guild.my_flags = 0;
            guild.joined_at = joined.to_rfc3339();
            // `role`: the server's highest role other than @everyone becomes its verification role.
            guild.verification_role_id = words
                .contains(&"role")
                .then(|| {
                    guild
                        .roles
                        .keys()
                        .copied()
                        .filter(|role| *role != id)
                        .max_by_key(|role| {
                            guild.role_info.get(role).map_or(0, |info| info.position)
                        })
                })
                .flatten();
        }
        if words.contains(&"unverified") {
            self.stores.account.verified = false;
        }
        if words.contains(&"unclaimed") {
            self.stores.account.email = None;
        }
        self.stores.account.phone = None;
        cx.notify();
        format!("{:?}", self.verification_gate())
    }
}

#[cfg(test)]
mod role_tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn a_missing_verification_role_comes_after_claiming_and_before_the_phone() {
        let facts = GateFacts {
            level: VERY_HIGH,
            verification_role: true,
            missing_role: true,
            member: Some((0, true, Some(NOW))),
            claimed: true,
            email_verified: true,
            account_created_ms: NOW - 86_400_000,
            guild_joined_ms: Some(NOW - 86_400_000),
            ..GateFacts::default()
        };
        assert_eq!(gate(&facts, NOW), Some(Gate::MissingRole));
        assert_eq!(
            gate(
                &GateFacts {
                    claimed: false,
                    ..facts.clone()
                },
                NOW
            ),
            Some(Gate::NotClaimed)
        );
        // The role alone gates a server without a level, and holding it lets the member talk.
        let role_only = GateFacts {
            level: 0,
            ..facts.clone()
        };
        assert_eq!(gate(&role_only, NOW), Some(Gate::MissingRole));
        assert_eq!(
            gate(
                &GateFacts {
                    missing_role: false,
                    ..role_only
                },
                NOW
            ),
            None
        );
        // With a verification role, holding another role no longer exempts the member.
        assert_eq!(
            gate(
                &GateFacts {
                    missing_role: false,
                    ..facts
                },
                NOW
            ),
            Some(Gate::NotPhoneVerified)
        );
    }
}
