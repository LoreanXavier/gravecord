//! PinDMs (Vencord `src/plugins/pinDms`, GPL-3.0-or-later): private channels pinned into your own categories at the
//! top of the DM list. Each category is a header in its colour (the DM list header's look, with a chevron turned down
//! while open) that collapses on a click, followed by its conversations; a collapsed category still shows the
//! conversation that is open. Pinned conversations leave the plain list below. `pinOrder` sorts a category by the most
//! recent message (as the plain list) or keeps your own order (`Custom`, changed with Move Up / Move Down);
//! `canCollapseDmSection` lets the "Direct Messages" header collapse the plain list (`dmSectionCollapsed`).
//!
//! Menus: a conversation's menu gets "Pin DMs" (before Close DM, at the head of a group DM's Leave Group group, or
//! after Message in a user's menu) with Add Category and the categories, or, once pinned, Unpin DM and Move Up / Move
//! Down (Custom order). A category header's menu has Edit Category, Move Up / Move Down, and Delete Category. Add and
//! Edit open the category dialog (name, colour swatches and a custom colour), the folder settings dialog's layout
//! without its default swatch. Categories are kept per account, as `userBasedCategoryList` is, apart from the
//! settings (storage key `plugin-pin-dms`).
//!
//! GAP: `friendsSidebarGroupDms` and the pinned sections of the friends sidebar (the app has no friends sidebar), the
//! quick switcher's and Ctrl+Alt+Up/Down's order with pins first, and the brand/danger tints of the submenu rows.
//! Not part of the pinned client.

use super::super::Root;
use super::super::context_menu::{Act, Entry, Row, SubRow, Target};
use crate::plugins::State;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashSet;

const ID: &str = "PinDMs";
const STORAGE_KEY: &str = "plugin-pin-dms";
/// `DEFAULT_COLOR`.
pub(in crate::ui) const DEFAULT_COLOR: u32 = 10_070_709;

/// A category (`Category`).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Category {
    pub(in crate::ui) id: String,
    pub(in crate::ui) name: String,
    pub(in crate::ui) color: u32,
    pub(in crate::ui) channels: Vec<Id>,
    pub(in crate::ui) collapsed: bool,
}

impl Category {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            name: value["name"].as_str().unwrap_or_default().to_owned(),
            color: value["color"].as_u64().unwrap_or(u64::from(DEFAULT_COLOR)) as u32,
            channels: value["channels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str()?.parse().ok())
                .collect(),
            collapsed: value["collapsed"].as_bool().unwrap_or(false),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "color": self.color,
            "channels": self.channels.iter().map(Id::to_string).collect::<Vec<_>>(),
            "collapsed": self.collapsed,
        })
    }
}

/// The settings that shape the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Look {
    pub(in crate::ui) custom_order: bool,
    pub(in crate::ui) can_collapse: bool,
    pub(in crate::ui) dm_collapsed: bool,
}

fn look(state: &State) -> Option<Look> {
    state.is_on(ID).then(|| Look {
        custom_order: state.value(ID, "pinOrder") == json!(1),
        can_collapse: state.value(ID, "canCollapseDmSection") == Value::Bool(true),
        dm_collapsed: state.value(ID, "dmSectionCollapsed") == Value::Bool(true),
    })
}

fn current() -> Option<Look> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(look)
}

thread_local! {
    /// The account the categories belong to, and its categories.
    static CATEGORIES: RefCell<Option<(Id, Vec<Category>)>> = const { RefCell::new(None) };
}

fn stored_all() -> Value {
    if cfg!(test) {
        return json!({});
    }
    crate::storage::get(STORAGE_KEY).unwrap_or_else(|| json!({}))
}

/// The categories of `account` (`currentUserCategories`), loaded once per account.
fn categories(account: Id) -> Vec<Category> {
    CATEGORIES.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().is_none_or(|(owner, _)| *owner != account) {
            let list = stored_all()[account.to_string()]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Category::from_value)
                .collect();
            *cell = Some((account, list));
        }
        cell.as_ref()
            .map(|(_, list)| list.clone())
            .unwrap_or_default()
    })
}

/// Changes `account`'s categories and writes them through.
fn change(account: Id, edit: impl FnOnce(&mut Vec<Category>)) {
    let mut list = categories(account);
    edit(&mut list);
    if !cfg!(test) {
        let mut all = stored_all();
        if !all.is_object() {
            all = json!({});
        }
        all[account.to_string()] = Value::Array(list.iter().map(Category::to_value).collect());
        crate::storage::set(STORAGE_KEY, all);
    }
    CATEGORIES.with(|cell| *cell.borrow_mut() = Some((account, list)));
}

