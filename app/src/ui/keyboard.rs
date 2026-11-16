//! The official keyboard shortcuts, as data, and the order in which a key is offered to the layers of the window.
//!
//! The table is the keybind layout of the pinned build: module 158781 (`eS`, the layout the Keyboard Shortcuts help
//! lists), with the binds each action module declares (194482 servers, 980372 channels and history, 513291 unread
//! navigation, 92960 and 795737 mark read, 314519, 552049, 877991, 332779, 274794, 675704, 14214, 151199, 552992,
//! 806964, 774603, 406975, 64460) and the desktop shell's menu accelerators (`discord_desktop_core` bundle, Windows
//! template). Binds keep the official strings; `mod` is Ctrl on Windows (module 649334, `Mu`). The Mousetrap
//! registration (775121) is mirrored: a bind with a modifier repeated never matches, and every action is bound
//! globally, so it also fires while a text field has focus.
//!
//! Submodules: `nav` (server, channel and unread stepping as pure functions), `focus` (the focus supervisor, the
//! Escape chain and type-to-focus) and `actions` (what each action does to the window).

use gpui::Keystroke;

mod actions;
mod context_key;
mod explainer;
mod focus;
mod grid;
pub(in crate::ui) mod help;
mod layers;
mod messages;
mod nav;
mod stops;

pub(super) use focus::State;
pub(super) use grid::{first as grid_first, reveal_row, step as grid_step};
pub(super) use layers::{Layer, outside};
pub(super) use stops::{
    Nav, Stops, focusable, hollow_ring, inset_ring_when_focused, ring_when_focused, tab_stop,
};

/// One action of the official table (`IWg` in module 652215).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Action {
    ServerNext,
    ServerPrev,
    ChannelNext,
    ChannelPrev,
    NavigateBack,
    NavigateForward,
    UnreadNext,
    UnreadPrev,
    MentionChannelNext,
    MentionChannelPrev,
    TogglePreviousGuild,
    JumpToGuild,
    MarkChannelRead,
    MarkServerRead,
    ToggleChannelPins,
    ToggleInbox,
    MarkTopInboxChannelRead,
    ToggleUsers,
    ToggleHelp,
    VibeWithWumpus,
    ToggleMute,
    ToggleDeafen,
    ToggleCategoryCollapsed,
    SearchSoundboard,
    ScrollUp,
    ScrollDown,
    QuickSwitcherShow,
    CreateDmGroup,
    CreateThread,
    OpenChannelTab,
    CloseChannelTab,
    ToggleChannelTabPin,
    SearchEmojis,
    SearchGifs,
    SearchStickers,
    ToggleHotkeys,
    JumpToFirstUnread,
    CreateGuild,
    UploadFile,
    RecordVoiceMessage,
    ReturnToAudioChannel,
    CallAccept,
    CallStart,
    FocusSearch,
    JumpToCurrentCall,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenAppDirectory,
    BrowserDevtools,
    OpenContextMenu,
    CopyChannelLink,
    /// Desktop menu "Options" (`Ctrl+,`).
    OpenSettings,
    /// Desktop menu "Toggle Full Screen" (`F11`).
    ToggleFullscreen,
}

/// The official guard some actions start with: `hasLayers()` (module 186111) returns without doing anything while a
/// settings layer is open, and the bind still swallows the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Guard {
    None,
    NoLayers,
}

/// Whether a bind can fire in the pinned build on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Availability {
    Active,
    /// Registered, but the key can never match (Mousetrap sees a repeated modifier).
    Unreachable(&'static str),
    /// Behind an experiment or setting that is off in the pinned build.
    Disabled(&'static str),
}

#[derive(Debug)]
pub(super) struct Entry {
    pub action: Action,
    /// The official action name.
    pub name: &'static str,
    pub binds: &'static [&'static str],
    pub guard: Guard,
    pub availability: Availability,
    /// The official module that declares the bind.
    pub source: &'static str,
    /// Still works while a modal or menu is open: the official client binds these globally with no layer check, and
    /// none of them acts on what the modal shows.
    pub under_modal: bool,
}

