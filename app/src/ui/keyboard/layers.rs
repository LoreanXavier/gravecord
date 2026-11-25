//! The layers that sit over the page, and the rule that a press outside the top one closes it.
//!
//! Official rule: every popout, select list, picker and menu is a layer with `onRequestClose` (the layer stack of
//! module 82038, the popout layer `ZP` in module 549587, the menus of module 165862). A mouse press anywhere outside
//! the top layer closes that layer only; the press is not delivered to what is under it. Escape closes the same top
//! layer. The popouts of `is_popout` follow the official `Popout` instead: the press goes on, and a popout opened from
//! a row of another popout is its child (see `is_popout`). Modal dialogs close on a press on their backdrop (`ModalRoot` `onClose`), which `modal::modal` already does
//! for every dialog built with it.
//!
//! `Root::open_layers` lists what is open from bottom to top, `Root::close_layer` closes one the way Escape does, and
//! `outside` is the one-line hook a layer's card carries: `outside(card, Layer::X, cx)`.

use super::super::Root;
use super::super::settings::Choice;
use gpui::{
    Bounds, Context, InteractiveElement, IntoElement as _, MouseDownEvent, Pixels, Point,
    Styled as _,
};

/// How a layer answers a press outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Kind {
    /// A popout, menu, select list or picker: the press closes it and goes no further.
    Popover,
}

/// Every popover-like layer, in the order they stack (later is above).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum Layer {
    PlusMenu,
    Picker,
    Pins,
    Inbox,
    MentionsFilter,
    Notify,
    GuildMenu,
    FolderMenu,
    AccountMenu,
    StatusPopout,
    ThreadBrowser,
    SearchFilters,
    ForumMenu,
    InventoryPopout,
    AddGame,
    SettingsSelect,
    EmojiPopout,
    StickerPopout,
    RolePopout,
    HeaderPopout,
    VoicePanelPopout,
    Soundboard,
    Profile,
    RolePicker,
    /// Not in the pinned client: PermissionsViewer's member popout (`plugin_hooks/permissions_viewer.rs`).
    PluginPermissions,
    FileMenu,
    AudioMenu,
    FriendMenu,
    ProfileMenu,
    SnoozeMenu,
    ClipMenu,
    Menu,
    Submenu,
    ContextMenu,
    /// The list of a select under a bot message (`popout_uFkqU2`).
    ComponentSelect,
    /// The list of a select in an application's form dialog (`INTERACTION_MODAL_CREATE`).
    InteractionSelect,
}

/// Every layer, bottom to top. A test checks the count against the variants.
pub(in crate::ui) const ALL: &[Layer] = &[
    Layer::PlusMenu,
    Layer::Picker,
    Layer::Pins,
    Layer::Inbox,
    Layer::MentionsFilter,
    Layer::Notify,
    Layer::GuildMenu,
    Layer::FolderMenu,
    Layer::AccountMenu,
    Layer::StatusPopout,
    Layer::ThreadBrowser,
    Layer::SearchFilters,
    Layer::ForumMenu,
    Layer::InventoryPopout,
    Layer::AddGame,
    Layer::SettingsSelect,
    Layer::EmojiPopout,
    Layer::StickerPopout,
    Layer::RolePopout,
    Layer::HeaderPopout,
    Layer::VoicePanelPopout,
    Layer::Soundboard,
    Layer::Profile,
    Layer::RolePicker,
    Layer::PluginPermissions,
    Layer::FileMenu,
    Layer::AudioMenu,
    Layer::FriendMenu,
    Layer::ProfileMenu,
    Layer::SnoozeMenu,
    Layer::ClipMenu,
    Layer::Menu,
    Layer::Submenu,
    Layer::ContextMenu,
    Layer::ComponentSelect,
    Layer::InteractionSelect,
];

