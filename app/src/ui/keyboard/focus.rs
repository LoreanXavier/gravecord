//! Where keyboard focus is, and where it goes.
//!
//! Official rules this follows:
//! - The composer takes focus when a channel opens, when editing a message ends, and when the last dialog closes:
//!   module 699344 `componentDidUpdate` blurs the text area while `isEditing` or `hasModalOpen` and focuses it again
//!   the moment both are false.
//! - A printable key typed with nothing focused moves focus to the composer and the key lands in it: the
//!   `TEXTAREA_FOCUS` bind in module 82038 (skips modifier, function, media, arrow and page keys, Ctrl and Meta
//!   combos, and any press while a dialog or a settings layer is open or a text field has focus).
//! - A popout or menu returns focus to what had it when it closes (the focus lock of the official layers).
//! - Escape runs `y()` of module 92960: the top overlay, then the pending edit, reply, scheduled draft or sticker
//!   preview, then it marks the channel read and scrolls to the newest message.

use super::{Facts, Route, route};
use crate::editor::is_field;
use crate::ui::Root;
use dc_state::Id;
use gpui::{Context, FocusHandle, Focusable, KeyDownEvent, Keystroke, Window};

/// Whether a key press with nothing focused starts typing in the composer (module 82038).
pub(super) fn starts_typing(keystroke: &Keystroke) -> bool {
    let held = &keystroke.modifiers;
    if held.control || held.platform || held.function {
        return false;
    }
    match keystroke.key.as_str() {
        "space" | "enter" | "backspace" | "delete" => true,
        // Tab from nowhere focuses the composer without typing anything.
        "tab" => !held.shift && !held.alt,
        key if key.chars().count() == 1 => !held.alt || keystroke.key_char.is_some(),
        _ => false,
    }
}

/// Module 82038: Enter and Space belong to a focused button, link, tab or menu item; and once keyboard mode is on,
/// anything focused other than the page itself keeps every key but letters and digits.
pub(super) fn type_to_focus_allowed(
    keystroke: &Keystroke,
    on_control: bool,
    keyboard_mode: bool,
) -> bool {
    if !on_control {
        return true;
    }
    let key = keystroke.key.as_str();
    if matches!(key, "enter" | "space") {
        return false;
    }
    !keyboard_mode || (key.chars().count() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// The text a press types: the platform's own character when it has one, else the key itself.
pub(super) fn typed_text(keystroke: &Keystroke) -> Option<String> {
    if let Some(text) = keystroke.key_char.as_ref() {
        return Some(text.clone());
    }
    match keystroke.key.as_str() {
        "space" => Some(" ".to_owned()),
        "enter" if keystroke.modifiers.shift => Some("\n".to_owned()),
        key if key.chars().count() == 1 && !keystroke.modifiers.alt => {
            Some(if keystroke.modifiers.shift {
                key.to_uppercase()
            } else {
                key.to_owned()
            })
        }
        _ => None,
    }
}

/// What the Escape chain sees of the chat (module 92960 `y` and its helpers `D` and `b`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EscapeState {
    pub channel_open: bool,
    pub search_results: bool,
    pub editing: bool,
    pub reply: bool,
    pub scheduled_draft: bool,
    pub sticker_preview: bool,
    pub create_thread_panel: bool,
    pub sidebar_open: bool,
    pub has_more_after: bool,
    pub unread: bool,
    pub at_bottom: bool,
    pub voice_chat_open: bool,
}

/// What one Escape press does, in the order the official chain reaches it. At most the effects of one stage are set,
/// except the last stage, where marking read and scrolling to the newest message go together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EscapeEffects {
    pub close_search: bool,
    pub end_edit: bool,
    pub cancel_reply: bool,
    pub clear_scheduled: bool,
    pub remove_sticker: bool,
    pub close_sidebar: bool,
    pub jump_to_present: bool,
    pub mark_read: bool,
    pub scroll_to_present: bool,
    pub close_voice_chat: bool,
}

impl EscapeEffects {
    pub fn any(self) -> bool {
        self != Self::default()
    }
}

