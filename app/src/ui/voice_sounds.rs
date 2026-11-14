//! Which voice sound an event makes, as the `useSound` hooks of module 596339 decide it (`Sa`, `Sl`, `So`, `Su`, `SE`).
//! These are the rules only: they take the before and after of what changed and name the sound or none. `voice.rs` feeds
//! them from the call and plays the result through [`crate::sounds::play_event`], which checks the sound's switch.

use dc_state::experiments::Assignments;

/// `Si` (module 955283), `2026-09-voice-connect-feedback`: variations 1 and 2 set `rtcConnectionJoinSounds`.
pub(super) const CONNECT_FEEDBACK: &str = "2026-09-voice-connect-feedback";

/// `Si.A.getConfig(..).rtcConnectionJoinSounds`.
pub(super) fn rtc_join_sounds(assignments: &Assignments) -> bool {
    matches!(assignments.variant(CONNECT_FEEDBACK), Some(1 | 2))
}

/// `Sa`: a change of deafened (the first test) wins over a change of muted. Deafening mutes, so "muted" is the effective
/// one (`isSelfMute` includes `isSelfDeaf`).
pub(super) fn toggle_sound(before: (bool, bool), after: (bool, bool)) -> Option<&'static str> {
    let ((was_muted, was_deaf), (muted, deaf)) = (before, after);
    if was_deaf != deaf {
        Some(if deaf { "deafen" } else { "undeafen" })
    } else if was_muted != muted {
        Some(if muted { "mute" } else { "unmute" })
    } else {
        None
    }
}

/// `So`: push to talk went down or up, in push to talk mode, while not muted.
pub(super) fn ptt_sound(held: bool, push_to_talk: bool, muted: bool) -> Option<&'static str> {
    (push_to_talk && !muted).then_some(if held { "ptt_start" } else { "ptt_stop" })
}

/// A stage channel (type 13) has its own sounds; none of the join and leave ones play there (`np.Do`).
const STAGE_VOICE: u64 = 13;

/// Where a voice channel stands for join and leave sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Room {
    /// Kind of channel, as the API numbers it.
    pub kind: u64,
    pub is_afk: bool,
}

impl Room {
    /// `SE` returns no sound in the AFK channel or a stage channel.
    fn is_silent(self) -> bool {
        self.is_afk || self.kind == STAGE_VOICE
    }
}

/// `SE`, without the rtc experiment: the number of others in the channel went up or down. `T <= 25` is the old count.
pub(super) fn count_sound(room: Room, before: usize, after: usize) -> Option<&'static str> {
    if room.is_silent() || before > 25 {
        return None;
    }
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => Some("user_join"),
        std::cmp::Ordering::Less => Some("user_leave"),
        std::cmp::Ordering::Equal => None,
    }
}

/// `SE` with `rtcConnectionJoinSounds`: people who connect to the call, or drop from it, rather than who appear in the
/// channel's voice states. The ones already in the channel when this client connects are not announced as they come
/// up (`expected`).
#[derive(Clone, Debug, Default)]
pub(super) struct RtcTracker {
    connected: bool,
    users: Vec<u64>,
    expected: Vec<u64>,
}

fn difference(a: &[u64], b: &[u64]) -> Vec<u64> {
    a.iter().copied().filter(|id| !b.contains(id)).collect()
}

impl RtcTracker {
    /// `connected`: this client's media connection is up. `users`: who else is connected to it. `in_channel`: who the
    /// voice states place in the channel now.
    pub fn step(
        &mut self,
        room: Room,
        connected: bool,
        users: &[u64],
        in_channel: &[u64],
    ) -> Option<&'static str> {
        let mut expected = std::mem::take(&mut self.expected);
        if connected {
            if !self.connected {
                expected = in_channel.to_vec();
            }
        } else {
            expected.clear();
        }
        let arrived = difference(users, &self.users);
        let joined =
            self.connected && self.users.len() <= 25 && !difference(&arrived, &expected).is_empty();
        let left = self.connected
            && connected
            && self.users.len() <= 25
            && !difference(&self.users, users).is_empty();
        self.expected = difference(&expected, &arrived);
        self.connected = connected;
        self.users = users.to_vec();
        if room.is_silent() {
            return None;
        }
        if joined {
            Some("user_join")
        } else if left {
            Some("user_leave")
        } else {
            None
        }
    }
}

