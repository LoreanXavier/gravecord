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

/// The thin scrollbar of a menu (`--scrollbar-thin-thumb`): an 8px gutter whose thumb has a 2px transparent border, so
/// it is 4px wide and starts 2px below the top; the thumb's length, border included, is the viewport's share of the
/// content (read from the pixels of `design/golden/windows/message-menu/link-own-dm` and `member-menu/user-guild`).
pub(super) fn menu_scrollbar(handle: gpui::ScrollHandle) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let max = handle.max_offset().y;
            if max <= px(0.) {
                return;
            }
            let viewport = bounds.size.height;
            let thumb = (viewport * (viewport / (viewport + max))).max(px(40.));
            let top = bounds.top() + (viewport - thumb) * (-handle.offset().y / max);
            let area = gpui::Bounds::new(
                gpui::point(bounds.right() - px(6.), top + px(2.)),
                gpui::size(px(4.), thumb - px(4.)),
            );
            window
                .paint_quad(gpui::fill(area, color("--scrollbar-thin-thumb")).corner_radii(px(2.)));
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(8.))
    .h_full()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Action {
    AddReaction,
    ViewReactions,
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
    /// "Resend message" of a message that failed to send (module 761304 `t_`).
    Resend,
    CopyLink,
    Speak,
    /// "End Poll Now" (module 761304): the author of a poll that is still open.
    EndPoll,
    /// The text selected in a message (module 373963 and 826308): copy it, or search it.
    CopySelection,
    SearchGoogle,
    /// The link or picture under the pointer (module 777933): copy it, copy the address inside a mail or phone
    /// link, send a text message, open it.
    CopyUrl,
    CopyContact,
    SendSms,
    OpenUrl,
    /// A picture of the message (module 115184).
    CopyImage,
    SaveImage,
    /// "Report Message" (module 847342): other people's messages, a danger row at the bottom.
    Report,
    /// "Remove Reactions" (a submenu of one row per emoji, `tT`) and "Remove All Reactions" (`tG`) of module 761304:
    /// for who may manage messages.
    RemoveReactions,
    RemoveAllReactions,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Item {
    /// Label, icon file, action, and whether it opens a submenu (a caret on the right).
    Entry(&'static str, &'static str, Action, bool),
    Danger(&'static str, &'static str, Action),
    Separator,
}

/// What the saved-messages store says about the message the menu is for (module 880457).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SavedState {
    pub bookmarked: bool,
    pub reminder: bool,
    /// Speak Message is reading this message: the entry becomes Stop Speaking Message.
    pub speaking: bool,
    /// No text: Speak Message is left out.
    pub empty: bool,
    /// Pinned: Pin Message becomes Unpin Message.
    pub pinned: bool,
    /// Has reactions: View Reactions follows Add Reaction (`message-reactions` in the capture).
    pub reacted: bool,
    /// Has reactions and the account manages messages here: Remove Reactions and Remove All Reactions (`tT`, `tG`).
    pub manage_reactions: bool,
    /// A poll of this account that has not closed: "End Poll Now" follows Speak Message.
    pub poll_end: bool,
    /// A poll message: it cannot be edited (module 697470 `isPoll`) or forwarded (module 843626 `poll == null`).
    pub poll: bool,
    /// Text is selected in a message: Copy and Search with Google open the menu.
    pub selection: bool,
    /// What the pointer was on when the menu opened.
    pub link: LinkKind,
    /// The pointer was on a picture the app can copy and save.
    pub image: bool,
    /// A direct message: threads grow only in server channels.
    pub no_thread: bool,
    /// The message failed to send (`state === SEND_FAILED`): no Edit, Reply, Forward or Pin, and Resend message.
    pub failed: bool,
}

/// How the link group of a message menu reads (module 777933).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LinkKind {
    #[default]
    None,
    Link,
    /// A picture on Discord's CDN.
    ImageLink,
    /// A video, sound or animation by its extension.
    MediaLink,
    Mail,
    Tel,
}

impl LinkKind {
    /// `b`, `k` and `_` of module 777933.
    pub(super) fn of(url: &str) -> Self {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("mailto:") {
            return Self::Mail;
        }
        if lower.starts_with("tel:") || lower.starts_with("sms:") {
            return Self::Tel;
        }
        let path = lower.split(['?', '#']).next().unwrap_or("");
        let host_cdn = path.contains("//cdn.discordapp.com/");
        let ends = |list: &[&str]| list.iter().any(|ext| path.ends_with(ext));
        let picture = [".png", ".jpg", ".jpeg", ".webp", ".avif", ".bmp", ".svg"];
        if host_cdn && ends(&picture) {
            return Self::ImageLink;
        }
        let media = [
            ".png", ".jpg", ".jpeg", ".webp", ".avif", ".bmp", ".svg", ".gif", ".mp4", ".webm",
            ".mov",
        ];
        if ends(&media) {
            return Self::MediaLink;
        }
        Self::Link
    }
}