/// The Escape chain below the overlays (an open dialog, popout or menu has already taken the key by then).
pub(super) fn escape_effects(state: EscapeState) -> EscapeEffects {
    let mut effects = EscapeEffects::default();
    if !state.channel_open {
        return effects;
    }
    if state.search_results {
        effects.close_search = true;
    } else if state.editing {
        effects.end_edit = true;
    } else if state.reply {
        effects.cancel_reply = true;
    } else if state.scheduled_draft {
        effects.clear_scheduled = true;
    } else if state.sticker_preview {
        effects.remove_sticker = true;
    } else if state.create_thread_panel {
        effects.close_sidebar = true;
    } else {
        effects.jump_to_present = state.has_more_after;
        effects.mark_read = state.unread;
        let acted = effects.jump_to_present || effects.mark_read;
        if acted || !state.sidebar_open {
            if !acted && state.at_bottom && state.voice_chat_open {
                effects.close_voice_chat = true;
            } else {
                effects.scroll_to_present = true;
            }
        } else {
            effects.close_sidebar = true;
        }
    }
    effects
}

/// The layers a window has open, with the focus each of them took over from.
#[derive(Debug)]
pub(crate) struct FocusStack<T> {
    saved: Vec<T>,
}

impl<T> Default for FocusStack<T> {
    fn default() -> Self {
        Self { saved: Vec::new() }
    }
}

impl<T: Clone> FocusStack<T> {
    /// Called once per frame with the number of layers open now and the focus the frame before ended with. A layer
    /// that opened remembers that focus; when layers closed, the one that was under the earliest of them comes back.
    pub fn observe(&mut self, layers: usize, before: &T) -> Option<T> {
        let mut restored = None;
        while self.saved.len() < layers {
            self.saved.push(before.clone());
        }
        while self.saved.len() > layers {
            restored = self.saved.pop();
        }
        restored
    }

    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.saved.len()
    }
}

/// What had focus.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum Anchor {
    #[default]
    Nothing,
    Composer,
    Edit,
    Root,
    Other(FocusHandle),
}

/// Where focus goes when a layer closes and gives back what it took over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Restore {
    Composer,
    Edit,
    Handle,
    /// The window's own handle: what had focus is the page itself or is not drawn here, and the layer's own field
    /// is gone. A focus handle that is not drawn receives no keys.
    Window,
    Keep,
}

pub(super) fn restore_target(anchor: &Anchor, composer_reachable: bool, editing: bool) -> Restore {
    match anchor {
        Anchor::Composer if composer_reachable => Restore::Composer,
        Anchor::Edit if editing => Restore::Edit,
        Anchor::Edit if composer_reachable => Restore::Composer,
        Anchor::Edit => Restore::Keep,
        Anchor::Other(_) => Restore::Handle,
        Anchor::Composer | Anchor::Root | Anchor::Nothing => Restore::Window,
    }
}

/// A row of a menu had focus at the start of the frame and no menu is open any more: the row is not drawn.
pub(super) fn menu_row_focus_is_gone(menu_open: bool, row_had_focus: bool) -> bool {
    row_had_focus && !menu_open
}

/// Whether the message box gives up focus: a dialog is open, or the box is not drawn.
pub(super) fn blurs_composer(anchor: &Anchor, modal: bool, reachable: bool) -> bool {
    matches!(anchor, Anchor::Composer) && (modal || !reachable)
}

/// What a frame saw of the things that decide whether the composer takes focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Seen {
    pub layers: usize,
    pub modals: usize,
    pub editing: bool,
    pub replying: bool,
    pub files: usize,
    pub channel: Option<Id>,
}

/// Whether the composer takes focus between two frames: nothing is open over the chat and nothing is being edited,
/// and either the last dialog closed, an edit ended (saved or cancelled), the channel changed, a reply ended (sent or
/// cancelled) or files were attached. Module 699344 gives the first two; the others follow from the text area being
/// mounted anew or focused by its handlers.
pub(super) fn composer_returns(before: Seen, now: Seen) -> bool {
    now.layers == 0
        && !now.editing
        && ((before.modals > 0 && now.modals == 0)
            || (before.editing && !now.editing)
            || before.channel != now.channel
            || (before.replying && !now.replying)
            || now.files > before.files)
}

/// Whether the inline edit box should take focus on this frame: an edit just started, or an active edit moved into
/// view with a channel switch (module 699344 pairs this with `composer_returns`).
pub(super) fn edit_starts(before: Seen, now: Seen) -> bool {
    now.editing && (!before.editing || before.channel != now.channel)
}

