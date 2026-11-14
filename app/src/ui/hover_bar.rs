//! The message hover bar (`nA`, `np`, `no` and `nx` of module 891522; `popover_DpHsW6` and `hoverBarButton_DpHsW6`
//! of stylesheet 28636, `wrapper_Z0MHyc` of 319714, `container_V_VY9Q` of 870423): which buttons it holds for a
//! message, where it sits, and its look while the pointer is over it.

use super::{Root, Theme, theme};
use dc_state::permissions::{
    ADD_REACTIONS, MANAGE_MESSAGES, READ_MESSAGE_HISTORY, SEND_MESSAGES, VIEW_CHANNEL,
};
use dc_state::{ChannelKind, Id, Message};
use gpui::{Bounds, BoxShadow, Pixels, Point, hsla, point, px};
use std::cell::Cell;

/// Module 448761 `USER_MESSAGE`: the types a person writes.
pub(super) const USER_MESSAGE: [u64; 9] = [0, 19, 20, 23, 26, 41, 45, 47, 68];
/// Module 448761 `REPLYABLE`.
pub(super) const REPLYABLE: [u64; 21] = [
    0, 7, 19, 20, 23, 24, 25, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 45, 46, 69,
];
/// Module 448761 `FORWARDABLE`.
pub(super) const FORWARDABLE: [u64; 5] = [0, 19, 20, 23, 35];
/// Module 448761 `UNDELETABLE`.
pub(super) const UNDELETABLE: [u64; 11] = [1, 2, 3, 4, 5, 21, 35, 56, 57, 64, 68];
/// `MessageTypes.REPLY`.
const REPLY: u64 = 19;
/// `MessageFlags.HAS_THREAD`.
pub(super) const HAS_THREAD: u64 = 1 << 5;
/// `Permissions.CREATE_PUBLIC_THREADS` and `PIN_MESSAGES` (module 375389: bits 35 and 51).
pub(super) const CREATE_PUBLIC_THREADS: u64 = 1 << 35;
pub(super) const PIN_MESSAGES: u64 = 1 << 51;
/// Module 95701 `wE`: the channel types a thread can start in (text, announcement, forum, media).
pub(super) const THREAD_PARENTS: [u64; 4] = [0, 5, 15, 16];

thread_local! {
    /// Shift is held (module 311283: the window's `keydown`, `keyup` and `mousemove` read `shiftKey`).
    static SHIFT: Cell<bool> = const { Cell::new(false) };
    /// The message whose bar was drawn last, and where.
    static BAR: Cell<Option<(Id, Bounds<Pixels>)>> = const { Cell::new(None) };
    static ROW: Cell<Option<(Id, Bounds<Pixels>)>> = const { Cell::new(None) };
}

/// Records where the bar of `message` was laid out this frame.
pub(super) fn record_bar(message: Id, bounds: Bounds<Pixels>) {
    BAR.with(|cell| cell.set(Some((message, bounds))));
}

/// Records where the message whose bar is drawn was laid out this frame.
pub(super) fn record_row(message: Id, bounds: Bounds<Pixels>) {
    ROW.with(|cell| cell.set(Some((message, bounds))));
}

