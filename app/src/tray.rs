//! SystemTrayManager state (596339) and the Windows menu from desktop-core 1275.
use crate::{branding::IconSet, net::Update};
use tokio::sync::mpsc::UnboundedSender;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voice {
    pub connected: bool,
    pub speaking: bool,
    pub self_mute: bool,
    pub server_mute: bool,
    pub suppressed: bool,
    pub self_deaf: bool,
    pub server_deaf: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub voice: Voice,
    pub unread: bool,
    pub icons: IconSet,
    pub close_to_tray: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            voice: Voice::default(),
            unread: false,
            icons: IconSet::default(),
            close_to_tray: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Default,
    Unread,
    Connected,
    Speaking,
    Muted,
    Deafened,
}
impl Icon {
    pub fn file(self) -> &'static str {
        match self {
            Self::Default => "tray",
            Self::Unread => "tray-unread",
            Self::Connected => "tray-connected",
            Self::Speaking => "tray-speaking",
            Self::Muted => "tray-muted",
            Self::Deafened => "tray-deafened",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Action {
    Mute = 1,
    Deafen = 2,
    VoiceSettings = 3,
    Acknowledgements = 4,
    Quit = 5,
    CheckUpdates = 6,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub action: Option<Action>,
    pub checked: bool,
    pub enabled: bool,
}
impl MenuItem {
    fn action(label: &'static str, action: Action) -> Self {
        Self {
            label,
            action: Some(action),
            checked: false,
            enabled: true,
        }
    }
    fn disabled(label: &'static str) -> Self {
        Self {
            label,
            action: None,
            checked: false,
            enabled: false,
        }
    }
}

impl State {
    pub fn icon(self) -> Icon {
        let v = self.voice;
        if !v.connected {
            return if self.unread {
                Icon::Unread
            } else {
                Icon::Default
            };
        }
        if v.self_deaf || v.server_deaf {
            Icon::Deafened
        } else if v.self_mute || v.server_mute || v.suppressed {
            Icon::Muted
        } else if v.speaking {
            Icon::Speaking
        } else {
            Icon::Connected
        }
    }

    pub fn hide_on_close(self, available: bool) -> bool {
        self.close_to_tray && available
    }

    /// None is a separator (desktop-core 1275).
    pub fn menu(self) -> Vec<Option<MenuItem>> {
        let mut items = vec![Some(MenuItem::disabled("gravecord"))];
        let v = self.voice;
        if v.connected {
            let mut mute = MenuItem::action("Mute", Action::Mute);
            mute.checked = v.self_mute || v.server_mute || v.suppressed;
            if v.suppressed || v.server_mute {
                mute.label = if v.suppressed {
                    "Suppressed"
                } else {
                    "Server Muted"
                };
                mute.enabled = false;
            }
            let mut deaf = MenuItem::action("Deafen", Action::Deafen);
            deaf.checked = v.self_deaf || v.server_deaf;
            if v.server_deaf {
                deaf.label = "Server Deafened";
                deaf.enabled = false;
            }
            items.extend([
                None,
                Some(mute),
                Some(deaf),
                Some(MenuItem::action(
                    "Open Voice && Video Settings",
                    Action::VoiceSettings,
                )),
            ]);
        }
        items.extend([
            None,
            Some(MenuItem::action(
                "Check For Updates...",
                Action::CheckUpdates,
            )),
            Some(MenuItem::action(
                "Acknowledgements",
                Action::Acknowledgements,
            )),
            None,
            Some(MenuItem::action("Quit gravecord", Action::Quit)),
        ]);
        items
    }

    pub fn allows(self, action: Action) -> bool {
        self.menu()
            .into_iter()
            .flatten()
            .any(|item| item.enabled && item.action == Some(action))
    }
}

/// Wait before attaching again after the window could not be hooked.
#[cfg(windows)]
const ATTACH_RETRY: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Runtime {
    capture: bool,
    state: State,
    observed: Option<State>,
    #[cfg(windows)]
    binding: Option<windows::Binding>,
    /// When a failed attach may be tried again; `attach` is called from render and must not retry every frame.
    #[cfg(windows)]
    attach_after: Option<std::time::Instant>,
    #[cfg(windows)]
    sender: UnboundedSender<Update>,
}
impl Runtime {
    pub fn new(sender: UnboundedSender<Update>) -> Self {
        #[cfg(not(windows))]
        let _ = sender;
        Self {
            capture: crate::storage::is_capture_run(),
            state: State::default(),
            observed: None,
            #[cfg(windows)]
            binding: None,
            #[cfg(windows)]
            attach_after: None,
            #[cfg(windows)]
            sender,
        }
    }
    pub fn attach(&mut self, window: &gpui::Window) {
        if self.capture {
            return;
        }
        #[cfg(windows)]
        if self.binding.is_none()
            && self
                .attach_after
                .is_none_or(|after| std::time::Instant::now() >= after)
        {
            self.binding = windows::Binding::new(window, self.sender.clone(), self.state);
            self.attach_after = self
                .binding
                .is_none()
                .then(|| std::time::Instant::now() + ATTACH_RETRY);
        }
        #[cfg(not(windows))]
        let _ = window;
    }
    pub fn refresh(&mut self, state: State) {
        self.state = state;
        if self.capture {
            if self.observed != Some(state) {
                tracing::info!(icon=?state.icon(), set=state.icons.name(), "tray state (capture no-op)");
                self.observed = Some(state);
            }
            return;
        }
        #[cfg(windows)]
        if let Some(binding) = &self.binding {
            binding.refresh(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connected_voice_overrides_unread_with_deaf_mute_speaking_priority() {
        let mut state = State {
            unread: true,
            ..State::default()
        };
        assert_eq!(state.icon(), Icon::Unread);
        state.voice.connected = true;
        assert_eq!(state.icon(), Icon::Connected);
        state.voice.speaking = true;
        assert_eq!(state.icon(), Icon::Speaking);
        state.voice.suppressed = true;
        assert_eq!(state.icon(), Icon::Muted);
        state.voice.server_deaf = true;
        assert_eq!(state.icon(), Icon::Deafened);
        state.voice.connected = false;
        assert_eq!(state.icon(), Icon::Unread);
        state.unread = false;
        assert_eq!(state.icon(), Icon::Default);
    }
    #[test]
    fn update_check_is_enabled_in_idle_and_voice_menus() {
        for connected in [false, true] {
            let state = State {
                voice: Voice {
                    connected,
                    ..Voice::default()
                },
                ..State::default()
            };
            assert!(state.allows(Action::CheckUpdates));
            let items: Vec<_> = state.menu().into_iter().flatten().collect();
            let update = items
                .iter()
                .position(|item| item.action == Some(Action::CheckUpdates))
                .unwrap();
            assert_eq!(items[update].label, "Check For Updates...");
            assert_eq!(items[update + 1].action, Some(Action::Acknowledgements));
            assert_eq!(items.last().unwrap().action, Some(Action::Quit));
        }
    }

    #[test]
    fn server_voice_restrictions_disable_menu_actions_and_disconnect_removes_them() {
        let mut state = State::default();
        assert!(!state.allows(Action::Mute));
        state.voice.connected = true;
        assert!(state.allows(Action::Mute));
        assert!(state.allows(Action::Deafen));
        state.voice.server_mute = true;
        state.voice.server_deaf = true;
        assert!(!state.allows(Action::Mute));
        assert!(!state.allows(Action::Deafen));
        let items: Vec<_> = state.menu().into_iter().flatten().collect();
        assert!(
            items
                .iter()
                .any(|item| item.label == "Server Muted" && item.checked)
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == "Server Deafened" && item.checked)
        );
        state.voice.suppressed = true;
        assert!(
            state
                .menu()
                .into_iter()
                .flatten()
                .any(|item| item.label == "Suppressed")
        );
        assert!(state.allows(Action::Quit));
        assert!(!state.hide_on_close(false));
        assert!(state.hide_on_close(true));
        state.close_to_tray = false;
        assert!(!state.hide_on_close(true));
    }
}
