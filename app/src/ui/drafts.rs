//! The composer's per-channel draft (`DraftStore`, `dc_state::draft`). Module 699344's `handleTextareaChange`
//! dispatches `DRAFT_CHANGE` on every change of the open channel's text; module 84046 saves it with `DRAFT_SAVE`
//! through `throttle(saveCurrentText, 500)` while the text changes, and at once (cancelling the throttle) when the box
//! goes away or the window unloads. Opening a channel reads its draft back (`getDraft` in the form's initial state).
//! Sending clears the box, and the save of the empty text deletes the draft (`clearValue` of module 84046).

use super::Root;
use dc_state::Id;
use dc_state::draft::{DraftType, PERSIST_KEY};
use gpui::{Context, Task};
use std::time::{Duration, Instant};

/// `throttle(this.saveCurrentText, 500)`.
pub(super) const SAVE_THROTTLE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(super) struct ComposerDraft {
    /// The channel and user the composer's text belongs to, once synced.
    synced: Option<(Option<Id>, Option<Id>)>,
    /// The text the store last heard of, so caret moves do not count as changes.
    last_text: String,
    /// When the throttled save last ran.
    last_save: Option<Instant>,
    /// The throttle's trailing call.
    pending: Option<Task<()>>,
}

/// lodash `throttle` with its default leading and trailing edges: `None` saves now, `Some(wait)` waits that long for
/// the trailing save.
pub(super) fn throttle_wait(last_save: Option<Instant>, now: Instant) -> Option<Duration> {
    let elapsed = now.duration_since(last_save?);
    (elapsed < SAVE_THROTTLE).then(|| SAVE_THROTTLE - elapsed)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Root {
    /// Writes the store when it emitted a change.
    pub(super) fn persist_drafts(&mut self) {
        if self.stores.drafts.take_dirty() {
            crate::storage::set(PERSIST_KEY, self.stores.drafts.persisted());
        }
    }

    fn draft_owner(&self) -> Option<Id> {
        match self.composer_draft.synced {
            Some((Some(channel), Some(_))) => Some(channel),
            _ => None,
        }
    }

    /// `saveCurrentText`: `DRAFT_SAVE` of the box's text for the channel it belongs to.
    fn save_current_draft(&mut self, cx: &gpui::App) {
        self.composer_draft.pending = None;
        self.composer_draft.last_save = Some(Instant::now());
        let Some(channel) = self.draft_owner() else {
            return;
        };
        let text = self.composer.read(cx).text().to_owned();
        self.stores
            .drafts
            .save(channel, &text, DraftType::ChannelMessage, None, now_ms());
        self.persist_drafts();
    }

    /// Each frame: when the open channel (or the account) changed, the text so far is saved for the channel it was
    /// typed in, and the box shows the new channel's draft.
    pub(super) fn drafts_frame(&mut self, cx: &mut Context<Self>) {
        let user = self.stores.current_user.as_ref().map(|user| user.id);
        let key = (self.channel, user);
        let previous = self.composer_draft.synced;
        if previous == Some(key) {
            return;
        }
        if previous.is_some_and(|(_, was)| was == user) {
            // The box went away for this channel: save at once, cancelling the throttle.
            self.save_current_draft(cx);
        }
        self.composer_draft.synced = Some(key);
        let current = self.composer.read(cx).text().to_owned();
        let same_channel = previous.is_some_and(|(channel, _)| channel == self.channel);
        let text = if same_channel && !current.is_empty() {
            // Only the account became known (READY after a cached start): what was typed stays.
            current
        } else {
            match key {
                (Some(channel), Some(_)) => self
                    .stores
                    .drafts
                    .get_draft(channel, DraftType::ChannelMessage)
                    .to_owned(),
                _ => String::new(),
            }
        };
        self.composer_draft.last_text = text.clone();
        if self.composer.read(cx).text() != text {
            self.composer.update(cx, |editor, cx| {
                editor.set_text(&text);
                cx.notify();
            });
        }
    }

    /// The composer changed: `DRAFT_CHANGE`, then the throttled save.
    pub(super) fn draft_edited(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.draft_owner() else {
            return;
        };
        // Module 699344: a normal box only reports for the selected channel.
        if self.channel != Some(channel) {
            return;
        }
        let text = self.composer.read(cx).text();
        if text == self.composer_draft.last_text {
            return;
        }
        let text = text.to_owned();
        self.stores
            .drafts
            .change(channel, &text, DraftType::ChannelMessage, None, now_ms());
        self.composer_draft.last_text = text;
        match throttle_wait(self.composer_draft.last_save, Instant::now()) {
            None => self.save_current_draft(cx),
            Some(_) if self.composer_draft.pending.is_some() => {}
            Some(wait) => {
                self.composer_draft.pending = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |this, cx| this.save_current_draft(cx));
                }));
            }
        }
    }

    /// `beforeunload` and unmount: the text is saved at once.
    pub(super) fn flush_draft(&mut self, cx: &gpui::App) {
        if self.draft_owner().is_some() {
            self.save_current_draft(cx);
        }
    }

    /// Channels with a draft, newest first (`getRecentlyEditedDrafts`).
    pub(super) fn draft_channels(&self) -> Vec<Id> {
        self.stores
            .drafts
            .get_recently_edited_drafts(DraftType::ChannelMessage)
            .into_iter()
            .filter(|draft| !draft.draft.trim().is_empty())
            .map(|draft| draft.channel)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_save_throttle_has_a_leading_and_a_trailing_edge() {
        let start = Instant::now();
        assert_eq!(throttle_wait(None, start), None);
        assert_eq!(
            throttle_wait(Some(start), start + Duration::from_millis(200)),
            Some(Duration::from_millis(300))
        );
        assert_eq!(
            throttle_wait(Some(start), start + Duration::from_millis(500)),
            None
        );
    }
}