impl Layer {
    /// Exhaustive on purpose: a new layer must say how it answers a press outside it.
    pub(in crate::ui) fn kind(self) -> Kind {
        match self {
            Layer::PlusMenu
            | Layer::Picker
            | Layer::Pins
            | Layer::Inbox
            | Layer::MentionsFilter
            | Layer::Notify
            | Layer::GuildMenu
            | Layer::FolderMenu
            | Layer::AccountMenu
            | Layer::StatusPopout
            | Layer::ThreadBrowser
            | Layer::SearchFilters
            | Layer::ForumMenu
            | Layer::InventoryPopout
            | Layer::AddGame
            | Layer::SettingsSelect
            | Layer::EmojiPopout
            | Layer::StickerPopout
            | Layer::RolePopout
            | Layer::HeaderPopout
            | Layer::VoicePanelPopout
            | Layer::Soundboard
            | Layer::Profile
            | Layer::RolePicker
            | Layer::PluginPermissions
            | Layer::FileMenu
            | Layer::AudioMenu
            | Layer::FriendMenu
            | Layer::ProfileMenu
            | Layer::SnoozeMenu
            | Layer::ClipMenu
            | Layer::Menu
            | Layer::Submenu
            | Layer::ContextMenu
            | Layer::ComponentSelect
            | Layer::InteractionSelect => Kind::Popover,
        }
    }
}

/// The layers Escape closes through `escape_layer`; every other layer has its own branch in the root key handler.
const ESCAPE_HERE: &[Layer] = &[
    Layer::AddGame,
    Layer::SettingsSelect,
    Layer::HeaderPopout,
    Layer::VoicePanelPopout,
    Layer::Soundboard,
    Layer::MentionsFilter,
    Layer::FileMenu,
    Layer::ProfileMenu,
    Layer::RolePicker,
    Layer::PluginPermissions,
    Layer::ComponentSelect,
    Layer::InteractionSelect,
    Layer::FolderMenu,
    Layer::AudioMenu,
];

/// The layers built on the official `Popout` (module 453903) without `clickTrap`: the role member list of a role
/// mention (module 593284) and the user popout (module 342296). Menus (module 318009, `blockPointerEvents`) and the
/// other popovers keep the trapping rule above.
///
/// A `Popout` listens for presses on the whole document and closes on one that is not inside its own trigger, where
/// "inside" follows the chain of layers (module 623646 `H`): a press inside a popout opened from a row of another
/// popout is inside both. It never stops the press, so a press outside closes the popout and still reaches what is
/// under it, and a press on its own trigger toggles it closed (`handleClick`). Escape closes every open popout at once
/// (`POPOUT_CLOSE` through module 390435, and a child unmounts with the row it was opened from).
pub(in crate::ui) fn is_popout(layer: Layer) -> bool {
    matches!(layer, Layer::RolePopout | Layer::Profile)
}

/// Which popout each open popout was opened from, and where the popout cards were last drawn.
#[derive(Default)]
pub(in crate::ui) struct Links {
    parents: std::collections::HashMap<Layer, Layer>,
    cards: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<Layer, Bounds<Pixels>>>>,
}

impl Links {
    pub(in crate::ui) fn parent(&self, child: Layer) -> Option<Layer> {
        self.parents.get(&child).copied()
    }

    /// `child` just opened while `open` (bottom to top, `child` among them) was on screen: it belongs to the popout
    /// it was opened from, which is the top layer under it when that is a popout (a press anywhere else would have
    /// closed that popout first).
    fn adopt(&mut self, child: Layer, open: &[Layer]) {
        let under = open.iter().rev().find(|layer| **layer != child).copied();
        match under {
            Some(parent) if is_popout(parent) && is_popout(child) => {
                self.parents.insert(child, parent);
            }
            _ => {
                self.parents.remove(&child);
            }
        }
    }

    /// The popouts opened from `layer`, and from them, in the order they must close.
    fn descendants(&self, layer: Layer) -> Vec<Layer> {
        let mut out = Vec::new();
        let mut frontier = vec![layer];
        while let Some(parent) = frontier.pop() {
            for (child, of) in &self.parents {
                if *of == parent && !out.contains(child) {
                    out.push(*child);
                    frontier.push(*child);
                }
            }
        }
        out
    }

    fn forget(&mut self, layer: Layer) {
        self.parents.remove(&layer);
        self.cards.borrow_mut().remove(&layer);
    }

    fn card(&self, layer: Layer) -> Option<Bounds<Pixels>> {
        self.cards.borrow().get(&layer).copied()
    }
}