/// `SE`, the stream half: someone in the channel began going live (`stream_started`, ours included), or a stream we send or
/// watch, or ours, ended (`stream_ended`). `held` are the owners of the streams this client has open.
pub(super) fn stream_sound(
    room: Room,
    me: u64,
    before: &[u64],
    after: &[u64],
    held: &[u64],
) -> Option<&'static str> {
    if room.is_silent() {
        return None;
    }
    if !difference(after, before).is_empty() {
        return Some("stream_started");
    }
    difference(before, after)
        .iter()
        .any(|owner| *owner == me || held.contains(owner))
        .then_some("stream_ended")
}

/// `SE`, the viewer half: the single stream's viewers (not counting us) went up or down while at most 25 watched.
pub(super) fn viewer_sound(before: usize, after: usize) -> Option<&'static str> {
    if before > 25 {
        return None;
    }
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => Some("stream_user_joined"),
        std::cmp::Ordering::Less => Some("stream_user_left"),
        std::cmp::Ordering::Equal => None,
    }
}

/// `Su`: the server moved this session to another channel. A change this client made itself has already been applied
/// to the local state when the server answers, so only a different channel counts.
pub(super) fn was_moved(known_channel: u64, update_channel: Option<u64>) -> bool {
    update_channel.is_some_and(|channel| channel != known_channel)
}

/// `Sl`: the sound for the connection coming up, going down. `started`: this attempt has begun; `connected`: media is
/// up. With the experiment the join sound waits for the connection; without it, it plays when connecting starts.
/// Only Guild Stage channels suppress `Sl`; AFK channels still play the local join/disconnect sounds.
pub(super) fn connect_start_sound(rtc_join_sounds: bool, room: Room) -> Option<&'static str> {
    (!rtc_join_sounds && room.kind != STAGE_VOICE).then_some("user_join")
}

pub(super) fn connected_sound(rtc_join_sounds: bool, room: Room) -> Option<&'static str> {
    (rtc_join_sounds && room.kind != STAGE_VOICE).then_some("user_join")
}

