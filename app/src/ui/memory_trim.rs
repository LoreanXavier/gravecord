//! Lets go of memory nothing needs, on a timer, so a view that was closed or a burst of work gives its memory back to
//! the system instead of keeping it until the next time something draws.

use super::Root;
use gpui::{App, Entity};
use std::time::{Duration, Instant};

/// How often the timer looks.
const PERIOD: Duration = Duration::from_secs(10);
/// The official message store (module 755364 `ChannelMessages.truncate`) shortens a channel that holds more than this
/// many messages to [`KEEP_MESSAGES`] once it is not the open one.
const MESSAGE_LIMIT: usize = 120;
const KEEP_MESSAGES: usize = 60;

/// Starts the timer for `root`, for as long as the view exists.
pub(crate) fn start(root: &Entity<Root>, cx: &mut App) {
    let root = root.downgrade();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PERIOD).await;
            if root.update(cx, |root, _| root.trim_memory()).is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Keeps the newest [`KEEP_MESSAGES`] of a list that is over [`MESSAGE_LIMIT`], oldest first as the store keeps them.
/// Whether it shortened the list.
#[cfg(test)]
fn keep_newest<T>(messages: &mut Vec<T>) -> bool {
    if messages.len() <= MESSAGE_LIMIT {
        return false;
    }
    messages.drain(..messages.len() - KEEP_MESSAGES);
    true
}

impl Root {
    /// Releases what the surfaces hold that no view is using.
    fn trim_memory(&mut self) {
        let now = Instant::now();
        self.sticker_ui.player.borrow_mut().trim_idle(now);
        self.profile_effects.borrow_mut().trim_idle(now);
        self.truncate_closed_channels();
    }

    /// A channel that is not open and holds more than [`MESSAGE_LIMIT`] messages keeps its newest
    /// [`KEEP_MESSAGES`]. Its older messages are fetched again when it is opened and scrolled back.
    fn truncate_closed_channels(&mut self) {
        let open = self.channel;
        // `truncateTop(60)`: the older messages go to the window's cache and `hasMoreBefore` is true again.
        for (channel, messages) in &mut self.stores.messages {
            if Some(*channel) != open && messages.len() > MESSAGE_LIMIT {
                self.stores
                    .windows
                    .entry(*channel)
                    .or_default()
                    .truncate_top(messages, KEEP_MESSAGES);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_list_keeps_its_newest_messages() {
        let mut list: Vec<u32> = (0..130).collect();
        assert!(keep_newest(&mut list));
        assert_eq!(list.len(), KEEP_MESSAGES);
        assert_eq!(list.first(), Some(&70));
        assert_eq!(list.last(), Some(&129));
    }

    #[test]
    fn a_list_at_the_limit_is_left_alone() {
        let mut list: Vec<u32> = (0..MESSAGE_LIMIT as u32).collect();
        assert!(!keep_newest(&mut list));
        assert_eq!(list.len(), MESSAGE_LIMIT);
    }
}
