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
    pub(super) controls: dc_voice::Controls,
    /// The connection was handed to `dc-voice`.
    started: bool,
    pub connected: bool,
    /// When the media connection came up: the call timer in the sidebar counts from here.
    pub connected_at: Option<std::time::Instant>,
    /// Who the media connection says is in the call (`ClientConnected` / `ClientDisconnected`), for the
    /// `rtcConnectionJoinSounds` experiment.
    rtc_users: Vec<u64>,
    tracker: super::voice_sounds::RtcTracker,
    /// `RTCConnection`'s measurements, for the panel.
    pub stats: super::voice_panel::CallStats,
    /// Our camera is on (`Event::VideoState`), reported as `self_video` in op 4.
    pub camera: bool,
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
        .h(px(16.))
        .pl(px(28.))
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

/// The colour of a stream source without an icon (module 118277): one of six, chosen by the name's hash.
fn source_color(name: &str) -> u32 {
    const COLORS: [u32; 6] = [1752220, 3066993, 3447003, 0x9b59b6, 0xe91e63, 0xf1c40f];
    let mut hash: i32 = 5381;
    for unit in name.encode_utf16() {
        hash = hash.wrapping_mul(33) ^ i32::from(unit);
    }
    COLORS[(hash as u32 % 6) as usize]
}

