//! Keyboard focus stops: the ring, the roving tab stop of a list and the arrow keys inside one.
//!
//! Official rules this follows:
//! - Keyboard mode (`keyboardModeEnabled`, module 775602) is switched on by F6, Tab, the arrows, Home and End when
//!   no field has focus (Tab works from a field too), without Ctrl, Alt or Meta, and Shift only with Tab; module
//!   203480 (`A`). A mouse press switches it off. The ring shows only in that mode (module 596339 `setRingsEnabled`).
//! - The ring (module 559106, `.focus-rings-ring` in 193433.css): a 4px `box-shadow` outside the target, following
//!   the target's corner radius, in `--focus-primary` with the stylesheet's own fallback `#00b0f4`.
//! - A list keeps one tab stop, the item that has focus or is selected (the roving `tabIndex` of 36544 and 837381);
//!   the others are reached with the arrows.
//! - A list navigator (36544) stops at both ends and answers Home and End; a menu navigator (165862) wraps at both
//!   ends, has no Home or End, and Left and Right only step into and out of a submenu.
//! - Enter and Space activate the focused item as a click does: gpui turns them into the element's own `on_click`.
//!
//! Elements are made focusable with [`Root::stop`] (one persistent handle per key), [`Root::join`] (the group the
//! arrows move in, in render order) and [`focusable`] (tracked, tab stop and ring).

use super::Keystroke;
use crate::ui::Root;
use gpui::{
    App, BoxShadow, Context, FocusHandle, InteractiveElement, Styled, Window, point, px, rgba,
};
use std::cell::RefCell;
use std::collections::HashMap;

/// `--focus-primary` is not defined by any stylesheet of the build, so the ring uses the fallback of
/// `.focus-rings-ring` in 193433.css.
const RING: u32 = 0x00b0f4ff;

/// The width of the ring's `box-shadow`.
const RING_WIDTH: f32 = 4.;