/// Media went down after it had come up.
pub(super) fn disconnect_sound(room: Room) -> Option<&'static str> {
    (room.kind != STAGE_VOICE).then_some("disconnect")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROOM: Room = Room {
        kind: 2,
        is_afk: false,
    };

    #[test]
    fn deafen_beats_mute() {
        // (muted, deafened) before and after.
        assert_eq!(toggle_sound((false, false), (true, false)), Some("mute"));
        assert_eq!(toggle_sound((true, false), (false, false)), Some("unmute"));
        // Deafening also mutes: one sound, the deafen one.
        assert_eq!(toggle_sound((false, false), (true, true)), Some("deafen"));
        // Unmuting while deafened undeafens too.
        assert_eq!(toggle_sound((true, true), (false, false)), Some("undeafen"));
        // Undeafening while the mic was muted before leaves it muted.
        assert_eq!(toggle_sound((true, true), (true, false)), Some("undeafen"));
        assert_eq!(toggle_sound((true, true), (true, true)), None);
    }

    #[test]
    fn push_to_talk_sounds_need_the_mode_and_an_open_mic() {
        assert_eq!(ptt_sound(true, true, false), Some("ptt_start"));
        assert_eq!(ptt_sound(false, true, false), Some("ptt_stop"));
        assert_eq!(ptt_sound(true, false, false), None, "voice activity mode");
        assert_eq!(ptt_sound(true, true, true), None, "muted");
    }

    #[test]
    fn joins_and_leaves_follow_the_head_count() {
        assert_eq!(count_sound(ROOM, 1, 2), Some("user_join"));
        assert_eq!(count_sound(ROOM, 2, 1), Some("user_leave"));
        assert_eq!(count_sound(ROOM, 0, 0), None);
        assert_eq!(count_sound(ROOM, 26, 27), None, "a crowd is silent");
        assert_eq!(count_sound(ROOM, 25, 26), Some("user_join"));
        let afk = Room {
            is_afk: true,
            ..ROOM
        };
        let stage = Room { kind: 13, ..ROOM };
        assert_eq!(count_sound(afk, 1, 2), None);
        assert_eq!(count_sound(stage, 1, 2), None);
    }

    #[test]
    fn stream_sounds_follow_who_goes_live_and_who_watches() {
        assert_eq!(
            stream_sound(ROOM, 1, &[], &[2], &[]),
            Some("stream_started")
        );
        assert_eq!(
            stream_sound(ROOM, 1, &[2], &[], &[]),
            None,
            "a stream we hold nothing of"
        );
        assert_eq!(stream_sound(ROOM, 1, &[2], &[], &[2]), Some("stream_ended"));
        assert_eq!(
            stream_sound(ROOM, 1, &[1], &[], &[]),
            Some("stream_ended"),
            "ours"
        );
        assert_eq!(stream_sound(ROOM, 1, &[2], &[2], &[2]), None);
        let afk = Room {
            is_afk: true,
            ..ROOM
        };
        assert_eq!(stream_sound(afk, 1, &[], &[2], &[]), None);
        assert_eq!(viewer_sound(0, 1), Some("stream_user_joined"));
        assert_eq!(viewer_sound(2, 1), Some("stream_user_left"));
        assert_eq!(viewer_sound(1, 1), None);
        assert_eq!(viewer_sound(26, 27), None, "a crowd is silent");
    }

    #[test]
    fn the_rtc_path_announces_who_connects_after_us() {
        let mut tracker = RtcTracker::default();
        // The channel already holds 10 and 11; connecting announces nobody.
        assert_eq!(tracker.step(ROOM, true, &[10], &[10, 11]), None);
        // 11 comes up: it was there when we joined.
        assert_eq!(tracker.step(ROOM, true, &[10, 11], &[10, 11]), None);
        // 12 is new.
        assert_eq!(
            tracker.step(ROOM, true, &[10, 11, 12], &[10, 11, 12]),
            Some("user_join")
        );
        assert_eq!(
            tracker.step(ROOM, true, &[10, 12], &[10, 12]),
            Some("user_leave")
        );
        // Dropping the whole call is not a person leaving.
        assert_eq!(tracker.step(ROOM, false, &[], &[10, 12]), None);
        // A silent room never makes a sound.
        let mut stage = RtcTracker::default();
        let room = Room { kind: 13, ..ROOM };
        stage.step(room, true, &[], &[]);
        assert_eq!(stage.step(room, true, &[5], &[5]), None);
    }

    #[test]
    fn the_connect_sounds_move_with_the_experiment() {
        assert_eq!(connect_start_sound(false, ROOM), Some("user_join"));
        assert_eq!(connected_sound(false, ROOM), None);
        assert_eq!(connect_start_sound(true, ROOM), None);
        assert_eq!(connected_sound(true, ROOM), Some("user_join"));
        assert_eq!(disconnect_sound(ROOM), Some("disconnect"));
        let afk = Room { is_afk: true, ..ROOM };
        assert_eq!(connect_start_sound(false, afk), Some("user_join"));
        assert_eq!(connected_sound(true, afk), Some("user_join"));
        assert_eq!(disconnect_sound(afk), Some("disconnect"));
        let stage = Room { kind: 13, ..ROOM };
        assert_eq!(disconnect_sound(stage), None);
        assert_eq!(connect_start_sound(false, stage), None);
    }

    #[test]
    fn the_experiment_reads_its_variations() {
        let with = |variant: i64| {
            Assignments::from_ready(
                &json!({"assignments": {"1": {"1": {"assignments": [[dc_state::experiments::hash(CONNECT_FEEDBACK), variant, 0, 1]]}}}}),
                "1",
            )
        };
        assert!(!rtc_join_sounds(&Assignments::default()));
        assert!(!rtc_join_sounds(&with(0)));
        assert!(rtc_join_sounds(&with(1)));
        assert!(rtc_join_sounds(&with(2)));
        assert!(!rtc_join_sounds(&with(3)));
    }

    #[test]
    fn only_another_channel_is_a_move() {
        assert!(was_moved(5, Some(6)));
        assert!(!was_moved(5, Some(5)), "our own switch is already applied");
        assert!(!was_moved(5, None), "leaving is not being moved");
    }
}