/// What a right press landed on, recorded by the element under the pointer before the message's own handler runs.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hit {
    Link {
        url: String,
        label: String,
    },
    /// A picture of the message: its address, and the key its pixels are cached under.
    Media {
        url: String,
        key: String,
    },
}

/// What the menu that is open (or was) was opened on.
#[derive(Clone, Debug, Default)]
pub(super) struct MenuExtra {
    pub hit: Option<Hit>,
    pub selection: String,
    /// The confirmation of a bulk reaction removal, asked for by Remove All Reactions or by a row of Remove Reactions.
    pub reactions_confirm: Option<ReactionsConfirm>,
}

/// What `ConfirmModal` of module 761304 (`eH.A.show`) is asked to remove: every reaction of the message, or those of
/// one emoji (the unicode character, or `name:id`, as the reactions route names it).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ReactionsConfirm {
    pub channel: Id,
    pub message: Id,
    pub emoji: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SubmenuKind {
    Reaction,
    Reminder,
    Apps,
    /// The emoji of the message, one row each (`remove-emoji-reactions`).
    RemoveReactions,
}

/// The submenu hanging off a menu row.
#[derive(Clone, Copy, Debug)]
pub(super) struct Submenu {
    pub kind: SubmenuKind,
    pub message: Id,
    pub row: gpui::Bounds<Pixels>,
}

/// `message` menu order from the capture; `own` adds the author's entries.
#[cfg(test)]
pub(super) fn message_items(own: bool) -> Vec<Item> {
    message_items_saved(own, SavedState::default())
}

