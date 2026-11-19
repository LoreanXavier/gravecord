//! ExpressionManager (Vencord `src/plugins/expressionManager`, GPL-3.0-or-later; ExpressionCloner in the backup). A
//! sticker's menu in the picker and in a message gets Clone Sticker (a dialog: the name to give it, then a server to
//! clone it into, each a round picture or acronym with its name in a tooltip; the sticker is fetched from the media
//! proxy at the largest size under 512 KiB and posted to the server), and with `editAndDelete` Edit Sticker and Delete
//! Sticker in a server whose expressions you manage; `stickerCopyAndOpen` swaps Copy Sticker Image Link for Copy
//! Sticker Link and Open Sticker Link. A custom emoji's menu in the picker gets Delete Emoji the same way, and a
//! default emoji's Copy Emoji its CopyIcon. `recents` adds Remove From Recents after the favourite row of a frecent
//! emoji or sticker: the frecency settings go back without its entry, after a confirmation outside the reaction picker
//! unless Shift is held. Not part of the pinned client.
//!
//! GAP: Clone Emoji (picker, message, forum tag and status emoji menus), Edit Emoji and Upload as Emoji open Discord's
//! Emoji Studio, which the app does not have yet (its Upload Emoji posts files as dropped ones), so those rows are not
//! drawn. Edit Sticker opens the sticker's server settings on Stickers with its dialog, where the original opens the
//! dialog over the chat. The clone dialog's title has no picture of the sticker before it.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, Target};
use dc_state::Id;
use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, div, img, px,
};
use serde_json::Value;
use std::cell::RefCell;

const ID: &str = "ExpressionCloner";
pub(in crate::ui) const CLONE_KEY: &str = "plugin-em-clone:";
const DELETE_KEY: &str = "plugin-em-delete:";
/// `StickerFormatType.LOTTIE`.
const LOTTIE: u64 = 3;
/// `CREATE_GUILD_EXPRESSIONS`.
const CREATE_EXPRESSIONS: u64 = 1 << 43;

/// The open clone dialog.
struct CloneDialog {
    sticker: Id,
    format: u64,
    name: Entity<Editor>,
    tags: String,
    description: String,
    cloning: bool,
    _events: Subscription,
}

use crate::editor::{Editor, EditorEvent};

/// What a confirmation is about.
#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    Delete {
        guild: Id,
        id: Id,
        sticker: bool,
        name: String,
    },
    Recents {
        field: u64,
        key: String,
        sticker: bool,
    },
}

thread_local! {
    static CLONE: RefCell<Option<CloneDialog>> = const { RefCell::new(None) };
    static CONFIRM: RefCell<Option<Confirm>> = const { RefCell::new(None) };
    /// The sticker whose menu is open (picker or message).
    static MENU_STICKER: RefCell<Option<(Id, u64)>> = const { RefCell::new(None) };
    /// The server whose sticker dialog should open on Edit once its list has come.
    static PENDING_EDIT: RefCell<Option<(Id, Id)>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("ExpressionCloner")
}

fn flag(name: &str) -> bool {
    crate::plugins::with(|state| state.value(ID, name) == Value::Bool(true))
}

/// `StickerExtMap`.
fn sticker_ext(format: u64) -> &'static str {
    match format {
        3 => "json",
        4 => "gif",
        _ => "png",
    }
}

/// `formatStickerURL`.
pub(crate) fn sticker_url(sticker: Id, format: u64, size: u32) -> String {
    format!(
        "https://media.discordapp.net/stickers/{sticker}.{}?size={size}&lossless=true&animated=true",
        sticker_ext(format)
    )
}

/// `getGuildMaxStickerSlots`.
fn sticker_slots(guild: &dc_state::Guild) -> usize {
    if guild
        .features
        .iter()
        .any(|feature| feature == "MORE_STICKERS")
        && guild.premium_tier == 3
    {
        return 120;
    }
    match guild.premium_tier {
        1 => 15,
        2 => 30,
        3 => 60,
        _ => 5,
    }
}

