//! The System page's keybinds: the custom list (module 158935 `l$` and its row `lH`, `Add a Keybind` from
//! `useHeaderDecoration` of `SYSTEM_CUSTOM_KEYBINDS_CATEGORY`) and the default list (`l4`, `l6`, with the groups
//! of `l5`), stored as `KeybindsStore` (module 532624, `persistKey` `keybinds`: a map of id to
//! `{id, enabled, action, shortcut, managed, params}`, a shortcut being `[type, code, platform]` parts).
//!
//! A keybind is acted on from two places: the system-wide hook (`dc_hotkey`, never started in a harness run) and
//! the keys of the window itself, so the same decision runs in both and a script can prove it.

use super::model::Choice;
use super::render::{plain_switch, select_field_open};
use super::voice::{
    KEYBOARD_KEY, MOUSE_BUTTON, WINDOWS, modifier_codes, shortcut_down, shortcut_text, windows_code,
};
use super::{Page, Root, color, mask_icon};
#[cfg(windows)]
use crate::net::Update;
use crate::storage;
use crate::ui::keyboard::help::{Section, combo, sections};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Modifiers, MouseButton, SharedString, div,
    prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(in crate::ui) const CUSTOM: &str = "custom_keybinds_setting";
pub(in crate::ui) const DEFAULT: &str = "default_keybinds_setting";
pub(in crate::ui) const CATEGORY: &str = "system_custom_keybinds_category";
/// The open flag every settings select keeps (`is_select_key`): the layer closes it on an outside press or Escape.
const SELECT_OPEN: &str = "keybind_action/open";
const STORE: &str = "keybinds";

const OVERLAY_CHAT: &str = "OVERLAY_ACTIVATE_REGION_TEXT_WIDGET";

/// Module 158935 `l$` `keybindActionTypes` as the captured Windows build lists them (`system.json`), each with its
/// label (module 994778) and the line under the row (`keybindDescriptions`).
const ACTIONS: &[(&str, &str, &str)] = &[
    (
        "UNASSIGNED",
        "Unassigned",
        "This action doesn't do anything! Please choose one from the drop down.",
    ),
    (
        "PUSH_TO_TALK",
        "Push to Talk (Normal)",
        "Hold to temporarily enable your microphone while in Push to Talk mode.",
    ),
    (
        "PUSH_TO_TALK_PRIORITY",
        "Push to Talk (Priority)",
        "Hold to temporarily enable your microphone while in Push to Talk mode. Other speakers\nwho are not also transmitting with priority will temporarily have their volume lowered\nwhile you are speaking.",
    ),
    (
        "PUSH_TO_MUTE",
        "Push to Mute",
        "Hold to temporarily disable your microphone while in Voice Activity mode.",
    ),
    (
        "VAD_PRIORITY",
        "Voice Activity Priority",
        "Hold to transmit your voice with priority in Voice Activity mode.\nOther speakers who are not also transmitting with priority will temporarily have their volume lowered\nwhile you are speaking.",
    ),
    (
        "TOGGLE_MUTE",
        "Toggle Mute",
        "Toggle your microphone's transmission on and off.",
    ),
    (
        "TOGGLE_DEAFEN",
        "Toggle Deafen",
        "Toggle your speaker's playback on and off. Also disables your microphone while deafened.",
    ),
    (
        "TOGGLE_VOICE_MODE",
        "Toggle VAD",
        "Toggle to switch between Push To Talk and Voice Activity mode.",
    ),
    (
        "TOGGLE_STREAMER_MODE",
        "Toggle Streamer Mode",
        "Toggle to enable/disable Streamer Mode.",
    ),
    (
        "TOGGLE_VOICE_CHANNEL_CHAT",
        "Toggle Voice Channel Chat",
        "Toggle the text chat panel for the voice channel you are connected to.",
    ),
    (
        "TOGGLE_CAMERA",
        "Toggle Camera",
        "Toggle your camera's on while in a voice channel.",
    ),
    (
        "TOGGLE_OVERLAY_INPUT_LOCK",
        "Toggle Overlay Lock",
        "Toggle overlay interactivity lock.",
    ),
    (OVERLAY_CHAT, "Activate Overlay Chat", ""),
];

#[derive(Clone, PartialEq, Debug)]
pub(in crate::ui) struct Keybind {
    pub id: String,
    pub action: String,
    pub shortcut: Vec<[u32; 3]>,
    pub enabled: bool,
    pub managed: bool,
}

