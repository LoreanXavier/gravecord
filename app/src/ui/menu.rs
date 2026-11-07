//! Context menu and confirm modal. Items, sizes and colors are from `dumps/menus/message-own.json` and the delete modal capture.

use super::{Root, color, parse_block_text};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, Pixels, Point, SharedString,
    Window, anchored, deferred, div, prelude::*, px,
};

/// The official menu keeps 8px from the window edge and clamps instead of flipping.
const EDGE_MARGIN: f32 = 8.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    Edit,
    Reply,
    CopyText,
    CopyLink,
    Delete,
    /// Listed by the official menu, not built yet. Shown so the menu keeps its shape, and does nothing.
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    Entry(&'static str, Action),
    Danger(&'static str, Action),
    Separator,
}

/// `message` menu order from the capture; `own` adds the author's entries.
pub(super) fn message_items(own: bool) -> Vec<Item> {
    let mut items = vec![
        Item::Entry("Add Reaction", Action::Pending),
        Item::Separator,
    ];
    if own {
        items.push(Item::Entry("Edit Message", Action::Edit));
    }
    items.extend([
        Item::Entry("Reply", Action::Reply),
        Item::Entry("Forward", Action::Pending),
        Item::Entry("Create Thread", Action::Pending),
        Item::Separator,
        Item::Entry("Copy Text", Action::CopyText),
        Item::Entry("Pin Message", Action::Pending),
        Item::Entry("Bookmark Message", Action::Pending),
        Item::Entry("Create Reminder", Action::Pending),
        Item::Entry("Apps", Action::Pending),
        Item::Entry("Mark Unread", Action::Pending),
        Item::Entry("Copy Message Link", Action::CopyLink),
        Item::Entry("Speak Message", Action::Pending),
    ]);
    if own {
        items.push(Item::Separator);
        items.push(Item::Danger("Delete Message", Action::Delete));
    }
    items
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OpenMenu {
    pub position: Point<Pixels>,
    pub message: Id,
    pub own: bool,
}

impl Root {
    fn run_action(
        &mut self,
        action: Action,
        message: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        let found = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|known| known.id == message))
            .cloned();
        let Some(found) = found else {
            return;
        };
        match action {
            Action::Edit => self.begin_edit(message, &found.content, window, cx),
            Action::Reply => self.begin_reply(found, window, cx),
            Action::CopyText => {
                cx.write_to_clipboard(ClipboardItem::new_string(parse_block_text(&found.content)))
            }
            Action::CopyLink => {
                let guild = self
                    .guild
                    .map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
                let link = format!(
                    "https://discord.com/channels/{guild}/{}/{message}",
                    found.channel_id
                );
                cx.write_to_clipboard(ClipboardItem::new_string(link));
            }
            Action::Delete => self.confirm_delete = Some(message),
            Action::Pending => {}
        }
        cx.notify();
    }

    fn menu_row(
        &self,
        index: usize,
        label: &'static str,
        tint: &str,
        run: Action,
        target: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint = color(tint);
        let hover_bg = color("--background-mod-subtle");
        div()
            .id(SharedString::from(format!("menu-{index}")))
            .h(px(36.))
            .px(px(8.))
            .rounded(px(2.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(tint)
            .hover(move |style| style.bg(hover_bg))
            .on_click(
                cx.listener(move |this, _, window, cx| this.run_action(run, target, window, cx)),
            )
            .child(label)
            .into_any_element()
    }

    pub(super) fn context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let open = self.menu?;
        let rows: Vec<AnyElement> = message_items(open.own)
            .into_iter()
            .enumerate()
            .map(|(index, item)| match item {
                Item::Entry(label, run) => self.menu_row(
                    index,
                    label,
                    "--interactive-text-default",
                    run,
                    open.message,
                    cx,
                ),
                Item::Danger(label, run) => self.menu_row(
                    index,
                    label,
                    "--text-feedback-critical",
                    run,
                    open.message,
                    cx,
                ),
                Item::Separator => div()
                    .my(px(4.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle"))
                    .into_any_element(),
            })
            .collect();
        Some(deferred(
            anchored()
                .position(open.position)
                .snap_to_window_with_margin(px(EDGE_MARGIN))
                .child(
                    div()
                        .id("context-menu")
                        .occlude()
                        .w(px(222.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--background-mod-strong"))
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .children(rows),
                ),
        ))
    }

    pub(super) fn delete_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let target = self.confirm_delete?;
        let button = |id: &'static str, label: &'static str, background: gpui::Rgba| {
            div()
                .id(id)
                .w(px(211.))
                .h(px(40.))
                .rounded(px(8.))
                .bg(background)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .font_weight(FontWeight(500.))
                .line_height(px(20.))
                .text_color(color("--white"))
                .child(label)
        };
        Some(deferred(
            div()
                .id("modal-layer")
                .occlude()
                .absolute()
                .inset_0()
                .bg(gpui::rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(478.))
                        .rounded(px(12.))
                        .bg(color("--modal-background"))
                        .border_1()
                        .border_color(color("--background-mod-subtle"))
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child("Delete Message"),
                        )
                        .child(
                            div()
                                .mt(px(8.))
                                .line_height(px(20.))
                                .text_color(color("--interactive-text-default"))
                                .child("Are you sure you want to delete this message?"),
                        )
                        .child(
                            div()
                                .mt(px(24.))
                                .flex()
                                .justify_between()
                                .child(
                                    button(
                                        "modal-cancel",
                                        "Cancel",
                                        color("--background-mod-subtle"),
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.confirm_delete = None;
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    button("modal-delete", "Delete", gpui::rgba(0xd22d39ff))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(channel) = this.channel {
                                                let _ = this.requests.send(Request::Delete {
                                                    channel,
                                                    message: target,
                                                });
                                            }
                                            this.confirm_delete = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                ),
        ))
    }

    pub(super) fn open_menu(
        &mut self,
        position: Point<Pixels>,
        message: Id,
        own: bool,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(OpenMenu {
            position,
            message,
            own,
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Item, message_items};

    fn labels(own: bool) -> Vec<&'static str> {
        message_items(own)
            .into_iter()
            .filter_map(|item| match item {
                Item::Entry(label, _) | Item::Danger(label, _) => Some(label),
                Item::Separator => None,
            })
            .collect()
    }

    #[test]
    fn own_messages_get_the_captured_order() {
        assert_eq!(
            labels(true),
            [
                "Add Reaction",
                "Edit Message",
                "Reply",
                "Forward",
                "Create Thread",
                "Copy Text",
                "Pin Message",
                "Bookmark Message",
                "Create Reminder",
                "Apps",
                "Mark Unread",
                "Copy Message Link",
                "Speak Message",
                "Delete Message"
            ]
        );
    }

    #[test]
    fn other_peoples_messages_have_no_edit_or_delete() {
        let items = labels(false);
        assert!(!items.contains(&"Edit Message"));
        assert!(!items.contains(&"Delete Message"));
    }

    #[test]
    fn delete_is_the_only_danger_item() {
        let danger: Vec<Item> = message_items(true)
            .into_iter()
            .filter(|item| matches!(item, Item::Danger(..)))
            .collect();
        assert_eq!(danger, [Item::Danger("Delete Message", Action::Delete)]);
    }
}
