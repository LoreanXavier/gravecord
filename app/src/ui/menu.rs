//! The message menu (`message-actions`, module 761304) and the delete modal. Rows, icons, sizes and colors are from
//! `design/dom/menus/message-more-darker-cozy.json` (opened from the hover bar's More) and the right-click capture.

use super::{Root, color, parse_block_text};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, Hsla, IntoElement, Pixels, Point, SharedString,
    Window, anchored, deferred, div, img, prelude::*, px, svg,
};

/// A right-clicked menu keeps 8px from the window edge and clamps instead of flipping.
const EDGE_MARGIN: f32 = 8.;
/// The hover bar's menu sits 8px left of its More button, top-aligned, and keeps 12px from the window's bottom.
const MORE_GAP: f32 = 8.;
const MORE_EDGE: f32 = 12.;
const MENU_WIDTH: f32 = 222.;

/// Reaction emoji the quick row never offers (`eP` in module 761304), when drawn from the sprite sheet.
const NOT_QUICK: [&str; 5] = [
    "slight_smile",
    "frowning",
    "smile",
    "stuck_out_tongue",
    "wink",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    AddReaction,
    Edit,
    Reply,
    Forward,
    Thread,
    CopyText,
    Pin,
    Bookmark,
    Reminder,
    Apps,
    MarkUnread,
    CopyLink,
    Speak,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    /// Label, icon file, action, and whether it opens a submenu (a caret on the right).
    Entry(&'static str, &'static str, Action, bool),
    Danger(&'static str, &'static str, Action),
    Separator,
}