/// What a press outside the top layer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Outside {
    /// Nothing: the layer is not the top one, or another layer already answered this press.
    Ignored,
    /// The layer closed and the press stops here.
    Trapped,
    /// The layer closed (with the popouts it was opened from that the press was outside of) and the press goes on.
    Delivered,
}

/// Where a press lands for a popout chain, from the top popout down: `Trapped` on the top popout's own trigger
/// (`handleClick` toggles it closed), otherwise the popouts the press is outside of close from the top until one
/// holds the press, and the press goes on (`Delivered`). `inside(layer)` tells whether the press is inside that
/// popout's card; `on_trigger` whether it is on the top popout's trigger. Returns the layers to close.
fn chain_press(
    top: Layer,
    parent: impl Fn(Layer) -> Option<Layer>,
    inside: impl Fn(Layer) -> bool,
    on_trigger: bool,
) -> (Vec<Layer>, Outside) {
    let mut close = vec![top];
    if on_trigger {
        return (close, Outside::Trapped);
    }
    let mut at = top;
    while let Some(up) = parent(at) {
        if inside(up) {
            break;
        }
        close.push(up);
        at = up;
    }
    (close, Outside::Delivered)
}

/// A submenu is a layer only while the menu it hangs off is open.
fn submenu_is_open(submenu: bool, menu: bool) -> bool {
    submenu && menu
}

/// The open flag of a settings select (`select_with_list`'s `open_key`); the add-game popout's flag is not one.
fn is_select_key(key: &str) -> bool {
    key.ends_with("/open") && key != "games_add_popout/open"
}

/// Only the top layer answers its own outside press, so a press inside a submenu is not "outside" the menu under it.
pub(in crate::ui) fn is_top(open: &[Layer], layer: Layer) -> bool {
    open.last() == Some(&layer)
}

impl Root {
    /// What is open, bottom to top.
    pub(in crate::ui) fn open_layers(&self) -> Vec<Layer> {
        ALL.iter()
            .copied()
            .filter(|layer| self.layer_open(*layer))
            .collect()
    }

    /// Whether any layer is open, without listing them.
    pub(in crate::ui) fn any_layer_open(&self) -> bool {
        ALL.iter().any(|layer| self.layer_open(*layer))
    }

    fn layer_open(&self, layer: Layer) -> bool {
        match layer {
            Layer::PlusMenu => self.plus_menu,
            Layer::Picker => self.picker.is_some() || self.skin_menu,
            Layer::Pins => self.pins_open.is_some(),
            Layer::Inbox => self.inbox.is_some(),
            Layer::MentionsFilter => self.mentions_filter_open,
            Layer::Notify => self.notify_open,
            Layer::GuildMenu => self.guild_menu,
            Layer::FolderMenu => self.rail_folders.menu.is_some(),
            Layer::AccountMenu => self.account_switcher.more.is_some(),
            Layer::StatusPopout => self.status_popout.is_some(),
            Layer::ThreadBrowser => self.thread_browser.is_some(),
            Layer::SearchFilters => self
                .search_filters
                .as_ref()
                .is_some_and(|filters| filters.open.is_some()),
            Layer::ForumMenu => self.forum_menu_is_open(),
            Layer::InventoryPopout => self.inventory_popout_is_open(),
            Layer::AddGame => self.add_popout_is_open(),
            Layer::SettingsSelect => self.settings_select_is_open(),
            Layer::EmojiPopout => self.emoji_popout.is_some(),
            Layer::StickerPopout => self.sticker_popout.is_some(),
            Layer::RolePopout => self.role_popout.is_some(),
            Layer::HeaderPopout => self.header_popout.is_some(),
            Layer::VoicePanelPopout => matches!(
                self.panel.popout,
                Some(
                    super::super::voice_panel::Popout::Connection(_)
                        | super::super::voice_panel::Popout::Noise
                )
            ),
            Layer::Soundboard => {
                self.soundboard.gift_picker.is_some()
                    || matches!(
                        self.panel.popout,
                        Some(super::super::voice_panel::Popout::Soundboard)
                    )
            }
            Layer::Profile => self.profile.is_some(),
            Layer::RolePicker => self.role_picker.is_some(),
            Layer::PluginPermissions => {
                super::super::plugin_hooks::permissions_viewer_popout_open()
            }
            Layer::FileMenu => self.file_menu.is_some(),
            Layer::AudioMenu => self.audio_menu.is_some(),
            Layer::FriendMenu => self.friend_menu.is_some(),
            Layer::ProfileMenu => self.profile_menu.is_some(),
            Layer::SnoozeMenu => self.snooze_menu.is_some(),
            Layer::ClipMenu => self.clips_menu_open(),
            Layer::Menu => self.menu.is_some(),
            // The submenu hangs off the menu; with the menu gone nothing of it is drawn.
            Layer::Submenu => submenu_is_open(self.submenu.is_some(), self.menu.is_some()),
            Layer::ContextMenu => self.context.open.is_some(),
            Layer::ComponentSelect => self.components.open.is_some(),
            Layer::InteractionSelect => self.interaction_select_is_open(),
        }
    }