pub(in crate::ui) fn parse(value: &Value) -> Vec<Keybind> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(id, entry)| {
            let shortcut = entry["shortcut"]
                .as_array()?
                .iter()
                .filter_map(|part| {
                    let part = part.as_array()?;
                    Some([
                        part.first()?.as_u64()? as u32,
                        part.get(1)?.as_u64()? as u32,
                        part.get(2).and_then(Value::as_u64).unwrap_or(1) as u32,
                    ])
                })
                .collect();
            Some(Keybind {
                id: id.clone(),
                action: entry["action"].as_str()?.to_owned(),
                shortcut,
                // The store's last migration: a missing flag means enabled.
                enabled: entry["enabled"].as_bool().unwrap_or(true),
                managed: entry["managed"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

pub(in crate::ui) fn to_json(list: &[Keybind]) -> Value {
    let mut map = serde_json::Map::new();
    for keybind in list {
        map.insert(
            keybind.id.clone(),
            json!({
                "id": keybind.id,
                "enabled": keybind.enabled,
                "action": keybind.action,
                "shortcut": keybind.shortcut,
                "managed": keybind.managed,
                "params": {},
            }),
        );
    }
    Value::Object(map)
}

/// `K` (module 532624): the counter after the highest id.
pub(in crate::ui) fn next_id(list: &[Keybind]) -> String {
    list.iter()
        .filter_map(|keybind| keybind.id.parse::<u64>().ok())
        .max()
        .map_or(0, |highest| highest + 1)
        .to_string()
}

/// The stored list; the managed overlay chat keybind (`z(OVERLAY_ACTIVATE_REGION_TEXT_WIDGET)`) is always part of it.
pub(in crate::ui) fn load() -> Vec<Keybind> {
    let mut list = storage::get(STORE)
        .map(|value| parse(&value))
        .unwrap_or_default();
    if !list
        .iter()
        .any(|keybind| keybind.managed && keybind.action == OVERLAY_CHAT)
    {
        let id = next_id(&list);
        list.push(Keybind {
            id,
            action: OVERLAY_CHAT.to_owned(),
            shortcut: Vec::new(),
            enabled: true,
            managed: true,
        });
    }
    list
}

fn save(list: &[Keybind]) {
    storage::set(STORE, to_json(list));
}

/// `customizableKeybinds` (`l$`): the managed ones other than the overlay chat are left out; managed first, the
/// rest by id (a string sort, as lodash's `sortBy` on the id).
pub(in crate::ui) fn visible(list: &[Keybind]) -> Vec<Keybind> {
    let mut shown: Vec<Keybind> = list
        .iter()
        .filter(|keybind| !keybind.managed || keybind.action == OVERLAY_CHAT)
        .cloned()
        .collect();
    shown.sort_by(|a, b| a.id.cmp(&b.id));
    shown.sort_by_key(|keybind| !keybind.managed);
    shown
}

fn label_of(action: &str) -> &'static str {
    ACTIONS
        .iter()
        .find(|(value, ..)| *value == action)
        .map_or("", |(_, label, _)| label)
}

/// What a keybind does in this client when it goes down or up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Effect {
    ToggleMute,
    ToggleDeafen,
    ToggleVoiceMode,
    ToggleStreamerMode,
    ToggleVoiceChat,
    /// `PUSH_TO_TALK` and its priority form: the microphone is open while held.
    Talk(bool),
    /// `PUSH_TO_MUTE`: the microphone is closed while held.
    PushToMute(bool),
}

/// The decision of `ACTIONS`' callbacks (`U[action]`, module 532624): toggles act on the key going down, holds on
/// both edges, and a hold only counts in the voice mode it is for. None: the keybind is stored and does nothing
/// here (the overlay and camera actions, which this client has no engine for).
pub(in crate::ui) fn effect(action: &str, down: bool, push_to_talk: bool) -> Option<Effect> {
    match action {
        "TOGGLE_MUTE" => down.then_some(Effect::ToggleMute),
        "TOGGLE_DEAFEN" => down.then_some(Effect::ToggleDeafen),
        "TOGGLE_VOICE_MODE" => down.then_some(Effect::ToggleVoiceMode),
        "TOGGLE_STREAMER_MODE" => down.then_some(Effect::ToggleStreamerMode),
        "TOGGLE_VOICE_CHANNEL_CHAT" => down.then_some(Effect::ToggleVoiceChat),
        "PUSH_TO_TALK" | "PUSH_TO_TALK_PRIORITY" => push_to_talk.then_some(Effect::Talk(down)),
        "PUSH_TO_MUTE" => (!push_to_talk).then_some(Effect::PushToMute(down)),
        _ => None,
    }
}

/// `W` (module 532624): only a bound, enabled keybind with an action is registered.
fn registered(keybind: &Keybind) -> bool {
    keybind.enabled
        && !keybind.shortcut.is_empty()
        && keybind.action != "UNASSIGNED"
        && effect_possible(&keybind.action)
}

fn effect_possible(action: &str) -> bool {
    [true, false]
        .into_iter()
        .any(|mode| effect(action, true, mode).is_some() || effect(action, false, mode).is_some())
}

#[derive(Default)]
pub(in crate::ui) struct State {
    /// The keybind whose shortcut is being recorded.
    recording: Option<String>,
    /// `[type, code]` of what is down while recording.
    held: Vec<[u32; 2]>,
    /// The row whose action list is open.
    select_for: String,
    /// The keybinds whose keys are down in the window right now.
    active: HashSet<String>,
    hooks: Vec<(String, dc_hotkey::Hook)>,
    /// `PUSH_TO_MUTE` is held.
    pub temp_mute: bool,
}

// ----- the actions -----

impl Root {
    /// The System page is on screen: every keybind is off then (`enableAll(false)` in `initialize`).
    fn keybinds_page_visible(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|open| open.page == Page::System)
    }

    fn kb_change(&mut self, list: &[Keybind]) {
        save(list);
        self.kb_sync_hooks();
    }

    fn kb_add(&mut self, cx: &mut Context<Self>) {
        let mut list = load();
        let id = next_id(&list);
        list.push(Keybind {
            id,
            action: "UNASSIGNED".to_owned(),
            shortcut: Vec::new(),
            enabled: true,
            managed: false,
        });
        self.kb_change(&list);
        cx.notify();
    }

    fn kb_update(&mut self, id: &str, change: impl FnOnce(&mut Keybind), cx: &mut Context<Self>) {
        let mut list = load();
        if let Some(keybind) = list.iter_mut().find(|keybind| keybind.id == id) {
            change(keybind);
            self.kb_change(&list);
        }
        cx.notify();
    }

    fn kb_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut list = load();
        list.retain(|keybind| keybind.id != id || keybind.managed);
        if self.settings_extra.kb.recording.as_deref() == Some(id) {
            self.settings_extra.kb.recording = None;
        }
        self.kb_change(&list);
        cx.notify();
    }

    /// The hooks follow the list: one per registered keybind, none in a harness run (nothing there may read the
    /// real keyboard) and none while a shortcut is being recorded.
    pub(in crate::ui) fn kb_sync_hooks(&mut self) {
        let wanted: Vec<(String, Vec<[u32; 3]>)> =
            if storage::is_capture_run() || self.settings_extra.kb.recording.is_some() {
                Vec::new()
            } else {
                load()
                    .into_iter()
                    .filter(registered)
                    .map(|keybind| (keybind.id, keybind.shortcut))
                    .collect()
            };
        let old = std::mem::take(&mut self.settings_extra.kb.hooks);
        let mut kept: Vec<(String, dc_hotkey::Hook)> = Vec::new();
        for (id, hook) in old {
            match wanted.iter().find(|(wanted_id, _)| *wanted_id == id) {
                Some((_, shortcut)) => {
                    hook.set_shortcut(shortcut.clone());
                    kept.push((id, hook));
                }
                // Stopping lets go of a held keybind (`on_change(false)`).
                None => hook.stop(),
            }
        }
        #[cfg(windows)]
        for (id, shortcut) in &wanted {
            if kept.iter().any(|(kept_id, _)| kept_id == id) {
                continue;
            }
            let updates = self.updates.clone();
            let keybind = id.clone();
            match dc_hotkey::start(shortcut.clone(), move |down| {
                let _ = updates.send(Update::Keybind {
                    id: keybind.clone(),
                    down,
                });
            }) {
                Ok(hook) => kept.push((id.clone(), hook)),
                Err(error) => tracing::warn!("keybind hook not started: {error}"),
            }
        }
        #[cfg(not(windows))]
        let _ = &wanted;
        self.settings_extra.kb.hooks = kept;
    }

    /// A keybind went down or up, from its hook or from the window's keys.
    pub(in crate::ui) fn kb_fire(&mut self, id: &str, down: bool, cx: &mut Context<Self>) {
        let Some(keybind) = load().into_iter().find(|keybind| keybind.id == id) else {
            return;
        };
        if !keybind.enabled || self.keybinds_page_visible() {
            return;
        }
        let push_to_talk = self.kb_push_to_talk();
        let Some(effect) = effect(&keybind.action, down, push_to_talk) else {
            return;
        };
        tracing::info!("keybind {} down={down} -> {effect:?}", keybind.action);
        match effect {
            Effect::ToggleMute => self.toggle_self_mute(cx),
            Effect::ToggleDeafen => self.toggle_self_deaf(cx),
            Effect::ToggleVoiceMode => self.kb_toggle_voice_mode(),
            Effect::ToggleStreamerMode => {
                let user = self.stores.current_user.as_ref().map(|user| user.id);
                let key = "streaming_enable_streamer_mode";
                let on = crate::notifications::switch_value(key, user).unwrap_or(false);
                crate::notifications::set_switch(key, !on, user);
            }
            Effect::ToggleVoiceChat => {
                if let Some(channel) = self.call.as_ref().map(|call| call.channel)
                    && !self.vv.chat_open.remove(&channel)
                {
                    self.vv.chat_open.insert(channel);
                }
            }
            Effect::Talk(held) => {
                super::voice::set_push_to_talk_held(held);
                self.ptt_held_changed(held);
            }
            Effect::PushToMute(held) => {
                self.settings_extra.kb.temp_mute = held;
                if let Some(call) = &self.call {
                    call.controls
                        .set_self_mute(held || self.self_mute || self.self_deaf);
                }
            }
        }
        cx.notify();
    }

    /// The window's own keys against the registered keybinds, after `voice.held` changed.
    pub(in crate::ui) fn kb_window_keys(&mut self, cx: &mut Context<Self>) {
        let held = self.kb_held();
        let page = self.keybinds_page_visible();
        let mut edges: Vec<(String, bool)> = Vec::new();
        for keybind in load().iter().filter(|keybind| registered(keybind)) {
            let down = !page && shortcut_down(&keybind.shortcut, &held);
            let was = self.settings_extra.kb.active.contains(&keybind.id);
            if down == was {
                continue;
            }
            if down {
                self.settings_extra.kb.active.insert(keybind.id.clone());
            } else {
                self.settings_extra.kb.active.remove(&keybind.id);
            }
            // With a hook running the hook reports it, with the window's keys included.
            if !self
                .settings_extra
                .kb
                .hooks
                .iter()
                .any(|(hooked, _)| *hooked == keybind.id)
            {
                edges.push((keybind.id.clone(), down));
            }
        }
        for (id, down) in edges {
            self.kb_fire(&id, down, cx);
        }
    }

    // ----- recording -----

    /// A key went down while a shortcut is being recorded (true: nobody else takes it).
    pub(in crate::ui) fn kb_key_down(
        &mut self,
        keystroke: &gpui::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_extra.kb.recording.is_none() {
            return false;
        }
        // The modifiers held with the key count as pressed before it.
        if keystroke.modifiers != self.voice_modifiers_now() {
            self.voice_modifiers(keystroke.modifiers, cx);
        }
        if keystroke.key == "escape" {
            self.kb_stop_recording();
        } else if let Some(code) = windows_code(&keystroke.key) {
            self.kb_record_part(KEYBOARD_KEY, code, cx);
        }
        cx.notify();
        true
    }

    pub(in crate::ui) fn kb_key_up(&mut self, keystroke: &gpui::Keystroke) {
        if let Some(code) = windows_code(&keystroke.key) {
            self.kb_release(KEYBOARD_KEY, code);
        }
    }

    /// gpui reports modifiers as a change of the set held; `before` is the set until now.
    pub(in crate::ui) fn kb_modifiers(
        &mut self,
        before: &Modifiers,
        after: &Modifiers,
        cx: &mut Context<Self>,
    ) {
        if self.settings_extra.kb.recording.is_none() {
            return;
        }
        let (was, now) = (modifier_codes(before), modifier_codes(after));
        for code in &was {
            if !now.contains(code) {
                self.kb_release(KEYBOARD_KEY, *code);
            }
        }
        for code in now {
            if !was.contains(&code) {
                self.kb_record_part(KEYBOARD_KEY, code, cx);
            }
        }
        cx.notify();
    }

    pub(in crate::ui) fn kb_mouse(
        &mut self,
        button: MouseButton,
        down: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(code) = Self::mouse_code(button) else {
            return false;
        };
        if self.settings_extra.kb.recording.is_none() {
            return false;
        }
        if down {
            self.kb_record_part(MOUSE_BUTTON, code, cx);
            cx.notify();
            return true;
        }
        self.kb_release(MOUSE_BUTTON, code);
        false
    }

    fn kb_stop_recording(&mut self) {
        self.settings_extra.kb.recording = None;
        self.settings_extra.kb.held.clear();
        self.kb_sync_hooks();
    }

    /// The desktop recorder takes one combo: it ends when the last of its keys is let go.
    fn kb_release(&mut self, kind: u32, code: u32) {
        let state = &mut self.settings_extra.kb;
        let before = state.held.len();
        state.held.retain(|held| *held != [kind, code]);
        if state.recording.is_some() && before > 0 && state.held.is_empty() {
            self.kb_stop_recording();
        }
    }

    /// Each part that goes down grows the combo, saved at once as each `onChange` calls `setKeybind`.
    fn kb_record_part(&mut self, kind: u32, code: u32, cx: &mut Context<Self>) {
        let Some(id) = self.settings_extra.kb.recording.clone() else {
            return;
        };
        if self.settings_extra.kb.held.contains(&[kind, code]) {
            return;
        }
        self.settings_extra.kb.held.push([kind, code]);
        let combo: Vec<[u32; 3]> = self
            .settings_extra
            .kb
            .held
            .iter()
            .map(|[kind, code]| [*kind, *code, WINDOWS])
            .collect();
        self.kb_update(&id, |keybind| keybind.shortcut = combo, cx);
    }

    // ----- drawing -----

    /// The Add a Keybind button of the category header (`variant: secondary`, a 20px plus).
    pub(in crate::ui) fn kb_add_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("add-keybind")
            .h(px(40.))
            .px(px(16.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .border_1()
            .border_color(color("--control-secondary-border-default"))
            .bg(color("--control-secondary-background-default"))
            .hover(|style| style.bg(color("--control-secondary-background-hover")))
            .on_click(cx.listener(|this, _, _, cx| this.kb_add(cx)))
            .child(mask_icon(
                "plus_small",
                20.,
                "--control-secondary-text-default",
            ))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-secondary-text-default"))
                    .child("Add a Keybind"),
            )
            .into_any_element()
    }

    /// `l0` / `l$`: the notice, then the rows (or "You haven't added any custom keybinds yet.").
    pub(in crate::ui) fn kb_custom(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let shown = visible(&load());
        let body = if shown.is_empty() {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-subtle"))
                .child("You haven't added any custom keybinds yet.")
                .into_any_element()
        } else {
            let last = shown.len() - 1;
            div()
                .flex()
                .flex_col()
                .children(shown.into_iter().enumerate().flat_map(|(at, keybind)| {
                    let row = self.kb_row(&keybind, width, cx);
                    let divider = (at < last).then(|| {
                        div()
                            .my(px(24.))
                            .h(px(1.))
                            .bg(color("--border-subtle"))
                            .into_any_element()
                    });
                    std::iter::once(row).chain(divider)
                }))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            // `iK.w` with `type: "info"` and no close button.
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .p(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--inlinenotice-border-info"))
                    .bg(color("--background-feedback-mod-info"))
                    .child(div().h(px(18.)).flex().items_center().child(mask_icon(
                        "settings_notice_info",
                        16.,
                        "--icon-feedback-info",
                    )))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-strong"))
                            .child("Keybinds are disabled while this page is visible."),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn kb_row(&self, keybind: &Keybind, width: f32, cx: &mut Context<Self>) -> AnyElement {
        // Columns `1fr 1fr auto` with a 16px gap; the last holds the delete button and the switch.
        let last = if keybind.managed { 74. } else { 74. + 8. + 32. };
        let column = (width - 32. - last) / 2.;
        let id = keybind.id.clone();
        let label = |text: &'static str, dim: bool| {
            div()
                .pb(px(8.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .when(dim, |this| this.opacity(0.5))
                .child(text)
        };
        let description = ACTIONS
            .iter()
            .find(|(value, ..)| *value == keybind.action)
            .map_or("", |(_, _, description)| *description);
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .items_end()
                    .child(
                        div()
                            .w(px(column))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .child(label("Keybind Action", keybind.managed))
                            .child(self.kb_select(keybind, column, cx)),
                    )
                    .child(
                        div()
                            .w(px(column))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .child(label("Keybind", false))
                            .child(self.kb_recorder(keybind, cx)),
                    )
                    .child(
                        div()
                            .w(px(last))
                            .h(px(42.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .when(!keybind.managed, |this| {
                                let id = id.clone();
                                this.child(
                                    div()
                                        .id(SharedString::from(format!("keybind-delete-{id}")))
                                        .size(px(32.))
                                        .rounded(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style.bg(color("--control-icon-only-background-hover"))
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.kb_delete(&id, cx);
                                        }))
                                        .child(mask_icon(
                                            "ob_trash",
                                            20.,
                                            "--control-icon-only-icon-default",
                                        )),
                                )
                            })
                            .child({
                                let id = id.clone();
                                plain_switch(
                                    SharedString::from(format!("keybind-enabled-{id}")),
                                    keybind.enabled,
                                    move |this, on| {
                                        let id = id.clone();
                                        let mut list = load();
                                        if let Some(keybind) =
                                            list.iter_mut().find(|keybind| keybind.id == id)
                                        {
                                            keybind.enabled = on;
                                        }
                                        this.kb_change(&list);
                                    },
                                    cx,
                                )
                            }),
                    ),
            )
            .when(!description.is_empty(), |this| {
                this.child(
                    div()
                        .pt(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(color("--text-subtle"))
                        .child(description),
                )
            })
            .into_any_element()
    }

    /// The action list of one row: the field and, open, the 13 options of `ACTIONS`.
    fn kb_select(&self, keybind: &Keybind, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let open = self.setting_choices.get(SELECT_OPEN) == Some(&Choice::On(true))
            && self.settings_extra.kb.select_for == keybind.id;
        let id = keybind.id.clone();
        let rows: Vec<AnyElement> = ACTIONS
            .iter()
            .enumerate()
            .map(|(index, (value, text, _))| {
                let id = id.clone();
                let chosen = *value == keybind.action;
                div()
                    .id(SharedString::from(format!("keybind-option-{index}")))
                    .flex_none()
                    .h(px(40.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .whitespace_nowrap()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-strong"))
                            .child(*text),
                    )
                    .when(chosen, |this| {
                        this.child(div().p(px(4.)).child(crate::ui::icon(
                            "select_check",
                            20.,
                            "--text-strong",
                        )))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.setting_choices.insert(SELECT_OPEN, Choice::On(false));
                        this.kb_update(&id, |keybind| keybind.action = (*value).to_owned(), cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let field = select_field_open(label_of(&keybind.action), Some(width), open)
            .id(SharedString::from(format!("keybind-action-{id}")))
            .when(keybind.managed, |this| this.opacity(0.5))
            .when(!keybind.managed, |this| {
                let id = id.clone();
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_extra.kb.select_for = id.clone();
                        this.setting_choices.insert(SELECT_OPEN, Choice::On(!open));
                        cx.notify();
                    }))
            });
        div()
            .relative()
            .flex_none()
            .child(field)
            .when(open, |this| {
                this.child(
                    gpui::deferred(
                        crate::ui::keyboard::outside(
                            div().id("keybind-action-list"),
                            crate::ui::keyboard::Layer::SettingsSelect,
                            cx,
                        )
                        .occlude()
                        .absolute()
                        .top(px(44.))
                        .left_0()
                        .right_0()
                        .p(px(4.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-higher"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.16),
                            offset: gpui::point(px(0.), px(2.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .child(
                            div()
                                .id("keybind-action-rows")
                                .flex()
                                .flex_col()
                                // `listBox_b5PEJH`: 208 tall, then it scrolls.
                                .max_h(px(208.))
                                .overflow_y_scroll()
                                .children(rows),
                        ),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    /// `keybindInput_0PgvPQ`: the combo (or "No Keybind Set") with the record button inside the field.
    fn kb_recorder(&self, keybind: &Keybind, cx: &mut Context<Self>) -> AnyElement {
        let recording = self.settings_extra.kb.recording.as_deref() == Some(keybind.id.as_str());
        let red = color("--red-400");
        let critical = color("--text-feedback-critical");
        let (text, text_color, weight) = if recording {
            (String::new(), critical, 600.)
        } else if keybind.shortcut.is_empty() {
            (
                "No Keybind Set".to_owned(),
                color("--input-placeholder-text-default"),
                400.,
            )
        } else {
            (
                shortcut_text(&keybind.shortcut),
                color("--text-strong"),
                600.,
            )
        };
        let button = div()
            .flex_none()
            .h(px(32.))
            .px(px(11.))
            .rounded(px(8.))
            .border_1()
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .whitespace_nowrap()
            .when(recording, |this| {
                this.border_color(color("--control-critical-secondary-border-default"))
                    .bg(with_alpha(red, 0.1))
                    .text_color(critical)
            })
            .when(!recording, |this| {
                this.border_color(color("--control-secondary-border-default"))
                    .bg(color("--control-secondary-background-default"))
                    .text_color(color("--control-secondary-text-default"))
            })
            .child(if recording {
                "Stop Recording"
            } else {
                "Record Keybind"
            });
        let id = keybind.id.clone();
        div()
            .id(SharedString::from(format!("keybind-recorder-{id}")))
            .h(px(42.))
            .rounded(px(8.))
            .border_1()
            .bg(color("--input-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .when(recording, |this| this.border_color(with_alpha(red, 0.6)))
            .when(!recording, |this| {
                this.border_color(color("--input-border-default"))
                    .hover(|style| style.border_color(color("--border-strong")))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.voice_stop_recording();
                let state = &mut this.settings_extra.kb;
                state.held.clear();
                state.recording = if state.recording.as_deref() == Some(id.as_str()) {
                    None
                } else {
                    Some(id.clone())
                };
                this.kb_sync_hooks();
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(px(10.))
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .font_weight(FontWeight(weight))
                    .text_color(text_color)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(text),
            )
            .child(div().flex_none().p(px(4.)).child(button))
            .into_any_element()
    }

    /// `l4`: the shortcut lists the client dispatches, as the key caps of `475358`, in the order of `l5`.
    pub(in crate::ui) fn kb_defaults(&self) -> AnyElement {
        fn dnd() -> Section {
            use crate::ui::keyboard::help::Item;
            Section {
                title: "Drag and Drop",
                note: Some("These shortcuts work when focused on a draggable item"),
                items: vec![
                    Item {
                        description: "Start Drag and Drop",
                        binds: vec!["mod+d"],
                    },
                    Item {
                        description: "Move",
                        binds: vec!["up", "down"],
                    },
                    Item {
                        description: "Drop item",
                        binds: vec!["spacebar", "enter"],
                    },
                    Item {
                        description: "Cancel",
                        binds: vec!["esc"],
                    },
                ],
            }
        }
        fn card(items: Vec<(&'static str, Vec<&'static str>)>) -> AnyElement {
            let last = items.len().saturating_sub(1);
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .py(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(color("--background-surface-higher"))
                .children(
                    items
                        .into_iter()
                        .enumerate()
                        .flat_map(|(at, (text, binds))| {
                            let row = div()
                                .px(px(16.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-default"))
                                        .child(text),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_end()
                                        .gap(px(4.))
                                        .children(binds.into_iter().map(combo)),
                                )
                                .into_any_element();
                            let divider = (at < last).then(|| {
                                div()
                                    .h(px(1.))
                                    .bg(color("--border-subtle"))
                                    .into_any_element()
                            });
                            std::iter::once(row).chain(divider)
                        }),
                )
                .into_any_element()
        }
        let mut all = sections();
        let mut order: Vec<Section> = Vec::new();
        order.extend(pick_remove(&mut all, "Messages"));
        order.extend(pick_remove(&mut all, "Navigation"));
        order.push(dnd());
        order.extend(pick_remove(&mut all, "Chat"));
        order.extend(pick_remove(&mut all, "Voice and Video"));
        order.extend(pick_remove(&mut all, "Miscellaneous"));
        let mut page = div()
            .pt(px(9.))
            .flex()
            .flex_col()
            .gap(px(48.))
            .child(card(vec![("Show Keyboard Shortcuts List", vec!["mod+/"])]));
        for section in order {
            let items = section
                .items
                .into_iter()
                // `.filter(e => e.description !== HnNtEI)`: the easter egg is not listed.
                .filter(|item| item.description != "Raging Demon")
                .map(|item| (item.description, item.binds))
                .collect();
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(section.title),
                    )
                    .children(section.note.map(|note| {
                        div()
                            .pt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(note)
                    }))
                    .child(
                        div()
                            .pt(px(if section.note.is_some() { 18. } else { 27. }))
                            .child(card(items)),
                    ),
            );
        }
        page.into_any_element()
    }
}

fn with_alpha(mut value: gpui::Rgba, alpha: f32) -> gpui::Rgba {
    value.a = alpha;
    value
}

fn pick_remove(all: &mut Vec<Section>, title: &str) -> Option<Section> {
    let at = all.iter().position(|section| section.title == title)?;
    Some(all.remove(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keybind(id: &str, action: &str, managed: bool) -> Keybind {
        Keybind {
            id: id.to_owned(),
            action: action.to_owned(),
            shortcut: vec![[0, 162, 1], [0, 77, 1]],
            enabled: true,
            managed,
        }
    }

    #[test]
    fn the_store_round_trips_in_the_official_shape() {
        let list = vec![keybind("0", "TOGGLE_MUTE", false)];
        let json = to_json(&list);
        assert_eq!(json["0"]["shortcut"], json!([[0, 162, 1], [0, 77, 1]]));
        assert_eq!(json["0"]["managed"], json!(false));
        assert_eq!(parse(&json), list);
    }

    #[test]
    fn a_missing_enabled_flag_means_enabled() {
        let parsed = parse(&json!({"3": {"id": "3", "action": "TOGGLE_MUTE", "shortcut": []}}));
        assert!(parsed[0].enabled);
    }

    #[test]
    fn ids_count_up_and_managed_come_first() {
        let list = vec![
            keybind("10", "TOGGLE_MUTE", false),
            keybind("2", "TOGGLE_DEAFEN", false),
            keybind("5", OVERLAY_CHAT, true),
            keybind("6", "SAVE_CLIP", true),
        ];
        assert_eq!(next_id(&list), "11");
        let order: Vec<String> = visible(&list).into_iter().map(|k| k.id).collect();
        // The managed save clip is not customizable here; ids sort as strings ("10" before "2").
        assert_eq!(order, ["5", "10", "2"]);
    }

    #[test]
    fn toggles_act_on_the_press_and_holds_on_both_edges() {
        assert_eq!(effect("TOGGLE_MUTE", true, false), Some(Effect::ToggleMute));
        assert_eq!(effect("TOGGLE_MUTE", false, false), None);
        assert_eq!(effect("PUSH_TO_TALK", true, true), Some(Effect::Talk(true)));
        assert_eq!(
            effect("PUSH_TO_TALK", false, true),
            Some(Effect::Talk(false))
        );
        // Hold actions only count in their own voice mode.
        assert_eq!(effect("PUSH_TO_TALK", true, false), None);
        assert_eq!(
            effect("PUSH_TO_MUTE", true, false),
            Some(Effect::PushToMute(true))
        );
        assert_eq!(effect("PUSH_TO_MUTE", true, true), None);
        assert_eq!(effect("UNASSIGNED", true, false), None);
    }

    #[test]
    fn only_bound_enabled_actionable_keybinds_are_registered() {
        let mut bound = keybind("1", "TOGGLE_MUTE", false);
        assert!(registered(&bound));
        bound.enabled = false;
        assert!(!registered(&bound));
        let mut unbound = keybind("2", "TOGGLE_MUTE", false);
        unbound.shortcut.clear();
        assert!(!registered(&unbound));
        assert!(!registered(&keybind("3", "UNASSIGNED", false)));
        assert!(!registered(&keybind("4", OVERLAY_CHAT, true)));
    }

    #[test]
    fn the_action_list_is_the_captured_one() {
        let labels: Vec<&str> = ACTIONS.iter().map(|(_, label, _)| *label).collect();
        assert_eq!(labels.len(), 13);
        assert_eq!(labels[0], "Unassigned");
        assert_eq!(labels[7], "Toggle VAD");
        assert_eq!(labels[12], "Activate Overlay Chat");
    }
}
