//! Unread state on screen, measured in `design/dom/channel/unread-darker-cozy.json`: the 4 by 8 pill beside an
//! unread channel, the red NEW divider before the first unread message (`---new-messages-bar`), and the blue bar
//! `N new messages since T` with Mark As Read (`newMessagesBar_j9Vvzj`). Reading a channel acks it (`last_viewed`
//! in days since 2015); Mark Unread acks the message before (`manual: true`) and leaves the channel unread.

use super::{Root, color, header_time};
use crate::net::Request;
use dc_state::Id;
use dc_state::read_state::days_since_epoch;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, svg};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

impl Root {
    /// Acks the newest message of the open channel, unless Mark Unread holds it, and keeps where the unread
    /// messages started so the NEW divider stays while the channel is open.
    pub(super) fn read_current_channel(&mut self) {
        if !self.notification_focused {
            return;
        }
        let Some(channel) = self.channel else {
            return;
        };
        self.notification_runtime.clear(channel);
        if !self.stores.is_unread(channel) {
            return;
        }
        let state = self.stores.read_states.states.get(&channel).copied();
        if state.is_some_and(|state| state.manual) {
            return;
        }
        let Some(latest) = self.stores.read_states.latest.get(&channel).copied() else {
            return;
        };
        let joined = self
            .current_guild()
            .map_or(0, dc_state::Guild::joined_snowflake);
        self.unread_from = Some((channel, self.stores.read_states.acked(channel, joined)));
        self.stores.read_states.ack_local(channel, latest, false);
        self.data_revision += 1;
        let _ = self.requests.send(Request::Ack {
            channel,
            message: latest,
            manual: false,
            mention_count: 0,
            last_viewed: days_since_epoch(now_ms()),
        });
    }

    /// Mark Unread: ack the message before `message` by hand, so `message` and those after it read as new.
    pub(super) fn mark_unread(&mut self, channel: Id, message: Id) {
        let before = self
            .stores
            .messages
            .get(&channel)
            .and_then(|list| list.iter().rev().find(|known| known.id < message))
            .map_or(message.saturating_sub(1), |known| known.id);
        self.stores.read_states.ack_local(channel, before, true);
        self.data_revision += 1;
        self.unread_from = Some((channel, before));
        let _ = self.requests.send(Request::Ack {
            channel,
            message: before,
            manual: true,
            mention_count: 0,
            last_viewed: 0,
        });
        self.rebuild_rows();
    }

    fn mark_current_read(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        if let Some(state) = self.stores.read_states.states.get_mut(&channel) {
            state.manual = false;
        }
        self.unread_from = None;
        let latest = self.stores.read_states.latest.get(&channel).copied();
        if let Some(latest) = latest {
            self.stores.read_states.ack_local(channel, latest, false);
            self.data_revision += 1;
            let _ = self.requests.send(Request::Ack {
                channel,
                message: latest,
                manual: false,
                mention_count: 0,
                last_viewed: days_since_epoch(now_ms()),
            });
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// The ack the NEW divider follows in the open channel.
    pub(super) fn divider_after(&self) -> Option<Id> {
        let (channel, acked) = self.unread_from?;
        (Some(channel) == self.channel).then_some(acked)
    }

    /// `---new-messages-bar`: a 1px line in `--background-feedback-notification`, 16px in from the left and 14px
    /// from the right, 8px into the gap above the group, with the NEW pill at its end.
    pub(super) fn new_divider(&self) -> AnyElement {
        let red = color("--background-feedback-notification");
        div()
            .relative()
            .h(px(0.))
            .child(
                div()
                    .absolute()
                    .top(px(8.))
                    .left(px(16.))
                    .right(px(14.))
                    .h(px(1.))
                    .bg(red)
                    .child(
                        div()
                            .absolute()
                            .right(px(0.))
                            .top(px(-6.))
                            .h(px(13.))
                            .flex()
                            .child(
                                svg()
                                    .path("icons/new_pill_cap.svg")
                                    .w(px(8.))
                                    .h(px(13.))
                                    .flex_none()
                                    .text_color(red),
                            )
                            .child(
                                div()
                                    .h(px(13.))
                                    .pl(px(1.))
                                    .pr(px(4.))
                                    .rounded_r(px(4.))
                                    .bg(red)
                                    .text_size(px(10.))
                                    .line_height(px(13.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--white"))
                                    .child("NEW"),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The blue bar over the messages while a channel is held unread.
    pub(super) fn new_messages_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        let state = self.stores.read_states.states.get(&channel)?;
        if !state.manual {
            return None;
        }
        let unread: Vec<&dc_state::Message> = self
            .stores
            .messages
            .get(&channel)?
            .iter()
            .filter(|message| message.id > state.acked)
            .collect();
        let first = unread.first()?;
        let count = unread.len();
        let label = format!(
            "{count} new message{} since {}",
            if count == 1 { "" } else { "s" },
            header_time(first.local_time(), chrono::Local::now())
        );
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left(px(8.))
                .right(px(16.))
                .h(px(34.))
                .pl(px(16.))
                .pr(px(16.))
                .rounded_b(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--brand-500"))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .h(px(32.))
                        .pl(px(4.))
                        .pr(px(12.))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child(label),
                )
                .child(
                    div()
                        .id("mark-as-read")
                        .h(px(26.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--control-secondary-background-hover")))
                        .flex()
                        .items_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .on_click(cx.listener(|this, _, _, cx| this.mark_current_read(cx)))
                        .child("Mark As Read")
                        .child(
                            svg()
                                .path("icons/mark_as_read.svg")
                                .size(px(16.))
                                .ml(px(8.))
                                .text_color(color("--white")),
                        ),
                )
                .into_any_element(),
        )
    }
}