/// Keyboard bookkeeping that lives between frames.
#[derive(Default)]
pub(in crate::ui) struct State {
    stack: FocusStack<Anchor>,
    anchor: Anchor,
    seen: Seen,
    /// The open picker's search field, as of the last frame.
    picker_search: Option<FocusHandle>,
    /// The last input was the keyboard: rings show on focused controls only then.
    pub(in crate::ui) keyboard_modality: bool,
    /// One top popover layer already closed on the current mouse press (`capture_any_mouse_down` resets it).
    pub(in crate::ui) outside_closed_this_press: bool,
    /// The server before the current one, for `TOGGLE_PREVIOUS_GUILD`.
    pub(super) previous_guild: Option<Id>,
    pub(super) current_guild: Option<Id>,
    pub(super) last_step: Option<std::time::Instant>,
    /// The Keyboard Shortcuts help, while it is open.
    pub(super) help: Option<super::help::Help>,
    /// The focus stops of lists and controls, and the groups the arrow keys move in.
    pub(in crate::ui) stops: super::Stops,
}

impl Root {
    /// A settings layer is open: the official `LayerStore` has entries.
    pub(super) fn layers_open(&self) -> bool {
        self.settings.is_some()
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
            || self.profile_editor.is_some()
    }

    /// Dialogs and menus: what a key goes to before the global table.
    pub(in crate::ui) fn modal_open(&self) -> usize {
        usize::from(self.layers_open())
            + [
                self.quick_switcher.is_some(),
                self.menu.is_some(),
                self.context.open.is_some(),
                self.plus_menu,
                self.too_long.is_some(),
                self.confirm_delete.is_some(),
                self.menu_extra.reactions_confirm.is_some(),
                self.confirm_pin.is_some(),
                self.confirm_unpin.is_some(),
                self.leaving.is_some(),
                self.leave_guild.is_some(),
                self.destructive.is_some(),
                self.logout_confirm,
                self.rail_folders.settings.is_some(),
                self.account_switcher_open(),
                self.settings_extra.has_dialog(),
                self.add_server.is_some(),
                self.group_dm.is_some(),
                self.forward.is_some(),
                self.invite.is_some(),
                self.create_channel.is_some(),
                self.report.is_some(),
                self.member_action.is_some(),
                self.group_edit.is_some(),
                self.group_leave.is_some(),
                self.security_actions.is_some(),
                self.sent_edit.is_some(),
                self.upload_error.is_some(),
                self.shop.modal.is_some(),
                self.screening.is_some(),
                self.inventory.about,
                self.thread_modal,
                self.guild_notifications.is_some(),
                self.channel_status.is_some(),
                self.boost_apply_open(),
                self.follow.is_some(),
                self.start_stage.is_some(),
                self.stage_welcome.is_some(),
                self.stage_end_confirm.is_some(),
                self.publish_confirm.is_some(),
                self.full_profile.is_some(),
                self.custom_status.is_some(),
                self.custom_reminder.is_some(),
                self.modify_attachment.is_some(),
                self.view_reactions.is_some(),
                self.badge_modal.is_some(),
                self.polls.modal_open(),
                // Send a Gift and Select Plan: the page behind is out of the tab order.
                self.nitro.purchase.is_some(),
                self.search_filters.is_some(),
                self.lightbox.is_some(),
                self.clips_modal_open(),
                self.poll.is_some(),
                self.schedule.modal.is_some(),
                self.snooze_menu.is_some(),
                self.friend_menu.is_some(),
                self.confirm_remove_friend.is_some(),
                self.friend_nickname.is_some(),
                self.add_friend.is_some(),
                self.profile_menu.is_some(),
                self.remediation.is_some(),
                self.file_menu.is_some(),
                self.audio_menu.is_some(),
                self.camera_error.is_some(),
                self.go_live.is_some(),
                self.activity_shelf.is_some(),
                self.guild_events.is_some(),
                self.keys.help.is_some(),
            ]
            .into_iter()
            .filter(|open| *open)
            .count()
    }

    /// Everything that sits over the page and gives focus back when it closes: dialogs and menus plus popouts.
    fn layer_count(&self) -> usize {
        self.modal_open()
            + [
                self.picker.is_some(),
                self.gifs.is_some(),
                self.pins_open.is_some(),
                self.inbox.is_some(),
                self.status_popout.is_some(),
                self.profile.is_some(),
                self.notify_open,
                self.submenu.is_some(),
                self.skin_menu,
                self.thread_browser.is_some(),
                self.guild_menu,
                self.mentions_filter_open,
            ]
            .into_iter()
            .filter(|open| *open)
            .count()
    }

