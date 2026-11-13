//! The voice widget's rules (module 502812, styles `ieJ4Sq` of `8199095f027905c0.css`).
//!
//! - [`order`]: who comes first when the list is capped (`eN`): the first `max` in channel order, with whoever speaks
//!   and this account moved into view.
//! - [`split`]: the rows drawn and the rest (`ew`): one extra person is drawn rather than "+1 others", more become the
//!   "+N others in voice" pill, and only with Display Users on Always.
//! - Each row: hidden while locked when it only shows speakers and the person is quiet; the name pill hidden while
//!   locked by Display Names; the row's opacity from `voiceUser`, `speaking`, `interactive` and `justConnected`.

use crate::store::{DisplayNameMode, DisplayUserMode};

/// One person in the call, in the channel's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: u64,
    /// Speaking or playing a soundboard sound, and not muted (`u.R` with `checkSoundSharing`, `checkIsMuted`).
    pub speaking: bool,
    pub current: bool,
}

/// `eN`'s ordering: the list as it is when it fits; else the first `max`, with every speaker and this account past
/// them moved in at the end of the visible part, pushing the last visible ones out.
pub fn order(members: &[Member], max: i64) -> Vec<u64> {
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<Member> = members
        .iter()
        .filter(|member| seen.insert(member.id))
        .copied()
        .collect();
    let ids = || unique.iter().map(|member| member.id).collect::<Vec<_>>();
    if max == -1 || unique.len() as i64 <= max {
        return ids();
    }
    let max = max.max(0) as usize;
    let (mut first, mut promoted, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    for (index, member) in unique.iter().enumerate() {
        if index < max {
            first.push(member.id);
        } else if member.speaking || member.current {
            promoted.push(member.id);
        } else {
            rest.push(member.id);
        }
    }
    if promoted.is_empty() {
        return ids();
    }
    let keep = max.saturating_sub(promoted.len()).min(first.len());
    let pushed = first.split_off(keep);
    first
        .into_iter()
        .chain(promoted)
        .chain(pushed)
        .chain(rest)
        .collect()
}

/// What the widget draws of an ordered list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub rows: Vec<u64>,
    /// The people behind the "+N others in voice" pill (two or more).
    pub others: Vec<u64>,
}

/// `ew`: the first `max` rows (all with -1); past them one more row, or the pill.
pub fn split(ordered: &[u64], max: i64, users: DisplayUserMode) -> Split {
    if max < 0 {
        return Split {
            rows: ordered.to_vec(),
            others: Vec::new(),
        };
    }
    let cut = (max as usize).min(ordered.len());
    let mut rows = ordered[..cut].to_vec();
    let hidden = &ordered[cut..];
    let mut others = Vec::new();
    if users == DisplayUserMode::Always {
        match hidden.len() {
            0 => {}
            1 => rows.push(hidden[0]),
            _ => others = hidden.to_vec(),
        }
    }
    Split { rows, others }
}

/// `y` of `ew`: the list is drawn when there is someone, a channel, and the overlay is unlocked or the widget pinned.
pub fn widget_shown(has_states: bool, in_channel: bool, locked: bool, pinned: bool) -> bool {
    has_states && in_channel && (!locked || pinned)
}

/// Unlocked with nobody to list: "Connect to a voice channel to see a list of users here." (`O.g`).
pub fn shows_empty_state(drawn: bool, locked: bool, show_empty: bool) -> bool {
    !drawn && !locked && show_empty
}

/// `ey`'s first check: a quiet person is left out while locked (or in the settings preview) when only speakers show.
pub fn row_shown(locked_or_preview: bool, users: DisplayUserMode, speaking: bool) -> bool {
    !(users == DisplayUserMode::OnlyWhileSpeaking && locked_or_preview && !speaking)
}

/// The name pill: hidden while locked by Never, or by Only While Speaking for someone quiet.
pub fn name_shown(
    locked_or_preview: bool,
    names: DisplayNameMode,
    speaking_or_sounds: bool,
) -> bool {
    !(locked_or_preview
        && (names == DisplayNameMode::Never
            || (!speaking_or_sounds && names == DisplayNameMode::OnlyWhileSpeaking)))
}

