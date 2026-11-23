//! The layers that sit over the page, and the rule that a press outside the top one closes it.
//!
//! Official rule: every popout, select list, picker and menu is a layer with `onRequestClose` (the layer stack of
//! module 82038, the popout layer `ZP` in module 549587, the menus of module 165862). A mouse press anywhere outside
//! the top layer closes that layer only; the press is not delivered to what is under it. Escape closes the same top
//! layer. Modal dialogs close on a press on their backdrop (`ModalRoot` `onClose`), which `modal::modal` already does
//! for every dialog built with it.
//!
//! `Root::open_layers` lists what is open from bottom to top, `Root::close_layer` closes one the way Escape does, and
//! `outside` is the one-line hook a layer's card carries: `outside(card, Layer::X, cx)`.

use super::super::Root;
use super::super::settings::Choice;
use gpui::{Context, InteractiveElement, IntoElement as _, MouseDownEvent, Styled as _};

/// How a layer answers a press outside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Kind {
    /// A popout, menu, select list or picker: the press closes it and goes no further.
    Popover,
}

/// Every popover-like layer, in the order they stack (later is above).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        cx.notify();
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
        if this.press_outside(layer, cx) && event.button != gpui::MouseButton::Right {
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
    fn only_the_top_layer_answers_its_own_outside_press() {
        let open = [Layer::Menu, Layer::Submenu];
        assert!(is_top(&open, Layer::Submenu));
        assert!(!is_top(&open, Layer::Menu));
        assert!(!is_top(&[], Layer::Menu));
    }
}