    /// Closes one layer the way its Escape does.
    pub(in crate::ui) fn close_layer(&mut self, layer: Layer, cx: &mut Context<Self>) {
        match layer {
            Layer::PlusMenu => self.plus_menu = false,
            Layer::Picker => {
                self.picker = None;
                self.skin_menu = false;
            }
            Layer::Pins => self.pins_open = None,
            Layer::Inbox => self.inbox = None,
            Layer::MentionsFilter => self.mentions_filter_open = false,
            Layer::Notify => self.notify_open = false,
            Layer::GuildMenu => self.guild_menu = false,
            Layer::FolderMenu => self.rail_folders.menu = None,
            Layer::AccountMenu => self.account_switcher.more = None,
            Layer::StatusPopout => self.status_popout = None,
            Layer::ThreadBrowser => self.thread_browser = None,
            Layer::SearchFilters => {
                if let Some(filters) = self.search_filters.as_mut() {
                    filters.open = None;
                }
            }
            Layer::ForumMenu => {
                self.close_forum_menu();
            }
            Layer::InventoryPopout => {
                self.close_inventory_popout();
            }
            Layer::AddGame => self.close_add_popout(cx),
            Layer::SettingsSelect => self.close_settings_selects(),
            Layer::EmojiPopout => self.close_emoji_popout(cx),
            Layer::StickerPopout => self.close_sticker_popout(cx),
            Layer::RolePopout => self.close_role_popout(cx),
            Layer::HeaderPopout => self.close_header_popout(cx),
            Layer::VoicePanelPopout => self.close_panel_popout(cx),
            Layer::Soundboard => {
                if self.soundboard.gift_picker.is_some() {
                    self.soundboard.gift_picker = None;
                } else {
                    self.close_panel_popout(cx);
                }
            }
            Layer::Profile => self.profile = None,
            Layer::RolePicker => self.role_picker = None,
            Layer::PluginPermissions => self.permissions_viewer_popout_close(),
            Layer::FileMenu => self.file_menu = None,
            Layer::AudioMenu => self.audio_menu = None,
            Layer::FriendMenu => self.friend_menu = None,
            Layer::ProfileMenu => self.profile_menu = None,
            Layer::SnoozeMenu => self.snooze_menu = None,
            Layer::ClipMenu => self.clips_close_menu(),
            Layer::Menu => self.menu = None,
            Layer::Submenu => self.submenu = None,
            Layer::ContextMenu => self.close_context_menu(cx),
            Layer::ComponentSelect => self.close_component_select(cx),
            Layer::InteractionSelect => self.close_interaction_selects(),
        }
        self.drop_popout_links(layer);
        cx.notify();
    }

    /// A popout went away: the popouts opened from it go with it (their trigger was a row of it), and its links are
    /// forgotten. Safe to call for any layer.
    pub(in crate::ui) fn drop_popout_links(&mut self, layer: Layer) {
        if !is_popout(layer) {
            return;
        }
        for child in self.keys.links.descendants(layer) {
            match child {
                Layer::Profile => self.profile = None,
                Layer::RolePopout => self.role_popout = None,
                _ => {}
            }
            self.keys.links.forget(child);
        }
        self.keys.links.forget(layer);
    }

    /// A popout just opened: it remembers the popout it was opened from, if any (call after setting its state).
    pub(in crate::ui) fn link_popout(&mut self, layer: Layer) {
        let open = self.open_layers();
        self.keys.links.adopt(layer, &open);
    }