    /// Whether the chat column with its composer is what the window shows.
    pub(in crate::ui) fn composer_reachable(&self) -> bool {
        let Some(channel) = self.channel else {
            return false;
        };
        if self.discover_active()
            || self.nitro_open
            || self.quests_active()
            || self.shop_active()
            || self.guild_guide_open
            || self.channel_browser_open
            || self.screening_pending()
            || self.member_view.is_some()
            || self.voice_view_open()
        {
            return false;
        }
        if self
            .current_channel()
            .is_some_and(dc_state::Channel::is_forum)
        {
            return false;
        }
        let blocked_dm = self
            .current_dm()
            .and_then(|dm| self.stores.dm_recipient(dm))
            .is_some_and(|user| self.is_blocked(user.id));
        !blocked_dm && channel != 0
    }

    pub(in crate::ui) fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn anchor_now(&self, window: &Window, cx: &mut Context<Self>) -> Anchor {
        let Some(focused) = window.focused(cx) else {
            return Anchor::Nothing;
        };
        if focused == self.composer.read(cx).focus_handle(cx) {
            return Anchor::Composer;
        }
        if self
            .editing
            .as_ref()
            .is_some_and(|(_, editor)| focused == editor.read(cx).focus_handle(cx))
        {
            return Anchor::Edit;
        }
        if focused == self.focus {
            return Anchor::Root;
        }
        Anchor::Other(focused)
    }

    fn restore_anchor(&mut self, anchor: &Anchor, window: &mut Window, cx: &mut Context<Self>) {
        match restore_target(anchor, self.composer_reachable(), self.editing.is_some()) {
            Restore::Composer => self.focus_composer(window, cx),
            Restore::Edit => {
                if let Some((_, editor)) = self.editing.as_ref() {
                    let handle = editor.read(cx).focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            Restore::Handle => {
                if let Anchor::Other(handle) = anchor {
                    window.focus(handle, cx);
                }
            }
            Restore::Window => window.focus(&self.focus, cx),
            Restore::Keep => {}
        }
    }

    /// Called at the start of every frame, before the tree is built: moves focus where the rules above say after
    /// whatever the last input changed.
    pub(in crate::ui) fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A click on a menu row focuses the row; when that click closed the menu (or replaced it with a dialog) the
        // row is no longer drawn, and a focus handle that is not drawn receives no keys, Escape included.
        if menu_row_focus_is_gone(
            self.menu.is_some() || self.context.open.is_some() || self.plus_menu,
            self.keys.stops.menu_had_focus.get(),
        ) {
            window.focus(&self.focus, cx);
        }
        let layers = self.layer_count();
        let composer_tab = layers == 0 && self.composer_reachable_or_voice();
        self.composer
            .update(cx, |editor, cx| editor.set_tab_stop(composer_tab, cx));
        if let Some((_, editor)) = self.editing.as_ref() {
            editor.update(cx, |editor, cx| editor.set_tab_stop(layers == 0, cx));
        }
        let now = Seen {
            layers,
            modals: self.modal_open(),
            editing: self.editing.is_some(),
            replying: self.replying.is_some(),
            files: self.pending_files.len(),
            channel: self.channel,
        };
        let before = self.keys.seen;
        let anchor_before = self.keys.anchor.clone();
        let restore = self.keys.stack.observe(layers, &anchor_before);
        let mut target = restore;
        if composer_returns(before, now) && self.composer_reachable() {
            target = Some(Anchor::Composer);
        } else if layers == 0 && edit_starts(before, now) {
            target = Some(Anchor::Edit);
        }
        if let Some(anchor) = target {
            self.restore_anchor(&anchor, window, cx);
        } else if self.picker.is_none()
            && matches!(&anchor_before, Anchor::Other(handle)
                if self.keys.picker_search.as_ref() == Some(handle)
                    && window.focused(cx).as_ref() == Some(handle))
        {
            // The picker closed in the same frame as a full-window layer opened (Add Emoji opens the server settings): its
            // search field is gone and the layer count did not change, so nothing gave focus back. A focus handle that is
            // not drawn receives no keys, so Escape would never reach the new layer; the window's own handle takes them.
            window.focus(&self.focus, cx);
        } else if blurs_composer(
            &self.anchor_now(window, cx),
            now.modals > 0,
            self.composer_reachable(),
        ) {
            // The text area is blurred while a dialog, menu or settings layer is open (699344 `componentDidUpdate`
            // `hasModalOpen`); otherwise it would keep its caret and take what is typed under the layer. A text area
            // that is not drawn (a channel where the user cannot talk, a page without a chat) must not keep focus
            // either: a focused element that is not in the tree receives no keys, shortcuts included.
            window.focus(&self.focus, cx);
        } else if layers > before.layers
            && matches!(anchor_before, Anchor::Other(_))
            // A layer that put its own field in focus (the picker's search) keeps it.
            && !matches!(self.anchor_now(window, cx), Anchor::Other(handle) if is_field(&handle))
        {
            // A layer has just opened over a focused control of the page below. That control may not be drawn under
            // the layer, and a focus handle that is not drawn receives no keys, so Escape would never reach the layer.
            // The window's own handle takes the keys; the stack above gives the control its focus back on close.
            window.focus(&self.focus, cx);
        }
        if self.keys.current_guild != self.guild {
            // `getLastSelectedGuildId`: the server left last, whatever was opened since.
            if let Some(left) = self.keys.current_guild {
                self.keys.previous_guild = Some(left);
            }
            self.keys.current_guild = self.guild;
        }
        self.keys.seen = now;
        self.keys.picker_search = self.picker.as_ref().map(|picker| picker.search_handle(cx));
        self.keys.anchor = self.anchor_now(window, cx);
    }

    /// Ends the inline edit without saving and gives the composer focus (Escape in the edit box, module 91624 and
    /// `endEditMessage`).
    pub(in crate::ui) fn end_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(in crate::ui) fn cancel_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replying = None;
        self.focus_composer(window, cx);
        cx.notify();
    }