/// With the saved state: Remove Bookmark when bookmarked, no bookmark entry and Edit Reminder for a reminder.
pub(super) fn message_items_saved(own: bool, saved: SavedState) -> Vec<Item> {
    let mut items = Vec::new();
    if saved.selection {
        // Modules 373963 and 826308: each of the two is a group of its own, ahead of the rest.
        items.push(Item::Entry(
            "Copy",
            "menu_copy_text",
            Action::CopySelection,
            false,
        ));
        items.push(Item::Separator);
        items.push(Item::Entry(
            "Search with Google",
            "search",
            Action::SearchGoogle,
            false,
        ));
        items.push(Item::Separator);
    }
    items.push(Item::Entry(
        "Add Reaction",
        "menu_add_reaction",
        Action::AddReaction,
        true,
    ));
    if saved.reacted {
        items.push(Item::Entry(
            "View Reactions",
            "menu_view_reactions",
            Action::ViewReactions,
            false,
        ));
    }
    items.push(Item::Separator);
    if own && !saved.poll && !saved.failed {
        items.push(Item::Entry(
            "Edit Message",
            "menu_edit_message",
            Action::Edit,
            false,
        ));
    }
    if !saved.failed {
        items.push(Item::Entry("Reply", "menu_reply", Action::Reply, false));
    }
    if !saved.poll && !saved.failed {
        items.push(Item::Entry(
            "Forward",
            "menu_forward",
            Action::Forward,
            false,
        ));
    }
    if !saved.no_thread {
        items.push(Item::Entry(
            "Create Thread",
            "menu_create_thread",
            Action::Thread,
            false,
        ));
    }
    items.push(Item::Separator);
    // Module 761304 `tf`: the row is left out when the message has no text.
    if !saved.empty {
        items.push(Item::Entry(
            "Copy Text",
            "menu_copy_text",
            Action::CopyText,
            false,
        ));
    }
    if !saved.failed {
        items.push(if saved.pinned {
            Item::Entry("Unpin Message", "menu_pin_message", Action::Pin, false)
        } else {
            Item::Entry("Pin Message", "menu_pin_message", Action::Pin, false)
        });
    }
    if !saved.reminder {
        items.push(if saved.bookmarked {
            Item::Entry(
                "Remove Bookmark",
                "menu_remove_bookmark",
                Action::Bookmark,
                false,
            )
        } else {
            Item::Entry(
                "Bookmark Message",
                "menu_bookmark_message",
                Action::Bookmark,
                false,
            )
        });
    }
    items.push(Item::Entry(
        if saved.reminder {
            "Edit Reminder"
        } else {
            "Create Reminder"
        },
        "menu_create_reminder",
        Action::Reminder,
        true,
    ));
    items.extend([Item::Entry("Apps", "menu_apps", Action::Apps, true)]);
    if saved.failed {
        // `lXHojr`, between Apps and Mark Unread.
        items.push(Item::Entry(
            "Resend message",
            "system_refresh",
            Action::Resend,
            false,
        ));
    }
    items.extend([
        Item::Entry("Mark Unread", "menu_mark_unread", Action::MarkUnread, false),
        Item::Entry(
            "Copy Message Link",
            "menu_copy_message_link",
            Action::CopyLink,
            false,
        ),
    ]);
    if !saved.empty {
        items.push(Item::Entry(
            if saved.speaking {
                "Stop Speaking Message"
            } else {
                "Speak Message"
            },
            "menu_speak_message",
            Action::Speak,
            false,
        ));
    }
    if saved.poll_end {
        items.push(Item::Entry(
            "End Poll Now", // grdwwt
            "menu_end_poll",
            Action::EndPoll,
            false,
        ));
    }
    // Module 761304: one group for Remove Reactions, Remove All Reactions, Delete (`tL`) and Report (`tR`).
    let mut danger = Vec::new();
    if saved.manage_reactions {
        danger.push(Item::Danger(
            "Remove Reactions",
            "menu_remove_reactions",
            Action::RemoveReactions,
        ));
        danger.push(Item::Danger(
            "Remove All Reactions",
            "menu_remove_all_reactions",
            Action::RemoveAllReactions,
        ));
    }
    if !own {
        danger.push(Item::Danger("Report Message", "menu_flag", Action::Report));
    }
    if own {
        danger.push(Item::Danger(
            "Delete Message",
            "menu_delete_message",
            Action::Delete,
        ));
    }
    if !danger.is_empty() {
        items.push(Item::Separator);
        items.extend(danger);
    }
    // Module 761304 puts the picture rows (115184, in `tz`) and the link rows (777933, `tX`) in a group each, after
    // the delete group when the menu is the hover bar's or a right press's (`to`).
    if saved.image {
        items.push(Item::Separator);
        items.push(Item::Entry(
            "Copy Image",
            "clips_image",
            Action::CopyImage,
            false,
        ));
        items.push(Item::Entry(
            "Save Image",
            "clips_image",
            Action::SaveImage,
            false,
        ));
    }
    let mut media = Vec::new();
    let (copy, open) = match saved.link {
        LinkKind::None => (None, None),
        LinkKind::ImageLink => (Some("Copy Image Link"), Some("Open Image Link")),
        LinkKind::MediaLink => (Some("Copy Media Link"), Some("Open Media Link")),
        _ => (Some("Copy Link"), Some("Open Link")),
    };
    if let Some(copy) = copy {
        media.push(Item::Entry(
            copy,
            "menu_copy_message_link",
            Action::CopyUrl,
            false,
        ));
        match saved.link {
            LinkKind::Mail => media.push(Item::Entry(
                "Copy Email Address",
                "menu_copy_message_link",
                Action::CopyContact,
                false,
            )),
            LinkKind::Tel => {
                media.push(Item::Entry(
                    "Copy Phone Number",
                    "menu_copy_message_link",
                    Action::CopyContact,
                    false,
                ));
                media.push(Item::Entry(
                    "Send Text Message",
                    "menu_reply",
                    Action::SendSms,
                    false,
                ));
            }
            _ => {}
        }
    }
    if let Some(open) = open {
        media.push(Item::Entry(open, "open_external", Action::OpenUrl, false));
    }
    if !media.is_empty() {
        items.push(Item::Separator);
        items.extend(media);
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
    pub(super) fn saved_flags(&self, message: Id) -> SavedState {
        let state = self
            .channel
            .and_then(|channel| self.saved_state(channel, message));
        let found = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|list| list.iter().find(|known| known.id == message));
        let empty = found.is_some_and(|known| known.content.is_empty());
        let pinned = found.is_some_and(|known| known.pinned);
        let reacted = found.is_some_and(|known| !known.reactions.is_empty());
        SavedState {
            bookmarked: state.is_some(),
            reminder: state.is_some_and(|due| due.is_some()),
            speaking: self.speaking_message == Some(message),
            empty,
            pinned,
            reacted,
            manage_reactions: reacted
                && found.is_some_and(|known| known.extra.poll.is_none())
                && self.current_channel().is_some_and(|channel| {
                    self.can_in_channel(channel, dc_state::permissions::MANAGE_MESSAGES)
                }),
            poll_end: found.is_some_and(|known| self.poll_endable(known)),
            poll: found.is_some_and(|known| known.extra.poll.is_some()),
            selection: !self.menu_extra.selection.is_empty(),
            link: match &self.menu_extra.hit {
                Some(Hit::Link { url, .. } | Hit::Media { url, .. }) => LinkKind::of(url),
                None => LinkKind::None,
            },
            no_thread: self.guild.is_none(),
            failed: self.send_state_of(message) == Some(super::send_state::SendState::Failed),
            image: matches!(&self.menu_extra.hit, Some(Hit::Media { key, .. }) if self.images.contains_key(key)),
        }
    }

    /// The quick reaction row: the four most frecent reactions (`Z.D6`), unicode emoji from the dataset.
    pub(super) fn quick_reactions(&self) -> Vec<&'static dc_tokens::emoji_data::Emoji> {
        self.frequent_reactions()
            .into_iter()
            .filter_map(|name| dc_tokens::emoji_by_name(&name))
            .filter(|emoji| !(emoji.sprite.is_some() && NOT_QUICK.contains(&emoji.names[0])))
            .take(4)
            .collect()
    }

    pub(super) fn run_action(
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
                let text = parse_block_text(&found.content);
                super::actions::log_effect("Copy Text", "clipboard text", text.len());
                cx.write_to_clipboard(ClipboardItem::new_string(text))
            }
            Action::CopyLink => {
                let guild = self
                    .guild
                    .map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
                let link = format!(
                    "https://discord.com/channels/{guild}/{}/{message}",
                    found.channel_id
                );
                super::actions::log_effect("Copy Message Link", "clipboard text", link.len());
                cx.write_to_clipboard(ClipboardItem::new_string(link));
            }
            // A failed message is deleted here only and asks nothing; Resend sends it again (module 519222 `RC`, `Io`).
            Action::Delete
                if self.send_state_of(message) == Some(super::send_state::SendState::Failed) =>
            {
                self.delete_failed(message, cx)
            }
            Action::Resend => self.retry_send(message, cx),
            Action::Delete => self.confirm_delete = Some(message),
            Action::RemoveAllReactions => {
                self.menu_extra.reactions_confirm = Some(ReactionsConfirm {
                    channel: found.channel_id,
                    message,
                    emoji: None,
                });
            }
            other => self.run_message_action(other, found, window, cx),
        }
        cx.notify();
    }

    /// Harness: runs the open message menu's row with this label, as a click on it would.
    #[cfg(feature = "capture")]
    pub(crate) fn script_menu_run(
        &mut self,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let Some(open) = self.menu else {
            return self.script_context_run(label, window, cx);
        };
        let action = message_items_saved(open.own, self.saved_flags(open.message))
            .into_iter()
            .find_map(|item| match item {
                Item::Entry(name, _, action, _) | Item::Danger(name, _, action)
                    if name == label =>
                {
                    Some(action)
                }
                _ => None,
            });
        match action {
            Some(action) => {
                let before = crate::inert::line_count();
                super::actions::take_effects();
                self.run_action(action, open.message, window, cx);
                let mut lines = vec![format!("ran {label}")];
                lines.extend(super::actions::take_effects());
                lines.extend(crate::inert::lines_since(before));
                lines.join(" | ")
            }
            None => format!("no row {label}"),
        }
    }

    pub(super) fn run_unconfirmed(&mut self, action: Action, message: Id, cx: &mut Context<Self>) {
        self.menu = None;
        if action == Action::Delete
            && self.send_state_of(message) == Some(super::send_state::SendState::Failed)
        {
            self.delete_failed(message, cx);
            return;
        }
        if let Some(channel) = self.channel {
            let pinned = self
                .stores
                .messages
                .get(&channel)
                .and_then(|list| list.iter().find(|known| known.id == message))
                .is_some_and(|known| known.pinned);
            let request = match action {
                Action::Delete => Request::Delete { channel, message },
                Action::RemoveAllReactions => {
                    self.remove_reactions(channel, message, None);
                    cx.notify();
                    return;
                }
                _ => Request::Pin {
                    channel,
                    message,
                    pin: !pinned,
                },
            };
            let _ = self.requests.send(request);
        }
        cx.notify();
    }

    /// `Go` and `Jf` of module 649963: `DELETE /channels/{c}/messages/{m}/reactions`, or `.../reactions/{emoji}`.
    pub(super) fn remove_reactions(&mut self, channel: Id, message: Id, emoji: Option<&str>) {
        let route = match emoji {
            Some(emoji) => format!(
                "/channels/{channel}/messages/{message}/reactions/{}",
                super::reactions_modal::encode(emoji)
            ),
            None => format!("/channels/{channel}/messages/{message}/reactions"),
        };
        super::actions::log_effect(
            if emoji.is_some() {
                dc_intl::tr("Remove Reactions")
            } else {
                dc_intl::tr("Remove All Reactions")
            },
            "DELETE reactions",
            route.len(),
        );
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::DELETE,
            route,
            body: None,
        });
    }

    /// The submenu of Remove Reactions: one row per distinct emoji, named `:name:` (`b3` of module 815807). A row asks
    /// first, as `eH.A.show` does; shift removes at once.
    fn remove_reaction_rows(
        &self,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let reactions = self
            .stores
            .messages
            .get(&channel)
            .and_then(|list| list.iter().find(|known| known.id == message))
            .map(|known| known.reactions.clone())
            .unwrap_or_default();
        let mut seen: Vec<String> = Vec::new();
        let mut rows = Vec::new();
        for reaction in reactions {
            let route = super::reaction_route_name(&reaction);
            if seen.contains(&route) {
                continue;
            }
            seen.push(route.clone());
            let image = match reaction.emoji_id {
                Some(id) => self.images.get(&format!("emoji-id:{id}")).cloned(),
                None => self
                    .images
                    .get(&format!("emoji:{}", reaction.name))
                    .cloned(),
            };
            let label = match reaction.emoji_id {
                Some(_) => format!(":{}:", reaction.name),
                None => dc_tokens::emoji_data::EMOJIS
                    .iter()
                    .find(|emoji| emoji.surrogates == reaction.name)
                    .map_or_else(
                        || reaction.name.clone(),
                        |emoji| format!(":{}:", emoji.names[0]),
                    ),
            };
            let leading = div()
                .size(px(20.))
                .children(image.map(|image| img(image).size(px(20.))))
                .into_any_element();
            rows.push((
                self.submenu_item(
                    SharedString::from(format!("remove-reaction-{}", rows.len())),
                    Some(leading),
                    label,
                    36.,
                )
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                    this.submenu = None;
                    this.menu = None;
                    if event.modifiers().shift {
                        this.remove_reactions(channel, message, Some(&route));
                    } else {
                        this.menu_extra.reactions_confirm = Some(ReactionsConfirm {
                            channel,
                            message,
                            emoji: Some(route.clone()),
                        });
                    }
                    cx.notify();
                }))
                .into_any_element(),
                36.,
            ));
        }
        rows
    }

    /// The question of a bulk removal: `73GqTz` / `dmy5bn` for one emoji, `iz3vYX` / `VpjOCo` for all of them, answered
    /// with Yes (`p89ACt`, critical) or No (`gm1Vej`).
    pub(super) fn reactions_confirm_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ask = self.menu_extra.reactions_confirm.clone()?;
        let (title, body) = if ask.emoji.is_some() {
            (
                dc_intl::tr("Remove Reactions for Emoji"),
                dc_intl::tr(
                    "Are you sure you want to remove all of the reactions for this emoji from this message?",
                ),
            )
        } else {
            (
                dc_intl::tr("Remove All Reactions"),
                dc_intl::tr("Are you sure you want to remove all reactions from this message?"),
            )
        };
        let emoji = ask.emoji.clone();
        Some(super::modal::render(
            super::modal::Spec {
                id: "remove-reactions-confirm",
                title: title.into(),
                subtitle: Some(div().child(body).into_any_element()),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "remove-reactions-no",
                        dc_intl::tr("No"),
                        super::modal::Tone::Secondary,
                        |this, _, _| this.menu_extra.reactions_confirm = None,
                    ),
                    super::modal::button(
                        "remove-reactions-yes",
                        dc_intl::tr("Yes"),
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            this.menu_extra.reactions_confirm = None;
                            this.remove_reactions(ask.channel, ask.message, emoji.as_deref());
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.menu_extra.reactions_confirm = None),
            },
            cx,
        ))
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
            Item::Danger(label, icon_name, run) => {
                (label, icon_name, run, run == Action::RemoveReactions, true)
            }
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
        let probe_key = format!("menu-row-{index}");
        let hover_key = probe_key.clone();
        let click_key = probe_key.clone();
        self.menu_stop(
            div().id(SharedString::from(format!("menu-{index}"))),
            &format!("menu-{index}"),
            hover_bg,
            cx,
        )
        .relative()
        .child(self.anchor_probe(probe_key))
        .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
            if !*inside {
                return;
            }
            // Hovering the reminder row opens its submenu; any other row closes it.
            let row = this.tip_anchors.borrow().get(hover_key.as_str()).copied();
            let kind = match run {
                Action::AddReaction => Some(SubmenuKind::Reaction),
                Action::Reminder => Some(SubmenuKind::Reminder),
                Action::Apps => Some(SubmenuKind::Apps),
                Action::RemoveReactions => Some(SubmenuKind::RemoveReactions),
                _ => None,
            };
            if kind == Some(SubmenuKind::Apps) {
                this.request_command_index();
            }
            this.submenu = match (kind, row) {
                (Some(kind), Some(row)) => Some(Submenu {
                    kind,
                    message: target,
                    row,
                }),
                _ => None,
            };
            cx.notify();
        }))
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
                if event.modifiers().shift
                    && matches!(
                        run,
                        Action::Delete | Action::Pin | Action::RemoveAllReactions
                    )
                {
                    this.run_unconfirmed(run, target, cx);
                } else {
                    if matches!(
                        run,
                        Action::Reminder | Action::Apps | Action::RemoveReactions
                    ) {
                        // These rows open their submenus, on a click as on a hover.
                        let kind = if run == Action::Apps {
                            this.request_command_index();
                            SubmenuKind::Apps
                        } else if run == Action::RemoveReactions {
                            SubmenuKind::RemoveReactions
                        } else {
                            SubmenuKind::Reminder
                        };
                        let row = this.tip_anchors.borrow().get(click_key.as_str()).copied();
                        this.submenu = row.map(|row| Submenu {
                            kind,
                            message: target,
                            row,
                        });
                        cx.notify();
                        return;
                    }
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
                // `label_M3qk4j`: one line, cut with an ellipsis.
                .truncate()
                .child(dc_intl::tr(label)),
        )
        // The keyboard shortcut of Copy (module 373963 `keyCombo`).
        .when(run == Action::CopySelection, |this| {
            this.child(
                div()
                    .ml(px(8.))
                    .flex_none()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--interactive-text-default"))
                    .child(if cfg!(target_os = "macos") {
                        "Cmd+C"
                    } else {
                        "Ctrl+C"
                    }),
            )
        })
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
        // Text selected: the menu opens with Copy instead of the quick reactions (selection-own-dm).
        if self.menu_extra.selection.is_empty() {
            rows.extend(self.quick_row(open.message, cx));
        }
        for (index, item) in message_items_saved(open.own, self.saved_flags(open.message))
            .into_iter()
            .enumerate()
        {
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
                let estimate = self.menu_height(open.own, open.message);
                let bottom_room = f32::from(window.viewport_size().height) - MORE_EDGE;
                let top = f32::from(at.y).min(bottom_room - estimate).max(MORE_EDGE);
                (
                    gpui::point(at.x - px(MORE_GAP + MENU_WIDTH), px(top)),
                    MORE_EDGE,
                )
            }
        };
        let max_height = f32::from(window.viewport_size().height) - 96.;
        Some(deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(margin))
                .child(
                    div()
                        .parity_with_border("message-menu", 1.)
                        .id("context-menu")
                        .relative()
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
                        .on_mouse_down_out(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                // A click in the submenu is not outside the menu.
                                let in_submenu = this.submenu.is_some()
                                    && this
                                        .tip_anchors
                                        .borrow()
                                        .get("submenu")
                                        .is_some_and(|bounds| bounds.contains(&event.position));
                                if !in_submenu {
                                    this.submenu = None;
                                    if this.press_outside(super::keyboard::Layer::Menu, cx) {
                                        cx.stop_propagation();
                                    }
                                }
                            },
                        ))
                        .child(
                            div()
                                .id("context-menu-scroller")
                                .track_scroll(&self.menu_scroll)
                                .overflow_y_scroll()
                                .max_h(px(max_height - 2.))
                                .pt(px(8.))
                                // A menu that scrolls ends flush with its last row (image-fixture-end: the last row
                                // stands at 675..711 in a menu that ends at 712).
                                .pb(px(
                                    if self.menu_height(open.own, open.message) > max_height {
                                        0.
                                    } else {
                                        8.
                                    },
                                ))
                                .px(px(8.))
                                .flex()
                                .flex_col()
                                // Rows keep their height when the menu scrolls.
                                .children(rows.into_iter().map(|row| div().flex_none().child(row))),
                        )
                        .child(menu_scrollbar(self.menu_scroll.clone())),
                ),
        ))
    }

    /// The submenu of the open menu, beside the row it hangs off.
    pub(super) fn menu_submenu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.menu?;
        let submenu = self.submenu?;
        let channel = self.channel?;
        let rows = match submenu.kind {
            SubmenuKind::Reminder => self.reminder_rows(channel, submenu.message, cx),
            SubmenuKind::Reaction => self.reaction_rows(channel, submenu.message, cx),
            SubmenuKind::Apps => self.app_rows(channel, submenu.message, cx),
            SubmenuKind::RemoveReactions => self.remove_reaction_rows(channel, submenu.message, cx),
        };
        Some(self.submenu_frame(rows, submenu.row, window))
    }

    /// `submenu_M3qk4j`: 188 wide, its first row level with the row it hangs off (8px padding and the border
    /// above it), to the right when it fits, else to the left, 4px off the row; nudged 8px inside the window.
    pub(super) fn submenu_frame(
        &self,
        rows: Vec<(AnyElement, f32)>,
        row: gpui::Bounds<Pixels>,
        window: &Window,
    ) -> AnyElement {
        self.submenu_frame_wide(rows, row, window, 188.)
    }

    /// The submenu at another width: it grows with its longest label (the invite list's server names).
    pub(super) fn submenu_frame_wide(
        &self,
        rows: Vec<(AnyElement, f32)>,
        row: gpui::Bounds<Pixels>,
        window: &Window,
        width: f32,
    ) -> AnyElement {
        let height = 2. + 16. + rows.iter().map(|(_, height)| height).sum::<f32>();
        let viewport = window.viewport_size();
        let right_side = f32::from(row.right()) + 4. + width <= f32::from(viewport.width) - 8.;
        let left = if right_side {
            f32::from(row.right()) + 4.
        } else {
            f32::from(row.left()) - 4. - width
        };
        let top = (f32::from(row.top()) - 9.)
            .min(f32::from(viewport.height) - 8. - height)
            .max(8.);
        deferred(
            div()
                .id("submenu")
                .child(self.anchor_probe("submenu"))
                .occlude()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(12.)),
                    blur_radius: px(24.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .p(px(8.))
                .flex()
                .flex_col()
                .children(rows.into_iter().map(|(element, _)| element)),
        )
        .with_priority(2)
        .into_any_element()
    }

    pub(super) fn submenu_item(
        &self,
        id: SharedString,
        leading: Option<AnyElement>,
        label: String,
        height: f32,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(height))
            .p(px(8.))
            .rounded(px(2.))
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(color("--interactive-background-hover"))
                    .rounded(px(4.))
            })
            .flex()
            .items_center()
            .children(leading.map(|leading| div().mr(px(8.)).child(leading)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .truncate()
                    .child(label),
            )
    }

    /// Add Reaction's submenu (module 761304): up to twelve frecent reactions, then View More for the picker.
    fn reaction_rows(
        &self,
        channel: Id,
        message: Id,
        cx: &mut Context<Self>,
    ) -> Vec<(AnyElement, f32)> {
        let mut rows: Vec<(AnyElement, f32)> = self
            .frequent_reactions()
            .into_iter()
            .filter_map(|name| dc_tokens::emoji_by_name(&name))
            .take(12)
            .enumerate()
            .map(|(index, emoji)| {
                let picture = self
                    .images
                    .get(&format!("emoji:{}", emoji.surrogates))
                    .cloned();
                let leading = div()
                    .size(px(20.))
                    .children(picture.map(|picture| img(picture).size(px(20.))))
                    .into_any_element();
                (
                    self.submenu_item(
                        SharedString::from(format!("reaction-row-{index}")),
                        Some(leading),
                        format!(":{}:", emoji.names[0]),
                        36.,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.submenu = None;
                        this.react_from_menu(message, emoji.surrogates, cx);
                    }))
                    .into_any_element(),
                    36.,
                )
            })
            .collect();
        rows.push((
            div()
                .my(px(8.))
                .h(px(1.))
                .bg(color("--border-subtle"))
                .into_any_element(),
            17.,
        ));
        let leading = svg()
            .path("icons/menu_view_reactions.svg")
            .size(px(20.))
            .text_color(color("--interactive-text-default"))
            .into_any_element();
        rows.push((
            self.submenu_item(
                "reaction-view-more".into(),
                Some(leading),
                dc_intl::tr("View More").to_owned(),
                36.,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                let at = this.menu.map(|open| match open.anchor {
                    Anchor::Pointer(at) | Anchor::MoreButton(at) => at,
                });
                this.menu = None;
                this.submenu = None;
                if let Some(at) = at {
                    this.open_picker(
                        super::picker::Target::Reaction { channel, message },
                        at,
                        window,
                        cx,
                    );
                }
            }))
            .into_any_element(),
            36.,
        ));
        rows
    }

    /// The menu's height before layout, for placing the hover bar's menu: rows of 36, separators of 17, the quick row
    /// of 60, 16 of padding and 2 of border.
    fn menu_height(&self, own: bool, message: Id) -> f32 {
        let items = message_items_saved(own, self.saved_flags(message));
        let rows = items
            .iter()
            .filter(|item| !matches!(item, Item::Separator))
            .count() as f32;
        let separators = items.len() as f32 - rows;
        let quick = if self.quick_reactions().is_empty() || !self.menu_extra.selection.is_empty() {
            0.
        } else {
            60.
        };
        rows * 36. + separators * 17. + quick + 16. + 2.
    }

    /// "Delete Message" (`AMvpS4`): the message, the shift protip (`2Ld8DM`, `IxY7E6`), Cancel and a critical Delete.
    /// Measured in `design/dom/modals/delete-message-darker-cozy.json`.
    pub(super) fn delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(target) = self.confirm_delete else {
            return self.reactions_confirm_modal(cx);
        };
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
                            .child(dc_intl::tr("Protip:")),
                    )
                    .child(crate::inline::inline(tip_pieces, px(16.))),
            )
            .into_any_element();
        let channel = message.channel_id;
        Some(super::modal::render(
            super::modal::Spec {
                id: "delete-modal",
                title: dc_intl::tr("Delete Message").into(),
                subtitle: Some(
                    div()
                        .child(dc_intl::tr("Are you sure you want to delete this message?"))
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "delete-cancel",
                        dc_intl::tr("Cancel"),
                        super::modal::Tone::Secondary,
                        |this, _, _| this.confirm_delete = None,
                    ),
                    super::modal::button(
                        "delete-confirm",
                        dc_intl::tr("Delete"),
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
        // The element under a right press recorded what it was before this handler ran; only for the same press.
        let hit = self.menu_hit.take().and_then(|(at, hit)| {
            matches!(anchor, Anchor::Pointer(pointer) if pointer == at).then_some(hit)
        });
        self.menu_extra = MenuExtra {
            reactions_confirm: None,
            hit,
            selection: if matches!(anchor, Anchor::Pointer(_)) {
                self.selected_text()
            } else {
                String::new()
            },
        };
        // A menu always opens at its top, whatever the one before was scrolled to.
        self.menu_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.menu = Some(OpenMenu {
            anchor,
            message,
            own,
        });
        self.request_quick_reaction_images();
        self.request_reaction_images(message);
        cx.notify();
    }

    /// The pictures of the emoji in Remove Reactions' rows.
    fn request_reaction_images(&mut self, message: Id) {
        let Some(reactions) = self
            .channel
            .and_then(|channel| self.stores.messages.get(&channel))
            .and_then(|list| list.iter().find(|known| known.id == message))
            .map(|known| known.reactions.clone())
        else {
            return;
        };
        for reaction in reactions {
            match reaction.emoji_id {
                Some(id) => self.request_image(
                    format!("emoji-id:{id}"),
                    format!("https://cdn.discordapp.com/emojis/{id}.webp?size=48"),
                ),
                None => self.request_emoji_image(&reaction.name),
            }
        }
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

    /// The menu's labels go through `dc_intl::tr`: each is the text of an en-US message, so every locale has it.
    #[test]
    fn every_label_is_an_official_message() {
        let mut states = vec![super::SavedState::default()];
        for link in [
            super::LinkKind::Link,
            super::LinkKind::ImageLink,
            super::LinkKind::MediaLink,
            super::LinkKind::Mail,
        ] {
            states.push(super::SavedState {
                bookmarked: true,
                reminder: true,
                speaking: true,
                pinned: true,
                reacted: true,
                manage_reactions: true,
                poll_end: true,
                poll: true,
                selection: true,
                link,
                image: true,
                ..Default::default()
            });
        }
        states.push(super::SavedState {
            failed: true,
            ..Default::default()
        });
        for state in states {
            for own in [true, false] {
                for item in super::message_items_saved(own, state) {
                    if let Item::Entry(label, ..) | Item::Danger(label, ..) = item {
                        assert!(
                            dc_intl::keys_of(label).next().is_some(),
                            "{label:?} is no en-US message"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn other_peoples_messages_have_no_edit_or_delete() {
        let items = labels(false);
        assert!(!items.contains(&"Edit Message"));
        assert!(!items.contains(&"Delete Message"));
        assert_eq!(items.last(), Some(&"Report Message"));
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

    #[test]
    fn a_picture_message_has_one_group_for_the_picture_and_one_for_its_link() {
        let saved = super::SavedState {
            image: true,
            link: super::LinkKind::ImageLink,
            ..Default::default()
        };
        let items = super::message_items_saved(true, saved);
        let tail: Vec<&str> = items
            .iter()
            .rev()
            .take(6)
            .rev()
            .map(|item| match item {
                Item::Entry(label, ..) | Item::Danger(label, ..) => *label,
                Item::Separator => "-",
            })
            .collect();
        assert_eq!(
            tail,
            [
                "-",
                "Copy Image",
                "Save Image",
                "-",
                "Copy Image Link",
                "Open Image Link"
            ]
        );
    }
}
