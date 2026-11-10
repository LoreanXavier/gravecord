use super::Root;
use crate::tray::{Action, State, Voice};
use gpui::Context;

impl Root {
    fn tray_voice(&self) -> Voice {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let state = self
            .call
            .as_ref()
            .and_then(|call| self.stores.voice_states.get(&call.guild))
            .and_then(|states| states.iter().find(|state| Some(state.user_id) == me));
        Voice {
            connected: self.call.as_ref().is_some_and(|call| call.connected),
            speaking: me.is_some_and(|me| self.speaking.contains(&me)),
            self_mute: self.self_mute || self.self_deaf,
            self_deaf: self.self_deaf,
            server_mute: state.is_some_and(|state| state.mute),
            server_deaf: state.is_some_and(|state| state.deaf),
            suppressed: state.is_some_and(|state| state.suppress),
        }
    }
    pub(super) fn refresh_tray(&mut self, unread: bool) {
        self.tray_runtime.refresh(State {
            voice: self.tray_voice(),
            unread,
            icons: crate::branding::selected(),
            close_to_tray: self.pref_on("os_minimize_to_tray", true),
        });
    }
    pub(super) fn tray_action(&mut self, action: Action, cx: &mut Context<Self>) {
        let state = State {
            voice: self.tray_voice(),
            ..State::default()
        };
        // Recheck after queuing: a disconnect or server restriction can arrive while the menu is open.
        if !state.allows(action) {
            return;
        }
        match action {
            Action::Mute => self.toggle_self_mute(cx),
            Action::Deafen => self.toggle_self_deaf(cx),
            Action::VoiceSettings => self.open_settings_section("Voice & Video", cx),
            Action::Acknowledgements => cx.open_url("https://discord.com/acknowledgements"),
            Action::Quit => cx.quit(),
        }
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_tray(&mut self) -> String {
        if !crate::storage::is_capture_run() {
            return "tray preview requires capture".into();
        }
        let mut output = Vec::new();
        for icons in [
            crate::branding::IconSet::Gravecord,
            crate::branding::IconSet::Original,
        ] {
            let mut state = State {
                icons,
                ..State::default()
            };
            for step in 0..6 {
                match step {
                    1 => state.unread = true,
                    2 => state.voice.connected = true,
                    3 => state.voice.speaking = true,
                    4 => state.voice.self_mute = true,
                    5 => state.voice.self_deaf = true,
                    _ => {}
                }
                self.tray_runtime.refresh(state);
                output.push(format!("{} {:?}", icons.name(), state.icon()));
            }
        }
        self.refresh_notification_badge();
        format!("tray capture no-op: {}", output.join(", "))
    }
}