/// Keyboard mode, mirrored for the dialog and control builders that have no `Root` to ask.
static KEYBOARD_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether keyboard mode is on (module 775602 `keyboardModeEnabled`).
pub(in crate::ui) fn keyboard_mode_on() -> bool {
    KEYBOARD_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

impl super::State {
    /// Switches keyboard mode on or off.
    pub(in crate::ui) fn set_modality(&mut self, on: bool) {
        self.keyboard_modality = on;
        KEYBOARD_MODE.store(on, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The ring of a control the builders make without a `Root`: a 4px `box-shadow` while it has focus in keyboard mode.
pub(in crate::ui) fn ring_when_focused<E: InteractiveElement>(element: E) -> E {
    if keyboard_mode_on() {
        element.focus(|style| style.shadow(ring_shadow()))
    } else {
        element
    }
}

/// How a group answers the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Nav {
    /// Up and Down, stopping at the ends, Home and End (`useListNavigator`, 36544).
    List,
    /// Up and Down, wrapping at the ends, nothing else (the menu navigator of 165862).
    Menu,
    /// Left and Right, wrapping, Home and End (a tab bar's navigator).
    #[allow(dead_code)] // tab bars: not wired to a screen yet
    Tabs,
    /// Up, Down, Left and Right over rows of equal length, Home and End (an emoji or sticker grid).
    #[allow(dead_code)] // emoji and sticker grids: not wired yet
    Grid(usize),
}

/// One step of a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Prev,
    Next,
    First,
    Last,
    Above(usize),
    Below(usize),
    RowStart(usize),
    RowEnd(usize),
}

/// The step a key makes in a group of this kind, or `None` when the group leaves it alone. `ctrl` matters only to a
/// grid, where Ctrl+Home and Ctrl+End reach the first and the last cell (788413).
pub(super) fn step_for(nav: Nav, key: &str, ctrl: bool) -> Option<Step> {
    match (nav, key) {
        (Nav::List | Nav::Menu, "up") | (Nav::Tabs, "left") | (Nav::Grid(_), "left") => {
            Some(Step::Prev)
        }
        (Nav::List | Nav::Menu, "down") | (Nav::Tabs, "right") | (Nav::Grid(_), "right") => {
            Some(Step::Next)
        }
        (Nav::List | Nav::Tabs, "home") => Some(Step::First),
        (Nav::List | Nav::Tabs, "end") => Some(Step::Last),
        (Nav::Grid(_), "home") if ctrl => Some(Step::First),
        (Nav::Grid(_), "end") if ctrl => Some(Step::Last),
        (Nav::Grid(columns), "home") => Some(Step::RowStart(columns)),
        (Nav::Grid(columns), "end") => Some(Step::RowEnd(columns)),
        (Nav::Grid(columns), "up") => Some(Step::Above(columns)),
        (Nav::Grid(columns), "down") => Some(Step::Below(columns)),
        _ => None,
    }
}

/// Where a step lands from `at` among `count` items; `None` when the item stays.
pub(super) fn target(nav: Nav, step: Step, at: usize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    let wraps = matches!(nav, Nav::Menu | Nav::Tabs);
    let to = match step {
        Step::First => 0,
        Step::Last => last,
        Step::Prev if at == 0 => {
            if wraps {
                last
            } else {
                0
            }
        }
        Step::Prev => at - 1,
        Step::Next if at == last => {
            if wraps {
                0
            } else {
                last
            }
        }
        Step::Next => at + 1,
        Step::Above(columns) => at.checked_sub(columns).unwrap_or(at),
        // The next row's cell below, or its last cell when that row is shorter; the last row stays.
        Step::Below(columns) => {
            if at / columns < last / columns {
                (at + columns).min(last)
            } else {
                at
            }
        }
        Step::RowStart(columns) => at - at % columns,
        Step::RowEnd(columns) => (at - at % columns + columns - 1).min(last),
    };
    (to != at).then_some(to)
}

/// One member of a group: its handle, the list row it sits in (to scroll it into view) and the id of what it shows.
struct Member {
    handle: FocusHandle,
    row: Option<usize>,
    id: Option<u64>,
    /// The view that laid the stop out (the main view or a region), so a region that is not redrawn keeps its stops.
    view: Option<gpui::EntityId>,
}

/// A group of focus stops that share the arrow keys, in render order.
struct Group {
    name: &'static str,
    nav: Nav,
    items: Vec<Member>,
}

impl Group {
    fn position(&self, handle: &FocusHandle) -> Option<usize> {
        self.items
            .iter()
            .position(|member| member.handle == *handle)
    }
}

/// The handles that live across frames and the groups the last frame laid out.
#[derive(Default)]
pub(in crate::ui) struct Stops {
    handles: RefCell<HashMap<String, FocusHandle>>,
    groups: RefCell<Vec<Group>>,
    /// A menu was laid out in the last frame.
    menu_seen: std::cell::Cell<bool>,
    /// A dialog was open in the last frame, and is waiting for its first control to take focus.
    dialog_seen: std::cell::Cell<bool>,
    dialog_wait: std::cell::Cell<bool>,
    /// The focus the frame was started with, for the overlays that cannot ask the window.
    focused: RefCell<Option<FocusHandle>>,
    /// The first row that is fully in view, read before the list renders (its state is borrowed while it does).
    pub(super) first_row: std::cell::Cell<usize>,
}

impl Stops {
    /// A frame starts: the groups the main view laid out are laid out again, the handles stay. What the regions laid
    /// out is kept until the region is drawn again (`begin_region`).
    pub(in crate::ui) fn begin_frame(&self, focused: Option<&FocusHandle>, first_row: usize) {
        self.first_row.set(first_row);
        crate::ui::modal::begin_frame();
        let root = crate::ui::motion::current_view();
        self.clear_view(|view| view.is_none() || view == root);
        *self.focused.borrow_mut() = focused.cloned();
    }

    /// A region is drawn again: the stops it laid out last time are laid out again.
    pub(in crate::ui) fn begin_region(&self, region: Option<gpui::EntityId>) {
        self.clear_view(|view| view == region);
    }

    /// Drops the stops laid out by the views `gone` accepts, and the groups that are left empty.
    fn clear_view(&self, gone: impl Fn(Option<gpui::EntityId>) -> bool) {
        let mut groups = self.groups.borrow_mut();
        for group in groups.iter_mut() {
            group.items.retain(|member| !gone(member.view));
        }
        groups.retain(|group| !group.items.is_empty());
    }

    /// Whether `handle` had focus when the frame started.
    /// The key a stop was made with.
    pub(in crate::ui) fn key_of(&self, handle: &FocusHandle) -> Option<String> {
        self.handles
            .borrow()
            .iter()
            .find(|(_, other)| *other == handle)
            .map(|(key, _)| key.clone())
    }

    pub(in crate::ui) fn has_focus(&self, handle: &FocusHandle) -> bool {
        self.focused.borrow().as_ref() == Some(handle)
    }

    /// The message a Tab into the list lands on: the first one at or below `first` row, or the newest.
    pub(super) fn entry_message(&self, first: Option<usize>) -> Option<(FocusHandle, usize)> {
        let groups = self.groups.borrow();
        let group = groups.iter().find(|group| group.name == "messages")?;
        let pick = first
            .and_then(|first| {
                group
                    .items
                    .iter()
                    .find(|member| member.row.is_some_and(|row| row >= first))
            })
            .or(group.items.last())?;
        Some((pick.handle.clone(), pick.row?))
    }

    /// The id of the message that has focus, for the shortcuts that act on it.
    pub(super) fn focused_message(&self, focused: &FocusHandle) -> Option<u64> {
        let groups = self.groups.borrow();
        let group = groups.iter().find(|group| group.name == "messages")?;
        group
            .items
            .iter()
            .find(|member| member.handle == *focused)?
            .id
    }

    /// The group name and key of the focused stop, for the harness.
    #[cfg(feature = "capture")]
    pub(super) fn describe(&self, focused: &FocusHandle) -> Option<String> {
        let handles = self.handles.borrow();
        let (key, _) = handles.iter().find(|(_, handle)| *handle == focused)?;
        let groups = self.groups.borrow();
        let group = groups
            .iter()
            .find(|group| group.position(focused).is_some())
            .map(|group| group.name)
            .unwrap_or("stop");
        Some(format!("{group}:{key}"))
    }
}

/// The corner radius of a ring is the target's own; the shadow follows the element it is set on, so only the colour
/// and the width live here.
fn ring_shadow() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: rgba(RING).into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(RING_WIDTH),
        inset: false,
    }]
}

