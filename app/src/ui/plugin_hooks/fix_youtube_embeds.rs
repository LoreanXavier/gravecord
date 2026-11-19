//! FixYoutubeEmbeds (Vencord `src/plugins/fixYoutubeEmbeds.desktop`, GPL-3.0-or-later): with `youtubeDescription` a
//! video embed shows its description (the pinned client leaves it out): up to 20 characters as it is, longer ones
//! folded to their first 20 and "..." (8px above), a click opening the whole and closing it again. Not part of the
//! pinned client.
//!
//! The plugin's other half sends `Referer: https://media.discordapp.com/` with YouTube's embedded player page, so a
//! blocked video plays in Discord's frame; this client opens no player page (a video embed's play button opens the
//! video), so there is no request to change.

use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashSet;

const ID: &str = "FixYoutubeEmbeds";

thread_local! {
    /// The descriptions opened, by message and embed.
    static OPEN: RefCell<HashSet<(u64, usize)>> = RefCell::new(HashSet::new());
}

/// How a video embed's description shows, if the plugin shows one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum VideoDescription {
    /// Short, or opened: the description as the client draws one.
    Whole,
    /// Folded: its first 20 characters and "...".
    Folded(String),
}

fn folded(description: &str, open: bool) -> Option<VideoDescription> {
    if description.is_empty() {
        return None;
    }
    if open || description.chars().count() <= 20 {
        return Some(VideoDescription::Whole);
    }
    Some(VideoDescription::Folded(format!(
        "{}...",
        description.chars().take(20).collect::<String>()
    )))
}

/// The description of video embed `embed` of `message`, with `youtubeDescription`.
pub(in crate::ui) fn video_description(
    message: u64,
    embed: usize,
    description: &str,
) -> Option<VideoDescription> {
    if crate::plugins::bypassed() {
        return None;
    }
    let on = crate::plugins::with(|state| {
        let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
        state.is_on(ID) && flag("youtubeDescription")
    });
    if !on {
        return None;
    }
    let open = OPEN.with(|set| set.borrow().contains(&(message, embed)));
    folded(description, open)
}

/// A click on the description: it opens or folds.
pub(in crate::ui) fn toggle_video_description(message: u64, embed: usize) {
    OPEN.with(|set| {
        let mut set = set.borrow_mut();
        if !set.remove(&(message, embed)) {
            set.insert((message, embed));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_descriptions_fold_to_twenty_characters() {
        assert_eq!(folded("", false), None);
        assert_eq!(folded("short one", false), Some(VideoDescription::Whole));
        assert_eq!(
            folded("abcdefghijklmnopqrstuvwxyz", false),
            Some(VideoDescription::Folded(
                "abcdefghijklmnopqrst...".to_owned()
            ))
        );
        assert_eq!(
            folded("abcdefghijklmnopqrstuvwxyz", true),
            Some(VideoDescription::Whole)
        );
    }
}