/// `swapElementsInArray` in `direction` (-1 up, 1 down); false when there is no neighbour.
fn swap<T>(list: &mut [T], index: usize, direction: i32) -> bool {
    let Some(other) = index.checked_add_signed(direction as isize) else {
        return false;
    };
    if other >= list.len() || index >= list.len() {
        return false;
    }
    list.swap(index, other);
    true
}

/// `getCategoryChannels`: by the plain list's order (`LastMessage`), or as kept.
pub(in crate::ui) fn ordered(category: &Category, custom: bool, recent: &[Id]) -> Vec<Id> {
    if custom {
        return category.channels.clone();
    }
    let mut channels = category.channels.clone();
    channels.sort_by_key(|id| {
        recent
            .iter()
            .position(|known| known == id)
            .unwrap_or(usize::MAX)
    });
    channels
}

/// What the DM list draws with the plugin on.
pub(in crate::ui) struct Layout {
    pub(in crate::ui) look: Look,
    /// The categories, each with its conversations in order.
    pub(in crate::ui) sections: Vec<(usize, Category, Vec<Id>)>,
    pub(in crate::ui) pinned: HashSet<Id>,
}

/// Where a submenu row of "Pin DMs" leads.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PinRow {
    AddCategory,
    Into(usize),
    Unpin,
    Up,
    Down,
}

fn pin_rows(list: &[Category], channel: Id, custom: bool) -> Vec<(PinRow, String)> {
    match list
        .iter()
        .find(|category| category.channels.contains(&channel))
    {
        None => std::iter::once((PinRow::AddCategory, "Add Category".to_owned()))
            .chain(
                list.iter()
                    .enumerate()
                    .map(|(index, category)| (PinRow::Into(index), category.name.clone())),
            )
            .collect(),
        Some(category) => {
            let at = category
                .channels
                .iter()
                .position(|id| *id == channel)
                .unwrap_or(0);
            let mut rows = vec![(PinRow::Unpin, "Unpin DM".to_owned())];
            if custom && at > 0 {
                rows.push((PinRow::Up, "Move Up".to_owned()));
            }
            if custom && at + 1 < category.channels.len() {
                rows.push((PinRow::Down, "Move Down".to_owned()));
            }
            rows
        }
    }
}

impl Root {
    fn pin_account(&self) -> Option<Id> {
        self.stores.current_user.as_ref().map(|me| me.id)
    }

    /// The categories with their conversations, or `None` with the plugin off.
    pub(in crate::ui) fn pin_dms_layout(&self) -> Option<Layout> {
        let look = current()?;
        let account = self.pin_account()?;
        let recent: Vec<Id> = dc_state::private_channels::sorted_channels(&self.stores)
            .iter()
            .map(|channel| channel.id)
            .collect();
        let list = categories(account);
        let pinned = list
            .iter()
            .flat_map(|category| category.channels.iter().copied())
            .collect();
        let sections = list
            .into_iter()
            .enumerate()
            .map(|(index, category)| {
                let channels = ordered(&category, look.custom_order, &recent);
                (index, category, channels)
            })
            .collect();
        Some(Layout {
            look,
            sections,
            pinned,
        })
    }

