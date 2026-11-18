//! "Turn Off/On Activity In Your Servers?" (module 32167, after Activity Privacy's default for servers changes) and
//! "Share activity too?" / "Stop sharing activity too?" (module 413201, after Share my full profile with changes):
//! the servers whose activity sharing no longer matches the new default (`g8` and `gS` of module 365258) in the card
//! of module 929120, Not Now and the confirm button, which puts those servers in or takes them out of the account's
//! `activity_restricted_guild_ids` (`gF`) and says so in a success toast.

use super::Root;
use super::chat_notices::{Alert, AlertButton, Content, Then};
use super::modal::Tone;
use dc_proto::settings::ACTIVITY_RESTRICTED_GUILDS;
use dc_state::Id;
use gpui::Context;

/// `eJ.Qd` of module 873298: the activity default for servers.
pub(super) const ACTIVITY_STATUS_OFF: u64 = 1;
pub(super) const ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS: u64 = 2;
pub(super) const ACTIVITY_STATUS_ON: u64 = 3;
/// `eJ.KP`: the profile visibility.
pub(super) const FRIENDS_AND_ALL_GUILDS: u64 = 3;
pub(super) const FRIENDS_AND_SMALL_GUILDS: u64 = 2;
pub(super) const FRIENDS_ONLY: u64 = 1;
/// The member count that makes a server large (`n>200` in `p` of module 365258).
const LARGE_GUILD: u64 = 200;

/// `E` of module 365258: how widely a default shares (the OFF value of the restriction shares everywhere).
fn breadth(value: u64) -> i32 {
    match value {
        ACTIVITY_STATUS_OFF => 2,
        ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS => 1,
        ACTIVITY_STATUS_ON => 0,
        _ => -1,
    }
}

/// `_` of module 365258: the activity default that goes with a profile visibility.
pub(super) fn activity_for_profile(profile: u64) -> u64 {
    match profile {
        FRIENDS_AND_SMALL_GUILDS => ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS,
        FRIENDS_ONLY => ACTIVITY_STATUS_ON,
        _ => ACTIVITY_STATUS_OFF,
    }
}

/// An account that never chose a default (`ACTIVITY_STATUS_UNSET`): module 253932's `eX` reads it as null and falls
/// back to `ineligibleDefault`, `ACTIVITY_STATUS_OFF` (Share in all servers, the option the captured page shows
/// selected); GAP: `eligibleDefault` (ON_FOR_LARGE_GUILDS) applies with the GUILD_ACTIVITY_STATUS eligibility, which is
/// not tracked here. An unset profile visibility likewise reads as FRIENDS_AND_ALL_GUILDS (`eN`), which
/// `activity_for_profile` already maps from 0.
fn shown_default(value: u64) -> u64 {
    if value == 0 {
        ACTIVITY_STATUS_OFF
    } else {
        value
    }
}

/// One server as `p` of module 365258 weighs it.
pub(super) struct GuildFact {
    pub id: Id,
    pub members: Option<u64>,
    /// When the account joined, in milliseconds (`joinedAt`).
    pub joined: Option<i64>,
}

/// `p` (`g8`) of module 365258: which servers to change, and whether they stop sharing (`true`, "restricting") or start.
/// `guilds` are in the server list's order, `restricted` is `activity_restricted_guild_ids`.
pub(super) fn affected(
    old: u64,
    new: u64,
    guilds: &[GuildFact],
    restricted: &[Id],
) -> Option<(bool, Vec<Id>)> {
    if old == new {
        return None;
    }
    let (from, to) = (breadth(old), breadth(new));
    if from < 0 || to < 0 {
        return None;
    }
    let restricting = to < from;
    let pair = |a: u64, b: u64| (old == a && new == b) || (old == b && new == a);
    // `large_only`, `small_only`, else every server.
    let large_only = pair(ACTIVITY_STATUS_OFF, ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS);
    let small_only = pair(ACTIVITY_STATUS_ON, ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS);
    let mut list: Vec<&GuildFact> = guilds
        .iter()
        .filter(|guild| restricted.contains(&guild.id) != restricting)
        .filter(|guild| {
            if !large_only && !small_only {
                return true;
            }
            match guild.members {
                None => restricting,
                Some(count) if large_only => count > LARGE_GUILD,
                Some(count) => count <= LARGE_GUILD,
            }
        })
        .collect();
    if list.is_empty() {
        return None;
    }
    // The newest join first; servers without a join date last.
    list.sort_by(|a, b| match (a.joined, b.joined) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) => b.cmp(&a),
    });
    Some((
        restricting,
        list.into_iter().map(|guild| guild.id).collect(),
    ))
}