/// The ring of a control that has nothing behind it and no `Root` to ask about focus: an inset frame of the same
/// width inside its box, which a shadow-less control can show without a fill.
pub(in crate::ui) fn inset_ring_when_focused<E: InteractiveElement>(element: E) -> E {
    if keyboard_mode_on() {
        element.focus(|style| {
            style.shadow(vec![BoxShadow {
                color: rgba(RING).into(),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(RING_WIDTH),
                inset: true,
            }])
        })
    } else {
        element
    }
}

/// A ring drawn as a frame around the control, for a control with nothing behind it: 4px wide, outside the control's
/// own box, with the corner radius the control has plus the ring's width.
pub(in crate::ui) fn hollow_ring(radius: f32) -> impl gpui::IntoElement {
    gpui::div()
        .absolute()
        .left(px(-RING_WIDTH))
        .right(px(-RING_WIDTH))
        .top(px(-RING_WIDTH))
        .bottom(px(-RING_WIDTH))
        .rounded(px(radius + RING_WIDTH))
        .border(px(RING_WIDTH))
        .border_color(rgba(RING))
}

/// Makes `element` tracked by `handle`, in the tab order only while `current`, with no ring of its own (a message
/// draws its ring as a child).
pub(in crate::ui) fn tab_stop<E: InteractiveElement>(
    element: E,
    handle: &FocusHandle,
    current: bool,
) -> E {
    let handle = handle.clone().tab_index(0).tab_stop(current);
    element.track_focus(&handle)
}

/// Makes `element` a tab stop with a ring: tracked by `handle`, in the tab order only while `current`, and ringed
/// while it has focus in keyboard mode. Enter and Space click it, through the `on_click` the element already has.
pub(in crate::ui) fn focusable<E: InteractiveElement>(
    element: E,
    handle: &FocusHandle,
    current: bool,
    keyboard_mode: bool,
) -> E {
    // gpui reads the tab order from the tracked handle itself when the element is given one.
    let handle = handle.clone().tab_index(0).tab_stop(current);
    let element = element.track_focus(&handle);
    if keyboard_mode {
        element.focus(|style| style.shadow(ring_shadow()))
    } else {
        element
    }
}