/// `message` menu order from the capture; `own` adds the author's entries.
pub(super) fn message_items(own: bool) -> Vec<Item> {
    let mut items = vec![
        Item::Entry(
            "Add Reaction",
            "menu_add_reaction",
            Action::AddReaction,
            true,
        ),
        Item::Separator,
    ];
    if own {
        items.push(Item::Entry(
            "Edit Message",
            "menu_edit_message",
            Action::Edit,
            false,
        ));
    }
    items.extend([
        Item::Entry("Reply", "menu_reply", Action::Reply, false),
        Item::Entry("Forward", "menu_forward", Action::Forward, false),
        Item::Entry("Create Thread", "menu_create_thread", Action::Thread, false),
        Item::Separator,
        Item::Entry("Copy Text", "menu_copy_text", Action::CopyText, false),
        Item::Entry("Pin Message", "menu_pin_message", Action::Pin, false),
        Item::Entry(
            "Bookmark Message",
            "menu_bookmark_message",
            Action::Bookmark,
            false,
        ),
        Item::Entry(
            "Create Reminder",
            "menu_create_reminder",
            Action::Reminder,
            true,
        ),
        Item::Entry("Apps", "menu_apps", Action::Apps, true),
        Item::Entry("Mark Unread", "menu_mark_unread", Action::MarkUnread, false),
        Item::Entry(
            "Copy Message Link",
            "menu_copy_message_link",
            Action::CopyLink,
            false,
        ),
        Item::Entry("Speak Message", "menu_speak_message", Action::Speak, false),
    ]);
    if own {
        items.push(Item::Separator);
        items.push(Item::Danger(
            "Delete Message",
            "menu_delete_message",
            Action::Delete,
        ));
    }
    items
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Anchor {
    /// Right-click: the menu's corner at the pointer.
    Pointer(Point<Pixels>),
    /// The hover bar's More button: its left edge and top.
    MoreButton(Point<Pixels>),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OpenMenu {
    pub anchor: Anchor,
    pub message: Id,
    pub own: bool,
}

impl Root {
    /// The quick reaction row: the four most frecent reactions (`Z.D6`), unicode emoji from the dataset.
    pub(super) fn quick_reactions(&self) -> Vec<&'static dc_tokens::emoji_data::Emoji> {
        self.frequent_reactions()
            .into_iter()
            .filter_map(|name| dc_tokens::emoji_by_name(&name))
            .filter(|emoji| !(emoji.sprite.is_some() && NOT_QUICK.contains(&emoji.names[0])))
            .take(4)
            .collect()
    }

    fn run_action(
        &mut self,
        action: Action,
        message: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_menu = self.menu.take();
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
            other => self.run_message_action(other, found, window, cx),
        }
        cx.notify();
    }

    fn run_unconfirmed(&mut self, action: Action, message: Id, cx: &mut Context<Self>) {
        self.menu = None;
        if let Some(channel) = self.channel {
            let request = match action {
                Action::Delete => Request::Delete { channel, message },
                _ => Request::Pin {
                    channel,
                    message,
                    pin: true,
                },
            };
            let _ = self.requests.send(request);
        }
        cx.notify();
    }

    fn react_from_menu(&mut self, message: Id, surrogates: &'static str, cx: &mut Context<Self>) {
        self.menu = None;
        self.track_reaction(surrogates);
        if let Some(channel) = self.channel {
            let _ = self.requests.send(Request::React {
                channel,
                message,
                emoji: surrogates.to_owned(),
                add: true,
                location: "Message Context Menu",
                burst: false,
            });
        }
        cx.notify();
    }

    fn menu_row(&self, index: usize, item: Item, target: Id, cx: &mut Context<Self>) -> AnyElement {
        let (label, icon_name, run, caret, danger) = match item {
            Item::Entry(label, icon_name, run, caret) => (label, icon_name, run, caret, false),
            Item::Danger(label, icon_name, run) => (label, icon_name, run, false, true),
            Item::Separator => unreachable!("separators are drawn apart"),
        };
        let group = SharedString::from(format!("menu-row-{index}"));
        let (icon_tint, label_tint, hover_icon): (Hsla, Hsla, Hsla) = if danger {
            let critical: Hsla = color("--text-feedback-critical").into();
            (critical, critical, critical)
        } else {
            (
                color("--interactive-text-default").into(),
                color("--text-strong").into(),
                color("--interactive-text-active").into(),
            )
        };
        let hover_bg = if danger {
            color("--background-feedback-mod-critical")
        } else {
            color("--interactive-background-hover")
        };
        div()
            .id(SharedString::from(format!("menu-{index}")))
            .group(group.clone())
            .h(px(36.))
            .p(px(8.))
            .rounded(px(2.))
            .flex()
            .items_center()
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg).rounded(px(4.)))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    // Shift skips the confirmation of Delete and Pin, as the protip says.
                    if event.modifiers().shift && matches!(run, Action::Delete | Action::Pin) {
                        this.run_unconfirmed(run, target, cx);
                    } else {
                        this.run_action(run, target, window, cx);
                    }
                }),
            )
            .child(
                svg()
                    .path(format!("icons/{icon_name}.svg"))
                    .size(px(20.))
                    .flex_none()
                    .mr(px(8.))
                    .text_color(icon_tint)
                    .group_hover(group.clone(), move |style| style.text_color(hover_icon)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(label_tint)
                    .child(label),
            )
            .when(caret, |this| {
                this.child(
                    svg()
                        .path("icons/menu_caret.svg")
                        .size(px(20.))
                        .flex_none()
                        .ml(px(8.))
                        .text_color(color("--text-muted"))
                        .group_hover(group, |style| style.text_color(color("--white"))),
                )
            })
            .into_any_element()
    }

    fn quick_row(&self, message: Id, cx: &mut Context<Self>) -> Option<AnyElement> {
        let reactions = self.quick_reactions();
        if reactions.is_empty() {
            return None;
        }
        Some(
            div()
                .p(px(8.))
                .flex()
                .gap(px(4.))
                .children(reactions.into_iter().enumerate().map(|(index, emoji)| {
                    let picture = self
                        .images
                        .get(&format!("emoji:{}", emoji.surrogates))
                        .cloned();
                    div()
                        .id(SharedString::from(format!("quickreact-{index}")))
                        .size(px(44.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-subtle"))
                        .hover(|style| style.bg(color("--background-mod-normal")))
                        .active(|style| style.bg(color("--background-mod-muted")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(picture.map(|picture| img(picture).size(px(20.))))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.react_from_menu(message, emoji.surrogates, cx)
                        }))
                }))
                .into_any_element(),
        )
    }

    pub(super) fn context_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let open = self.menu?;
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.extend(self.quick_row(open.message, cx));
        for (index, item) in message_items(open.own).into_iter().enumerate() {
            rows.push(match item {
                Item::Separator => div()
                    .my(px(8.))
                    .h(px(1.))
                    .bg(color("--border-subtle"))
                    .into_any_element(),
                item => self.menu_row(index, item, open.message, cx),
            });
        }
        let (position, margin) = match open.anchor {
            Anchor::Pointer(at) => (at, EDGE_MARGIN),
            Anchor::MoreButton(at) => {
                let estimate = self.menu_height(open.own);
                let bottom_room = f32::from(window.viewport_size().height) - MORE_EDGE;
                let top = f32::from(at.y).min(bottom_room - estimate).max(MORE_EDGE);
                (
                    gpui::point(at.x - px(MORE_GAP + MENU_WIDTH), px(top)),
                    MORE_EDGE,
                )
            }
        };
        let max_height = f32::from(window.viewport_size().height) - 32.;
        Some(deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(margin))
                .child(
                    div()
                        .parity_with_border("message-menu", 1.)
                        .id("context-menu")
                        .occlude()
                        .w(px(MENU_WIDTH))
                        .max_h(px(max_height))
                        .rounded(px(8.))
                        .bg(color("--background-surface-higher"))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.24),
                            offset: gpui::point(px(0.), px(12.)),
                            blur_radius: px(24.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.menu = None;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .id("context-menu-scroller")
                                .overflow_y_scroll()
                                .max_h(px(max_height - 2.))
                                .py(px(8.))
                                .px(px(8.))
                                .flex()
                                .flex_col()
                                .children(rows),
                        ),
                ),
        ))
    }

    /// The menu's height before layout, for placing the hover bar's menu: rows of 36, separators of 17, the quick row
    /// of 60, 16 of padding and 2 of border.
    fn menu_height(&self, own: bool) -> f32 {
        let items = message_items(own);
        let rows = items
            .iter()
            .filter(|item| !matches!(item, Item::Separator))
            .count() as f32;
        let separators = items.len() as f32 - rows;
        let quick = if self.quick_reactions().is_empty() {
            0.
        } else {
            60.
        };
        rows * 36. + separators * 17. + quick + 16. + 2.
    }

    /// "Delete Message" (`AMvpS4`): the message, the shift protip (`2Ld8DM`, `IxY7E6`), Cancel and a critical Delete.
    /// Measured in `design/dom/modals/delete-message-darker-cozy.json`.
    pub(super) fn delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let target = self.confirm_delete?;
        let message = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|messages| messages.iter().find(|known| known.id == target))
            .cloned()?;
        let tip_spans = dc_markdown::parse(
            "You can hold down shift when clicking **delete message** to bypass this confirmation entirely.",
        );
        let tip_pieces = tip_spans
            .iter()
            .flat_map(|block| super::block_spans(block).to_vec())
            .map(|span| self.piece(&span, color("--text-default").into(), 14., 400., 16.))
            .collect();
        let body = div()
            .flex()
            .flex_col()
            .child(self.message_preview(&message))
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-feedback-positive"))
                            .child("Protip:"),
                    )
                    .child(crate::inline::inline(tip_pieces, px(16.))),
            )
            .into_any_element();
        let channel = message.channel_id;
        Some(super::modal::render(
            super::modal::Spec {
                id: "delete-modal",
                title: "Delete Message".into(),
                subtitle: div()
                    .child("Are you sure you want to delete this message?")
                    .into_any_element(),
                body: Some(body),
                buttons: [
                    super::modal::button(
                        "delete-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| this.confirm_delete = None,
                    ),
                    super::modal::button(
                        "delete-confirm",
                        "Delete",
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            this.confirm_delete = None;
                            let _ = this.requests.send(Request::Delete {
                                channel,
                                message: target,
                            });
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.confirm_delete = None),
            },
            cx,
        ))
    }

    pub(super) fn open_menu(
        &mut self,
        anchor: Anchor,
        message: Id,
        own: bool,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(OpenMenu {
            anchor,
            message,
            own,
        });
        self.request_quick_reaction_images();
        cx.notify();
    }

    fn request_quick_reaction_images(&mut self) {
        let wanted: Vec<&'static str> = self
            .quick_reactions()
            .into_iter()
            .map(|emoji| emoji.surrogates)
            .collect();
        for surrogates in wanted {
            self.request_emoji_image(surrogates);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Item, message_items};

    fn labels(own: bool) -> Vec<&'static str> {
        message_items(own)
            .into_iter()
            .filter_map(|item| match item {
                Item::Entry(label, ..) | Item::Danger(label, ..) => Some(label),
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
        assert_eq!(
            danger,
            [Item::Danger(
                "Delete Message",
                "menu_delete_message",
                Action::Delete
            )]
        );
    }
}
