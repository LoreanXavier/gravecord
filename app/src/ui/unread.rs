//! Unread state on screen, measured in `design/dom/channel/unread-darker-cozy.json`: the 4 by 8 pill beside an
//! unread channel, the red NEW divider before the first unread message (`---new-messages-bar`), and the blue bar
//! `N new messages since T` with Mark As Read (`newMessagesBar_j9Vvzj`). Reading a channel acks it (`last_viewed`
//! in days since 2015); Mark Unread acks the message before (`manual: true`) and leaves the channel unread.

use super::{Root, color};
use crate::net::Request;
use dc_state::Id;
use dc_state::message_window;
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
        // A window that stops short of the newest message is not the end of the channel: nothing is read there.
        if self.window_detached() || !self.stores.is_unread(channel) {
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

    pub(super) fn mark_current_read(&mut self, cx: &mut Context<Self>) {
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

    /// The blue bar over the messages (`NewMessagesBar`, module 383055, built in module 891522 when
    /// `unreadCount > 0`): shown while the open channel has unread messages, which the NEW divider keeps after the
    /// automatic ack, and while Mark Unread holds one.
    pub(super) fn new_messages_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        let acked = match self.divider_after() {
            Some(acked) => acked,
            None => {
                let state = self.stores.read_states.states.get(&channel)?;
                if !state.manual {
                    return None;
                }
                state.acked
            }
        };
        let list = self.stores.messages.get(&channel)?;
        let ids: Vec<Id> = list.iter().map(|message| message.id).collect();
        let summary = unread_summary(&ids, acked)?;
        let first = list.iter().find(|message| message.id == summary.first)?;
        let local = first.local_time();
        let same_day = local.date_naive() == chrono::Local::now().date_naive();
        let label = bar_text(
            summary.count,
            summary.estimated,
            &super::settings::prefs::clock(&local).to_string(),
            (!same_day).then(|| local.format("%B %-d, %Y").to_string()),
        );
        let first_id = summary.first;
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
                        .id("new-messages-jump")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // `jumpToMessage` to the first unread message.
                            this.scroll_to_message(first_id);
                            cx.notify();
                        }))
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

thread_local! {
    /// Channels whose loaded messages stop before the newest one (`messages.hasMoreAfter`).
    static DETACHED: std::cell::RefCell<std::collections::HashSet<Id>> = std::cell::RefCell::new(Default::default());
}

/// The harness's way to show the bar without a loaded window (`motion older on|off`). The message store's own window
/// (`ChannelWindow::has_more_after`) is what decides in normal use.
pub(super) fn set_window_detached(channel: Id, detached: bool) {
    DETACHED.with(|set| {
        if detached {
            set.borrow_mut().insert(channel);
        } else {
            set.borrow_mut().remove(&channel);
        }
    });
}

/// `messages.hasMoreAfter && ready`: the only state in which the official client draws the bar. Scrolling up inside
/// a window that holds the newest message does not.
pub(super) fn has_more_after(window_detached: bool, scrolled_up: bool) -> bool {
    let _ = scrolled_up;
    window_detached
}