impl Root {
    /// The persistent focus handle of a stop, made on first use.
    pub(in crate::ui) fn stop(&self, key: &str, cx: &mut App) -> FocusHandle {
        self.keys
            .stops
            .handles
            .borrow_mut()
            .entry(key.to_owned())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Adds `handle` to the group the arrow keys move in. True when it is the group's first item.
    pub(in crate::ui) fn join(&self, name: &'static str, nav: Nav, handle: &FocusHandle) -> bool {
        self.join_member(name, nav, handle, None, None)
    }

    /// As `join`, for an item of a virtual list: the row it is in and the id of what it shows.
    pub(in crate::ui) fn join_member(
        &self,
        name: &'static str,
        nav: Nav,
        handle: &FocusHandle,
        row: Option<usize>,
        id: Option<u64>,
    ) -> bool {
        let member = Member {
            handle: handle.clone(),
            row,
            id,
            view: crate::ui::motion::current_view(),
        };
        let mut groups = self.keys.stops.groups.borrow_mut();
        match groups.iter_mut().find(|group| group.name == name) {
            Some(group) => {
                // A virtual list renders from the bottom up: members of rows stay in the order of their rows.
                match row {
                    Some(row) => {
                        let at = group
                            .items
                            .partition_point(|other| other.row.is_some_and(|other| other < row));
                        group.items.insert(at, member);
                    }
                    None => group.items.push(member),
                }
                false
            }
            None => {
                groups.push(Group {
                    name,
                    nav,
                    items: vec![member],
                });
                true
            }
        }
    }

    /// A toolbar button or a composer button: a tab stop of its own (the official buttons all have `tabIndex 0`),
    /// ringed in keyboard mode; Enter and Space click it. It sits out of the tab order under a dialog or a menu.
    pub(in crate::ui) fn header_stop<E: InteractiveElement + Styled + gpui::ParentElement>(
        &self,
        element: E,
        key: &str,
        cx: &mut App,
    ) -> E {
        let handle = self.stop(&format!("tool-{key}"), cx);
        let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        // These controls have no background, so the ring is a hollow frame, not a shadow.
        let element = tab_stop(
            element.relative().rounded(px(4.)),
            &handle,
            self.modal_open() == 0,
        );
        if ringed {
            element.child(hollow_ring(4.))
        } else {
            element
        }
    }

    /// A title bar button: a tab stop of its own, ringed in keyboard mode with a frame around its own corners (the
    /// bar's buttons have `tabIndex 0`); Enter and Space click it. It sits out of the tab order under a dialog.
    pub(in crate::ui) fn title_stop<E: InteractiveElement + gpui::ParentElement>(
        &self,
        element: E,
        key: &str,
        radius: f32,
        cx: &mut App,
    ) -> E {
        let handle = self.stop(&format!("title-{key}"), cx);
        let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        let element = tab_stop(element, &handle, self.modal_open() == 0);
        if ringed {
            element.child(hollow_ring(radius))
        } else {
            element
        }
    }

    /// A row of an open menu: focused through the menu's own navigator (module 165862), which marks the focused row
    /// with the same background as hover; it is never a tab stop, and Enter or Space click it.
    pub(in crate::ui) fn menu_stop<E: InteractiveElement>(
        &self,
        element: E,
        key: &str,
        focused_background: gpui::Rgba,
        cx: &mut App,
    ) -> E {
        let handle = self.stop(key, cx);
        self.join("menu", Nav::Menu, &handle);
        let handle = handle.tab_stop(false);
        let element = element.track_focus(&handle);
        if self.keyboard_mode() {
            element.focus(move |style| style.bg(focused_background))
        } else {
            element
        }
    }

    /// A menu that opens in keyboard mode starts with its first row focused (`initialFocusPath`, 165862).
    pub(in crate::ui) fn focus_new_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = {
            let groups = self.keys.stops.groups.borrow();
            groups
                .iter()
                .find(|group| group.name == "menu")
                .and_then(|group| group.items.first().map(|member| member.handle.clone()))
        };
        let was_open = self.keys.stops.menu_seen.replace(first.is_some());
        if let (Some(first), false, true) = (first, was_open, self.keys.keyboard_modality) {
            window.focus(&first, cx);
        }
    }