/// `gS` of module 365258: after the profile visibility went from `old` to `new` while the activity default is
/// `current`, the activity default to offer and the servers it changes.
pub(super) fn profile_prompt(
    old: u64,
    new: u64,
    current: u64,
    guilds: &[GuildFact],
    restricted: &[Id],
) -> Option<(bool, Vec<Id>, u64)> {
    let mapped = activity_for_profile(new);
    if current == mapped
        || (breadth(activity_for_profile(old)) > breadth(mapped))
            != (breadth(current) > breadth(mapped))
    {
        return None;
    }
    let (restricting, guilds) = affected(current, mapped, guilds, restricted)?;
    Some((restricting, guilds, mapped))
}

/// `Xc` of module 365258: the activity default's name, lower case.
pub(super) fn activity_name(value: u64) -> String {
    match value {
        ACTIVITY_STATUS_OFF => dc_intl::t("FzgQna").to_lowercase(),
        ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS => dc_intl::t("1hvuGH").to_lowercase(),
        ACTIVITY_STATUS_ON => dc_intl::t("fQc5la").to_lowercase(),
        _ => String::new(),
    }
}

/// The profile visibility's name, lower case (`a` in `gS`).
pub(super) fn profile_name(value: u64) -> String {
    match value {
        FRIENDS_AND_ALL_GUILDS => dc_intl::t("Boxc8R").to_lowercase(),
        FRIENDS_AND_SMALL_GUILDS => dc_intl::t("YOIKBt").to_lowercase(),
        FRIENDS_ONLY => dc_intl::t("u0nlJv").to_lowercase(),
        _ => String::new(),
    }
}

fn bold_runs(key: &str, setting: &str) -> Vec<(String, bool)> {
    dc_intl::format_runs(key, &[("settingName", setting.into())])
        .into_iter()
        .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
        .collect()
}

/// The dialog of module 32167 (`profile` None) or 413201 (`profile`: the activity default it sets): `ae` / `vz` of
/// module 365258 name its title, subtitle and confirm button; the cancel is Not Now (`X1rGEm`).
pub(super) fn sync_alert(
    restricting: bool,
    guilds: Vec<Id>,
    setting: &str,
    profile: Option<u64>,
) -> Alert {
    let (title, subtitle, confirm) = match (profile.is_some(), restricting) {
        (false, true) => ("jRx1Aa", "Fs96LO", "4DM5HJ"),
        (false, false) => ("S0Y0bh", "GcoYX8", "WRrDtI"),
        (true, true) => ("eYDA7D", "c5/jDc", "6uPZV1"),
        (true, false) => ("9jYwjo", "ajzh8S", "a9PIyD"),
    };
    Alert {
        id: if profile.is_some() {
            "activity-profile-sync"
        } else {
            "activity-default-sync"
        },
        title: dc_intl::t(title).to_owned(),
        subtitle: Some(bold_runs(subtitle, setting)),
        content: Some(Content::AffectedGuilds {
            guilds: guilds.clone(),
            restricting,
            clickable: profile.is_some(),
        }),
        buttons: vec![
            AlertButton {
                id: "activity-sync-not-now",
                label: dc_intl::t("X1rGEm").to_owned(),
                tone: Tone::Secondary,
                then: Then::Close,
            },
            AlertButton {
                id: "activity-sync-confirm",
                label: dc_intl::t(confirm).to_owned(),
                tone: Tone::Primary,
                then: Then::SyncActivity {
                    restricting,
                    guilds,
                    activity: profile,
                },
            },
        ],
    }
}

impl Root {
    /// The servers in the list's order with what `p` of module 365258 reads of them.
    fn activity_guild_facts(&self) -> Vec<GuildFact> {
        self.stores
            .guilds
            .iter()
            .map(|guild| GuildFact {
                id: guild.id,
                members: self.stores.member_counts.get_member_count(guild.id),
                joined: dc_state::scheduled_message::parse_iso(&guild.joined_at)
                    .map(|at| at.timestamp_millis()),
            })
            .collect()
    }

    /// Settings > Activity Privacy's default for servers went from `old` to `new` (`setValue` of module 158935's
    /// `ACTIVITY_PRIVACY_PER_GUILD_DEFAULT_SETTING`).
    pub(super) fn activity_default_changed(&mut self, old: u64, new: u64) {
        let old = shown_default(old);
        let restricted = self.stores.proto_ids(&ACTIVITY_RESTRICTED_GUILDS);
        let Some((restricting, guilds)) =
            affected(old, new, &self.activity_guild_facts(), &restricted)
        else {
            return;
        };
        self.chat_extra.alert = Some(sync_alert(restricting, guilds, &activity_name(new), None));
    }