    fn escape_state(&self) -> EscapeState {
        let unread = self.channel.is_some_and(|channel| {
            self.stores.is_unread(channel)
                && !self
                    .stores
                    .read_states
                    .states
                    .get(&channel)
                    .is_some_and(|state| state.manual)
        });
        EscapeState {
            channel_open: self.channel.is_some() && self.composer_reachable_or_voice(),
            search_results: self
                .search
                .as_ref()
                .is_some_and(|search| search.shown.is_some()),
            editing: self.editing.is_some(),
            reply: self.replying.is_some(),
            // Scheduled drafts and sticker previews are not kept in this client.
            scheduled_draft: false,
            sticker_preview: false,
            create_thread_panel: self.new_thread.is_some(),
            sidebar_open: self.new_thread.is_some(),
            has_more_after: self.window_detached(),
            unread,
            at_bottom: self.list.is_scrolled_to_end().unwrap_or(true),
            voice_chat_open: self.voice_chat_docked(),
        }
    }

    fn composer_reachable_or_voice(&self) -> bool {
        self.composer_reachable() || self.voice_chat_docked()
    }

    /// Escape below the overlays, or Shift+Page Down: module 92960. True when something happened.
    pub(in crate::ui) fn escape_chain(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let effects = escape_effects(self.escape_state());
        if !effects.any() {
            return false;
        }
        if effects.close_search {
            self.clear_search(cx);
        }
        if effects.end_edit {
            self.end_edit(window, cx);
        }
        if effects.cancel_reply {
            self.cancel_reply(window, cx);
        }
        if effects.close_sidebar {
            self.new_thread = None;
        }
        if effects.mark_read {
            self.mark_current_read(cx);
        }
        if effects.jump_to_present {
            // `jumpToPresent(channelId, 30)`, module 92960.
            self.jump_to_present_with(30);
        }
        if effects.scroll_to_present {
            self.list.scroll_to_end();
        }
        if effects.close_voice_chat
            && let Some(call) = self.call.as_ref()
        {
            let channel = call.channel;
            self.vv.chat_open.remove(&channel);
        }
        cx.notify();
        true
    }

    /// A key arrives with nothing focused (or a control that does not use it): it starts typing in the composer.
    fn type_to_composer(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.composer_reachable() {
            return false;
        }
        self.focus_composer(window, cx);
        // The platform delivers the character after this key down, but the composer's input handler only exists once
        // the next frame is drawn, so the character is put in here instead.
        let text = typed_text(keystroke);
        let enter =
            keystroke.key == "enter" && !keystroke.modifiers.shift && !keystroke.modifiers.alt;
        self.composer.update(cx, |composer, cx| {
            if enter {
                composer.submit(cx);
            } else if let Some(text) = text {
                composer.insert(&text, cx);
            }
        });
        true
    }