    /// A dialog that has just opened takes focus on its first control, as the layer's focus lock does, unless a field
    /// inside it already has focus. The dialog is painted one frame before its controls can be reached.
    pub(in crate::ui) fn focus_new_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let menu = self.keys.stops.menu_seen.get();
        let open = self.modal_open() > 0 && !menu;
        let was = self.keys.stops.dialog_seen.replace(open);
        if open && !was {
            self.keys.stops.dialog_wait.set(true);
            cx.notify();
        } else if open && self.keys.stops.dialog_wait.replace(false) {
            let typing = window
                .focused(cx)
                .is_some_and(|handle| crate::editor::is_field(&handle));
            if !typing {
                // The primary button when the dialog has one (Enter confirms), else the first control.
                match crate::ui::modal::dialog_primary() {
                    Some(primary) => window.focus(&primary, cx),
                    None => window.focus_next(cx),
                }
            }
        } else if !open {
            self.keys.stops.dialog_wait.set(false);
        }
    }

    /// Whether the ring shows: the last input switched keyboard mode on.
    pub(in crate::ui) fn keyboard_mode(&self) -> bool {
        self.keys.keyboard_modality
    }

    /// A stop of `group` that is in the tab order and ringed as the group's rules say. `current` is the item that
    /// keeps the group's tab stop (the selected one).
    pub(in crate::ui) fn group_stop<E: InteractiveElement>(
        &self,
        element: E,
        group: &'static str,
        nav: Nav,
        key: &str,
        current: bool,
        cx: &mut App,
    ) -> E {
        let handle = self.stop(key, cx);
        self.join(group, nav, &handle);
        // Under a dialog or a menu the page behind is out of the tab order (the focus lock of the layer).
        focusable(
            element,
            &handle,
            current && self.modal_open() == 0,
            self.keyboard_mode(),
        )
    }

    /// Tab and Shift+Tab move to the next and the previous tab stop, as the browser does with `tabindex`.
    pub(super) fn tab_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        if keystroke.key != "tab" || modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        if modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if let Some(focused) = window.focused(cx) {
            self.enter_messages(&focused, window, cx);
        }
        self.keys.set_modality(true);
        cx.notify();
        true
    }

    /// As `group_stop`, for a row with nothing behind it: the ring is a hollow frame around it, not a shadow.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn group_stop_hollow<E: InteractiveElement + Styled + gpui::ParentElement>(
        &self,
        element: E,
        group: &'static str,
        nav: Nav,
        key: &str,
        current: bool,
        radius: f32,
        cx: &mut App,
    ) -> E {
        let handle = self.stop(key, cx);
        self.join(group, nav, &handle);
        let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&handle);
        let element = tab_stop(
            element.relative(),
            &handle,
            current && self.modal_open() == 0,
        );
        if ringed {
            element.child(hollow_ring(radius))
        } else {
            element
        }
    }

    /// An arrow, Home or End moves focus inside the group that holds the focused stop. True when it did.
    pub(super) fn stops_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        let ctrl = modifiers.control && matches!(keystroke.key.as_str(), "home" | "end");
        if (modifiers.control && !ctrl) || modifiers.alt || modifiers.shift || modifiers.platform {
            return false;
        }
        let focused = window.focused(cx);
        // An open menu that has not taken focus yet: the first arrow focuses its first or last row.
        let entry = {
            let groups = self.keys.stops.groups.borrow();
            let menu = groups.iter().find(|group| group.name == "menu");
            let inside = focused
                .as_ref()
                .zip(menu)
                .is_some_and(|(handle, menu)| menu.position(handle).is_some());
            match (inside, menu) {
                (false, Some(menu)) => match keystroke.key.as_str() {
                    "down" => menu.items.first().map(|member| member.handle.clone()),
                    "up" => menu.items.last().map(|member| member.handle.clone()),
                    _ => None,
                },
                _ => None,
            }
        };
        if let Some(handle) = entry {
            window.focus(&handle, cx);
            self.keys.set_modality(true);
            cx.notify();
            return true;
        }
        let Some(focused) = focused else {
            return false;
        };
        let landing = {
            let groups = self.keys.stops.groups.borrow();
            let Some(group) = groups
                .iter()
                .find(|group| group.position(&focused).is_some())
            else {
                return false;
            };
            let Some(step) = step_for(group.nav, keystroke.key.as_str(), ctrl) else {
                return false;
            };
            let at = group.position(&focused).unwrap_or(0);
            let to = target(group.nav, step, at, group.items.len());
            if to.is_none() && group.name == "messages" && keystroke.key == "down" {
                // Down from the last message goes to the text area (`onNavigateNextAtEnd`, 891522).
                (None, true)
            } else {
                (
                    to.map(|to| {
                        let member = &group.items[to];
                        (member.handle.clone(), member.row)
                    }),
                    false,
                )
            }
        };
        let (landing, to_composer) = landing;
        if to_composer {
            self.focus_composer(window, cx);
        }
        if let Some((handle, row)) = landing {
            window.focus(&handle, cx);
            self.reveal_settings_nav(&handle);
            if let Some(row) = row {
                self.list.scroll_to_reveal_item(row);
            }
        }
        self.keys.set_modality(true);
        cx.notify();
        true
    }
}

