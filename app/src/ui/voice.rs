//! Voice: joining and leaving through op 4, handing `VOICE_SERVER_UPDATE` to `dc-voice`, and the members shown under a
//! voice channel. The join flow is `selectVoiceChannel` / `voiceStateUpdate` in the bundle; flags from
//! `computeVoiceFlags` (CLIPS_ENABLED 1, ALLOW_VOICE_RECORDING 2).

use super::tooltip::{Body, Side, Tip};
use super::voice_panel::{Popout, Tab};
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
    /// Who the media connection says is in the call (`ClientConnected` / `ClientDisconnected`), for the
    /// `rtcConnectionJoinSounds` experiment.
    rtc_users: Vec<u64>,
    tracker: super::voice_sounds::RtcTracker,
    /// `RTCConnection`'s measurements, for the panel.
    pub stats: super::voice_panel::CallStats,
}

impl Call {
    /// `getUserIds()` is more than this client (module 83942 `c`).
    pub fn has_others(&self, me: Option<Id>) -> bool {
        self.rtc_users.iter().any(|user| Some(*user) != me)
    }
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
            stats: Default::default(),
            guild,
            channel,
            controls,
            started: false,
            connected: false,
            rtc_users: Vec::new(),
            tracker: Default::default(),
        });
        self.stores.voice_server = None;
        if let Some(sound) = super::voice_sounds::connect_start_sound(
            self.rtc_join_sounds(),
            self.room(guild, channel),
        ) {
            crate::sounds::play_event(sound);
        }
        self.send_voice_state(guild, Some(channel));
        self.sync_ptt_hook();
        cx.notify();
    }

    pub(super) fn leave_voice(&mut self, cx: &mut Context<Self>) {
        if let Some(call) = self.call.take() {
            if call.connected
                && let Some(sound) =
                    super::voice_sounds::disconnect_sound(self.room(call.guild, call.channel))
            {
                crate::sounds::play_event(sound);
            }
            call.controls.disconnect();
            self.send_voice_state(call.guild, None);
            self.voice_call_ended();
        }
        self.sync_ptt_hook();
        cx.notify();
    }

    pub(super) fn toggle_self_mute(&mut self, cx: &mut Context<Self>) {
        let before = (self.self_mute || self.self_deaf, self.self_deaf);
        self.self_mute = !self.self_mute;
        // Unmuting while deafened undeafens too, as the official toggle does.
        if !self.self_mute {
            self.self_deaf = false;
        }
        self.after_voice_toggle(before, cx);
    }

    pub(super) fn toggle_self_deaf(&mut self, cx: &mut Context<Self>) {
        let before = (self.self_mute || self.self_deaf, self.self_deaf);
        self.self_deaf = !self.self_deaf;
        self.after_voice_toggle(before, cx);
    }

    /// `before`/after: (effectively muted, deafened). Deafen and undeafen always sound; mute and unmute only inside a
    /// call (`inVoiceChannel` in `Sa`).
    fn after_voice_toggle(&mut self, before: (bool, bool), cx: &mut Context<Self>) {
        if let Some(sound) = super::voice_sounds::toggle_sound(
            before,
            (self.self_mute || self.self_deaf, self.self_deaf),
        ) && (self.call.is_some() || sound.ends_with("deafen"))
        {
            crate::sounds::play_event(sound);
        }
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
                if let Some(call) = &self.call
                    && let Some(sound) = super::voice_sounds::connected_sound(
                        self.rtc_join_sounds(),
                        self.room(call.guild, call.channel),
                    )
                {
                    crate::sounds::play_event(sound);
                }
                self.voice_connection_started(cx);
                self.rtc_step();
                self.sync_ptt_hook();
            }
            dc_voice::Event::SpeakingChanged { user_id, speaking } => {
                if speaking {
                    self.speaking.insert(user_id);
                } else {
                    self.speaking.remove(&user_id);
                }
            }
            dc_voice::Event::Ping { ms } => {
                if let Some(call) = self.call.as_mut() {
                    call.stats.ping(ms);
                }
            }
            dc_voice::Event::OutboundLoss { percent } => {
                if let Some(call) = self.call.as_mut() {
                    call.stats.set_loss(percent);
                }
            }
            dc_voice::Event::PrivacyCode(code) => {
                if let Some(call) = self.call.as_mut() {
                    call.stats.set_privacy_code(code);
                }
            }
            dc_voice::Event::ClientConnected { user_id } => {
                if let Some(call) = self.call.as_mut()
                    && !call.rtc_users.contains(&user_id)
                {
                    call.rtc_users.push(user_id);
                }
                self.rtc_step();
            }
            dc_voice::Event::ClientDisconnected { user_id } => {
                if let Some(call) = self.call.as_mut() {
                    call.rtc_users.retain(|user| *user != user_id);
                }
                self.rtc_step();
            }
            dc_voice::Event::Disconnected { reason } => {
                info!("voice disconnected: {reason:?}");
                self.speaking.clear();
                let dropped = self
                    .call
                    .as_ref()
                    .filter(|call| call.connected)
                    .map(|call| (call.guild, call.channel));
                if let Some(call) = self.call.as_mut() {
                    call.connected = false;
                    call.rtc_users.clear();
                }
                if let Some((guild, channel)) = dropped
                    && let Some(sound) =
                        super::voice_sounds::disconnect_sound(self.room(guild, channel))
                {
                    crate::sounds::play_event(sound);
                }
                self.voice_call_ended();
                self.rtc_step();
                self.sync_ptt_hook();
            }
            _ => {}
        }
        cx.notify();
    }

    /// `rtcConnectionJoinSounds` (experiment `2026-09-voice-connect-feedback`, variations 1 and 2).
    fn rtc_join_sounds(&self) -> bool {
        super::voice_sounds::rtc_join_sounds(&self.stores.experiments)
    }

    /// The channel as join and leave sounds see it: its kind, and whether it is the server's AFK channel.
    fn room(&self, guild: Id, channel: Id) -> super::voice_sounds::Room {
        let kind = self
            .stores
            .guilds
            .iter()
            .filter(|candidate| candidate.id == guild)
            .flat_map(|guild| guild.channels.iter())
            .find(|candidate| candidate.id == channel)
            .map_or(2, |candidate| match candidate.kind {
                dc_state::ChannelKind::Text => 0,
                dc_state::ChannelKind::Voice => 2,
                dc_state::ChannelKind::Category => 4,
                dc_state::ChannelKind::Other(kind) => kind,
            });
        super::voice_sounds::Room {
            kind,
            is_afk: self.stores.afk_channels.get(&guild) == Some(&channel),
        }
    }

    /// Who else the voice states place in the channel of the call; none outside a call.
    pub(super) fn others_in_call(&self) -> Option<Vec<Id>> {
        let call = self.call.as_ref()?;
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        Some(
            self.stores
                .voice_members(call.guild, call.channel)
                .into_iter()
                .map(|state| state.user_id)
                .filter(|user| Some(*user) != me)
                .collect(),
        )
    }

    /// After a `VOICE_STATE_UPDATE` reached the stores: the server moved us (`Su`), or someone joined or left the
    /// channel (`SE` without the rtc experiment). `before` is who was in the channel before the update.
    pub(super) fn after_voice_state(&mut self, before: Option<Vec<Id>>, data: &serde_json::Value) {
        let parse =
            |value: &serde_json::Value| value.as_str().and_then(|text| text.parse::<Id>().ok());
        let (Some(me), Some(call)) = (
            self.stores.current_user.as_ref().map(|user| user.id),
            self.call.as_ref(),
        ) else {
            return;
        };
        let (guild, channel) = (call.guild, call.channel);
        let user = parse(&data["user_id"]);
        if user == Some(me) {
            let to = parse(&data["channel_id"]);
            if parse(&data["guild_id"]) == Some(guild)
                && let Some(to) = to.filter(|_| super::voice_sounds::was_moved(channel, to))
            {
                if let Some(call) = self.call.as_mut() {
                    call.channel = to;
                }
                crate::sounds::play_event("user_moved");
            }
            return;
        }
        if self.rtc_join_sounds() {
            return;
        }
        let (Some(before), Some(after)) = (before, self.others_in_call()) else {
            return;
        };
        if let Some(sound) =
            super::voice_sounds::count_sound(self.room(guild, channel), before.len(), after.len())
        {
            crate::sounds::play_event(sound);
        }
    }

    /// `SE` with the experiment: the media connection's own list of who is in the call.
    fn rtc_step(&mut self) {
        if !self.rtc_join_sounds() {
            return;
        }
        let Some((guild, channel)) = self.call.as_ref().map(|call| (call.guild, call.channel))
        else {
            return;
        };
        let room = self.room(guild, channel);
        let in_channel = self.others_in_call().unwrap_or_default();
        if let Some(call) = self.call.as_mut() {
            let users = call.rtc_users.clone();
            if let Some(sound) = call.tracker.step(room, call.connected, &users, &in_channel) {
                crate::sounds::play_event(sound);
            }
        }
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
        let quality = call.stats.quality(call.connected);
        let last_ping = call.stats.last();
        let (guild_id, channel_id) = (call.guild, call.channel);
        let noise_on = self.noise_cancellation();
        let noise_open = self.panel.popout == Some(Popout::Noise);
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
                .child(super::icon(name, 20., "--text-subtle"))
        };
        let wide = |id: &'static str, name: &'static str| {
            div()
                .id(id)
                .w_full()
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
        let ping = div()
            .id("voice-ping")
            .size(px(32.))
            .rounded(px(8.))
            .bg(color(quality.background()))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_panel_popout(Popout::Connection(Tab::Connection), cx);
            }))
            .child(super::icon("voice_ping", 18., quality.text()));
        let ping: AnyElement = match last_ping {
            Some(ms) if !matches!(self.panel.popout, Some(Popout::Connection(_))) => self
                .with_tip(
                    "voice-ping-tip",
                    Tip::body(Body::Text(format!("{ms:.0} ms").into()), Side::Top),
                    ping,
                    cx,
                )
                .into_any_element(),
            _ => ping.into_any_element(),
        };
        let noise = round(
            "voice-krisp",
            if noise_on {
                "voice_krisp"
            } else {
                "voice_krisp_off"
            },
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_panel_popout(Popout::Noise, cx)));
        let noise: AnyElement = if noise_open {
            noise.into_any_element()
        } else {
            self.with_tip(
                "voice-krisp-tip",
                Tip::text("Noise Suppression powered by DeepFilterNet", Side::Top),
                noise,
                cx,
            )
            .into_any_element()
        };
        let disconnect = self.with_tip(
            "voice-disconnect-tip",
            Tip::text("Disconnect", Side::Top),
            round("voice-disconnect", "voice_disconnect")
                .on_click(cx.listener(|this, _, _, cx| this.leave_voice(cx))),
            cx,
        );
        let camera = self.with_tip(
            "voice-camera-tip",
            Tip::text("Turn On Camera", Side::Top),
            wide("voice-camera", "voice_camera"),
            cx,
        );
        let screen = self.with_tip(
            "voice-screen-tip",
            Tip::text("Share Your Screen", Side::Top),
            wide("voice-screen", "voice_screen"),
            cx,
        );
        let activity = self.with_tip(
            "voice-activity-tip",
            Tip::text("Start An Activity", Side::Top),
            wide("voice-activity", "voice_activity"),
            cx,
        );
        let soundboard = self.with_tip(
            "voice-soundboard-tip",
            Tip::text("Open Soundboard", Side::Top),
            wide("voice-soundboard", "voice_soundboard"),
            cx,
        );
        Some(
            div()
                // `wrapper_9U6LUN`: a pixel into the panel's border on the left.
                .ml(px(-1.))
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
                        .child(ping)
                        .child(
                            div()
                                .ml(px(8.))
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                // `hoverRoll_Dq3ch1`: over the words, the state gives way to Voice Details.
                                .child(
                                    div()
                                        .id("voice-status")
                                        .group("voice-status")
                                        .relative()
                                        .h(px(20.))
                                        .cursor_pointer()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color(status_color))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_panel_popout(
                                                Popout::Connection(Tab::Connection),
                                                cx,
                                            );
                                        }))
                                        .child(
                                            div()
                                                .group_hover("voice-status", |style| {
                                                    style.opacity(0.)
                                                })
                                                .child(status),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .opacity(0.)
                                                .group_hover("voice-status", |style| {
                                                    style.opacity(1.)
                                                })
                                                .underline()
                                                .child("Voice Details"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("voice-panel-channel")
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-subtle"))
                                        .hover(|style| style.underline())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.panel_channel_clicked(guild_id, channel_id, cx);
                                        }))
                                        .child(format!("{channel} / {}", guild.name)),
                                ),
                        )
                        .child(
                            div()
                                .ml(px(10.))
                                .flex()
                                .gap(px(4.))
                                .child(noise)
                                .child(disconnect),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(div().flex_1().child(camera))
                        .child(div().flex_1().child(screen))
                        .child(div().flex_1().child(activity))
                        .child(div().flex_1().child(soundboard)),
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
