//! Not in the pinned client: a deleted message of the open channel fades, shrinks and collapses before it goes, as in
//! Discord's native client (`motion/departure.rs` has the motion and where it comes from). Its delete event is held
//! while the ghost runs, so the row is drawn from the message store until then; the event is applied when the ghost
//! ends, through the same path it would have taken at once.

use super::{Root, Row, motion, plugin_hooks};
use dc_state::Id;
use gpui::{AnyElement, Context};
use serde_json::Value;

/// Marks a delete event that has been held already, so applying it does not hold it again.
const RELEASED: &str = "gravecordGhostReleased";

impl Root {
    /// A `MESSAGE_DELETE` that arrives: true when it is held for the ghost of its row. Not held under reduced motion,
    /// while MessageLogger keeps deleted messages, for another channel than the open one, or for a message the
    /// store does not hold.
    pub(super) fn hold_message_delete(&mut self, data: &Value, cx: &mut Context<Self>) -> bool {
        self.release_other_channels(cx);
        if data.get(RELEASED).is_some()
            || data["mlDeleted"] == Value::Bool(true)
            || motion::reduced()
            || plugin_hooks::message_logger_on()
        {
            return false;
        }
        let id_of = |value: &Value| value.as_str().and_then(|id| id.parse::<Id>().ok());
        let (Some(message), Some(channel)) = (id_of(&data["id"]), id_of(&data["channel_id"]))
        else {
            return false;
        };
        let held = self.channel == Some(channel)
            && self
                .stores
                .messages
                .get(&channel)
                .is_some_and(|messages| messages.iter().any(|known| known.id == message));
        if !held || !motion::departure::begin(message, channel, data.clone()) {
            return false;
        }
        // A row that is never drawn (scrolled away, the window hidden) does not end its ghost: the event is applied
        // after a while all the same.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(
                    motion::departure::FALLBACK_MS,
                ))
                .await;
            let _ = this.update(cx, |this, cx| this.release_message_delete(message, cx));
        })
        .detach();
        cx.notify();
        true
    }

    /// Applies the held delete of `message`, if it is still held.
    pub(super) fn release_message_delete(&mut self, message: Id, cx: &mut Context<Self>) {
        let Some(mut data) = motion::departure::take(message) else {
            return;
        };
        data[RELEASED] = Value::Bool(true);
        self.apply_update(
            crate::net::Update::Dispatch {
                name: "MESSAGE_DELETE".to_owned(),
                data,
            },
            cx,
        );
    }

    /// Ghosts held for a channel that is no longer open are applied at once.
    fn release_other_channels(&mut self, cx: &mut Context<Self>) {
        for message in motion::departure::other_channels(self.channel) {
            self.release_message_delete(message, cx);
        }
    }

    /// The row at `index`, drawn as the ghost of its message while that message leaves; a day divider whose only
    /// message is leaving goes with it. Every other row is returned untouched. A ghost that has ended has its delete
    /// applied after this frame.
    pub(super) fn departure_row(
        &self,
        index: usize,
        row: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((message, divider)) = self.leaving_part(index) else {
            return row;
        };
        match motion::departure::frame(message) {
            Some(pose) => {
                motion::keep_animating();
                motion::departure::wrap(message, divider, pose, row)
            }
            None => {
                let root = cx.entity();
                cx.defer(move |cx| {
                    root.update(cx, |this, cx| this.release_message_delete(message, cx));
                });
                row
            }
        }
    }

    /// The leaving message drawn by the row at `index`: the message itself, or the day divider just above a leaving
    /// message that is the only one of its day (`true`).
    fn leaving_part(&self, index: usize) -> Option<(Id, bool)> {
        let message_at = |at: usize| match self.rows.get(at)? {
            Row::Message { index, .. } => self
                .channel
                .and_then(|channel| self.stores.messages.get(&channel))
                .and_then(|messages| messages.get(*index))
                .map(|message| message.id),
            _ => None,
        };
        match self.rows.get(index)? {
            Row::Message { .. } => message_at(index)
                .filter(|id| motion::departure::is_leaving(*id))
                .map(|id| (id, false)),
            Row::Divider(_) => {
                let message =
                    message_at(index + 1).filter(|id| motion::departure::is_leaving(*id))?;
                // Alone in its day: the next row after it is not a message of the same day.
                let alone = !matches!(self.rows.get(index + 2), Some(Row::Message { .. }));
                alone.then_some((message, true))
            }
            _ => None,
        }
    }
}