/// Whether a key press switches keyboard mode on (module 203480): F6, Tab, the arrows, Home and End; no Ctrl, Alt or
/// Meta; Shift only with Tab; and not while a field has focus unless the key is Tab.
pub(super) fn enables_keyboard_mode(keystroke: &Keystroke, typing: bool) -> bool {
    let modifiers = keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform {
        return false;
    }
    let key = keystroke.key.as_str();
    if modifiers.shift && key != "tab" {
        return false;
    }
    if typing && key != "tab" {
        return false;
    }
    matches!(
        key,
        "f6" | "tab" | "up" | "down" | "left" | "right" | "home" | "end"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    #[test]
    fn a_list_stops_at_its_ends_and_answers_home_and_end() {
        let nav = Nav::List;
        assert_eq!(
            target(nav, step_for(nav, "down", false).unwrap(), 0, 3),
            Some(1)
        );
        assert_eq!(
            target(nav, step_for(nav, "down", false).unwrap(), 2, 3),
            None
        );
        assert_eq!(target(nav, step_for(nav, "up", false).unwrap(), 0, 3), None);
        assert_eq!(
            target(nav, step_for(nav, "home", false).unwrap(), 2, 3),
            Some(0)
        );
        assert_eq!(
            target(nav, step_for(nav, "end", false).unwrap(), 0, 3),
            Some(2)
        );
        assert_eq!(step_for(nav, "left", false), None);
    }

    #[test]
    fn a_menu_wraps_and_has_no_home_or_end() {
        let nav = Nav::Menu;
        assert_eq!(
            target(nav, step_for(nav, "down", false).unwrap(), 2, 3),
            Some(0)
        );
        assert_eq!(
            target(nav, step_for(nav, "up", false).unwrap(), 0, 3),
            Some(2)
        );
        assert_eq!(step_for(nav, "home", false), None);
        assert_eq!(step_for(nav, "end", false), None);
    }

    #[test]
    fn tabs_move_sideways_and_wrap() {
        let nav = Nav::Tabs;
        assert_eq!(
            target(nav, step_for(nav, "right", false).unwrap(), 2, 3),
            Some(0)
        );
        assert_eq!(
            target(nav, step_for(nav, "left", false).unwrap(), 0, 3),
            Some(2)
        );
        assert_eq!(step_for(nav, "down", false), None);
    }

    #[test]
    fn a_grid_moves_by_rows_wraps_sideways_and_ends_in_a_short_row() {
        let nav = Nav::Grid(4);
        let go = |key: &str, ctrl: bool, at: usize| {
            target(nav, step_for(nav, key, ctrl).unwrap(), at, 10)
        };
        assert_eq!(go("down", false, 1), Some(5));
        assert_eq!(go("down", false, 7), Some(9));
        assert_eq!(go("down", false, 9), None);
        assert_eq!(go("up", false, 5), Some(1));
        assert_eq!(go("up", false, 2), None);
        assert_eq!(go("right", false, 3), Some(4));
        assert_eq!(go("left", false, 4), Some(3));
        assert_eq!(go("right", false, 9), None);
        assert_eq!(go("home", false, 6), Some(4));
        assert_eq!(go("end", false, 6), Some(7));
        assert_eq!(go("end", false, 8), Some(9));
        assert_eq!(go("home", true, 6), Some(0));
        assert_eq!(go("end", true, 6), Some(9));
    }

    #[test]
    fn an_empty_group_goes_nowhere() {
        assert_eq!(target(Nav::List, Step::Next, 0, 0), None);
    }

    #[test]
    fn navigation_keys_switch_keyboard_mode_on() {
        for key in [
            "tab",
            "shift-tab",
            "up",
            "down",
            "left",
            "right",
            "home",
            "end",
            "f6",
        ] {
            assert!(enables_keyboard_mode(&stroke(key), false), "{key}");
        }
        for key in [
            "a", "enter", "space", "escape", "pageup", "ctrl-tab", "alt-left", "shift-up",
        ] {
            assert!(!enables_keyboard_mode(&stroke(key), false), "{key}");
        }
    }

    #[test]
    fn a_field_only_lets_tab_switch_keyboard_mode_on() {
        assert!(enables_keyboard_mode(&stroke("tab"), true));
        assert!(!enables_keyboard_mode(&stroke("down"), true));
        assert!(!enables_keyboard_mode(&stroke("home"), true));
    }
}