/// The first character of the source's name in capitals (`text-transform: uppercase`), or a question mark.
fn source_letter(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned())
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

    pub(super) fn send_voice_state(&self, guild: Id, channel: Option<Id>) {
        let _ = self.requests.send(Request::VoiceState {
            guild,
            channel,
            self_mute: self.self_mute,
            self_deaf: self.self_deaf,
            self_video: self.call.as_ref().is_some_and(|call| call.camera),
            flags: self.voice_flags(),
        });
    }

    pub(super) fn join_voice(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            if call.channel == channel {
                // The row of the call this client is in opens its view.
                self.open_voice_view(guild, channel, cx);
                return;
            }
            call.controls.disconnect();
        }
        self.soundboard_stop();
        info!("joining voice");
        let controls = dc_voice::Controls::new();
        controls.set_self_mute(self.self_mute);
        controls.set_self_deaf(self.self_deaf);
        self.call = Some(Call {
            stats: Default::default(),
            camera: false,
            guild,
            channel,
            controls,
            started: false,
            connected: false,
            connected_at: None,
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
        self.video_call_started(cx);
        self.sync_ptt_hook();
        self.vv.invite_dismissed = false;
        self.open_voice_view(guild, channel, cx);
        cx.notify();
    }

    pub(super) fn leave_voice(&mut self, cx: &mut Context<Self>) {
        self.soundboard_stop();
        self.stream_stop(cx);
        self.leave_activity_on_call_end(cx);
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
        // Deafened, nothing the soundboard plays is heard.
        if self.self_deaf {
            self.soundboard_stop();
        }
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
                    call.connected_at = Some(std::time::Instant::now());
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
                self.soundboard_stop();
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
            other => {
                self.on_video_event(&other);
            }
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

    /// Who in the channel of the call goes live (`self_stream`), us included; none outside a call.
    pub(super) fn streamers_in_call(&self) -> Option<Vec<Id>> {
        let call = self.call.as_ref()?;
        Some(
            self.stores
                .voice_members(call.guild, call.channel)
                .into_iter()
                .filter(|state| state.self_stream)
                .map(|state| state.user_id)
                .collect(),
        )
    }

    /// After a `VOICE_STATE_UPDATE` reached the stores: the server moved us (`Su`), or someone joined or left the
    /// channel (`SE` without the rtc experiment). `before` is who was in the channel before the update.
    pub(super) fn after_voice_state(
        &mut self,
        before: Option<Vec<Id>>,
        streams_before: Option<Vec<Id>>,
        data: &serde_json::Value,
    ) {
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
        // `stream_started` and `stream_ended` come first in `SE`, before the join and leave sounds.
        if let (Some(before), Some(after)) = (streams_before, self.streamers_in_call()) {
            let held: Vec<Id> = self.video.owners();
            if let Some(sound) = super::voice_sounds::stream_sound(
                self.room(guild, channel),
                me,
                &before,
                &after,
                &held,
            ) {
                crate::sounds::play_event(sound);
                return;
            }
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
        let camera_on = call.camera;
        let own_stream = self.own_stream();
        let (status, status_color) = if call.connected {
            (
                if camera_on {
                    "Video Connected"
                } else {
                    "Voice Connected"
                },
                "--text-feedback-positive",
            )
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
        let wide_active = |id: &'static str, name: &'static str| {
            div()
                .id(id)
                .w_full()
                .h(px(32.))
                .rounded(px(8.))
                .border_1()
                .border_color(gpui::rgba(0x00854520))
                .bg(gpui::rgba(0x00854520))
                .flex()
                .items_center()
                .justify_center()
                .child(super::icon(name, 20., "--text-feedback-positive"))
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
            Tip::text(
                if camera_on {
                    "Turn Off Camera"
                } else {
                    "Turn On Camera"
                },
                Side::Top,
            ),
            if camera_on {
                wide_active("voice-camera", "voice_camera_on")
            } else {
                wide("voice-camera", "voice_camera")
            }
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
            cx,
        );
        let screen = self.with_tip(
            "voice-screen-tip",
            Tip::text("Share Your Screen", Side::Top),
            if own_stream.is_some() {
                wide_active("voice-screen", "voice_screen_active")
            } else {
                wide("voice-screen", "voice_screen")
            }
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.voice_screen_clicked(cx))),
            cx,
        );
        let activity = self.with_tip(
            "voice-activity-tip",
            Tip::text("Start An Activity", Side::Top),
            if self.activities.running.is_some() {
                // `greyButtonActive`: green while this session is in an activity.
                wide_active("voice-activity", "voice_activity")
            } else {
                wide("voice-activity", "voice_activity")
            }
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| this.voice_activity_clicked(window, cx))),
            cx,
        );
        // `greyButtonActive` while the picker is open, `disabled` (opacity .3) under a server mute, suppression or deafening.
        let soundboard_reason = self.soundboard_disabled();
        let soundboard_open = self.panel.popout == Some(Popout::Soundboard);
        let soundboard = wide("voice-soundboard", "voice_soundboard")
            .when(soundboard_open, |this| {
                this.bg(color("--background-mod-strong"))
            })
            .when(soundboard_reason.is_some(), |this| this.opacity(0.3))
            .when(soundboard_reason.is_none(), |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_soundboard(window, cx);
                    }))
            });
        let soundboard: AnyElement = if soundboard_open {
            soundboard.into_any_element()
        } else {
            self.with_tip(
                "voice-soundboard-tip",
                Tip::text(soundboard_reason.unwrap_or("Open Soundboard"), Side::Top),
                soundboard,
                cx,
            )
            .into_any_element()
        };
        let stream_row = own_stream.as_ref().map(|info| {
            let name = info.name.clone().unwrap_or_default();
            let stop = self.with_tip(
                "voice-stream-stop-tip",
                Tip::text("Stop Streaming", Side::Top),
                div()
                    .id("voice-stream-stop")
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .on_click(cx.listener(|this, _, _, cx| this.stream_stop(cx)))
                    .child(super::icon(
                        "voice_stream_stop",
                        20.,
                        "--interactive-text-default",
                    )),
                cx,
            );
            div()
                .ml(px(-1.))
                .h(px(57.))
                .px(px(12.))
                .py(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .bg(color("--background-surface-high"))
                .border_b_1()
                .border_color(color("--background-mod-subtle"))
                .child(
                    div()
                        .relative()
                        .size(px(32.))
                        .flex_none()
                        // `icon_Yjv1d0`: the source's icon, or its first letter on a colour taken from the name.
                        .child(match info.icon.clone() {
                            Some(image) => {
                                img(image).size(px(32.)).rounded(px(4.)).into_any_element()
                            }
                            None => div()
                                .size(px(32.))
                                .rounded(px(4.))
                                .flex()
                                .items_start()
                                .justify_center()
                                .bg(gpui::rgb(source_color(&name)))
                                .text_size(px(16.))
                                .line_height(px(34.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(color("--white"))
                                .child(source_letter(&name))
                                .into_any_element(),
                        })
                        // `svg-mask-status-screenshare`: the badge's corner is cut out of the icon.
                        .child(
                            div()
                                .absolute()
                                .left(px(16.))
                                .top(px(19.))
                                .w(px(16.))
                                .h(px(13.))
                                .rounded_tl(px(4.))
                                .bg(color("--background-surface-high")),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(19.))
                                .top(px(20.))
                                .child(super::icon("voice_member_camera", 14., "--text-strong")),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(16.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(name),
                )
                .child(stop)
        });
        Some(
            div()
                .flex()
                .flex_col()
                .children(stream_row)
                .child(
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
                                                        .child(if camera_on {
                                                            "Video Details"
                                                        } else {
                                                            "Voice Details"
                                                        }),
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
                                                    this.panel_channel_clicked(
                                                        guild_id, channel_id, cx,
                                                    );
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
                        ),
                )
                .into_any_element(),
        )
    }

    /// Under a voice channel's members, the activities running in it (`EMBEDDED_ACTIVITY_UPDATE_V2`): the app's icon
    /// and name, and Join, or Leave when this session is in it. The official row has no capture, so this one keeps to
    /// the member rows' 32px and type (DOM.md has none for activities).
    pub(super) fn voice_activity_rows(
        &self,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let in_call = self
            .call
            .as_ref()
            .is_some_and(|call| call.channel == channel);
        self.activities
            .in_channel(channel)
            .into_iter()
            .enumerate()
            .map(|(index, instance)| {
                let app = instance.application_id.clone();
                let mine = self
                    .activities
                    .running
                    .as_ref()
                    .is_some_and(|running| running.launch_id == instance.launch_id);
                let name = self
                    .activities
                    .names
                    .get(&app)
                    .cloned()
                    .unwrap_or_else(|| "Activity".to_owned());
                let picture = self.images.get(&format!("shelf-icon:{app}")).cloned();
                let people = instance.participants.len();
                let label = if mine { "Leave" } else { "Join" };
                let tone = if mine {
                    "--control-critical-primary-background-default"
                } else {
                    "--control-primary-background-default"
                };
                div()
                    .flex_none()
                    .h(px(32.))
                    .pl(px(44.))
                    .pr(px(17.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded(px(6.))
                            .overflow_hidden()
                            .children(picture.map(|picture| img(picture).size(px(24.)))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--channels-default"))
                            .child(format!("{name} ({people})")),
                    )
                    .when(mine || in_call, |this| {
                        this.child(
                            div()
                                .id(gpui::SharedString::from(format!("activity-row-{index}")))
                                .h(px(20.))
                                .px(px(8.))
                                .rounded(px(4.))
                                .bg(color(tone))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(gpui::rgba(0xffffffff))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if mine {
                                        this.leave_activity(cx);
                                    } else {
                                        this.launch_activity(&app, cx);
                                    }
                                })),
                        )
                    })
                    .into_any_element()
            })
            .collect()
    }

    /// Members in a voice channel, under its row: 24px avatar and name. A green ring marks who speaks.
    pub(super) fn voice_members(
        &self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
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
                let (muted, deaf) = (state.self_mute || state.mute, state.self_deaf || state.deaf);
                let speaking = self.speaking.contains(&state.user_id)
                    || self.soundboard.user_playing(state.user_id);
                let member = state.user_id;
                div()
                    .id(gpui::SharedString::from(format!("voice-member-{member}")))
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.open_context_menu(
                                super::context_menu::Target::User {
                                    user: member,
                                    guild: Some(guild),
                                },
                                event.position,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .flex_none()
                    .h(px(32.))
                    .pl(px(44.))
                    .pr(px(17.))
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
                            .text_color(color(if speaking {
                                "--interactive-text-active"
                            } else {
                                "--channels-default"
                            }))
                            .child(user.display_name().to_owned()),
                    )
                    // `icons_YUoOxm`: 16px, the muted microphone or the deafened headphones.
                    .when(deaf, |this| {
                        this.child(super::icon(
                            "headphones_deafened",
                            16.,
                            "--channels-default",
                        ))
                    })
                    .when(muted && !deaf, |this| {
                        this.child(super::icon("voice_member_muted", 16., "--channels-default"))
                    })
                    // `icons_YUoOxm`: the camera, then the red LIVE badge of someone who goes live.
                    .when(state.self_video, |this| {
                        this.child(super::icon(
                            "voice_member_camera",
                            16.,
                            "--channels-default",
                        ))
                    })
                    .when(state.self_stream, |this| {
                        this.child(
                            div()
                                .h(px(16.))
                                .px(px(8.))
                                .rounded_full()
                                .bg(color("--badge-critical-background"))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(600.))
                                .text_color(gpui::rgba(0xffffffff))
                                .child("LIVE"),
                        )
                    })
                    .into_any_element()
            })
            .collect()
    }
}