    /// The root's key handler calls this first for every key. True when the key was used.
    pub(in crate::ui) fn keyboard_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let keystroke = &event.keystroke;
        let typing = window.focused(cx).is_some_and(|handle| is_field(&handle));
        if super::stops::enables_keyboard_mode(keystroke, typing) {
            self.keys.set_modality(true);
            cx.notify();
        }
        if self.keys.help.is_some() && self.keyboard_help_key(keystroke.key.as_str(), cx) {
            return true;
        }
        // The picker's search field keeps focus while the arrows move over its grid.
        if self.picker.is_some()
            && matches!(
                keystroke.key.as_str(),
                "up" | "down" | "left" | "right" | "home" | "end"
            )
            && self.grid_key(keystroke, cx)
        {
            return true;
        }
        if !typing && self.stops_key(keystroke, window, cx) {
            return true;
        }
        if !typing
            && let Some(focused) = window.focused(cx)
            && self.message_key(keystroke, &focused, window, cx)
        {
            return true;
        }
        // An open autocomplete gets Tab before the browser-style tab order.
        if keystroke.key == "tab" && typing && self.composer_key(keystroke, window, cx) {
            return true;
        }
        if self.tab_key(keystroke, window, cx) {
            return true;
        }
        let facts = Facts {
            layers: self.layers_open(),
            modal: self.modal_open() > 0,
            typing,
        };
        match route(keystroke, facts) {
            Route::Global(hit) => {
                // Plain Escape reaches the chain after the overlays have had it (the end of the root handler).
                if hit.bind == "esc" {
                    return false;
                }
                self.run_shortcut(hit, window, cx)
            }
            Route::Swallow => true,
            Route::Typing if !event.is_held => {
                let on_control = matches!(self.anchor_now(window, cx), Anchor::Other(_));
                if !type_to_focus_allowed(keystroke, on_control, self.keys.keyboard_modality) {
                    return false;
                }
                self.type_to_composer(keystroke, window, cx)
            }
            Route::Typing | Route::Overlay | Route::Input | Route::Pass => false,
        }
    }

    /// For the harness: which element has focus, by name and kind.
    #[cfg(feature = "capture")]
    pub fn focus_report(&self, window: &Window, cx: &mut Context<Self>) -> String {
        match self.anchor_now(window, cx) {
            Anchor::Nothing => "focus=none kind=none".to_owned(),
            Anchor::Composer => "focus=composer kind=text-field".to_owned(),
            Anchor::Edit => "focus=message-edit kind=text-field".to_owned(),
            Anchor::Root => "focus=window kind=root".to_owned(),
            Anchor::Other(handle) => {
                let kind = if is_field(&handle) {
                    "text-field"
                } else {
                    "element"
                };
                let name = self.field_name(&handle, cx);
                format!("focus={name} kind={kind}")
            }
        }
    }

    #[cfg(feature = "capture")]
    fn field_name(&self, handle: &FocusHandle, cx: &Context<Self>) -> String {
        let _ = cx;
        if let Some(name) = self.keys.stops.describe(handle) {
            return name;
        }
        if self.quick_switcher.is_some() && is_field(handle) {
            return "quick-switcher-search".to_owned();
        }
        if self.picker.is_some() && is_field(handle) {
            return "picker-search".to_owned();
        }
        if self
            .search
            .as_ref()
            .is_some_and(|search| search.focused && is_field(handle))
        {
            return "channel-search".to_owned();
        }
        format!("{handle:?}")
    }
}

/// The step that a mouse press ends keyboard modality (rings hide for pointer focus).
impl State {
    pub(in crate::ui) fn pointer_used(&mut self) {
        self.set_modality(false);
        self.outside_closed_this_press = false;
    }