    /// Share my full profile with went from `old` to `new` (`setValue` of `PROFILE_PRIVACY_SETTING`).
    pub(super) fn profile_privacy_changed(&mut self, old: u64, new: u64) {
        let current = self
            .stores
            .prefs
            .get("activity_sharing_per_guild_default_setting")
            .copied()
            .map_or(ACTIVITY_STATUS_OFF, shown_default);
        let restricted = self.stores.proto_ids(&ACTIVITY_RESTRICTED_GUILDS);
        let Some((restricting, guilds, mapped)) =
            profile_prompt(old, new, current, &self.activity_guild_facts(), &restricted)
        else {
            return;
        };
        self.chat_extra.alert = Some(sync_alert(
            restricting,
            guilds,
            &profile_name(new),
            Some(mapped),
        ));
    }

    /// The confirm of both dialogs: 413201 first sets the activity default, then `gF` of module 365258 adds the servers
    /// to `activity_restricted_guild_ids` (restricting) or takes them out, and the toast says it is done.
    pub(super) fn sync_activity(
        &mut self,
        restricting: bool,
        guilds: &[Id],
        activity: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        if let Some(value) = activity
            && let Some(pref) = dc_proto::settings::PROTO_PREFS
                .iter()
                .find(|pref| pref.key == "activity_sharing_per_guild_default_setting")
        {
            let patch = self.stores.set_pref(pref, value);
            self.send_proto_patch(patch);
        }
        let current = self.stores.proto_ids(&ACTIVITY_RESTRICTED_GUILDS);
        let list: Vec<Id> = if restricting {
            let mut list = current;
            for guild in guilds {
                if !list.contains(guild) {
                    list.push(*guild);
                }
            }
            list
        } else {
            current
                .into_iter()
                .filter(|guild| !guilds.contains(guild))
                .collect()
        };
        self.set_ids(&ACTIVITY_RESTRICTED_GUILDS, &list);
        self.show_success_toast(
            dc_intl::t(if restricting { "AdpgML" } else { "Q7E+QF" }).to_owned(),
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Vec<GuildFact> {
        vec![
            GuildFact {
                id: 1,
                members: Some(5000),
                joined: Some(10),
            },
            GuildFact {
                id: 2,
                members: Some(20),
                joined: Some(30),
            },
            GuildFact {
                id: 3,
                members: None,
                joined: Some(20),
            },
        ]
    }

    #[test]
    fn a_narrower_default_stops_sharing_in_the_servers_that_still_share() {
        // Everywhere to friends only: every sharing server, newest join first.
        assert_eq!(
            affected(ACTIVITY_STATUS_OFF, ACTIVITY_STATUS_ON, &facts(), &[]),
            Some((true, vec![2, 3, 1]))
        );
        // Everywhere to large servers only: the large ones and those of unknown size.
        assert_eq!(
            affected(
                ACTIVITY_STATUS_OFF,
                ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS,
                &facts(),
                &[1]
            ),
            Some((true, vec![3]))
        );
        // Friends only to large servers: the restricted large servers start sharing again.
        assert_eq!(
            affected(
                ACTIVITY_STATUS_ON,
                ACTIVITY_STATUS_ON_FOR_LARGE_GUILDS,
                &facts(),
                &[1, 2]
            ),
            Some((false, vec![2]))
        );
        assert_eq!(
            affected(ACTIVITY_STATUS_ON, ACTIVITY_STATUS_ON, &facts(), &[]),
            None
        );
        assert_eq!(affected(0, ACTIVITY_STATUS_ON, &facts(), &[]), None);
    }

    #[test]
    fn a_profile_change_offers_the_matching_activity_default_once() {
        // Profile to friends only while activity still shares everywhere: stop sharing too.
        assert_eq!(
            profile_prompt(
                FRIENDS_AND_ALL_GUILDS,
                FRIENDS_ONLY,
                ACTIVITY_STATUS_OFF,
                &facts(),
                &[]
            ),
            Some((true, vec![2, 3, 1], ACTIVITY_STATUS_ON))
        );
        // Activity already matches.
        assert_eq!(
            profile_prompt(
                FRIENDS_AND_ALL_GUILDS,
                FRIENDS_ONLY,
                ACTIVITY_STATUS_ON,
                &facts(),
                &[]
            ),
            None
        );
    }

    #[test]
    fn the_dialog_words_follow_the_direction() {
        let alert = sync_alert(true, vec![1], "friends only", None);
        assert_eq!(alert.title, "Turn Off Activity In Your Servers?");
        assert_eq!(alert.buttons[0].label, "Not Now");
        assert_eq!(alert.buttons[1].label, "Turn Off");
        let alert = sync_alert(false, vec![1], "friends & all servers", Some(1));
        assert_eq!(alert.title, "Share activity too?");
        assert_eq!(alert.buttons[1].label, "Share");
        assert_eq!(activity_name(ACTIVITY_STATUS_ON), "friends only");
    }
}
