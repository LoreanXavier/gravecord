//! Voice: joining and leaving through op 4, handing `VOICE_SERVER_UPDATE` to `dc-voice`, and the members shown under a
//! voice channel. The join flow is `selectVoiceChannel` / `voiceStateUpdate` in the bundle; flags from
//! `computeVoiceFlags` (CLIPS_ENABLED 1, ALLOW_VOICE_RECORDING 2).

use super::{Root, avatar_source, color};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use tracing::info;

/// `ALLOW_VOICE_RECORDING`: the Data & Privacy switch "Allow my voice to be recorded in Clips".
const ALLOW_VOICE_RECORDING: u64 = 2;

pub(crate) struct Call {
    pub guild: Id,
    pub channel: Id,
    controls: dc_voice::Controls,
    /// The connection was handed to `dc-voice`.
    started: bool,
    pub connected: bool,
}

/// `linkBottom_TNyK-N`: under the channel this client is in, "Set a channel status" with a pencil, 12/16 medium.
pub(super) fn status_line() -> AnyElement {
    div()
        .flex_none()
        .mt(px(-5.))
        .mb(px(4.))
        .h(px(16.))
        .pl(px(44.))
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-muted"))
        .child("Set a channel status")
        .child(super::icon("channel_status_pencil", 12., "--text-muted"))
        .into_any_element()
}

impl Root {
    fn voice_flags(&self) -> u64 {
        let allowed = match self
            .setting_choices
            .get("clips_allow_voice_recording_setting")
        {
            Some(super::settings::Choice::On(on)) => *on,
            // The captured Data & Privacy page has it on.
            _ => true,
        };
        if allowed { ALLOW_VOICE_RECORDING } else { 0 }
    }

    fn send_voice_state(&self, guild: Id, channel: Option<Id>) {
        let _ = self.requests.send(Request::VoiceState {
            guild,
            channel,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            flags: self.voice_flags(),
        });
    }