const fn entry(
    action: Action,
    name: &'static str,
    binds: &'static [&'static str],
    guard: Guard,
    source: &'static str,
) -> Entry {
    Entry {
        action,
        name,
        binds,
        guard,
        availability: Availability::Active,
        source,
        under_modal: false,
    }
}

const fn always(mut entry: Entry) -> Entry {
    entry.under_modal = true;
    entry
}

const fn with(mut entry: Entry, availability: Availability) -> Entry {
    entry.availability = availability;
    entry
}

use Action as A;
use Guard::{NoLayers, None as Open};

/// The pinned build's default shortcuts on Windows.
pub(super) const TABLE: &[Entry] = &[
    entry(
        A::ServerNext,
        "SERVER_NEXT",
        &["mod+alt+down", "mod+shift+]", "ctrl+tab"],
        Open,
        "194482",
    ),
    entry(
        A::ServerPrev,
        "SERVER_PREV",
        &["mod+alt+up", "mod+shift+[", "ctrl+shift+tab"],
        Open,
        "194482",
    ),
    entry(
        A::ChannelNext,
        "CHANNEL_NEXT",
        &["alt+down"],
        Open,
        "980372",
    ),
    entry(A::ChannelPrev, "CHANNEL_PREV", &["alt+up"], Open, "980372"),
    // `mod+[` and `mod+]` are the macOS and Linux binds; Windows uses the arrows.
    always(entry(
        A::NavigateBack,
        "NAVIGATE_BACK",
        &["alt+left"],
        Open,
        "980372",
    )),
    always(entry(
        A::NavigateForward,
        "NAVIGATE_FORWARD",
        &["alt+right"],
        Open,
        "980372",
    )),
    entry(
        A::UnreadNext,
        "UNREAD_NEXT",
        &["alt+shift+down"],
        Open,
        "513291",
    ),
    entry(
        A::UnreadPrev,
        "UNREAD_PREV",
        &["alt+shift+up"],
        Open,
        "513291",
    ),
    entry(
        A::MentionChannelNext,
        "MENTION_CHANNEL_NEXT",
        &["mod+shift+alt+down"],
        Open,
        "513291",
    ),
    entry(
        A::MentionChannelPrev,
        "MENTION_CHANNEL_PREV",
        &["mod+shift+alt+up"],
        Open,
        "513291",
    ),
    entry(
        A::TogglePreviousGuild,
        "TOGGLE_PREVIOUS_GUILD",
        &["mod+alt+right"],
        Open,
        "980372",
    ),
    entry(
        A::JumpToGuild,
        "JUMP_TO_GUILD",
        &[
            "mod+1", "mod+2", "mod+3", "mod+4", "mod+5", "mod+6", "mod+7", "mod+8", "mod+9",
        ],
        Open,
        "64460",
    ),
    entry(
        A::MarkChannelRead,
        "MARK_CHANNEL_READ",
        &["esc", "shift+pagedown"],
        Open,
        "92960",
    ),
    entry(
        A::MarkServerRead,
        "MARK_SERVER_READ",
        &["shift+esc"],
        Open,
        "795737",
    ),
    entry(
        A::ToggleChannelPins,
        "TOGGLE_CHANNEL_PINS",
        &["mod+p"],
        Open,
        "158781",
    ),
    entry(
        A::ToggleInbox,
        "TOGGLE_INBOX",
        &["mod+i"],
        NoLayers,
        "158781",
    ),
    entry(
        A::MarkTopInboxChannelRead,
        "MARK_TOP_INBOX_CHANNEL_READ",
        &["mod+shift+e"],
        Open,
        "158781",
    ),
    entry(A::ToggleUsers, "TOGGLE_USERS", &["mod+u"], Open, "158781"),
    entry(
        A::ToggleHelp,
        "TOGGLE_HELP",
        &["mod+shift+h", "f1"],
        Open,
        "158781",
    ),
    with(
        entry(
            A::VibeWithWumpus,
            "VIBE_WITH_WUMPUS",
            &["mod+alt+shift+w"],
            Open,
            "158781",
        ),
        Availability::Disabled("opens the Lofi with Wumpus popout, which the client does not have"),
    ),
    always(entry(
        A::ToggleMute,
        "TOGGLE_MUTE",
        &["mod+shift+m"],
        Open,
        "14214",
    )),
    always(entry(
        A::ToggleDeafen,
        "TOGGLE_DEAFEN",
        &["mod+shift+d"],
        Open,
        "14214",
    )),
    entry(
        A::ToggleCategoryCollapsed,
        "TOGGLE_CATEGORY_COLLAPSED",
        &["mod+shift+a"],
        Open,
        "151199",
    ),
    entry(
        A::SearchSoundboard,
        "SEARCH_SOUNDBOARD",
        &["mod+shift+b"],
        Open,
        "158781",
    ),
    entry(A::ScrollUp, "SCROLL_UP", &["pageup"], Open, "552049"),
    entry(A::ScrollDown, "SCROLL_DOWN", &["pagedown"], Open, "552049"),
    entry(
        A::QuickSwitcherShow,
        "QUICKSWITCHER_SHOW",
        &["mod+k"],
        NoLayers,
        "675704",
    ),
    entry(
        A::CreateDmGroup,
        "CREATE_DM_GROUP",
        &["mod+shift+t"],
        Open,
        "158781",
    ),
    entry(
        A::CreateThread,
        "CREATE_THREAD",
        &["mod+shift+alt+n"],
        Open,
        "158781",
    ),
    // With the channel tabs experiment off, `mod+t` opens the quick switcher (158781 `OPEN_CHANNEL_TAB`).
    entry(
        A::OpenChannelTab,
        "OPEN_CHANNEL_TAB",
        &["mod+t"],
        Open,
        "158781",
    ),
    with(
        entry(
            A::CloseChannelTab,
            "CLOSE_CHANNEL_TAB",
            &["mod+w"],
            Open,
            "158781",
        ),
        Availability::Disabled("channel tabs are off (ChannelTabsStore.isEnabled)"),
    ),
    with(
        entry(
            A::ToggleChannelTabPin,
            "TOGGLE_CHANNEL_TAB_PIN",
            &["mod+shift+p"],
            Open,
            "158781",
        ),
        Availability::Disabled("channel tabs are off (ChannelTabsStore.isEnabled)"),
    ),
    entry(
        A::SearchEmojis,
        "SEARCH_EMOJIS",
        &["mod+e"],
        NoLayers,
        "877991",
    ),
    entry(A::SearchGifs, "SEARCH_GIFS", &["mod+g"], NoLayers, "332779"),
    entry(
        A::SearchStickers,
        "SEARCH_STICKERS",
        &["mod+s"],
        NoLayers,
        "274794",
    ),
    always(entry(
        A::ToggleHotkeys,
        "TOGGLE_HOTKEYS",
        &["mod+/", "mod+shift+/"],
        Open,
        "552992",
    )),
    entry(
        A::JumpToFirstUnread,
        "JUMP_TO_FIRST_UNREAD",
        &["shift+pageup"],
        Open,
        "314519",
    ),
    entry(
        A::CreateGuild,
        "CREATE_GUILD",
        &["mod+shift+n"],
        Open,
        "158781",
    ),
    entry(
        A::UploadFile,
        "UPLOAD_FILE",
        &["mod+shift+u"],
        Open,
        "158781",
    ),
    // Predicate `K` is the voice message experiment, which the official client has on for the fixture account;
    // the shortcut itself checks the gate (`voice_recording_enabled`).
    entry(
        A::RecordVoiceMessage,
        "RECORD_VOICE_MESSAGE",
        &["alt+r"],
        Open,
        "158781",
    ),
    entry(
        A::ReturnToAudioChannel,
        "RETURN_TO_AUDIO_CHANNEL",
        &["mod+alt+left"],
        Open,
        "806964",
    ),
    always(entry(
        A::CallAccept,
        "CALL_ACCEPT",
        &["mod+return"],
        Open,
        "774603",
    )),
    always(entry(
        A::CallStart,
        "CALL_START",
        &["ctrl+'", "ctrl+shift+'"],
        Open,
        "774603",
    )),
    entry(
        A::FocusSearch,
        "FOCUS_SEARCH",
        &["mod+f", "mod+shift+f"],
        Open,
        "158781",
    ),
    entry(
        A::JumpToCurrentCall,
        "JUMP_TO_CURRENT_CALL",
        &["mod+shift+alt+v"],
        Open,
        "158781",
    ),
    always(entry(A::ZoomIn, "ZOOM_IN", &["mod+plus"], Open, "406975")),
    always(entry(
        A::ZoomOut,
        "ZOOM_OUT",
        &["mod+minus"],
        Open,
        "406975",
    )),
    always(entry(
        A::ZoomReset,
        "ZOOM_RESET",
        &["mod+0"],
        Open,
        "406975",
    )),
    with(
        entry(
            A::OpenAppDirectory,
            "OPEN_APP_DIRECTORY",
            &["mod+ctrl+a"],
            Open,
            "158781",
        ),
        Availability::Unreachable(
            "`mod` becomes ctrl on Windows, so the bind repeats ctrl and never matches",
        ),
    ),
    // On desktop the handler only cancels the key (`HZ()` and host `discord.com`).
    entry(
        A::BrowserDevtools,
        "BROWSER_DEVTOOLS",
        &["mod+alt+i"],
        Open,
        "158781",
    ),
    entry(
        A::OpenContextMenu,
        "OPEN_CONTEXT_MENU",
        &["shift+f10"],
        Open,
        "158781",
    ),
    entry(
        A::CopyChannelLink,
        "COPY_CHANNEL_LINK",
        &["mod+shift+l"],
        Open,
        "158781",
    ),
    entry(
        A::OpenSettings,
        "MENU_OPEN_SETTINGS",
        &["ctrl+,"],
        Open,
        "desktop menu",
    ),
    entry(
        A::ToggleFullscreen,
        "MENU_TOGGLE_FULLSCREEN",
        &["f11"],
        Open,
        "desktop menu",
    ),
];