    /// A category's header: its name in its colour and the chevron; a click folds it, a right click opens its menu.
    pub(in crate::ui) fn pin_dms_header(
        &self,
        index: usize,
        category: &Category,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tint = gpui::rgb(category.color);
        let collapsed = category.collapsed;
        div()
            .id(SharedString::from(format!(
                "pin-dms-category-{}",
                category.id
            )))
            .flex_none()
            .mt(px(12.))
            .h(px(24.))
            .pl(px(17.))
            .pr(px(16.))
            .flex()
            .items_start()
            .pt(px(1.))
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(tint)
            .child(div().truncate().child(category.name.clone()))
            // `.vc-pindms-collapse-icon`: 16px, turned down (90deg) while open.
            .child(
                div().ml(px(4.)).mt(px(1.)).child(
                    gpui::svg()
                        .path("icons/menu_caret_right.svg")
                        .size(px(16.))
                        .text_color(super::super::color("--interactive-icon-default"))
                        .with_transformation(gpui::Transformation::rotate(gpui::radians(
                            if collapsed {
                                0.
                            } else {
                                std::f32::consts::FRAC_PI_2
                            },
                        ))),
                ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pin_dms_fold(index, cx)))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.open_context_menu(Target::PinCategory(index as u16), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    fn pin_dms_fold(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(account) = self.pin_account() else {
            return;
        };
        change(account, |list| {
            if let Some(category) = list.get_mut(index) {
                category.collapsed = !category.collapsed;
            }
        });
        cx.notify();
    }

    /// The "Direct Messages" header was pressed: with `canCollapseDmSection`, the plain list folds.
    pub(in crate::ui) fn pin_dms_fold_dm_section(&mut self, cx: &mut Context<Self>) {
        let Some(look) = current().filter(|look| look.can_collapse) else {
            return;
        };
        crate::plugins::update(|state| {
            state.set_value(ID, "dmSectionCollapsed", &Value::Bool(!look.dm_collapsed));
        });
        cx.notify();
    }

    /// The conversation of a menu, if it has one.
    fn pin_channel_of(&self, target: Target) -> Option<Id> {
        match target {
            Target::Dm(channel) => Some(channel),
            Target::User { user, .. } => self
                .stores
                .private_channels
                .iter()
                .find(|dm| dm.kind == 1 && dm.recipients == [user])
                .map(|dm| dm.id),
            _ => None,
        }
    }

    /// The menu with "Pin DMs" where the original puts it; as it is with the plugin off.
    pub(in crate::ui) fn with_pin_dms_row(
        &self,
        mut items: Vec<Entry>,
        target: Target,
    ) -> Vec<Entry> {
        if current().is_none() || self.pin_channel_of(target).is_none() {
            return items;
        }
        let mut row = Row::new(Act::PluginPinDms, "Pin DMs");
        row.caret = true;
        let at_act = |act: Act| {
            items
                .iter()
                .position(|entry| matches!(entry, Entry::Row(row) if row.act == act))
        };
        let at = if let Some(close) = at_act(Act::CloseDm) {
            close
        } else if let Some(leave) = at_act(Act::LeaveGroup) {
            // The head of the Leave Group row's group.
            items[..leave]
                .iter()
                .rposition(|entry| matches!(entry, Entry::Separator))
                .map_or(0, |separator| separator + 1)
        } else if let Some(message) = at_act(Act::Message) {
            message + 1
        } else {
            return items;
        };
        items.insert(at, Entry::Row(row));
        items
    }

    /// The rows under "Pin DMs".
    pub(in crate::ui) fn pin_dms_sub_rows(&self, target: Target) -> Vec<SubRow> {
        let (Some(look), Some(account), Some(channel)) =
            (current(), self.pin_account(), self.pin_channel_of(target))
        else {
            return Vec::new();
        };
        pin_rows(&categories(account), channel, look.custom_order)
            .into_iter()
            .map(|(_, label)| SubRow {
                label,
                ..SubRow::default()
            })
            .collect()
    }

    /// A row under "Pin DMs" ran.
    pub(in crate::ui) fn pin_dms_run_sub(
        &mut self,
        target: Target,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let (Some(look), Some(account), Some(channel)) =
            (current(), self.pin_account(), self.pin_channel_of(target))
        else {
            return;
        };
        let Some((row, _)) = pin_rows(&categories(account), channel, look.custom_order)
            .into_iter()
            .nth(index)
        else {
            return;
        };
        match row {
            PinRow::AddCategory => {
                self.open_pin_category_dialog(None, Some(channel), cx);
                return;
            }
            PinRow::Into(at) => change(account, |list| {
                if let Some(category) = list.get_mut(at)
                    && !category.channels.contains(&channel)
                {
                    category.channels.push(channel);
                }
            }),
            PinRow::Unpin => change(account, |list| {
                for category in list.iter_mut() {
                    category.channels.retain(|id| *id != channel);
                }
            }),
            PinRow::Up | PinRow::Down => change(account, |list| {
                if let Some(category) = list
                    .iter_mut()
                    .find(|category| category.channels.contains(&channel))
                {
                    let at = category
                        .channels
                        .iter()
                        .position(|id| *id == channel)
                        .unwrap_or(0);
                    swap(
                        &mut category.channels,
                        at,
                        if row == PinRow::Up { -1 } else { 1 },
                    );
                }
            }),
        }
        cx.notify();
    }

    /// A category header's menu (`vc-pindms-header-menu`).
    pub(in crate::ui) fn pin_category_items(&self, index: usize) -> Vec<Entry> {
        let Some(account) = self.pin_account().filter(|_| current().is_some()) else {
            return Vec::new();
        };
        let count = categories(account).len();
        if index >= count {
            return Vec::new();
        }
        let mut out = vec![Entry::Row(Row::new(
            Act::PluginPinEditCategory,
            "Edit Category",
        ))];
        if index > 0 {
            out.push(Entry::Row(Row::new(Act::PluginPinCategoryUp, "Move Up")));
        }
        if index + 1 < count {
            out.push(Entry::Row(Row::new(
                Act::PluginPinCategoryDown,
                "Move Down",
            )));
        }
        out.push(Entry::Separator);
        let mut delete = Row::new(Act::PluginPinDeleteCategory, "Delete Category");
        delete.danger = true;
        out.push(Entry::Row(delete));
        out
    }

    /// A row of a category's menu ran.
    pub(in crate::ui) fn pin_category_run(
        &mut self,
        act: Act,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.pin_account() else {
            return;
        };
        match act {
            Act::PluginPinEditCategory => {
                if let Some(category) = categories(account).get(index) {
                    self.open_pin_category_dialog(Some(category.clone()), None, cx);
                }
                return;
            }
            Act::PluginPinCategoryUp => change(account, |list| {
                swap(list, index, -1);
            }),
            Act::PluginPinCategoryDown => change(account, |list| {
                swap(list, index, 1);
            }),
            Act::PluginPinDeleteCategory => change(account, |list| {
                if index < list.len() {
                    list.remove(index);
                }
            }),
            _ => return,
        }
        cx.notify();
    }

    /// The category dialog was confirmed: the category is created (with its first conversation) or changed.
    pub(in crate::ui) fn pin_category_save(
        &mut self,
        draft: &PinCategoryDraft,
        name: String,
        color: u32,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.pin_account() else {
            return;
        };
        if name.is_empty() {
            return;
        }
        change(account, |list| match &draft.editing {
            Some(id) => {
                if let Some(category) = list.iter_mut().find(|category| category.id == *id) {
                    category.name = name;
                    category.color = color;
                }
            }
            None => list.push(Category {
                id: new_id(),
                name,
                color,
                channels: draft.channel.into_iter().collect(),
                collapsed: false,
            }),
        });
        cx.notify();
    }

    /// The default name of a new category: `Pin Category <n + 1>`.
    pub(in crate::ui) fn pin_category_default_name(&self) -> String {
        let count = self
            .pin_account()
            .map_or(0, |account| categories(account).len());
        format!("Pin Category {}", count + 1)
    }
}

/// What the category dialog edits: an existing category (its id), or a new one with its first conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct PinCategoryDraft {
    pub(in crate::ui) editing: Option<String>,
    pub(in crate::ui) channel: Option<Id>,
}

/// `crypto.randomUUID()`.
fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let salt = std::process::id() as u128;
    let value = nanos ^ (salt << 64) ^ 0x4000_0000_0000_8000_0000_0000_0000_0000;
    let hex = format!("{value:032x}");
    format!(
        "{}-{}-4{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(id: &str, channels: &[Id]) -> Category {
        Category {
            id: id.to_owned(),
            name: id.to_owned(),
            color: DEFAULT_COLOR,
            channels: channels.to_vec(),
            collapsed: false,
        }
    }