impl Root {
    /// `jumpToPresentBar_j9Vvzj` (`tv` in module 383055, rendered by module 891522 while `hasMoreAfter` and the list
    /// is ready): "You're Viewing Older Messages" with a primary small "Jump To Present" button, 28px above the
    /// composer, centred, 44px tall at least and 284px wide at least. The bar is mounted and unmounted with the
    /// condition; its `.barBase` `transition: opacity .15s` has nothing to run from, so it appears and goes at once.
    pub(super) fn jump_to_present_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel?;
        let state = self.stores.window(channel);
        let forced = DETACHED.with(|set| set.borrow().contains(&channel));
        // `hasMoreAfter && ready`.
        let detached = forced || (state.has_more_after && state.ready);
        if !has_more_after(detached, self.list.is_scrolled_to_end() == Some(false)) {
            return None;
        }
        // A jump from a reply keeps the message the reply was on: the bar offers it (`ks.REPLY`, module 383055).
        let reply = state.jump_return_target_id;
        let (label, button) = match reply {
            Some(_) => ("You Are Viewing a Reply.", "Jump To Original Message"),
            None => ("You're Viewing Older Messages", "Jump To Present"),
        };
        Some(
            div()
                .absolute()
                .bottom(px(28.))
                .left(px(0.))
                .right(px(0.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("jump-to-present-bar")
                        .occlude()
                        .min_h(px(44.))
                        .min_w(px(284.))
                        .py(px(8.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded(px(16.))
                        .border_1()
                        .border_color(color("--border-normal"))
                        .bg(color("--background-surface-highest"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.16),
                            offset: gpui::point(px(0.), px(4.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| match reply {
                            Some(message) => {
                                if let Some(channel) = this.channel {
                                    this.jump_to_message_id(channel, message, true, None);
                                }
                                cx.notify();
                            }
                            None => this.jump_to_present(cx),
                        }))
                        .child(
                            div()
                                .flex_1()
                                .pl(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(label),
                        )
                        .child(div().flex_none().child(super::sticker_popout::small_button(
                            "jump-to-present",
                            button,
                            true,
                        ))),
                )
                .into_any_element(),
        )
    }

    /// `jumpToPresent(channelId, lX.h("renderStream.jumpToPresent"))`: drops the window and loads the newest page,
    /// then the view scrolls to its end.
    pub(super) fn jump_to_present(&mut self, cx: &mut Context<Self>) {
        let page = message_window::page_size(self.viewport.1, false, 16.);
        self.jump_to_present_with(page);
        self.list.scroll_to_end();
        cx.notify();
    }
}

/// What the open channel's unread messages look like to the bar.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct UnreadSummary {
    pub count: usize,
    /// `ReadStateStore.isEstimated`: every loaded message is newer than the ack, so older unread ones may exist.
    pub estimated: bool,
    pub first: Id,
}

/// The loaded message ids (oldest first) after `acked`, or `None` when there is none (`unreadCount` is 0).
pub(super) fn unread_summary(ids: &[Id], acked: Id) -> Option<UnreadSummary> {
    let unread: Vec<Id> = ids.iter().copied().filter(|id| *id > acked).collect();
    let first = *unread.first()?;
    Some(UnreadSummary {
        count: unread.len(),
        estimated: unread.len() == ids.len(),
        first,
    })
}

/// The bar's words: `BctFH/` (`{count} new message(s) since {time}`), `3wXb9P` (the same plus `on {date}` when the
/// first unread message is from another day), and their estimated forms `wvtbbG` and `tHqbtg` (`{count}+ new
/// messages since ...`). The newline the strings hold collapses to a space in the nowrap bar.
pub(super) fn bar_text(count: usize, estimated: bool, time: &str, date: Option<String>) -> String {
    let head = if estimated {
        format!("{count}+ new messages since {time}")
    } else if count == 1 {
        format!("{count} new message since {time}")
    } else {
        format!("{count} new messages since {time}")
    };
    match date {
        Some(date) => format!("{head} on {date}"),
        None => head,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jump_bar_needs_a_window_detached_from_the_newest_message() {
        // Scrolled up with the newest message loaded: no bar.
        assert!(!has_more_after(false, true));
        // The loaded window detached from the newest message: bar, scrolled or not.
        assert!(has_more_after(true, false));
        assert!(has_more_after(true, true));
        set_window_detached(7, true);
        assert!(DETACHED.with(|set| set.borrow().contains(&7)));
        set_window_detached(7, false);
        assert!(!DETACHED.with(|set| set.borrow().contains(&7)));
    }

    #[test]
    fn counts_the_loaded_messages_after_the_ack() {
        assert_eq!(unread_summary(&[1, 2, 3], 3), None);
        let summary = unread_summary(&[1, 2, 3, 4], 2).unwrap();
        assert_eq!(
            (summary.count, summary.estimated, summary.first),
            (2, false, 3)
        );
    }

    #[test]
    fn all_loaded_messages_unread_is_an_estimate() {
        let summary = unread_summary(&[5, 6], 1).unwrap();
        assert!(summary.estimated);
    }

    #[test]
    fn the_text_follows_the_official_strings() {
        assert_eq!(
            bar_text(1, false, "9:05 AM", None),
            "1 new message since 9:05 AM"
        );
        assert_eq!(
            bar_text(3, false, "9:05 AM", None),
            "3 new messages since 9:05 AM"
        );
        assert_eq!(
            bar_text(3, false, "9:05 AM", Some("November 5, 2026".into())),
            "3 new messages since 9:05 AM on November 5, 2026"
        );
        assert_eq!(
            bar_text(50, true, "9:05 AM", None),
            "50+ new messages since 9:05 AM"
        );
        assert_eq!(
            bar_text(50, true, "9:05 AM", Some("November 5, 2026".into())),
            "50+ new messages since 9:05 AM on November 5, 2026"
        );
    }
}