/// A parsed bind: its modifiers and the key it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

/// Parses an official bind string (`mod+alt+down`). `mod` is Ctrl. A bind that repeats a modifier (the result of
/// replacing `mod` in `mod+ctrl+a`) is dropped: Mousetrap compares the modifier lists, and a key press never has the
/// duplicate.
pub(super) fn parse(bind: &str) -> Option<Combo> {
    let mut combo = Combo::default();
    let mut key = None;
    for part in bind.split('+') {
        let slot = match part {
            "mod" | "ctrl" => Some(&mut combo.ctrl),
            "alt" => Some(&mut combo.alt),
            "shift" => Some(&mut combo.shift),
            _ => None,
        };
        match slot {
            Some(flag) => {
                if *flag {
                    return None;
                }
                *flag = true;
            }
            None => {
                if key.is_some() {
                    return None;
                }
                key = Some(match part {
                    "return" => "enter",
                    "esc" => "escape",
                    other => other,
                });
            }
        }
    }
    combo.key = key?.to_owned();
    Some(combo)
}

impl Combo {
    pub fn matches(&self, keystroke: &Keystroke) -> bool {
        let held = &keystroke.modifiers;
        if held.platform || held.function || held.control != self.ctrl || held.alt != self.alt {
            return false;
        }
        let key = keystroke.key.as_str();
        match self.key.as_str() {
            // The plus key is Shift and the equals key on most layouts; the bind does not care about Shift.
            "plus" => {
                matches!(key, "+" | "=" | "add") || keystroke.key_char.as_deref() == Some("+")
            }
            "minus" => matches!(key, "-" | "subtract"),
            name => held.shift == self.shift && key == name,
        }
    }
}