    pub(in crate::ui) fn keyboard_mode_is_on(&self) -> bool {
        self.keyboard_modality
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    #[test]
    fn printable_keys_start_typing() {
        assert!(starts_typing(&stroke("a")));
        assert!(starts_typing(&stroke("shift-a")));
        assert!(starts_typing(&stroke("space")));
        assert!(starts_typing(&stroke("enter")));
        assert!(starts_typing(&stroke("backspace")));
        assert!(starts_typing(&stroke("1")));
        assert!(starts_typing(&stroke("/")));
    }

    #[test]
    fn navigation_modifier_and_function_keys_do_not() {
        for key in [
            "up", "down", "left", "right", "pageup", "pagedown", "home", "end", "f5", "escape",
            "ctrl-a", "shift", "control",
        ] {
            assert!(!starts_typing(&stroke(key)), "{key}");
        }
        assert!(!starts_typing(&stroke("alt-a")));
    }

    #[test]
    fn the_typed_text_is_the_key_or_its_shifted_form() {
        assert_eq!(typed_text(&stroke("a")).as_deref(), Some("a"));
        assert_eq!(typed_text(&stroke("shift-a")).as_deref(), Some("A"));
        assert_eq!(typed_text(&stroke("space")).as_deref(), Some(" "));
        assert_eq!(typed_text(&stroke("shift-enter")).as_deref(), Some("\n"));
        assert_eq!(typed_text(&stroke("enter")), None);
        assert_eq!(typed_text(&stroke("a->ä")).as_deref(), Some("ä"));
    }

    fn open() -> EscapeState {
        EscapeState {
            channel_open: true,
            ..EscapeState::default()
        }
    }

    #[test]
    fn escape_ends_an_edit_before_anything_else() {
        let effects = escape_effects(EscapeState {
            editing: true,
            reply: true,
            unread: true,
            ..open()
        });
        assert_eq!(
            effects,
            EscapeEffects {
                end_edit: true,
                ..EscapeEffects::default()
            }
        );
    }

    #[test]
    fn escape_cancels_a_reply_after_the_edit_and_before_marking_read() {
        let effects = escape_effects(EscapeState {
            reply: true,
            unread: true,
            ..open()
        });
        assert_eq!(
            effects,
            EscapeEffects {
                cancel_reply: true,
                ..EscapeEffects::default()
            }
        );
    }

    #[test]
    fn escape_closes_search_results_first() {
        let effects = escape_effects(EscapeState {
            search_results: true,
            editing: true,
            ..open()
        });
        assert!(effects.close_search && !effects.end_edit);
    }

    #[test]
    fn escape_marks_read_and_scrolls_to_the_newest_message() {
        let effects = escape_effects(EscapeState {
            unread: true,
            ..open()
        });
        assert!(effects.mark_read && effects.scroll_to_present && !effects.cancel_reply);
    }

    #[test]
    fn escape_in_a_read_channel_only_scrolls() {
        let effects = escape_effects(open());
        assert_eq!(
            effects,
            EscapeEffects {
                scroll_to_present: true,
                ..EscapeEffects::default()
            }
        );
    }

    #[test]
    fn escape_closes_the_voice_chat_when_already_at_the_bottom() {
        let effects = escape_effects(EscapeState {
            at_bottom: true,
            voice_chat_open: true,
            ..open()
        });
        assert!(effects.close_voice_chat && !effects.scroll_to_present);
        let effects = escape_effects(EscapeState {
            at_bottom: true,
            voice_chat_open: true,
            unread: true,
            ..open()
        });
        assert!(effects.mark_read && effects.scroll_to_present && !effects.close_voice_chat);
    }

    #[test]
    fn escape_with_a_side_panel_and_nothing_to_read_closes_the_panel() {
        let effects = escape_effects(EscapeState {
            sidebar_open: true,
            ..open()
        });
        assert_eq!(
            effects,
            EscapeEffects {
                close_sidebar: true,
                ..EscapeEffects::default()
            }
        );
    }

    #[test]
    fn escape_without_a_channel_does_nothing() {
        assert!(!escape_effects(EscapeState::default()).any());
    }

    fn seen() -> Seen {
        Seen {
            channel: Some(1),
            ..Seen::default()
        }
    }

    #[test]
    fn the_composer_takes_focus_when_an_edit_ends() {
        let editing = Seen {
            editing: true,
            ..seen()
        };
        assert!(composer_returns(editing, seen()));
        assert!(!composer_returns(editing, editing));
    }

    #[test]
    fn the_composer_waits_while_something_is_still_open() {
        let editing = Seen {
            editing: true,
            ..seen()
        };
        let open = Seen {
            layers: 1,
            modals: 1,
            ..seen()
        };
        assert!(!composer_returns(editing, open));
        assert!(composer_returns(open, seen()));
        assert!(!composer_returns(seen(), seen()));
    }

    #[test]
    fn the_composer_takes_focus_when_the_channel_changes() {
        let other = Seen {
            channel: Some(2),
            ..seen()
        };
        assert!(composer_returns(seen(), other));
    }

    #[test]
    fn the_composer_takes_focus_when_a_reply_ends_or_a_file_is_attached() {
        let replying = Seen {
            replying: true,
            ..seen()
        };
        assert!(composer_returns(replying, seen()));
        let attached = Seen { files: 1, ..seen() };
        assert!(composer_returns(seen(), attached));
        assert!(!composer_returns(attached, seen()));
    }

    #[test]
    fn a_focused_control_keeps_enter_and_space_and_in_keyboard_mode_every_non_alphanumeric_key() {
        let enter = stroke("enter");
        let space = stroke("space");
        let letter = stroke("a");
        let dash = stroke("-");
        assert!(!type_to_focus_allowed(&enter, true, false));
        assert!(!type_to_focus_allowed(&space, true, true));
        assert!(type_to_focus_allowed(&letter, true, true));
        assert!(type_to_focus_allowed(&dash, true, false));
        assert!(!type_to_focus_allowed(&dash, true, true));
        assert!(type_to_focus_allowed(&enter, false, true));
    }

    #[test]
    fn only_the_message_box_is_blurred_under_a_dialog() {
        assert!(blurs_composer(&Anchor::Composer, true, true));
        assert!(!blurs_composer(&Anchor::Composer, false, true));
        assert!(!blurs_composer(&Anchor::Edit, true, true));
        assert!(!blurs_composer(&Anchor::Root, true, true));
        assert!(!blurs_composer(&Anchor::Nothing, true, true));
    }

    #[test]
    fn a_closing_layer_returns_focus_to_the_window_when_nothing_drawn_had_it() {
        assert_eq!(restore_target(&Anchor::Root, true, false), Restore::Window);
        assert_eq!(
            restore_target(&Anchor::Nothing, false, false),
            Restore::Window
        );
        // A message box that is not drawn (Home, a forum) cannot take focus back.
        assert_eq!(
            restore_target(&Anchor::Composer, false, false),
            Restore::Window
        );
        assert_eq!(
            restore_target(&Anchor::Composer, true, false),
            Restore::Composer
        );
        assert_eq!(restore_target(&Anchor::Edit, true, true), Restore::Edit);
        assert_eq!(
            restore_target(&Anchor::Edit, true, false),
            Restore::Composer
        );
        assert_eq!(restore_target(&Anchor::Edit, false, false), Restore::Keep);
    }

    #[test]
    fn a_menu_row_that_had_focus_is_gone_with_its_menu() {
        assert!(menu_row_focus_is_gone(false, true));
        assert!(!menu_row_focus_is_gone(true, true));
        assert!(!menu_row_focus_is_gone(false, false));
    }

    #[test]
    fn an_undrawn_message_box_gives_up_focus() {
        assert!(blurs_composer(&Anchor::Composer, false, false));
    }

    #[test]
    fn a_closing_layer_gives_back_the_focus_it_took() {
        let mut stack: FocusStack<&str> = FocusStack::default();
        assert_eq!(stack.observe(0, &"composer"), None);
        assert_eq!(stack.observe(1, &"composer"), None);
        // The overlay moved focus to its own field; the next frame's "before" is that field.
        assert_eq!(stack.observe(2, &"search"), None);
        assert_eq!(stack.depth(), 2);
        assert_eq!(stack.observe(1, &"other"), Some("search"));
        assert_eq!(stack.observe(1, &"other"), None);
        assert_eq!(stack.observe(0, &"search"), Some("composer"));
    }

    #[test]
    fn two_layers_closing_together_return_the_earliest_focus() {
        let mut stack: FocusStack<&str> = FocusStack::default();
        stack.observe(1, &"composer");
        stack.observe(2, &"menu");
        assert_eq!(stack.observe(0, &"x"), Some("composer"));
    }

    #[test]
    fn pointer_used_resets_outside_closed_this_press_latch() {
        let mut state = State::default();
        assert!(!state.outside_closed_this_press);
        state.outside_closed_this_press = true;
        state.pointer_used();
        assert!(!state.outside_closed_this_press);
    }

    #[test]
    fn the_edit_box_takes_focus_when_edit_begins_or_channel_switches() {
        let same_before = Seen {
            channel: Some(10),
            modals: 1,
            ..Seen::default()
        };
        let same_now = Seen {
            channel: Some(10),
            editing: true,
            ..Seen::default()
        };
        assert!(!composer_returns(same_before, same_now));
        assert!(edit_starts(same_before, same_now));

        let switched_now = Seen {
            channel: Some(20),
            editing: true,
            ..Seen::default()
        };
        assert!(edit_starts(same_now, switched_now));

        assert!(!edit_starts(same_now, same_now));
    }
}