    pub(super) fn join_voice(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            if call.channel == channel {
                return;
            }
            call.controls.disconnect();
        }
        info!("joining voice");
        let controls = dc_voice::Controls::new();
        controls.set_self_mute(self.self_mute);
        controls.set_self_deaf(self.self_deaf);
        self.call = Some(Call {
            guild,
            channel,
            controls,
            started: false,
            connected: false,
        });
        self.stores.voice_server = None;
        self.send_voice_state(guild, Some(channel));
        cx.notify();
    }

    pub(super) fn leave_voice(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.take() {
            call.controls.disconnect();
            self.send_voice_state(call.guild, None);
        }
        cx.notify();
    }

    pub(super) fn toggle_self_mute(&mut self, cx: &mut Context<Self>) {
        self.self_mute = !self.self_mute;
        // Unmuting while deafened undeafens too, as the official toggle does.
        if !self.self_mute {
            self.self_deaf = false;
        }
        self.after_voice_toggle(cx);
    }

    pub(super) fn toggle_self_deaf(&mut self, cx: &mut Context<Self>) {
        self.self_deaf = !self.self_deaf;
        self.after_voice_toggle(cx);
    }

    fn after_voice_toggle(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            call.controls
                .set_self_mute(self.self_mute || self.self_deaf);
            call.controls.set_self_deaf(self.self_deaf);
            self.send_voice_state(call.guild, Some(call.channel));
        }
        cx.notify();
    }

    /// Once the gateway has named the voice server and our session, the call can connect.
    pub(super) fn maybe_connect_voice(&mut self) {
        let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) else {
            return;
        };
        let Some(call) = self.call.as_mut() else {
            return;
        };
        if call.started {
            return;
        }
        let Some(server) = self
            .stores
            .voice_server
            .as_ref()
            .filter(|server| server.guild_id == call.guild)
        else {
            return;
        };
        let Some(session) = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .find(|state| state.user_id == me)
            .map(|state| state.session_id.clone())
        else {
            return;
        };
        call.started = true;
        let join = dc_voice::Join {
            endpoint: server.endpoint.clone(),
            token: server.token.clone(),
            session_id: session,
            server_id: call.guild,
            channel_id: call.channel,
            user_id: me,
        };
        let _ = self.requests.send(Request::VoiceConnect {
            join,
            controls: call.controls.clone(),
        });
    }

    pub(super) fn on_voice_event(&mut self, event: dc_voice::Event, cx: &mut Context<Self>) {
        match event {
            dc_voice::Event::Connected => {
                info!("voice connected");
                if let Some(call) = self.call.as_mut() {
                    call.connected = true;
                }
            }
            dc_voice::Event::SpeakingChanged { user_id, speaking } => {
                if speaking {
                    self.speaking.insert(user_id);
                } else {
                    self.speaking.remove(&user_id);
                }
            }
            dc_voice::Event::Disconnected { reason } => {
                info!("voice disconnected: {reason:?}");
                self.speaking.clear();
                if let Some(call) = self.call.as_mut() {
                    call.connected = false;
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// `wrapper_9U6LUN`: connection state, where, the call buttons, and the camera, screen, activity and soundboard
    /// row. Measured in `design/dom/channel/voice-connected-darker-cozy.json`.
    pub(super) fn voice_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.call.as_ref()?;
        let guild = self.stores.guild(call.guild)?;
        let channel = guild
            .channels
            .iter()
            .find(|channel| channel.id == call.channel)
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        let (status, status_color) = if call.connected {
            ("Voice Connected", "--text-feedback-positive")
        } else {
            ("Connecting", "--text-feedback-warning")
        };
        let round = |id: &'static str, name: &'static str| {
            div()
                .id(id)
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .child(super::icon(name, 20., "--interactive-text-default"))
        };
        let wide = |name: &'static str| {
            div()
                .flex_1()
                .h(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-secondary-border-default"))
                .bg(color("--control-secondary-background-default"))
                .flex()
                .items_center()
                .justify_center()
                .child(super::icon(name, 20., "--control-secondary-text-default"))
        };
        Some(
            div()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .border_b_1()
                .border_color(color("--background-mod-subtle"))
                .child(
                    div()
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .size(px(32.))
                                .rounded(px(8.))
                                .bg(color("--background-feedback-mod-positive"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(super::icon("voice_ping", 18., "--text-feedback-positive")),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(19.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color(status_color))
                                        .child(status),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-default"))
                                        .child(format!("{channel} / {}", guild.name)),
                                ),
                        )
                        .child(
                            div()
                                .ml(px(10.))
                                .flex()
                                .gap(px(4.))
                                .child(round("voice-krisp", "voice_krisp"))
                                .child(
                                    round("voice-disconnect", "voice_disconnect").on_click(
                                        cx.listener(|this, _, _, cx| this.leave_voice(cx)),
                                    ),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(wide("voice_camera"))
                        .child(wide("voice_screen"))
                        .child(wide("voice_activity"))
                        .child(wide("voice_soundboard")),
                )
                .into_any_element(),
        )
    }

    /// Members in a voice channel, under its row: 24px avatar and name. A green ring marks who speaks.
    pub(super) fn voice_members(&self, guild: Id, channel: Id) -> Vec<AnyElement> {
        self.stores
            .voice_members(guild, channel)
            .into_iter()
            .map(|state| {
                let user = self
                    .stores
                    .users
                    .get(&state.user_id)
                    .cloned()
                    .or_else(|| {
                        self.stores
                            .current_user
                            .clone()
                            .filter(|user| user.id == state.user_id)
                    })
                    .unwrap_or_default();
                let avatar = self.images.get(&avatar_source(&user).0).cloned();
                let speaking = self.speaking.contains(&state.user_id);
                div()
                    .flex_none()
                    .h(px(32.))
                    .pl(px(44.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_full()
                            .when(speaking, |this| {
                                this.border_2().border_color(color("--status-positive"))
                            })
                            .children(
                                avatar.map(|picture| img(picture).size(px(24.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(user.display_name().to_owned()),
                    )
                    .into_any_element()
            })
            .collect()
    }
}