/// The entry and the bind a key press hit.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hit {
    pub entry: &'static Entry,
    pub bind: &'static str,
}

/// The active table entry a key press matches.
pub(super) fn lookup(keystroke: &Keystroke) -> Option<Hit> {
    TABLE
        .iter()
        .filter(|entry| entry.availability == Availability::Active)
        .find_map(|entry| {
            entry.binds.iter().find_map(|bind| {
                parse(bind)
                    .filter(|combo| combo.matches(keystroke))
                    .map(|_| Hit { entry, bind })
            })
        })
}

/// What the window knows when a key arrives, for `route`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Facts {
    /// A settings layer is open (`LayerStore.getLayers().length > 0`).
    pub layers: bool,
    /// A dialog or a menu is open.
    pub modal: bool,
    /// A text field has focus.
    pub typing: bool,
}

/// Who gets the key.
#[derive(Clone, Copy, Debug)]
pub(super) enum Route {
    /// The open dialog or menu keeps it.
    Overlay,
    /// A table entry runs.
    Global(Hit),
    /// A table entry whose guard fails: the key is cancelled and nothing happens.
    Swallow,
    /// The key starts typing in the composer (`TEXTAREA_FOCUS`, module 82038).
    Typing,
    /// The focused text field uses it.
    Input,
    /// Nobody here wants it.
    Pass,
}

