//! ClipsEnhancements (Equicord `src/equicordplugins/clipsEnhancements.discordDesktop`, GPL-3.0-or-later): Clips
//! settings offer more lengths (3, 4, 5, 6, 7, 10, 15, 20, 25 and 30 minutes), resolutions (240p, 360p, 2160p, 4320p)
//! and frame rates (45, 90, 120, 144, 165, 240), each list sorted by value (Source last); and a clip of a game the app
//! names without an application is tagged with your own playing activity's application (`richPresenceTagging`: Always,
//! Only when beginning or end of activity name matches, or Never).
//!
//! GAP: `clipsLink`, a button to Clips settings on the plugin's page (the page lists settings only); and the recorder
//! keeps at most 384 MB of video whatever the length, so a long clip at a high bit rate holds its last 384 MB.
//! Not part of the pinned client.

use super::super::Root;
use crate::plugins::State;
use serde_json::{Value, json};

const ID: &str = "ClipsEnhancements";

/// `extraTimeslots`, in minutes.
pub(in crate::ui) const EXTRA_MINUTES: [u64; 10] = [3, 4, 5, 6, 7, 10, 15, 20, 25, 30];
/// `extraResolutions`.
pub(in crate::ui) const EXTRA_HEIGHTS: [u64; 4] = [240, 360, 2160, 4320];
/// `extraFramerates`.
pub(crate) const EXTRA_RATES: [u64; 6] = [45, 90, 120, 144, 165, 240];

/// Whether the extra choices are offered.
pub(crate) fn extras() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// `CLIPS_LENGTH_MINUTES`: "{count} Minutes".
pub(in crate::ui) fn minutes_label(minutes: u64) -> &'static str {
    match minutes {
        3 => "3 Minutes",
        4 => "4 Minutes",
        5 => "5 Minutes",
        6 => "6 Minutes",
        7 => "7 Minutes",
        10 => "10 Minutes",
        15 => "15 Minutes",
        20 => "20 Minutes",
        25 => "25 Minutes",
        _ => "30 Minutes",
    }
}

/// `SCREENSHARE_RESOLUTION_ABBREVIATED`: "{resolution}p".
pub(in crate::ui) fn height_label(height: u64) -> &'static str {
    match height {
        240 => "240p",
        360 => "360p",
        2160 => "2160p",
        _ => "4320p",
    }
}

/// `SCREENSHARE_FPS_ABBREVIATED`: "{fps}FPS".
pub(in crate::ui) fn rate_label(fps: u64) -> &'static str {
    match fps {
        45 => "45FPS",
        90 => "90FPS",
        120 => "120FPS",
        144 => "144FPS",
        165 => "165FPS",
        _ => "240FPS",
    }
}

/// `richPresenceTagging`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tagging {
    Always,
    WhenMatched,
    Never,
}

fn tagging(state: &State) -> Option<Tagging> {
    state
        .is_on(ID)
        .then(|| match state.value(ID, "richPresenceTagging").as_str() {
            Some("always") => Tagging::Always,
            Some("never") => Tagging::Never,
            _ => Tagging::WhenMatched,
        })
}

/// `getApplicationId`: among your playing activities with an application, the one whose name begins or ends as the
/// game's name does; else, unless tagging only on a match, the first.
fn application_for(tagging: Tagging, game: &str, activities: &[Value]) -> Option<String> {
    if tagging == Tagging::Never {
        return None;
    }
    let playing: Vec<&Value> = activities
        .iter()
        .filter(|activity| activity["type"] == json!(0) && activity["application_id"].is_string())
        .collect();
    let words: Vec<&str> = game.split(' ').collect();
    let (first, last) = (
        words.first().copied().unwrap_or(""),
        words.last().copied().unwrap_or(""),
    );
    let matched = playing.iter().find(|activity| {
        let name = activity["name"].as_str().unwrap_or_default();
        name.ends_with(last) || name.starts_with(first)
    });
    if let Some(activity) = matched {
        return activity["application_id"].as_str().map(str::to_owned);
    }
    if tagging != Tagging::WhenMatched {
        return playing
            .first()
            .and_then(|activity| activity["application_id"].as_str().map(str::to_owned));
    }
    None
}

impl Root {
    /// The application a clip of `game` is tagged with: the plugin's choice first, else `detected`.
    pub(in crate::ui) fn clip_application(
        &self,
        game: &str,
        detected: Option<String>,
    ) -> Option<String> {
        if crate::plugins::bypassed() {
            return detected;
        }
        let Some(tagging) = crate::plugins::with(tagging) else {
            return detected;
        };
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let activities: Vec<Value> = me
            .and_then(|me| self.stores.presence.get(me, None))
            .map(|presence| presence.activities.clone())
            .unwrap_or_default();
        application_for(tagging, game, &activities).or(detected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagging_matches_by_the_name_s_ends() {
        let activities = vec![
            json!({"type": 2, "name": "Spotify", "application_id": "1"}),
            json!({"type": 0, "name": "Elden Ring", "application_id": "2"}),
            json!({"type": 0, "name": "Other Game", "application_id": "3"}),
        ];
        assert_eq!(
            application_for(Tagging::WhenMatched, "ELDEN Ring", &activities).as_deref(),
            Some("2")
        );
        assert_eq!(
            application_for(Tagging::WhenMatched, "Unknown", &activities),
            None
        );
        assert_eq!(
            application_for(Tagging::Always, "Unknown", &activities).as_deref(),
            Some("2")
        );
        assert_eq!(
            application_for(Tagging::Never, "Elden Ring", &activities),
            None
        );
    }

    #[test]
    fn labels_follow_the_messages() {
        assert_eq!(minutes_label(10), "10 Minutes");
        assert_eq!(height_label(2160), "2160p");
        assert_eq!(rate_label(144), "144FPS");
        let mut state = State::default();
        assert_eq!(tagging(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(tagging(&state), Some(Tagging::WhenMatched));
    }
}