    #[test]
    fn categories_round_trip_and_order_by_recency_or_as_kept() {
        let one = category("a", &[3, 1, 2]);
        assert_eq!(Category::from_value(&one.to_value()), Some(one.clone()));
        assert_eq!(ordered(&one, true, &[1, 2, 3]), vec![3, 1, 2]);
        assert_eq!(ordered(&one, false, &[2, 3, 1]), vec![2, 3, 1]);
        assert_eq!(
            ordered(&one, false, &[9]),
            vec![3, 1, 2],
            "unknown recency keeps the order"
        );
    }

    #[test]
    fn the_pin_menu_offers_categories_or_unpin_and_moves() {
        let list = vec![category("Work", &[5, 6]), category("Play", &[])];
        let labels =
            |rows: Vec<(PinRow, String)>| rows.into_iter().map(|(_, l)| l).collect::<Vec<_>>();
        assert_eq!(
            labels(pin_rows(&list, 9, false)),
            ["Add Category", "Work", "Play"]
        );
        assert_eq!(labels(pin_rows(&list, 5, false)), ["Unpin DM"]);
        assert_eq!(labels(pin_rows(&list, 5, true)), ["Unpin DM", "Move Down"]);
        assert_eq!(labels(pin_rows(&list, 6, true)), ["Unpin DM", "Move Up"]);
    }

    #[test]
    fn swaps_stay_inside_the_list_and_settings_default_off() {
        let mut list = vec![1, 2, 3];
        assert!(swap(&mut list, 0, 1));
        assert_eq!(list, [2, 1, 3]);
        assert!(!swap(&mut list, 0, -1));
        assert!(!swap(&mut list, 2, 1));
        let mut state = State::default();
        assert_eq!(look(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(
            look(&state),
            Some(Look {
                custom_order: false,
                can_collapse: false,
                dm_collapsed: false
            })
        );
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
    }
}