/// The name check of the clone dialog's field.
fn name_error(name: &str) -> Option<&'static str> {
    let length = name.chars().count();
    (!(2..=30).contains(&length)).then_some(
        "Name must be between 2 and 30 characters and only contain alphanumeric characters",
    )
}

impl Root {
    /// Whether the plugin is on and a sticker of this format takes its rows (not a Lottie one).
    fn em_sticker_rows(&self, format: u64) -> bool {
        on() && format != LOTTIE
    }

    /// `PermissionStore.can(MANAGE_GUILD_EXPRESSIONS, guild)` with `editAndDelete`.
    fn em_can_manage(&self, guild: Option<Id>) -> Option<Id> {
        let guild = guild?;
        (flag("editAndDelete")
            && self.my_permission(guild, None, dc_state::permissions::MANAGE_GUILD_EXPRESSIONS))
        .then_some(guild)
    }

    /// The frecency settings the app holds.
    fn em_frecency(&self) -> Option<String> {
        self.fetched
            .get(super::super::actions::FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(str::to_owned)
    }

    /// The Remove From Recents row of an expression that is frecent (`buildRemoveFrecentItem`).
    fn em_recent_row(&self, field: u64, key: &str) -> Option<Entry> {
        if !flag("recents") {
            return None;
        }
        let settings = self.em_frecency()?;
        let frecent = if field == dc_proto::frecency::STICKER_FRECENCY {
            let id = key.parse::<u64>().ok()?;
            dc_proto::frecency::id_map(&settings, field)
                .iter()
                .any(|(known, _)| *known == id)
        } else {
            dc_proto::frecency::emoji_map(&settings, field)
                .iter()
                .any(|(known, _)| known == key)
        };
        frecent.then(|| {
            Entry::Row(
                Row::new(Act::PluginRemoveFromRecents, "Remove From Recents")
                    .icon("plugin_history"),
            )
        })
    }

    /// The picker's expression menus with the plugin's rows (`patchExpressionPickerMenu`).
    pub(in crate::ui) fn with_expression_manager_rows(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if !on() {
            return items;
        }
        let reaction = self.picker_for_reaction();
        match target {
            Target::PickerSticker(id) => {
                let Some(sticker) = self.stores.stickers.sticker(id).cloned() else {
                    return items;
                };
                if !self.em_sticker_rows(sticker.format_type) {
                    return items;
                }
                MENU_STICKER.with(|menu| *menu.borrow_mut() = Some((id, sticker.format_type)));
                if let Some(row) =
                    self.em_recent_row(dc_proto::frecency::STICKER_FRECENCY, &id.to_string())
                {
                    let at = items.len().min(1);
                    items.insert(at, row);
                }
                if flag("stickerCopyAndOpen") {
                    items.retain(|entry| {
                        !matches!(entry, Entry::Row(row) if row.act == Act::CopyExpressionLink)
                    });
                    while matches!(items.last(), Some(Entry::Separator)) {
                        items.pop();
                    }
                    push_group(&mut items, sticker_link_rows());
                }
                let mut manage = vec![Entry::Row(
                    Row::new(Act::PluginCloneSticker, "Clone Sticker").icon("plugin_plus"),
                )];
                if sticker.is_guild()
                    && let Some(guild) = self.em_can_manage(sticker.guild_id)
                {
                    let _ = guild;
                    manage.extend(manage_rows("Sticker"));
                }
                push_group(&mut items, manage);
            }
            Target::PickerEmoji => {
                let Some(item) = self.context.picker_emoji.clone() else {
                    return items;
                };
                let field = if reaction {
                    dc_proto::frecency::EMOJI_REACTION_FRECENCY
                } else {
                    dc_proto::frecency::EMOJI_FRECENCY
                };
                match &item {
                    super::super::picker::Item::Custom { id, .. } => {
                        if let Some(row) = self.em_recent_row(field, &id.to_string()) {
                            let at = items.len().min(1);
                            items.insert(at, row);
                        }
                        let guild = self
                            .stores
                            .guilds
                            .iter()
                            .find(|guild| guild.has_emoji(*id))
                            .map(|guild| guild.id);
                        if self.em_can_manage(guild).is_some() {
                            // Clone Emoji and Edit Emoji need the Emoji Studio (see the GAP above).
                            let mut delete = Row::new(Act::PluginDeleteExpression, "Delete Emoji")
                                .icon("plugin_trash");
                            delete.danger = true;
                            push_group(&mut items, vec![Entry::Row(delete)]);
                        }
                    }
                    super::super::picker::Item::Unicode(index) => {
                        let name = dc_tokens::emoji_data::EMOJIS
                            .get(usize::from(*index))
                            .map(|emoji| emoji.names[0])
                            .unwrap_or_default();
                        if let Some(row) = self.em_recent_row(field, name) {
                            let at = items.len().min(1);
                            items.insert(at, row);
                        }
                        // Discord's Copy Emoji of a default emoji has no icon; the plugin gives it CopyIcon.
                        for entry in &mut items {
                            if let Entry::Row(row) = entry
                                && row.act == Act::CopyEmoji
                            {
                                row.icon = Some("clips_copy");
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        items
    }

    /// Whether the open picker is the reaction picker (`EmojiIntention.REACTION`).
    fn picker_for_reaction(&self) -> bool {
        self.picker.as_ref().is_some_and(|picker| {
            matches!(
                picker.target,
                super::super::picker::Target::Reaction { .. }
                    | super::super::picker::Target::InventoryReply(_)
            )
        })
    }

    /// The menu's chosen row, for the sticker or emoji the menu is about.
    pub(in crate::ui) fn expression_manager_act(
        &mut self,
        act: Act,
        target: Target,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        match act {
            Act::PluginCloneSticker => {
                if let Some((sticker, format)) = MENU_STICKER.with(|menu| *menu.borrow()) {
                    self.em_open_clone(sticker, format, cx);
                }
            }
            Act::PluginCopyStickerLink => {
                if let Some((sticker, format)) = MENU_STICKER.with(|menu| *menu.borrow()) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(sticker_url(
                        sticker, format, 512,
                    )));
                    self.show_success_toast("Link copied!".to_owned(), cx);
                }
            }
            Act::PluginOpenStickerLink => {
                if let Some((sticker, format)) = MENU_STICKER.with(|menu| *menu.borrow()) {
                    cx.open_url(&sticker_url(sticker, format, 512));
                }
            }
            Act::PluginEditExpression => {
                if let Some((sticker, _)) = MENU_STICKER.with(|menu| *menu.borrow())
                    && let Some(guild) = self
                        .stores
                        .stickers
                        .sticker(sticker)
                        .and_then(|known| known.guild_id)
                {
                    PENDING_EDIT.with(|pending| *pending.borrow_mut() = Some((guild, sticker)));
                    self.open_server_settings_at(
                        guild,
                        super::super::server_settings::Page::Stickers,
                        cx,
                    );
                    self.request_stickers(guild);
                }
            }
            Act::PluginDeleteExpression => {
                let confirm = match target {
                    Target::PickerSticker(id) => {
                        self.stores.stickers.sticker(id).and_then(|known| {
                            Some(Confirm::Delete {
                                guild: known.guild_id?,
                                id,
                                sticker: true,
                                name: known.name.clone(),
                            })
                        })
                    }
                    Target::PickerEmoji => match self.context.picker_emoji.clone() {
                        Some(super::super::picker::Item::Custom { id, name, .. }) => self
                            .stores
                            .guilds
                            .iter()
                            .find(|guild| guild.has_emoji(id))
                            .map(|guild| Confirm::Delete {
                                guild: guild.id,
                                id,
                                sticker: false,
                                name,
                            }),
                        _ => None,
                    },
                    _ => None,
                };
                CONFIRM.with(|open| *open.borrow_mut() = confirm);
            }
            Act::PluginRemoveFromRecents => {
                let reaction = self.picker_for_reaction();
                let confirm = match target {
                    Target::PickerSticker(id) => Some(Confirm::Recents {
                        field: dc_proto::frecency::STICKER_FRECENCY,
                        key: id.to_string(),
                        sticker: true,
                    }),
                    Target::PickerEmoji => {
                        self.context
                            .picker_emoji
                            .clone()
                            .map(|item| Confirm::Recents {
                                field: if reaction {
                                    dc_proto::frecency::EMOJI_REACTION_FRECENCY
                                } else {
                                    dc_proto::frecency::EMOJI_FRECENCY
                                },
                                key: match item {
                                    super::super::picker::Item::Custom { id, .. } => id.to_string(),
                                    super::super::picker::Item::Unicode(index) => {
                                        dc_tokens::emoji_data::EMOJIS
                                            .get(usize::from(index))
                                            .map(|emoji| emoji.names[0].to_owned())
                                            .unwrap_or_default()
                                    }
                                },
                                sticker: false,
                            })
                    }
                    _ => None,
                };
                // "The confirm modal closes the reaction popup, so skip confirmation."
                if let Some(confirm) = confirm {
                    if shift || reaction {
                        self.em_confirmed(confirm, cx);
                    } else {
                        CONFIRM.with(|open| *open.borrow_mut() = Some(confirm));
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// The message menu's sticker under the pointer (for its rows).
    pub(in crate::ui) fn expression_manager_message_sticker(&self, sticker: Id) -> Option<bool> {
        let format = self
            .stores
            .stickers
            .sticker(sticker)
            .map(|known| known.format_type)
            .or_else(|| self.message_sticker_format(sticker))?;
        if !self.em_sticker_rows(format) {
            return None;
        }
        MENU_STICKER.with(|menu| *menu.borrow_mut() = Some((sticker, format)));
        Some(flag("stickerCopyAndOpen"))
    }

    /// A sticker's format from the open channel's messages (`message.stickerItems`).
    fn message_sticker_format(&self, sticker: Id) -> Option<u64> {
        let channel = self.channel?;
        self.stores
            .messages
            .get(&channel)?
            .iter()
            .flat_map(|message| message.sticker_items.iter())
            .find(|(id, _, _)| *id == sticker)
            .map(|(_, _, format)| *format)
    }

    fn em_open_clone(&mut self, sticker: Id, format: u64, cx: &mut Context<Self>) {
        let known = self.stores.stickers.sticker(sticker).cloned();
        let name = known
            .as_ref()
            .map(|known| known.name.clone())
            .or_else(|| {
                let channel = self.channel?;
                self.stores
                    .messages
                    .get(&channel)?
                    .iter()
                    .flat_map(|message| message.sticker_items.iter())
                    .find(|(id, _, _)| *id == sticker)
                    .map(|(_, name, _)| name.clone())
            })
            .unwrap_or_default();
        let editor = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_text(&name);
            editor
        });
        let events = cx.subscribe(&editor, |_, _, _: &EditorEvent, cx| cx.notify());
        self.request_image(
            format!("em-sticker:{sticker}"),
            sticker_url(sticker, format, 128),
        );
        CLONE.with(|open| {
            *open.borrow_mut() = Some(CloneDialog {
                sticker,
                format,
                name: editor,
                tags: known
                    .as_ref()
                    .and_then(|known| known.tags.clone())
                    .unwrap_or_default(),
                description: known
                    .as_ref()
                    .and_then(|known| known.description.clone())
                    .unwrap_or_default(),
                cloning: false,
                _events: events,
            })
        });
    }

    /// `getGuildCandidates`: the servers you may add a sticker to that have a free slot, by name.
    fn em_candidates(&self) -> Vec<&dc_state::Guild> {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let mut guilds: Vec<&dc_state::Guild> = self
            .stores
            .guilds
            .iter()
            .filter(|guild| {
                (guild.owner_id.is_some() && guild.owner_id == me)
                    || self.my_permission(guild.id, None, CREATE_EXPRESSIONS)
            })
            .filter(|guild| {
                self.stores.stickers.guild_stickers(guild.id).len() < sticker_slots(guild)
            })
            .collect();
        guilds.sort_by(|a, b| a.name.cmp(&b.name));
        guilds
    }

    /// `doClone` into `guild`.
    fn em_clone_into(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some((sticker, format, name, tags, description)) = CLONE.with(|open| {
            let mut open = open.borrow_mut();
            let dialog = open.as_mut().filter(|dialog| !dialog.cloning)?;
            dialog.cloning = true;
            Some((
                dialog.sticker,
                dialog.format,
                dialog.name.read(cx).text().to_owned(),
                dialog.tags.clone(),
                dialog.description.clone(),
            ))
        }) else {
            return;
        };
        let _ = self.requests.send(crate::net::Request::PluginCloneSticker {
            key: format!("{CLONE_KEY}{guild}"),
            guild,
            sticker,
            format,
            name,
            tags,
            description,
        });
        cx.notify();
    }

    /// A clone's answer. True when the key was the plugin's.
    pub(in crate::ui) fn expression_manager_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(guild) = key.strip_prefix(CLONE_KEY) {
            CLONE.with(|open| {
                if let Some(dialog) = open.borrow_mut().as_mut() {
                    dialog.cloning = false;
                }
            });
            let name = body["name"].as_str().unwrap_or_default().to_owned();
            if body["ok"] == true {
                let place = guild
                    .parse::<Id>()
                    .ok()
                    .and_then(|guild| self.stores.guild(guild))
                    .map_or_else(|| "your server".to_owned(), |guild| guild.name.clone());
                self.show_success_toast(format!("Successfully cloned {name} to {place}!"), cx);
            } else {
                let message = body["message"]
                    .as_str()
                    .unwrap_or("Something went wrong (check console!)")
                    .to_owned();
                self.em_critical_toast(format!("Failed to clone: {message}"), cx);
            }
            cx.notify();
            return true;
        }
        if let Some(rest) = key.strip_prefix(DELETE_KEY) {
            let what = if rest.starts_with("sticker") {
                "Sticker"
            } else {
                "Emoji"
            };
            if body["ok"] == true {
                self.show_success_toast(format!("{what} deleted successfully."), cx);
            } else {
                self.em_critical_toast(format!("Failed to delete {what}"), cx);
            }
            return true;
        }
        false
    }

    /// The server's sticker list came: an Edit Sticker waiting for it opens its dialog.
    pub(in crate::ui) fn expression_manager_stickers_listed(
        &mut self,
        guild: Id,
        cx: &mut Context<Self>,
    ) {
        let Some((pending_guild, sticker)) = PENDING_EDIT.with(|pending| *pending.borrow()) else {
            return;
        };
        if pending_guild != guild {
            return;
        }
        PENDING_EDIT.with(|pending| pending.borrow_mut().take());
        let listed = self
            .fetched
            .get(&format!(
                "{}{guild}",
                super::super::server_stickers::STICKERS_PREFIX
            ))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        if let Some(value) = listed
            .iter()
            .find(|value| value["id"].as_str() == Some(sticker.to_string().as_str()))
        {
            self.edit_sticker(guild, value, cx);
        }
    }

    /// `variant: "critical"`.
    fn em_critical_toast(&mut self, text: String, cx: &mut Context<Self>) {
        self.put_icon_toast(
            text,
            ("icons/login_error.svg", "--icon-feedback-critical"),
            cx,
        );
    }

    /// A confirmed Delete or Remove From Recents.
    fn em_confirmed(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        match confirm {
            Confirm::Delete {
                guild, id, sticker, ..
            } => {
                let kind = if sticker { "stickers" } else { "emojis" };
                let _ = self.requests.send(crate::net::Request::Rest {
                    key: Some(format!(
                        "{DELETE_KEY}{}:{id}",
                        if sticker { "sticker" } else { "emoji" }
                    )),
                    method: reqwest::Method::DELETE,
                    route: format!("/guilds/{guild}/{kind}/{id}"),
                    body: None,
                });
            }
            Confirm::Recents {
                field,
                key,
                sticker,
            } => {
                let Some(settings) = self.em_frecency() else {
                    return;
                };
                let entry = if sticker {
                    key.parse::<u64>()
                        .ok()
                        .map(dc_proto::frecency::EntryKey::Id)
                } else {
                    Some(dc_proto::frecency::EntryKey::Text(&key))
                };
                let Some(updated) = entry
                    .and_then(|entry| dc_proto::frecency::without_entry(&settings, field, entry))
                else {
                    return;
                };
                // `FrecencyUserSettingsActionCreators.updateAsync`: the settings go back whole, and the app's copy
                // follows at once.
                if let Some(body) = self.fetched.get_mut(super::super::actions::FRECENCY_KEY) {
                    body["settings"] = Value::from(updated.clone());
                }
                let _ = self.requests.send(crate::net::Request::Rest {
                    key: None,
                    method: reqwest::Method::PATCH,
                    route: "/users/@me/settings-proto/2".to_owned(),
                    body: Some(serde_json::json!({"settings": updated})),
                });
            }
        }
        cx.notify();
    }

    /// The clone dialog and the confirmations.
    pub(in crate::ui) fn expression_manager_dialogs(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use super::super::{color, modal};
        if let Some(confirm) = CONFIRM.with(|open| open.borrow().clone()) {
            let (title, subtitle, confirm_text) = match &confirm {
                Confirm::Delete { sticker, name, .. } => (
                    format!("Delete {name}"),
                    format!(
                        "Are you sure you want to delete this {}?",
                        if *sticker { "sticker" } else { "emoji" }
                    ),
                    "Delete",
                ),
                Confirm::Recents { sticker, .. } => (
                    "Remove From Recents".to_owned(),
                    format!(
                        "This will reset all usage history for this {} and cannot be undone. Are you sure?",
                        if *sticker { "sticker" } else { "emoji" }
                    ),
                    "Do it",
                ),
            };
            let chosen = confirm.clone();
            let spec = modal::Spec {
                id: "em-confirm",
                title: title.into(),
                subtitle: Some(div().child(subtitle).into_any_element()),
                body: None,
                buttons: vec![
                    modal::button(
                        "em-confirm-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |_, _, cx| {
                            CONFIRM.with(|open| open.borrow_mut().take());
                            cx.notify();
                        },
                    ),
                    modal::button(
                        "em-confirm-ok",
                        confirm_text,
                        modal::Tone::Critical,
                        move |this, _, cx| {
                            CONFIRM.with(|open| open.borrow_mut().take());
                            this.em_confirmed(chosen.clone(), cx);
                        },
                    ),
                ],
                close: Box::new(|_: &mut Root, _, cx| {
                    CONFIRM.with(|open| open.borrow_mut().take());
                    cx.notify();
                }),
            };
            return Some(modal::render(spec, cx));
        }
        let (name, editor, cloning) = CLONE.with(|open| {
            open.borrow()
                .as_ref()
                .map(|dialog| (dialog.name.clone(), dialog.name.clone(), dialog.cloning))
        })?;
        let typed = name.read(cx).text().to_owned();
        let error = name_error(&typed);
        let guilds = self.em_candidates();
        let tiles = guilds.iter().map(|guild| {
            let id = guild.id;
            let picture = guild
                .icon
                .as_ref()
                .and_then(|_| self.images.get(&format!("guild:{id}")).cloned());
            let acronym = super::server_info::acronym(&guild.name);
            // `getAcronymFontSize`: by the acronym's length.
            let font = [20., 20., 18., 18., 16., 14., 12.]
                .get(acronym.chars().count())
                .copied()
                .unwrap_or(4.);
            let tile = div()
                .id(SharedString::from(format!("em-guild-{id}")))
                .size(px(64.))
                .rounded_full()
                .overflow_hidden()
                .bg(color("--background-base-lower"))
                .flex()
                .items_center()
                .justify_center()
                .when(cloning, |this| this.opacity(0.5))
                .when(!cloning, |this| this.cursor_pointer())
                .child(match picture {
                    Some(picture) => img(picture).size(px(64.)).rounded_full().into_any_element(),
                    None => div()
                        .w_full()
                        .text_center()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(font))
                        .text_color(color("--text-default"))
                        .child(acronym)
                        .into_any_element(),
                })
                .on_click(cx.listener(move |this, _, _, cx| this.em_clone_into(id, cx)));
            self.with_tip(
                format!("em-guild-tip-{id}"),
                super::super::tooltip::Tip::body(
                    super::super::tooltip::Body::Text(guild.name.clone().into()),
                    super::super::tooltip::Side::Top,
                ),
                tile,
                cx,
            )
            .into_any_element()
        });
        let body = div()
            .pb(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Custom Name"),
            )
            .child(
                div()
                    .h(px(40.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(if error.is_some() {
                        "--text-feedback-critical"
                    } else {
                        "--input-border-default"
                    }))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .child(editor),
            )
            .children(error.map(|error| {
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error)
            }))
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .items_center()
                    .gap(px(8.))
                    .children(tiles),
            )
            .into_any_element();
        let spec = modal::Spec {
            id: "em-clone",
            title: format!("Clone {typed}").into(),
            subtitle: None,
            body: Some(body),
            buttons: Vec::new(),
            close: Box::new(|_: &mut Root, _, cx| {
                CLONE.with(|open| open.borrow_mut().take());
                cx.notify();
            }),
        };
        Some(modal::render(spec, cx))
    }
}

/// Copy Sticker Link and Open Sticker Link (`buildStickerLinkItems`).
fn sticker_link_rows() -> Vec<Entry> {
    vec![
        Entry::Row(Row::new(Act::PluginCopyStickerLink, "Copy Sticker Link").icon("plugin_link")),
        Entry::Row(
            Row::new(Act::PluginOpenStickerLink, "Open Sticker Link")
                .icon("plugin_link")
                .trailing("plugin_top_right_arrow"),
        ),
    ]
}

/// Edit and Delete (`buildManageExpressionItems`).
fn manage_rows(kind: &str) -> Vec<Entry> {
    let mut delete =
        Row::new(Act::PluginDeleteExpression, format!("Delete {kind}")).icon("plugin_trash");
    delete.danger = true;
    vec![
        Entry::Row(
            Row::new(Act::PluginEditExpression, format!("Edit {kind}")).icon("menu_edit_message"),
        ),
        Entry::Row(delete),
    ]
}

/// A group of its own at the end (`children.push(<MenuGroup>)`).
fn push_group(items: &mut Vec<Entry>, group: Vec<Entry>) {
    if group.is_empty() {
        return;
    }
    if !items.is_empty() {
        items.push(Entry::Separator);
    }
    items.extend(group);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stickers_follow_the_originals_tables() {
        assert_eq!(
            sticker_url(5, 4, 512),
            "https://media.discordapp.net/stickers/5.gif?size=512&lossless=true&animated=true"
        );
        assert_eq!(sticker_ext(2), "png");
        assert_eq!(sticker_ext(3), "json");
        assert!(name_error("a").is_some());
        assert!(name_error("ab").is_none());
        assert!(name_error(&"x".repeat(31)).is_some());
        let mut guild = dc_state::Guild::from_value(
            &serde_json::json!({"id": "1", "name": "g", "channels": []}),
        )
        .unwrap();
        assert_eq!(sticker_slots(&guild), 5);
        guild.premium_tier = 3;
        assert_eq!(sticker_slots(&guild), 60);
        guild.features.push("MORE_STICKERS".to_owned());
        assert_eq!(sticker_slots(&guild), 120);
    }
}