/// The order in which the layers see a key: an open dialog or menu first, the focused text field next, the global
/// table last (module 775121 binds every action with `bindGlobal`, so a key that names an action reaches it even
/// from a text field). Plain printable keys are not in the table: they belong to the field, or start typing.
pub(super) fn route(keystroke: &Keystroke, facts: Facts) -> Route {
    if let Some(hit) = lookup(keystroke) {
        if facts.modal && !hit.entry.under_modal {
            return Route::Overlay;
        }
        if hit.entry.guard == Guard::NoLayers && facts.layers {
            return Route::Swallow;
        }
        return Route::Global(hit);
    }
    if facts.modal {
        return Route::Overlay;
    }
    if facts.typing {
        return Route::Input;
    }
    if focus::starts_typing(keystroke) {
        return Route::Typing;
    }
    Route::Pass
}

impl crate::ui::Root {
    /// Script step `unread`: a message newer than anything acked arrived in `channel`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_unread(&mut self, channel: u64, cx: &mut gpui::Context<Self>) {
        self.stores
            .read_states
            .message_created(channel, u64::MAX >> 1, None, false);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn stroke(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    #[test]
    fn every_action_has_keys() {
        for entry in TABLE {
            assert!(!entry.binds.is_empty(), "{} has no keys", entry.name);
            for bind in entry.binds {
                assert!(
                    parse(bind).is_some()
                        || matches!(entry.availability, Availability::Unreachable(_)),
                    "{} has an unparsable bind {bind}",
                    entry.name
                );
            }
        }
    }

    #[test]
    fn no_two_active_actions_share_keys_under_one_guard() {
        let mut seen: HashMap<(Combo, bool), &str> = HashMap::new();
        for entry in TABLE
            .iter()
            .filter(|entry| entry.availability == Availability::Active)
        {
            for bind in entry.binds {
                let combo = parse(bind).unwrap();
                let guarded = entry.guard == Guard::NoLayers;
                if let Some(other) = seen.insert((combo, guarded), entry.name) {
                    assert_eq!(other, entry.name, "{bind} is bound twice");
                }
            }
        }
    }

    #[test]
    fn the_official_table_has_every_named_action() {
        for name in [
            "NAVIGATE_BACK",
            "NAVIGATE_FORWARD",
            "SERVER_NEXT",
            "SERVER_PREV",
            "CHANNEL_NEXT",
            "CHANNEL_PREV",
            "UNREAD_NEXT",
            "UNREAD_PREV",
            "MENTION_CHANNEL_NEXT",
            "MARK_CHANNEL_READ",
            "MARK_SERVER_READ",
            "QUICKSWITCHER_SHOW",
            "TOGGLE_MUTE",
            "TOGGLE_DEAFEN",
            "CREATE_GUILD",
            "TOGGLE_CHANNEL_PINS",
            "TOGGLE_USERS",
            "TOGGLE_INBOX",
            "SCROLL_UP",
            "SCROLL_DOWN",
            "JUMP_TO_FIRST_UNREAD",
            "UPLOAD_FILE",
            "CALL_ACCEPT",
            "CALL_START",
            "TOGGLE_HOTKEYS",
            "ZOOM_IN",
        ] {
            assert!(TABLE.iter().any(|entry| entry.name == name), "{name}");
        }
    }

    #[test]
    fn ctrl_tab_and_ctrl_digits_are_the_server_actions() {
        let hit = lookup(&stroke("ctrl-tab")).unwrap();
        assert_eq!(hit.entry.action, Action::ServerNext);
        assert_eq!(hit.bind, "ctrl+tab");
        assert_eq!(
            lookup(&stroke("ctrl-shift-tab")).unwrap().entry.action,
            Action::ServerPrev
        );
        for digit in 1..=9 {
            let source: &'static str = Box::leak(format!("ctrl-{digit}").into_boxed_str());
            let hit = lookup(&stroke(source)).unwrap();
            assert_eq!(hit.entry.action, Action::JumpToGuild);
            assert_eq!(hit.bind, format!("mod+{digit}"));
        }
        assert!(lookup(&stroke("ctrl-0")).is_some_and(|hit| hit.entry.action == Action::ZoomReset));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        assert!(
            lookup(&stroke("alt-down")).is_some_and(|hit| hit.entry.action == Action::ChannelNext)
        );
        assert!(
            lookup(&stroke("alt-shift-down"))
                .is_some_and(|hit| hit.entry.action == Action::UnreadNext)
        );
        assert!(
            lookup(&stroke("ctrl-alt-shift-down"))
                .is_some_and(|hit| hit.entry.action == Action::MentionChannelNext)
        );
        assert!(lookup(&stroke("ctrl-down")).is_none());
        assert!(lookup(&stroke("shift-tab")).is_none());
    }

    #[test]
    fn a_repeated_modifier_never_matches() {
        assert!(parse("mod+ctrl+a").is_none());
        assert!(lookup(&stroke("ctrl-a")).is_none());
    }

    #[test]
    fn the_plus_bind_ignores_shift() {
        assert!(lookup(&stroke("ctrl-=")).is_some_and(|hit| hit.entry.action == Action::ZoomIn));
        assert!(
            lookup(&stroke("ctrl-shift-=")).is_some_and(|hit| hit.entry.action == Action::ZoomIn)
        );
        assert!(lookup(&stroke("ctrl--")).is_some_and(|hit| hit.entry.action == Action::ZoomOut));
    }

    #[test]
    fn a_modal_gets_the_key_before_the_table() {
        let modal = Facts {
            modal: true,
            ..Facts::default()
        };
        assert!(matches!(route(&stroke("ctrl-tab"), modal), Route::Overlay));
        assert!(matches!(route(&stroke("a"), modal), Route::Overlay));
        // Mute and deafen are not about the dialog.
        assert!(matches!(
            route(&stroke("ctrl-shift-m"), modal),
            Route::Global(hit) if hit.entry.action == Action::ToggleMute
        ));
    }

    #[test]
    fn a_text_field_keeps_plain_keys_but_not_global_combos() {
        let typing = Facts {
            typing: true,
            ..Facts::default()
        };
        assert!(matches!(route(&stroke("a"), typing), Route::Input));
        assert!(matches!(route(&stroke("left"), typing), Route::Input));
        assert!(matches!(
            route(&stroke("ctrl-tab"), typing),
            Route::Global(hit) if hit.entry.action == Action::ServerNext
        ));
        assert!(matches!(
            route(&stroke("pagedown"), typing),
            Route::Global(hit) if hit.entry.action == Action::ScrollDown
        ));
    }

    #[test]
    fn a_printable_key_with_nothing_focused_starts_typing() {
        let idle = Facts::default();
        assert!(matches!(route(&stroke("a"), idle), Route::Typing));
        assert!(matches!(route(&stroke("shift-a"), idle), Route::Typing));
        assert!(matches!(route(&stroke("left"), idle), Route::Pass));
        assert!(matches!(route(&stroke("ctrl-c"), idle), Route::Pass));
    }

    #[test]
    fn layer_guards_swallow_the_key() {
        let layers = Facts {
            layers: true,
            ..Facts::default()
        };
        assert!(matches!(route(&stroke("ctrl-k"), layers), Route::Swallow));
        assert!(matches!(route(&stroke("ctrl-e"), layers), Route::Swallow));
        assert!(matches!(
            route(&stroke("ctrl-tab"), layers),
            Route::Global(_)
        ));
    }
}