/// An empty overlay that records the box of `message` (`record_row`), or of its bar (`record_bar`).
pub(super) fn probe(message: Id, bar: bool) -> gpui::AnyElement {
    use gpui::{IntoElement, Styled};
    gpui::canvas(
        move |bounds, _, _| {
            if bar {
                record_bar(message, bounds);
            } else {
                record_row(message, bounds);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// The pointer at `at` is inside the box of `message` (as last laid out with its bar).
pub(super) fn row_contains(message: Id, at: Point<Pixels>) -> bool {
    ROW.with(Cell::get)
        .is_some_and(|(owner, bounds)| owner == message && bounds.contains(&at))
}

/// Which message's hover state the pointer at `at` belongs to, for the message `row` whose hover changed: the bar
/// is inside its message (`buttonContainer_2i8Bfm` is a child of `.message`), so where it overlaps the message above,
/// the pointer is still on the message that owns it. `shown` is the message whose bar is drawn now.
pub(super) fn hover_after(
    row: Id,
    inside: bool,
    current: Option<Id>,
    at: Point<Pixels>,
) -> Option<Id> {
    let on_bar = BAR
        .with(Cell::get)
        .filter(|(owner, bounds)| current == Some(*owner) && bounds.contains(&at))
        .map(|(owner, _)| owner);
    hover_rule(row, inside, current, on_bar)
}

/// The rule behind `hover_after`, with `on_bar` the owner of the bar under the pointer.
fn hover_rule(row: Id, inside: bool, current: Option<Id>, on_bar: Option<Id>) -> Option<Id> {
    match on_bar {
        Some(owner) => Some(owner),
        None if inside => Some(row),
        None if current == Some(row) => None,
        None => current,
    }
}

/// Records whether Shift is held; true when that changed.
pub(super) fn set_shift(held: bool) -> bool {
    SHIFT.with(|cell| cell.replace(held) != held)
}

pub(super) fn shift_held() -> bool {
    SHIFT.with(Cell::get)
}

/// `top` of `.container_V_VY9Q` from its message's box: -25px, or -16px for `.isHeader:not(.isReply)`. `isHeader` is
/// a cozy message that starts its group and is a type a person writes; `isReply` a cozy reply that has its reference
/// (`nx` in module 891522).
pub(super) fn bar_top(head: bool, compact: bool, kind: u64, has_reference: bool) -> f32 {
    let header = !compact && head && USER_MESSAGE.contains(&kind);
    let reply = !compact && kind == REPLY && has_reference;
    if header && !reply { -16. } else { -25. }
}

/// What the bar is built from (the hook at the top of `nA`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Caps {
    pub developer: bool,
    pub bookmarked: bool,
    pub reminder: bool,
    pub can_pin: bool,
    pub pinned: bool,
    pub can_start_thread: bool,
    pub can_view_thread: bool,
    pub can_forward: bool,
    pub can_reply: bool,
    pub can_edit: bool,
    pub can_react: bool,
    pub can_publish: bool,
    pub can_delete: bool,
    /// Shift held, not in keyboard mode, and no picker or menu of the bar open.
    pub expanded: bool,
}

/// One button of the bar, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Button {
    CopyId,
    CopyLink,
    MarkUnread,
    Bookmark,
    Pin,
    Thread,
    /// The three quick reactions and the separator after them.
    Quick,
    AddReaction,
    Reply,
    Edit,
    Forward,
    ViewThread,
    Publish,
    Delete,
    More,
}

/// The buttons of `nA`, left to right. The expanded bar (Shift held) leads with the shortcuts and drops the quick
/// reactions; Delete takes More's place when it is offered.
pub(super) fn buttons(caps: Caps) -> Vec<Button> {
    let mut out = Vec::new();
    let x = caps.expanded;
    if x {
        if caps.developer {
            out.push(Button::CopyId);
        }
        out.push(Button::CopyLink);
        out.push(Button::MarkUnread);
        if !caps.reminder {
            out.push(Button::Bookmark);
        }
        if caps.can_pin {
            out.push(Button::Pin);
        }
        if caps.can_start_thread && caps.can_forward {
            out.push(Button::Thread);
        }
        if caps.can_reply && caps.can_edit {
            out.push(Button::Reply);
        }
    }
    if caps.can_react {
        if !x {
            out.push(Button::Quick);
        }
        out.push(Button::AddReaction);
    }
    if caps.can_reply && !caps.can_edit {
        out.push(Button::Reply);
    }
    if caps.can_edit {
        out.push(Button::Edit);
    }
    if caps.can_forward {
        out.push(Button::Forward);
    }
    if caps.can_start_thread && !caps.can_forward {
        out.push(Button::Thread);
    }
    if !caps.can_start_thread && caps.can_view_thread {
        out.push(Button::ViewThread);
    }
    if caps.can_publish {
        out.push(Button::Publish);
    }
    if caps.can_delete && x {
        out.push(Button::Delete);
    } else {
        out.push(Button::More);
    }
    out
}

impl Root {
    /// The pointer is on a message of the group that the row `row_ix` starts (`F` in module 21299 dispatches
    /// `ANIMATE_CHAT_AVATAR` keyed by `<groupId>:<authorId>`, so the group's avatar plays while any of its messages is
    /// hovered).
    pub(super) fn group_hovered(&self, row_ix: usize) -> bool {
        let Some(hovered) = self.hovered else {
            return false;
        };
        if row_ix >= self.rows.len() {
            return false;
        }
        let Some(channel) = self.channel else {
            return false;
        };
        let Some(messages) = self.stores.messages.get(&channel) else {
            return false;
        };
        // A compact row of the group's middle looks back to the row that starts the group.
        let start = self.rows[..=row_ix]
            .iter()
            .rposition(|row| matches!(row, super::Row::Message { head: true, .. }))
            .unwrap_or(row_ix);
        let ids = self.rows.iter().skip(start).map_while(|row| match row {
            super::Row::Message { index, head } => Some((*index, *head)),
            _ => None,
        });
        group_contains(
            ids,
            |index| messages.get(index).map(|message| message.id),
            hovered,
        )
    }

    /// The reaction picker is open on this message: its row stays selected (tint and bar) as while its menu is.
    pub(super) fn reaction_picker_on(&self, message: &Message) -> bool {
        self.picker.as_ref().is_some_and(|open| {
            open.target
                == super::picker::Target::Reaction {
                    channel: message.channel_id,
                    message: message.id,
                }
        })
    }

    /// True while a message context menu or hover-bar reaction picker locks row hover switching.
    pub(super) fn message_hover_locked(&self) -> bool {
        self.menu.is_some()
            || self
                .picker
                .as_ref()
                .is_some_and(|open| matches!(open.target, super::picker::Target::Reaction { .. }))
    }

    /// The hook at the top of `nA` for this message in the channel in view.
    pub(super) fn bar_caps(&self, message: &Message, own: bool) -> Caps {
        let kind = message.kind;
        let user_message = USER_MESSAGE.contains(&kind);
        let saved = self.saved_flags(message.id);
        let channel = self.current_channel();
        let private = channel.is_none() && self.current_dm().is_some();
        let can = |permission: u64| {
            channel.is_some_and(|channel| self.can_in_channel(channel, permission))
        };
        let channel_type = channel.map(|channel| match channel.kind {
            ChannelKind::Text => 0,
            ChannelKind::Voice => 2,
            ChannelKind::Category => 4,
            ChannelKind::Other(other) => other,
        });
        let forum_like = matches!(channel_type, Some(15 | 16));
        // Module 406704 `n` (`g` and `C`).
        let can_start_thread = channel_type.is_some_and(|kind| THREAD_PARENTS.contains(&kind))
            && if forum_like {
                can(SEND_MESSAGES)
            } else {
                can(CREATE_PUBLIC_THREADS | READ_MESSAGE_HISTORY)
            }
            && message.flags & HAS_THREAD == 0
            && user_message;
        // Module 406704 `R`: the thread is known and may be seen.
        let can_view_thread = message.flags & HAS_THREAD != 0
            && self
                .current_guild()
                .is_some_and(|guild| guild.threads.iter().any(|thread| thread.id == message.id))
            && can(VIEW_CHANNEL);
        // Module 492841.
        let can_pin = user_message
            && (private || can(PIN_MESSAGES | READ_MESSAGE_HISTORY))
            && !matches!(channel_type, Some(2 | 13));
        // Module 707985 `disableReactionCreates` with the "Show emoji reactions" setting.
        let can_react =
            self.pref_on("chat_emoji_render_reactions", true) && (private || can(ADD_REACTIONS));
        let can_edit = own && user_message;
        Caps {
            developer: self.pref_on("developer_mode", false),
            bookmarked: saved.bookmarked,
            reminder: saved.reminder,
            can_pin,
            pinned: message.pinned,
            can_start_thread,
            can_view_thread,
            can_forward: FORWARDABLE.contains(&kind),
            can_reply: REPLYABLE.contains(&kind),
            can_edit,
            can_react,
            can_publish: self.can_publish(message),
            can_delete: (own || can(MANAGE_MESSAGES)) && !UNDELETABLE.contains(&kind),
            expanded: false,
        }
    }
}

/// Whether `hovered` is one of the messages of the group whose rows (message index, starts a group) begin at the
/// group's first row.
fn group_contains(
    rows: impl Iterator<Item = (usize, bool)>,
    id_of: impl Fn(usize) -> Option<Id>,
    hovered: Id,
) -> bool {
    rows.enumerate()
        .take_while(|(at, (_, head))| *at == 0 || !*head)
        .any(|(_, (index, _))| id_of(index) == Some(hovered))
}

/// The label and icon of a plain button (`ni.qv` of module 194085). Quick and Publish draw their own.
pub(super) fn label_icon(button: Button, caps: Caps) -> (&'static str, &'static str) {
    match button {
        Button::CopyId => (dc_intl::tr("Copy Message ID"), "menu_id"),
        Button::CopyLink => (dc_intl::tr("Copy Link"), "menu_copy_message_link"),
        Button::MarkUnread => (dc_intl::tr("Mark Unread"), "menu_mark_unread"),
        Button::Bookmark if caps.bookmarked => {
            (dc_intl::tr("Remove Bookmark"), "menu_remove_bookmark")
        }
        Button::Bookmark => (dc_intl::tr("Bookmark Message"), "menu_bookmark_message"),
        Button::Pin if caps.pinned => (dc_intl::tr("Unpin Message"), "menu_pin_message"),
        Button::Pin => (dc_intl::tr("Pin Message"), "menu_pin_message"),
        Button::Thread => (dc_intl::tr("Create Thread"), "menu_create_thread"),
        Button::ViewThread => (dc_intl::tr("View Thread"), "menu_create_thread"),
        Button::AddReaction | Button::Quick => (dc_intl::tr("Add Reaction"), "msg_react"),
        Button::Reply => (dc_intl::tr("Reply"), "menu_reply"),
        Button::Edit => (dc_intl::tr("Edit"), "msg_edit"),
        Button::Forward => (dc_intl::tr("Forward"), "msg_forward"),
        Button::Publish => (dc_intl::tr("Publish"), "announcement_channel"),
        Button::Delete => (dc_intl::tr("Delete"), "msg_trash"),
        Button::More => (dc_intl::tr("More"), "msg_more"),
    }
}

/// The second line of a quick reaction's tooltip (`wunKKA`, `XVx5BN`).
pub(super) fn quick_hint(reacted: bool) -> &'static str {
    if reacted {
        dc_intl::tr("Click to remove")
    } else {
        dc_intl::tr("Click to react")
    }
}

/// `.popover_DpHsW6` wears `--shadow-low`, and `--shadow-medium` while the pointer is over it, through
/// `transition: box-shadow .1s ease-out` (`wrapper_Z0MHyc`). `t` runs from rest (0) to hovered (1); the shadow's
/// parts are interpolated one by one, as CSS does. Dark, darker and midnight: `0 1px 4px 0 / .14` and
/// `0 4px 8px 0 / .16`; light: `.08` for both.
pub(super) fn popover_shadow(t: f32) -> BoxShadow {
    let (low, medium) = if matches!(theme(), Theme::Light) {
        (0.08, 0.08)
    } else {
        (0.14, 0.16)
    };
    let mix = |a: f32, b: f32| a + (b - a) * t;
    BoxShadow {
        color: hsla(0., 0., 0., mix(low, medium)),
        offset: point(px(0.), px(mix(1., 4.))),
        blur_radius: px(mix(4., 8.)),
        spread_radius: px(0.),
        inset: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Caps {
        Caps {
            can_react: true,
            can_reply: true,
            can_forward: true,
            ..Caps::default()
        }
    }

    #[test]
    fn others_messages_offer_reply_and_own_offer_edit() {
        assert_eq!(
            buttons(plain()),
            [
                Button::Quick,
                Button::AddReaction,
                Button::Reply,
                Button::Forward,
                Button::More
            ]
        );
        let own = Caps {
            can_edit: true,
            ..plain()
        };
        assert_eq!(
            buttons(own),
            [
                Button::Quick,
                Button::AddReaction,
                Button::Edit,
                Button::Forward,
                Button::More
            ]
        );
    }

    #[test]
    fn create_thread_shows_only_when_forward_does_not_until_shift() {
        let threadable = Caps {
            can_start_thread: true,
            ..plain()
        };
        assert!(!buttons(threadable).contains(&Button::Thread));
        let no_forward = Caps {
            can_forward: false,
            ..threadable
        };
        assert_eq!(
            buttons(no_forward),
            [
                Button::Quick,
                Button::AddReaction,
                Button::Reply,
                Button::Thread,
                Button::More
            ]
        );
        let viewing = Caps {
            can_view_thread: true,
            ..plain()
        };
        assert!(buttons(viewing).contains(&Button::ViewThread));
    }

    #[test]
    fn shift_expands_with_shortcuts_and_delete_replaces_more() {
        let own = Caps {
            can_edit: true,
            can_pin: true,
            can_start_thread: true,
            can_delete: true,
            developer: true,
            expanded: true,
            ..plain()
        };
        assert_eq!(
            buttons(own),
            [
                Button::CopyId,
                Button::CopyLink,
                Button::MarkUnread,
                Button::Bookmark,
                Button::Pin,
                Button::Thread,
                Button::Reply,
                Button::AddReaction,
                Button::Edit,
                Button::Forward,
                Button::Delete
            ]
        );
        let reminder = Caps {
            reminder: true,
            can_delete: false,
            ..own
        };
        let list = buttons(reminder);
        assert!(!list.contains(&Button::Bookmark));
        assert_eq!(list.last(), Some(&Button::More));
    }

    #[test]
    fn the_bar_sits_16px_up_only_on_a_cozy_group_head_written_by_a_person() {
        assert_eq!(bar_top(true, false, 0, false), -16.);
        assert_eq!(bar_top(false, false, 0, false), -25.);
        assert_eq!(bar_top(true, true, 0, false), -25.);
        // A reply with its reference, and a system line.
        assert_eq!(bar_top(true, false, 19, true), -25.);
        assert_eq!(bar_top(true, false, 19, false), -16.);
        assert_eq!(bar_top(true, false, 7, false), -25.);
    }

    #[test]
    fn the_shadow_runs_from_low_to_medium() {
        let rest = popover_shadow(0.);
        assert_eq!(rest.offset.y, px(1.));
        assert_eq!(rest.blur_radius, px(4.));
        let over = popover_shadow(1.);
        assert_eq!(over.offset.y, px(4.));
        assert_eq!(over.blur_radius, px(8.));
    }

    #[test]
    fn quick_reaction_hint_follows_the_own_reaction() {
        assert_eq!(quick_hint(false), "Click to react");
        assert_eq!(quick_hint(true), "Click to remove");
        assert_eq!(
            label_icon(Button::Bookmark, Caps::default()).0,
            "Bookmark Message"
        );
    }

    #[test]
    fn the_bar_keeps_its_message_hovered_over_the_message_above() {
        // Entering the message above while on the lower message's bar: the lower one stays.
        assert_eq!(hover_rule(1, true, Some(2), Some(2)), Some(2));
        // Leaving the lower message's box onto its bar: it stays.
        assert_eq!(hover_rule(2, false, Some(2), Some(2)), Some(2));
        // Off the bar, the ordinary rules.
        assert_eq!(hover_rule(1, true, Some(2), None), Some(1));
        assert_eq!(hover_rule(2, false, Some(2), None), None);
        assert_eq!(hover_rule(2, false, Some(1), None), Some(1));
        // A stale bar of a message that is no longer hovered counts for nothing.
        record_bar(
            7,
            Bounds::new(point(px(0.), px(0.)), gpui::size(px(10.), px(10.))),
        );
        assert_eq!(
            hover_after(1, true, Some(3), point(px(5.), px(5.))),
            Some(1)
        );
        assert_eq!(
            hover_after(1, true, Some(7), point(px(5.), px(5.))),
            Some(7)
        );
    }

    #[test]
    fn a_group_ends_at_the_next_head() {
        let rows = [(0, true), (1, false), (2, false), (3, true), (4, false)];
        let id_of = |index: usize| Some(100 + index as Id);
        assert!(group_contains(rows.iter().copied(), id_of, 102));
        assert!(group_contains(rows.iter().copied(), id_of, 100));
        assert!(!group_contains(rows.iter().copied(), id_of, 104));
        assert!(group_contains(rows[3..].iter().copied(), id_of, 104));
    }

    #[test]
    fn shift_changes_are_reported_once() {
        assert!(!set_shift(false));
        assert!(set_shift(true));
        assert!(shift_held());
        assert!(!set_shift(true));
        assert!(set_shift(false));
    }
}