    /// The popout `layer` was opened from.
    pub(in crate::ui) fn popout_parent(&self, layer: Layer) -> Option<Layer> {
        self.keys.links.parent(layer)
    }

    /// Escape on a popout: every popout of its chain closes (`POPOUT_CLOSE` reaches them all, and a child unmounts
    /// with its parent).
    pub(in crate::ui) fn close_popout_chain(&mut self, layer: Layer, cx: &mut Context<Self>) {
        let mut root = layer;
        while let Some(up) = self.keys.links.parent(root) {
            root = up;
        }
        self.close_layer(root, cx);
        if self.layer_open(layer) {
            self.close_layer(layer, cx);
        }
    }

    /// The trigger a popout was opened from, in window coordinates.
    fn popout_trigger(&self, layer: Layer) -> Option<Bounds<Pixels>> {
        match layer {
            Layer::Profile => self.profile.as_ref().map(|popout| popout.anchor),
            Layer::RolePopout => self.role_popout_anchor(),
            _ => None,
        }
    }

    /// A probe for a popout card: it records where the card was drawn, so a press can be told inside or outside it.
    pub(in crate::ui) fn popout_card_probe(&self, layer: Layer) -> gpui::AnyElement {
        let cards = self.keys.links.cards.clone();
        gpui::canvas(
            move |bounds, _, _| {
                cards.borrow_mut().insert(layer, bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// A press at `position` outside `layer`'s card. Popouts follow the official chain rule (`is_popout`); every
    /// other layer closes when it is the top one and traps the press.
    pub(in crate::ui) fn press_outside_at(
        &mut self,
        layer: Layer,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> Outside {
        if !is_popout(layer) {
            return if self.press_outside(layer, cx) {
                Outside::Trapped
            } else {
                Outside::Ignored
            };
        }
        if self.keys.outside_closed_this_press || !is_top(&self.open_layers(), layer) {
            return Outside::Ignored;
        }
        self.keys.outside_closed_this_press = true;
        let on_trigger = self
            .popout_trigger(layer)
            .is_some_and(|trigger| trigger.contains(&position));
        let links = &self.keys.links;
        let (close, outcome) = chain_press(
            layer,
            |child| links.parent(child),
            |card| {
                links
                    .card(card)
                    .is_some_and(|bounds| bounds.contains(&position))
            },
            on_trigger,
        );
        for each in close {
            if self.layer_open(each) {
                self.close_layer(each, cx);
            }
        }
        outcome
    }

    /// A settings select's list is open: its choice key ends in `/open` (the add-game popout's own key is a layer of
    /// its own).
    fn settings_select_is_open(&self) -> bool {
        self.clips.open.is_some()
            || self
                .setting_choices
                .iter()
                .any(|(key, value)| is_select_key(key) && *value == Choice::On(true))
    }

    fn close_settings_selects(&mut self) {
        self.clips.open = None;
        let open: Vec<&'static str> = self
            .setting_choices
            .iter()
            .filter(|(key, value)| is_select_key(key) && **value == Choice::On(true))
            .map(|(key, _)| *key)
            .collect();
        for key in open {
            self.setting_choices.insert(key, Choice::On(false));
        }
        self.voice_selects_closed_by_layer();
    }

    /// Escape for the layers that have no branch of their own in the root key handler: the top one closes.
    pub(in crate::ui) fn escape_layer(&mut self, cx: &mut Context<Self>) -> bool {
        match self.open_layers().last().copied() {
            Some(layer) if ESCAPE_HERE.contains(&layer) => {
                self.close_layer(layer, cx);
                true
            }
            _ => false,
        }
    }

    /// Escape inside the server settings: the dialogs and lists of its pages close one at a time, so the key never
    /// leaves the settings while one of them is open. True when something closed.
    pub(in crate::ui) fn close_server_settings_layer(&mut self) -> bool {
        if let Some(actions) = self.security_actions.as_mut() {
            if !std::mem::take(&mut actions.select_open) {
                self.security_actions = None;
            }
            return true;
        }
        self.close_roles_layer()
            || self.close_template_layer()
            || self.close_webhook_layer()
            || self.close_members_layer()
            || self.close_audit_layer()
            || self.close_safety_select()
            || self.close_overview_layer()
            || self.close_discovery_layer()
            || self.close_landing_layer()
            || self.close_engagement_layer()
            || self.close_welcome_layer()
    }

    /// `ContextMenuStore` (module 712687): opening any context menu closes any other open context menu first.
    pub(in crate::ui) fn close_all_context_menus(&mut self) {
        self.tip = None;
        self.tip_pending = None;
        self.menu = None;
        self.submenu = None;
        self.context.open = None;
        if matches!(
            self.vv.menu,
            Some(
                super::super::voice_view::Menu::Tile(..)
                    | super::super::voice_view::Menu::Stream(..)
            )
        ) {
            self.vv.menu = None;
        }
        self.vv.tile_menu = None;
        self.stage_tile_menu = None;
        self.clear_event_menu();
        self.rail_folders.menu = None;
        self.clips_close_menu();
    }

    /// Full-window pointer trap behind open right-click menus for left/middle outside clicks; right-clicks pass
    /// through to retarget another element in one click (`on_mouse_down_out` closes the previous menu first).
    pub(in crate::ui) fn menu_backdrop(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        gpui::deferred(
            gpui::div()
                .id("context-menu-backdrop")
                .absolute()
                .inset_0()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_all_context_menus();
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Middle,
                    cx.listener(|this, _, _, cx| {
                        this.close_all_context_menus();
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ),
        )
        .with_priority(2)
        .into_any_element()
    }

    /// Closes mutually exclusive sibling popouts when opening a top-level popout (`keep`).
    pub(in crate::ui) fn close_competing_popouts(&mut self, keep: Layer) {
        self.close_all_context_menus();
        if keep != Layer::PlusMenu {
            self.plus_menu = false;
        }
        if keep != Layer::Picker {
            self.picker = None;
            self.skin_menu = false;
        }
        if keep != Layer::Pins {
            self.pins_open = None;
        }
        if keep != Layer::Inbox {
            self.inbox = None;
            self.mentions_filter_open = false;
        }
        if keep != Layer::Notify {
            self.notify_open = false;
            self.notify_submenu = None;
        }
        if keep != Layer::GuildMenu {
            self.guild_menu = false;
        }
        if keep != Layer::StatusPopout {
            self.status_popout = None;
        }
        if keep != Layer::Profile {
            self.profile = None;
            self.profile_menu = None;
        }
        if keep != Layer::VoicePanelPopout && keep != Layer::Soundboard {
            self.panel.popout = None;
            self.soundboard.picker = None;
            self.soundboard.gift_picker = None;
        }
    }

    /// Window focus loss (`blur` / Alt-Tab): dismiss transient menus, tooltips, and dropdowns so none stay stuck open.
    pub(in crate::ui) fn dismiss_transient_on_blur(&mut self, cx: &mut Context<Self>) {
        self.close_all_context_menus();
        self.guild_menu = false;
        self.profile_menu = None;
        self.audio_menu = None;
        self.file_menu = None;
        self.friend_menu = None;
        self.snooze_menu = None;
        self.plus_menu = false;
        self.skin_menu = false;
        if let Some(open) = self.lightbox.as_mut() {
            open.more = false;
            open.details_open = false;
            open.more_at = None;
        }
        if let Some(popout) = self.status_popout.as_mut() {
            popout.submenu = super::super::status_menu::Submenu::None;
        }
        cx.notify();
    }

    /// A press outside `layer`'s card: closes it when it is the top layer. True when it did.
    pub(in crate::ui) fn press_outside(&mut self, layer: Layer, cx: &mut Context<Self>) -> bool {
        if self.keys.outside_closed_this_press {
            return false;
        }
        let open = self.open_layers();
        if layer.kind() != Kind::Popover || !is_top(&open, layer) {
            return false;
        }
        self.keys.outside_closed_this_press = true;
        self.close_layer(layer, cx);
        true
    }

    /// Capture only: the layers open, bottom to top, for `layers` in a script.
    #[cfg(feature = "capture")]
    pub(crate) fn layers_report(&self) -> String {
        let open = self.open_layers();
        if open.is_empty() {
            "layers: none".to_owned()
        } else {
            format!("layers: {open:?}")
        }
    }
}

/// The hook a layer's card carries: a press outside it closes the layer (when it is the top one); non-right presses
/// are swallowed so clicks do not activate what is under the layer, while right-clicks continue to retarget in one click.
pub(in crate::ui) fn outside<E: InteractiveElement>(
    element: E,
    layer: Layer,
    cx: &mut Context<Root>,
) -> E {
    element.on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
        if this.press_outside_at(layer, event.position, cx) == Outside::Trapped
            && event.button != gpui::MouseButton::Right
        {
            cx.stop_propagation();
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_is_listed_once_and_answers_to_a_press() {
        let mut seen = Vec::new();
        for layer in ALL {
            assert!(!seen.contains(layer), "{layer:?} listed twice");
            seen.push(*layer);
            assert_eq!(layer.kind(), Kind::Popover);
        }
        // `kind`'s match is exhaustive, so a variant left out of `ALL` is the only way to forget one: this is the
        // number of variants.
        assert_eq!(ALL.len(), 36);
    }

    #[test]
    fn a_select_inside_the_add_game_popout_is_above_it_and_the_popout_keeps_its_own_flag() {
        assert!(is_select_key("games_add_select/open"));
        assert!(!is_select_key("games_add_popout/open"));
        let open = [Layer::AddGame, Layer::SettingsSelect];
        // The popout does not close while its own select is the top layer; the select does.
        assert!(!is_top(&open, Layer::AddGame));
        assert!(is_top(&open, Layer::SettingsSelect));
    }

    #[test]
    fn a_submenu_without_its_menu_is_not_a_layer() {
        assert!(submenu_is_open(true, true));
        assert!(!submenu_is_open(true, false));
        assert!(!submenu_is_open(false, true));
    }

    #[test]
    fn escape_here_is_a_subset_of_the_layers() {
        assert!(ESCAPE_HERE.iter().all(|layer| ALL.contains(layer)));
    }

    #[test]
    fn a_popout_opened_from_a_popout_row_is_its_child_and_goes_with_it() {
        let mut links = Links::default();
        links.adopt(Layer::Profile, &[Layer::RolePopout, Layer::Profile]);
        assert_eq!(links.parent(Layer::Profile), Some(Layer::RolePopout));
        assert_eq!(links.descendants(Layer::RolePopout), vec![Layer::Profile]);
        // Opened from the page (nothing under it), or over a menu, it has no parent.
        links.adopt(Layer::Profile, &[Layer::Profile]);
        assert_eq!(links.parent(Layer::Profile), None);
        links.adopt(Layer::Profile, &[Layer::Menu, Layer::Profile]);
        assert_eq!(links.parent(Layer::Profile), None);
        assert!(links.descendants(Layer::RolePopout).is_empty());
    }

    #[test]
    fn a_press_closes_the_popouts_it_is_outside_of_and_goes_on() {
        let parent = |layer: Layer| (layer == Layer::Profile).then_some(Layer::RolePopout);
        // Inside the member list, outside the profile opened from it: the profile closes, the press reaches the row.
        assert_eq!(
            chain_press(Layer::Profile, parent, |l| l == Layer::RolePopout, false),
            (vec![Layer::Profile], Outside::Delivered)
        );
        // Outside both: both close, and the press still reaches the page.
        assert_eq!(
            chain_press(Layer::Profile, parent, |_| false, false),
            (vec![Layer::Profile, Layer::RolePopout], Outside::Delivered)
        );
        // On the row that opened the profile: it toggles closed and the row does not open it again.
        assert_eq!(
            chain_press(Layer::Profile, parent, |l| l == Layer::RolePopout, true),
            (vec![Layer::Profile], Outside::Trapped)
        );
        // A popout on its own.
        assert_eq!(
            chain_press(Layer::RolePopout, |_| None, |_| false, false),
            (vec![Layer::RolePopout], Outside::Delivered)
        );
    }

    #[test]
    fn only_the_top_layer_answers_its_own_outside_press() {
        let open = [Layer::Menu, Layer::Submenu];
        assert!(is_top(&open, Layer::Submenu));
        assert!(!is_top(&open, Layer::Menu));
        assert!(!is_top(&[], Layer::Menu));
    }
}