/// The row's opacity: `.voiceUser` .4, `.justConnected` .6, `.speaking`/`.playingSoundboard` .9, and
/// `.interactive` (unlocked) 1 over everything (`!important`). Later rules of the sheet win at equal weight.
pub fn row_opacity(locked: bool, speaking_or_sounds: bool, just_connected: bool) -> f32 {
    if !locked {
        return 1.;
    }
    if just_connected {
        0.6
    } else if speaking_or_sounds {
        0.9
    } else {
        0.4
    }
}

/// Row geometry from the stylesheet: rows `gap` 8 (`--space-8`), the name pill 24 high with a 4px start margin, 8px
/// side padding and 1px border, the voice icons 16px with 4px between them.
pub const ROW_GAP: f32 = 8.;
pub const PILL_HEIGHT: f32 = 24.;
pub const PILL_MARGIN: f32 = 4.;
pub const PILL_PADDING_X: f32 = 8.;
pub const ICON: f32 = 16.;
pub const ICON_GAP: f32 = 4.;

/// The list's height for `rows` rows of an avatar size (and the pill, when there is one).
pub fn list_height(rows: usize, avatar: f32, pill: bool) -> f32 {
    let count = rows + usize::from(pill);
    if count == 0 {
        return 0.;
    }
    let pill_height = if pill { avatar.max(PILL_HEIGHT) } else { 0. };
    rows as f32 * avatar + pill_height + (count - 1) as f32 * ROW_GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet(id: u64) -> Member {
        Member {
            id,
            speaking: false,
            current: false,
        }
    }

    #[test]
    fn speakers_and_self_are_moved_into_view() {
        let mut members: Vec<Member> = (1..=6).map(quiet).collect();
        assert_eq!(order(&members, -1), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(order(&members, 6), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(order(&members, 3), vec![1, 2, 3, 4, 5, 6]);
        members[4].speaking = true;
        members[5].current = true;
        // Two promoted into a list of three: one of the first three stays.
        assert_eq!(order(&members, 3), vec![1, 5, 6, 2, 3, 4]);
        // A duplicate id counts once.
        members.push(quiet(1));
        assert_eq!(order(&members, 3).len(), 6);
    }

    #[test]
    fn one_extra_is_drawn_more_become_the_pill() {
        let ids: Vec<u64> = (1..=10).collect();
        let always = split(&ids, 9, DisplayUserMode::Always);
        assert_eq!(always.rows.len(), 10);
        assert!(always.others.is_empty());
        let capped = split(&ids, 8, DisplayUserMode::Always);
        assert_eq!(capped.rows, (1..=8).collect::<Vec<_>>());
        assert_eq!(capped.others, vec![9, 10]);
        let speakers = split(&ids, 8, DisplayUserMode::OnlyWhileSpeaking);
        assert_eq!(speakers.rows.len(), 8);
        assert!(speakers.others.is_empty());
        assert_eq!(split(&ids, -1, DisplayUserMode::Always).rows.len(), 10);
    }

    #[test]
    fn locked_rules_follow_the_modes() {
        assert!(widget_shown(true, true, true, true));
        assert!(!widget_shown(true, true, true, false));
        assert!(widget_shown(true, true, false, false));
        assert!(!widget_shown(false, true, false, true));
        assert!(shows_empty_state(false, false, true));
        assert!(!shows_empty_state(false, true, true));
        assert!(!row_shown(true, DisplayUserMode::OnlyWhileSpeaking, false));
        assert!(row_shown(false, DisplayUserMode::OnlyWhileSpeaking, false));
        assert!(row_shown(true, DisplayUserMode::OnlyWhileSpeaking, true));
        assert!(!name_shown(true, DisplayNameMode::Never, true));
        assert!(name_shown(false, DisplayNameMode::Never, false));
        assert!(!name_shown(true, DisplayNameMode::OnlyWhileSpeaking, false));
        assert!(name_shown(true, DisplayNameMode::OnlyWhileSpeaking, true));
        assert_eq!(row_opacity(false, false, false), 1.);
        assert_eq!(row_opacity(true, false, false), 0.4);
        assert_eq!(row_opacity(true, true, false), 0.9);
        assert_eq!(row_opacity(true, true, true), 0.6);
    }

    #[test]
    fn the_preview_list_is_152_high() {
        // The settings preview: four rows of 32px, 8px apart (`voiceUserContainer_ieJ4Sq`, 152px in the capture).
        assert_eq!(list_height(4, 32., false), 152.);
        assert_eq!(list_height(0, 32., false), 0.);
        assert_eq!(list_height(2, 24., true), 2. * 24. + 24. + 16.);
    }
}
