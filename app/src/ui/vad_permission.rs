//! `PermissionVADStore` (module 596339 `wf`, `wA`): in Voice Activity mode, a server voice channel where the account
//! lacks Use Voice Activity mutes the connection (module 754712 `SET_VAD_PERMISSION` sets `tf`, which every default
//! connection's mute includes, and a self-mute toggle does nothing while it holds), and the "Push to Talk Required"
//! modal (`wI`, size `md`) offers the switch. The input mode itself stays Voice Activity until the user switches.
//! Every self-mute display (user panel, audio meter, call bar, context menus, overlay quick actions and tray) reads
//! [`Root::engine_mute`] so `isSelfMute` reports muted while Use Voice Activity is missing.

use super::Root;
use super::chat_notices::{Alert, Then};
use dc_state::permissions;
use gpui::Context;

/// `USE_VAD` (module 818348, bit 25).
pub(super) const USE_VAD: u64 = 1 << 25;

/// The alert id of `wI`, which `Alert::medium` draws at `md`.
pub(super) const WARNING_ID: &str = "vad-warning";

/// `w_` (`canUseVoiceActivity`) and `wE` (no warning pending).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VadState {
    pub has_permission: bool,
    pub warning_cleared: bool,
}

impl Default for VadState {
    fn default() -> Self {
        Self {
            has_permission: true,
            warning_cleared: true,
        }
    }
}

/// `wA`'s test: allowed outside a voice channel, in Push to Talk, in a private or Stage channel, with Use Voice
/// Activity, without a voice state, or while suppressed or asking to speak.
pub(super) fn has_permission(
    in_voice: bool,
    voice_activity: bool,
    server_channel: Option<(bool, bool)>,
    voice_state: Option<(bool, bool)>,
) -> bool {
    if !in_voice || !voice_activity {
        return true;
    }
    // `(stage, can USE_VAD)` of a server channel; a private channel (or an unknown one) is allowed.
    let Some((stage, can)) = server_channel else {
        return true;
    };
    // `(suppress, asking to speak)`.
    stage || can || voice_state.is_none_or(|(suppress, asking)| suppress || asking)
}

/// Effective self-mute (`t7` of 754712 / `isSelfMute` read across user panel, menus, overlay and tray): muted when
/// explicitly self-muted, self-deafened, or missing Use Voice Activity in Voice Activity mode.
pub(super) fn effective_self_mute(
    self_mute: bool,
    self_deaf: bool,
    has_vad_permission: bool,
) -> bool {
    self_mute || self_deaf || !has_vad_permission
}

impl Root {
    /// `wA` for the call this client is in.
    fn compute_vad_permission(&self) -> bool {
        let Some(call) = self.call.as_ref() else {
            return true;
        };
        let server_channel = self.stores.guild(call.guild).and_then(|guild| {
            let channel = guild
                .channels
                .iter()
                .find(|known| known.id == call.channel)?;
            let me = self.stores.me()?;
            Some((
                channel.kind == dc_state::ChannelKind::Other(13),
                permissions::can_in_channel(guild, channel, me, USE_VAD),
            ))
        });
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let voice_state = self
            .stores
            .voice_states
            .get(&call.guild)
            .and_then(|states| states.iter().find(|state| Some(state.user_id) == me))
            .map(|state| (state.suppress, state.request_to_speak_timestamp.is_some()));
        has_permission(
            true,
            self.voice_activity_mode(),
            server_channel,
            voice_state,
        )
    }

    /// Recomputes `w_` (on the connection's state, the mode, channel, role, member and own voice state changes);
    /// a change sets `wE` to it, mutes or unmutes the connection, and a denial opens the warning.
    /// True when it changed (the caller redraws).
    pub(super) fn sync_vad_permission(&mut self) -> bool {
        let now = self.compute_vad_permission();
        if now == self.vad.has_permission {
            return false;
        }
        self.vad = VadState {
            has_permission: now,
            warning_cleared: now,
        };
        self.apply_engine_mute();
        if !now {
            self.show_vad_warning();
        }
        true
    }

    /// The connection's mute: the account's choice, deafen, or a missing Use Voice Activity (`t7` of 754712).
    pub(super) fn engine_mute(&self) -> bool {
        effective_self_mute(self.self_mute, self.self_deaf, self.vad.has_permission)
    }

    fn apply_engine_mute(&self) {
        if let Some(call) = &self.call {
            call.controls.set_self_mute(self.engine_mute());
            self.send_voice_state(call.guild, Some(call.channel));
        }
    }

    /// `AUDIO_TOGGLE_SELF_MUTE` while denied: the toggle does nothing and `wE = w_` brings the warning back. True when
    /// the toggle was taken.
    pub(super) fn vad_blocks_mute_toggle(&mut self, cx: &mut Context<Self>) -> bool {
        if self.call.is_none() || self.vad.has_permission {
            return false;
        }
        self.vad.warning_cleared = false;
        self.show_vad_warning();
        cx.notify();
        true
    }

    /// `wI`: "Push to Talk Required", its subtitle, Don't switch (secondary, `clearVADWarning`) and Switch to Push to
    /// Talk (primary: `setMode(PUSH_TO_TALK)`, then `clearVADWarning`).
    fn show_vad_warning(&mut self) {
        let open = self.chat_extra.alert.take();
        if open.as_ref().is_some_and(|alert| alert.id == WARNING_ID) {
            // Already showing: it stays as it is.
            self.chat_extra.alert = open;
            return;
        }
        self.alert_closed(open);
        self.chat_extra.alert = Some(Alert::show(
            WARNING_ID,
            dc_intl::t("NYklhr"),
            Some(dc_intl::t("EJ26Oh").to_owned()),
            Some(dc_intl::t("UYW0dz").to_owned()),
            Some(dc_intl::t("E3Y7NH").to_owned()),
            Then::SwitchToPushToTalk,
        ));
    }

    /// `PERMISSION_CLEAR_VAD_WARNING`.
    pub(super) fn clear_vad_warning(&mut self) {
        self.vad.warning_cleared = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_activity_needs_the_permission_only_in_a_server_voice_channel() {
        let speaking = Some((false, false));
        // Denied: Voice Activity in a server voice channel without Use Voice Activity.
        assert!(!has_permission(true, true, Some((false, false)), speaking));
        // Allowed with the permission, on a Stage, in Push to Talk, outside voice, in a private channel.
        assert!(has_permission(true, true, Some((false, true)), speaking));
        assert!(has_permission(true, true, Some((true, false)), speaking));
        assert!(has_permission(true, false, Some((false, false)), speaking));
        assert!(has_permission(false, true, Some((false, false)), speaking));
        assert!(has_permission(true, true, None, speaking));
        // No voice state yet, suppressed, or asking to speak.
        assert!(has_permission(true, true, Some((false, false)), None));
        assert!(has_permission(
            true,
            true,
            Some((false, false)),
            Some((true, false))
        ));
        assert!(has_permission(
            true,
            true,
            Some((false, false)),
            Some((false, true))
        ));
    }

    #[test]
    fn missing_vad_permission_reports_self_muted() {
        assert!(!effective_self_mute(false, false, true));
        assert!(effective_self_mute(true, false, true));
        assert!(effective_self_mute(false, true, true));
        // Missing USE_VAD forces muted display even when self_mute and self_deaf are both off.
        assert!(effective_self_mute(false, false, false));
    }
}
