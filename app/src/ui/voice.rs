//! Voice: joining and leaving through op 4, handing `VOICE_SERVER_UPDATE` to `dc-voice`, and the members shown under a
//! voice channel. The join flow is `selectVoiceChannel` / `voiceStateUpdate` in the bundle; flags from
//! `computeVoiceFlags` (CLIPS_ENABLED 1, ALLOW_VOICE_RECORDING 2).

use super::lottie_icon::{self, Anim};
use super::tooltip::{Body, Side, Tip};
use super::voice_panel::{Popout, Tab};
use super::voice_user_popout::VoiceUserTarget;
use super::voice_view::lottie_hover;
use super::{Root, avatar_source, color};
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use tracing::info;

/// `ALLOW_VOICE_RECORDING`: the Data & Privacy switch "Allow my voice to be recorded in Clips".
const ALLOW_VOICE_RECORDING: u64 = 2;

pub(crate) struct Call {
    /// The number of the voice connection this call opened last; 0 before the first.
    pub(super) generation: u64,
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
    /// Negotiated DAVE protocol version (`0` when unencrypted / Stage transport-only; `1` once DAVE is active).
    pub dave_version: u16,
    /// The flags each participant's client announced (`RTC_CONNECTION_FLAGS`).
    pub flags: std::collections::HashMap<u64, u64>,
    /// Unexpected `DisconnectReason::Failed` auto-reconnect attempts on this call.
    failed_reconnects: u8,
    /// Someone else was ever connected to the call (`getWasEverMultiParticipant` of module 495867), which the voice
    /// feedback survey asks for (module 409181).
    pub(super) ever_multi: bool,
    /// `hasUsedBackgroundInCall` of VideoBackgroundStore (module 912630): our camera was on with a video background
    /// in this channel, so leaving asks for the video background survey (module 932140) instead of the call's.
    pub(super) used_background: bool,
}

impl Call {
    /// `getUserIds()` is more than this client (module 83942 `c`).
    pub fn has_others(&self, me: Option<Id>) -> bool {
        self.rtc_users.iter().any(|user| Some(*user) != me)
    }

    /// Connected media participants currently reported on the call.
    pub(super) fn rtc_users(&self) -> &[u64] {
        &self.rtc_users
    }

    /// Replaces the media connection's peer list for capture fixtures.
    #[cfg(feature = "capture")]
    pub(super) fn set_rtc_users(&mut self, users: Vec<u64>) {
        self.rtc_users = users;
    }
}

/// Whether a voice state of this account with the session `theirs` takes the call `(guild, channel)` of the session
/// `own` away: another session, in the same guild (a server) or the same channel (a call).
fn taken_elsewhere(own: &str, theirs: &str, ours: (Id, Id), state: (Id, Option<Id>)) -> bool {
    // A state without a channel (that session left) names no guild in the official check either.
    own != theirs
        && state
            .1
            .is_some_and(|channel| (ours.0 != 0 && state.0 == ours.0) || channel == ours.1)
}

/// After close code 4014 with our voice state still in the channel (a moderator move), how long the call waits for the
/// `VOICE_SERVER_UPDATE` that reconnects it. A value of ours: the pinned bundle shows no such wait.
const MOVE_SERVER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// What a call does when [`MOVE_SERVER_WAIT`] ran out after a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterMoveWait {
    /// The call reconnected, ended or was replaced meanwhile: nothing to do.
    Nothing,
    /// The voice server stored for the call is used again.
    Restart,
    /// No voice server for the call is known: it ends.
    End,
}

/// `call`: (started, connected, generation) of the call now; `generation` is the one the move closed; `server` whether
/// a voice server for the call is stored.
fn after_move_wait(
    call: Option<(bool, bool, u64)>,
    generation: u64,
    server: bool,
) -> AfterMoveWait {
    match call {
        Some((true, false, now)) if now == generation => {
            if server {
                AfterMoveWait::Restart
            } else {
                AfterMoveWait::End
            }
        }
        _ => AfterMoveWait::Nothing,
    }
}

/// The first character of the source's name in capitals (`text-transform: uppercase`), or a question mark.
fn source_letter(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned())
}

/// Maximum automatic reconnects after an unexpected [`dc_voice::DisconnectReason::Failed`].
const MAX_FAILED_RECONNECTS: u8 = 3;

/// What a call does after an unexpected [`dc_voice::DisconnectReason::Failed`] backoff fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFailedWait {
    /// The call reconnected, ended, or moved to another connection meanwhile.
    Nothing,
    /// Restart using the stored voice server if `server` is true, else request a fresh voice state/server.
    Restart { tell: bool },
    /// Retry budget is exhausted: tear down the dead call cleanly.
    End,
}

fn after_failed_wait(
    call: Option<(bool, bool, u64, u8)>,
    generation: u64,
    server: bool,
) -> AfterFailedWait {
    match call {
        Some((true, false, now, attempts)) if now == generation => {
            if attempts < MAX_FAILED_RECONNECTS {
                AfterFailedWait::Restart { tell: !server }
            } else {
                AfterFailedWait::End
            }
        }
        _ => AfterFailedWait::Nothing,
    }
}

/// The connection the wait after a move started over ([`MOVE_SERVER_WAIT`]); 0 for none.
static MOVE_RETRY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Numbers the voice connections this process opens.
static VOICE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
            // `isSelfMute()`, which holds while Use Voice Activity is missing (module 754712 `tf`).
            self_mute: self.self_mute || !self.vad.has_permission,
            self_deaf: self.self_deaf,
            self_video: self.call.as_ref().is_some_and(|call| call.camera),
            flags: self.voice_flags(),
        });
    }

    pub(super) fn join_voice(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        // A Stage with blocked or ignored people, or full of video seats, asks first (module 129095, 597798).
        if self
            .call
            .as_ref()
            .is_none_or(|call| call.channel != channel)
            && let Some(alert) = self.stage_join_alert(guild, channel)
        {
            self.open_voice_alert(alert, cx);
            return;
        }
        // Any other full channel: "Channel is full" (module 361698, from the same select of module 597798).
        if self
            .call
            .as_ref()
            .is_none_or(|call| call.channel != channel)
            && self.voice_channel_is_full(guild, channel)
        {
            self.open_voice_alert(super::voice_alerts::VoiceAlert::ChannelFull, cx);
            return;
        }
        self.join_voice_now(guild, channel, cx);
    }

    /// Joins without the Stage questions: what "Got it, thanks" of the blocked warning does.
    pub(super) fn join_voice_now(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        self.join_voice_shown(guild, channel, true, cx);
    }

    /// `selectVoiceChannel` without opening the channel's view: the idle move to the AFK channel (module 596339).
    pub(super) fn move_voice_quietly(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        self.join_voice_shown(guild, channel, false, cx);
    }

    fn join_voice_shown(&mut self, guild: Id, channel: Id, show: bool, cx: &mut Context<Self>) {
        if let Some(call) = &self.call {
            if call.channel == channel {
                if !show {
                    return;
                }
                // The row of the call this client is in opens its view.
                self.show_call_channel(guild, channel, cx);
                return;
            }
            call.controls.disconnect();
            // Nothing of the call that is left goes on in the next one: the streams watched and sent there end.
            self.streams_stop_all(cx);
        }
        self.soundboard_stop();
        self.soundboard_echo.set_channel(Some(channel));
        // A new connection of this session (`S = null`): the notice of a call taken elsewhere goes.
        self.vv.notices.clear();
        info!("joining voice");
        crate::sounds::prefetch_call_sounds();
        self.open_call(guild, channel);
        super::voice_panel::rtc_debug::on_voice_channel_select(Some(channel), cx);
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
        if show {
            self.show_call_channel(guild, channel, cx);
        }
        self.sync_dm_call_idle_timer(cx);
        cx.notify();
    }

    /// The call of this client, not yet connected: it waits for the voice server and our own voice state.
    fn open_call(&mut self, guild: Id, channel: Id) {
        self.open_call_with(guild, channel, dc_voice::Controls::new());
    }

    /// [`Self::open_call`] with these controls: a restart passes the old call's `rejoin()`, which keeps the media
    /// hub and playout that running Go Live streams were given (a fresh one left their sound on a playout no longer
    /// played).
    fn open_call_with(&mut self, guild: Id, channel: Id, controls: dc_voice::Controls) {
        controls.set_self_mute(self.engine_mute());
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
            flags: std::collections::HashMap::new(),
            dave_version: 0,
            tracker: Default::default(),
            generation: 0,
            failed_reconnects: 0,
            ever_multi: false,
            used_background: false,
        });
        self.stores.voice_server = None;
    }

    /// The voice connection of the call starts over, in the same channel: the service named another voice server
    /// (a region change, a failover), or the gateway session is a new one and the voice state has to be sent again
    /// (`tell`). Active Go Live streams remain running, and the camera is off until it is turned on again.
    fn restart_call(&mut self, tell: bool, cx: &mut Context<Self>) {
        let Some(call) = self.call.take() else {
            return;
        };
        info!("voice: connection starts over");
        let controls = call.controls.rejoin();
        call.controls.disconnect();
        self.speaking.clear();
        let server = self.stores.voice_server.take();
        self.open_call_with(call.guild, call.channel, controls);
        if !tell {
            self.stores.voice_server = server;
        }
        // The new connection sends no camera: the service hears it, whoever asked for the restart.
        if tell || call.camera {
            self.send_voice_state(call.guild, Some(call.channel));
        }
        self.video_call_started(cx);
        self.sync_ptt_hook();
        cx.notify();
    }

    /// After a dispatch reached the stores: what it means for the voice connection of the call here.
    pub(super) fn voice_after_dispatch(&mut self, name: &str, cx: &mut Context<Self>) {
        crate::sounds::set_current_user(self.stores.current_user.as_ref().map(|user| user.id));
        if name == "READY" {
            crate::sounds::prefetch_call_sounds();
            self.vv.notices.clear();
        }
        let Some(call) = &self.call else {
            return;
        };
        match name {
            // A session that could not be resumed: the service has dropped its voice state, and a call in a
            // direct message is asked for again, whichever channel is open.
            "READY" => {
                let (guild, channel) = (call.guild, call.channel);
                self.restart_call(true, cx);
                if guild == 0 {
                    let private = self
                        .stores
                        .private_channels
                        .iter()
                        .any(|dm| dm.id == channel);
                    self.stores.calls.connect_channel(channel, private, true);
                    self.calls_after_dispatch_effects();
                }
            }
            "VOICE_SERVER_UPDATE"
                if call.started
                    && self.stores.voice_server.as_ref().is_some_and(|server| {
                        server.guild_id == call.guild
                            && (call.guild != 0 || server.channel_id == Some(call.channel))
                    }) =>
            {
                self.restart_call(false, cx);
            }
            "CALL_DELETE" if call.guild == 0 => {
                // `dc-state` already deleted the DM call record and channel voice states for the deleted channel.
                // Clean up only if this active DM call's channel no longer has a call or our voice state.
                let channel = call.channel;
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let has_own_state = me.is_some_and(|own| {
                    self.stores
                        .voice_members(0, channel)
                        .iter()
                        .any(|state| state.user_id == own)
                });
                if self.stores.calls.get_call(channel).is_none() && !has_own_state {
                    self.end_voice(false, cx);
                }
            }
            _ => {}
        }
        self.sync_dm_call_idle_timer(cx);
    }

    #[cfg(feature = "capture")]
    fn fake_call_rtc_users(&self, guild: Id, channel: Id) -> Vec<u64> {
        let mut rtc_users = Vec::new();
        for state in self.stores.voice_members(guild, channel) {
            if !rtc_users.contains(&state.user_id) {
                rtc_users.push(state.user_id);
            }
        }
        if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id)
            && !rtc_users.contains(&me)
        {
            rtc_users.push(me);
        }
        rtc_users
    }

    /// Harness only: this account "in" the call of a DM without any voice state sent anywhere or any media opened,
    /// for frames of the in-call screens. Nothing leaves the process.
    #[cfg(feature = "capture")]
    pub(super) fn fake_dm_call(&mut self, channel: Id, cx: &mut Context<Self>) {
        let rtc_users = self.fake_call_rtc_users(0, channel);
        self.call = Some(Call {
            stats: Default::default(),
            camera: false,
            guild: 0,
            channel,
            controls: dc_voice::Controls::new(),
            started: true,
            connected: true,
            connected_at: Some(std::time::Instant::now()),
            rtc_users,
            flags: std::collections::HashMap::new(),
            dave_version: 1,
            tracker: Default::default(),
            generation: 0,
            failed_reconnects: 0,
            ever_multi: true,
            used_background: false,
        });
        self.start_voice_tick(cx);
        self.sync_dm_call_idle_timer(cx);
    }

    /// Harness only: this account "in" a Stage of a guild, as above: no voice state, no media, nothing leaves the
    /// process (`ch call <channel>`).
    #[cfg(feature = "capture")]
    pub(super) fn fake_guild_call(&mut self, guild: Id, channel: Id) {
        let rtc_users = self.fake_call_rtc_users(guild, channel);
        let dave_version = u16::from(!self.is_stage_voice_channel(guild, channel));
        self.call = Some(Call {
            stats: Default::default(),
            camera: false,
            guild,
            channel,
            controls: dc_voice::Controls::new(),
            started: true,
            connected: true,
            connected_at: Some(std::time::Instant::now()),
            rtc_users,
            flags: std::collections::HashMap::new(),
            dave_version,
            tracker: Default::default(),
            generation: 0,
            failed_reconnects: 0,
            ever_multi: true,
            used_background: false,
        });
    }

    /// A guild call opens the voice view; a DM or group DM call opens its channel, whose call area shows it.
    fn show_call_channel(&mut self, guild: Id, channel: Id, cx: &mut Context<Self>) {
        if guild == 0 {
            self.select_home(Some(channel));
        } else {
            self.open_voice_view(guild, channel, cx);
        }
    }

    pub(super) fn leave_voice(&mut self, cx: &mut Context<Self>) {
        self.voice_left_by_hand(cx);
        self.end_voice(true, cx);
    }

    /// The account joined voice from another device (`VOICE_STATE_UPDATES` of `SelectedChannelStore`, module 309010,
    /// and `RTCConnectionStore`, module 495867): a voice state of this user with another session, in the guild or the
    /// channel of the call here, takes the call away from this client. Nothing is sent: the service already moved it.
    pub(super) fn voice_state_of_other_session(
        &mut self,
        data: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let id =
            |value: &serde_json::Value| value.as_str().and_then(|text| text.parse::<Id>().ok());
        let Some(call) = &self.call else {
            return;
        };
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        if me.is_none() || id(&data["user_id"]) != me {
            return;
        }
        let (Some(own), Some(theirs)) = (
            self.stores.session_id.as_deref(),
            data["session_id"].as_str(),
        ) else {
            return;
        };
        // The service took this session out of voice (disconnected by a moderator, removed from the group, the
        // call ended): the call here is over. A call that has not connected yet may still see the state of the one
        // left before it, which is not about it.
        if own == theirs
            && call.started
            && id(&data["channel_id"]).is_none()
            && id(&data["guild_id"]).unwrap_or(0) == call.guild
        {
            self.end_voice(false, cx);
            return;
        }
        if taken_elsewhere(
            own,
            theirs,
            (call.guild, call.channel),
            (id(&data["guild_id"]).unwrap_or(0), id(&data["channel_id"])),
        ) {
            // Module 495867: `S = i.channelId`, which raises the VOICE_DISABLED notice (module 596339).
            let (guild, channel) = (call.guild, call.channel);
            self.vv.notices.voice_taken(guild, channel);
            self.end_voice(false, cx);
        }
    }

    /// Ends the call here; `tell` sends the voice state that leaves the channel.
    #[track_caller]
    fn end_voice(&mut self, tell: bool, cx: &mut Context<Self>) {
        // Which path ended the call: a stream that dropped with it was otherwise unexplained in the log.
        if self.call.is_some() {
            info!(at = %std::panic::Location::caller(), tell, "voice call ended");
        }
        self.soundboard_stop();
        self.soundboard_echo.set_channel(None);
        self.streams_stop_all(cx);
        self.leave_activity_on_call_end(cx);
        // Out of voice, Voice Activity is allowed again (`wA` with no channel).
        self.vad = super::vad_permission::VadState::default();
        if let Some(call) = self.call.take() {
            if call.connected
                && let Some(sound) =
                    super::voice_sounds::disconnect_sound(self.room(call.guild, call.channel))
            {
                crate::sounds::play_event(sound);
            }
            call.controls.disconnect();
            if tell {
                self.send_voice_state(call.guild, None);
            }
            self.voice_call_ended();
        }
        // The layout chosen in a call is that call's: the next one starts with its chat, its people and the grid.
        self.dmc.no_chat = false;
        self.vv.members_hidden = false;
        self.vv.hovered_controls.clear();
        self.vv.focus.tile = None;
        // The popped-out window and what was dragged belong to the call, whether or not the main window draws.
        self.call_popout_frame(cx);
        self.close_rtc_debug(cx);
        super::voice_panel::rtc_debug::clear_live_rtc_stats(cx);
        self.sync_dm_call_idle_timer(cx);
        self.sync_ptt_hook();
        cx.notify();
    }

    pub(super) fn toggle_self_mute(&mut self, cx: &mut Context<Self>) {
        if self.vad_blocks_mute_toggle(cx) {
            return;
        }
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
            call.controls.set_self_mute(self.engine_mute());
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
        let Some(server) = self.stores.voice_server.as_ref().filter(|server| {
            server.guild_id == call.guild
                && (call.guild != 0 || server.channel_id == Some(call.channel))
        }) else {
            return;
        };
        // Another device of this account may be in the same channel: only this client's own state names its session.
        let own = self.stores.session_id.as_deref();
        let Some(session) = self
            .stores
            .voice_members(call.guild, call.channel)
            .into_iter()
            .find(|state| state.user_id == me && own.is_none_or(|own| own == state.session_id))
            .map(|state| state.session_id.clone())
        else {
            return;
        };
        call.started = true;
        call.generation = VOICE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let generation = call.generation;
        let join = dc_voice::Join {
            endpoint: server.endpoint.clone(),
            token: server.token.clone(),
            session_id: session,
            // A private call names the channel as its server (`VOICE_SERVER_UPDATE` has no guild_id).
            server_id: if call.guild == 0 {
                call.channel
            } else {
                call.guild
            },
            channel_id: call.channel,
            user_id: me,
        };
        let _ = self.requests.send(Request::VoiceConnect {
            join,
            controls: call.controls.clone(),
            generation,
        });
    }

    pub(super) fn on_voice_event(
        &mut self,
        generation: u64,
        event: dc_voice::Event,
        cx: &mut Context<Self>,
    ) {
        // A connection that was replaced (another call, a new voice server) or whose call ended still reports its
        // last events: they are not about the call here.
        if self.call.as_ref().map(|call| call.generation) != Some(generation) {
            return;
        }
        let sync_idle = matches!(
            event,
            dc_voice::Event::Connected
                | dc_voice::Event::ClientConnected { .. }
                | dc_voice::Event::ClientDisconnected { .. }
                | dc_voice::Event::Disconnected { .. }
        );
        match event {
            dc_voice::Event::Connected => {
                info!("voice connected");
                let is_stage = self
                    .call
                    .as_ref()
                    .is_some_and(|call| self.is_stage_voice_channel(call.guild, call.channel));
                if let Some(call) = self.call.as_mut() {
                    call.connected = true;
                    call.connected_at = Some(std::time::Instant::now());
                    call.dave_version = u16::from(!is_stage);
                    call.failed_reconnects = 0;
                }
                // `RTC_CONNECTION_STATE` runs PermissionVADStore's check (`vad_permission.rs`).
                self.sync_vad_permission();
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
                super::voice_panel::rtc_debug::ingest_call_rtc_stats(self, cx);
                self.sync_ptt_hook();
                self.game_share_pending(cx);
            }
            dc_voice::Event::SpeakingChanged { user_id, speaking } => {
                // `SPEAKING` of the account itself is activity to `IdleStore` (module 885576).
                let mine = self.stores.current_user.as_ref().map(|me| me.id) == Some(user_id);
                if speaking && mine {
                    self.note_activity();
                }
                if mine {
                    self.spotify_speaking(speaking, cx);
                }
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
                super::voice_panel::rtc_debug::ingest_call_rtc_stats(self, cx);
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
            dc_voice::Event::Flags { user_id, flags } => {
                if let Some(call) = self.call.as_mut() {
                    call.flags.insert(user_id, flags);
                }
            }
            dc_voice::Event::ClientConnected { user_id } => {
                if let Some(call) = self.call.as_mut()
                    && !call.rtc_users.contains(&user_id)
                {
                    call.rtc_users.push(user_id);
                    call.ever_multi = true;
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
                self.soundboard_echo.set_channel(None);
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
                super::voice_panel::rtc_debug::clear_live_rtc_stats(cx);
                self.rtc_step();
                self.sync_ptt_hook();
                // Close code 4014: the service removed this session from the channel, and nothing rejoins it.
                // A move to another channel closes with the same code, and then our voice state is in that channel.
                let own = self.stores.session_id.as_deref();
                let me = self.stores.current_user.as_ref().map(|me| me.id);
                let still_in = self.call.as_ref().is_some_and(|call| {
                    self.stores
                        .voice_members(call.guild, call.channel)
                        .into_iter()
                        .any(|state| {
                            Some(state.user_id) == me
                                && own.is_none_or(|own| own == state.session_id)
                        })
                });
                // A connection that the wait below started over and that was closed the same way again is not tried a
                // third time: the stored server does not take this session.
                let retried = MOVE_RETRY.load(std::sync::atomic::Ordering::Relaxed) == generation;
                if reason == dc_voice::DisconnectReason::Kicked && (!still_in || retried) {
                    self.end_voice(false, cx);
                } else if reason == dc_voice::DisconnectReason::Kicked {
                    // Moved: the new voice server normally follows; without one the call would wait forever.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(MOVE_SERVER_WAIT).await;
                        let _ = this.update(cx, |root, cx| {
                            let call = root
                                .call
                                .as_ref()
                                .map(|call| (call.started, call.connected, call.generation));
                            let server = root.call.as_ref().is_some_and(|call| {
                                root.stores.voice_server.as_ref().is_some_and(|server| {
                                    server.guild_id == call.guild
                                        && (call.guild != 0
                                            || server.channel_id == Some(call.channel))
                                })
                            });
                            match after_move_wait(call, generation, server) {
                                AfterMoveWait::Restart => {
                                    root.restart_call(false, cx);
                                    root.maybe_connect_voice();
                                    if let Some(call) = &root.call {
                                        MOVE_RETRY.store(
                                            call.generation,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                }
                                AfterMoveWait::End => root.end_voice(false, cx),
                                AfterMoveWait::Nothing => {}
                            }
                        });
                    })
                    .detach();
                } else if matches!(reason, dc_voice::DisconnectReason::Failed(_)) {
                    let attempts = self
                        .call
                        .as_ref()
                        .map_or(MAX_FAILED_RECONNECTS, |call| call.failed_reconnects);
                    if attempts >= MAX_FAILED_RECONNECTS {
                        self.end_voice(true, cx);
                    } else {
                        let delay = std::time::Duration::from_millis(1000 << u32::from(attempts));
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(delay).await;
                            let _ = this.update(cx, |root, cx| {
                                let call = root.call.as_ref().map(|call| {
                                    (
                                        call.started,
                                        call.connected,
                                        call.generation,
                                        call.failed_reconnects,
                                    )
                                });
                                let server = root.call.as_ref().is_some_and(|call| {
                                    root.stores.voice_server.as_ref().is_some_and(|server| {
                                        server.guild_id == call.guild
                                            && (call.guild != 0
                                                || server.channel_id == Some(call.channel))
                                    })
                                });
                                match after_failed_wait(call, generation, server) {
                                    AfterFailedWait::Restart { tell } => {
                                        root.restart_call(tell, cx);
                                        if let Some(restarted) = root.call.as_mut() {
                                            restarted.failed_reconnects = attempts + 1;
                                        }
                                        root.maybe_connect_voice();
                                    }
                                    AfterFailedWait::End => root.end_voice(true, cx),
                                    AfterFailedWait::Nothing => {}
                                }
                            });
                        })
                        .detach();
                    }
                }
            }
            other => {
                self.on_video_event(&other);
            }
        }
        if sync_idle {
            self.sync_dm_call_idle_timer(cx);
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

    /// Stage channels (`GUILD_STAGE_VOICE`, channel type 13) suppress E2EE / DAVE privacy chrome (`259374`).
    pub(super) fn is_stage_voice_channel(&self, guild: Id, channel: Id) -> bool {
        guild != 0 && self.room(guild, channel).kind == 13
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
                    call.tracker = Default::default();
                }
                self.video.clear_watched_owners();
                crate::sounds::play_event("user_moved");
                return;
            }
        }
        // `stream_started` and `stream_ended` come first in `SE`, before the join and leave sounds.
        if let (Some(before), Some(after)) = (streams_before, self.streamers_in_call()) {
            let held: Vec<Id> = self.video.held_owners();
            let sound = super::voice_sounds::stream_sound(
                self.room(guild, channel),
                me,
                &before,
                &after,
                &held,
            );
            self.video.prune_watched_owners(&after);
            if let Some(sound) = sound {
                crate::sounds::play_event(sound);
                return;
            }
        }
        if user == Some(me) {
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
        // A DM call has no guild: the panel's second line is the DM's name (module 289287 `channel_9U6LUN`).
        let guild = self.stores.guild(call.guild);
        if guild.is_none() && call.guild != 0 {
            return None;
        }
        let channel = if call.guild == 0 {
            self.stores
                .private_channels
                .iter()
                .find(|known| known.id == call.channel)
                .map(|known| self.private_channel_name(known))
                .unwrap_or_default()
        } else {
            guild
                .and_then(|guild| {
                    guild
                        .channels
                        .iter()
                        .find(|channel| channel.id == call.channel)
                })
                .map(|channel| channel.name.clone())
                .unwrap_or_default()
        };
        let camera_on = call.camera;
        let stage = guild.is_some_and(|guild| {
            guild.channels.iter().any(|known| {
                known.id == call.channel && known.kind == dc_state::ChannelKind::Other(13)
            })
        });
        let own_stream = self.own_stream();
        let (status, status_color) = if call.connected {
            (
                dc_intl::tr(if camera_on {
                    "Video Connected"
                } else {
                    "Voice Connected"
                }),
                "--text-feedback-positive",
            )
        } else {
            (dc_intl::tr("Connecting"), "--text-feedback-warning")
        };
        let quality = call.stats.quality(call.connected);
        let last_ping = call.stats.last();
        let (guild_id, channel_id) = (call.guild, call.channel);
        let noise_on = self.noise_cancellation();
        let noise_open = self.panel.popout == Some(Popout::Noise);
        let round_box = |id: &'static str| {
            div()
                .id(id)
                .size(px(32.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
        };
        let round = |id: &'static str, name: &'static str| {
            round_box(id).child(super::icon(name, 20., "--text-subtle"))
        };
        // The row's icons are the animated hooks of module 596339 (`useVideoLottie`, `useScreenshareLottie`,
        // `useActivityLottie`, `useSoundboardLottie`), 20px (`buttonIcon_9U6LUN`), with the hover events on the button.
        let wide_box = |id: &'static str| {
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
        };
        let wide_active_box = |id: &'static str| {
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
        };
        let wide = |id: &'static str, lottie: &str, anim: Anim, state: Option<&'static str>| {
            wide_box(id).child(lottie_icon::icon(
                lottie,
                anim,
                state,
                20.,
                color("--control-secondary-text-default"),
            ))
        };
        let wide_active =
            |id: &'static str, lottie: &str, anim: Anim, state: Option<&'static str>| {
                wide_active_box(id).child(lottie_icon::icon(
                    lottie,
                    anim,
                    state,
                    20.,
                    color("--text-feedback-positive"),
                ))
            };
        let panel_id = |name: &str| format!("panel-{name}-{channel_id}");
        let (camera_id, screen_id, activity_id, soundboard_id, disconnect_id) = (
            panel_id("camera"),
            panel_id("screen"),
            panel_id("activity"),
            panel_id("soundboard"),
            panel_id("disconnect"),
        );
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
            .child(super::icon(quality.glyph(), 18., quality.text()));
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
            Tip::text(dc_intl::tr("Disconnect"), Side::Top),
            // `Tn` of module 596339: `useDisconnectLottie`, its events on the button.
            round_box("voice-disconnect")
                .child(lottie_icon::icon(
                    &disconnect_id,
                    Anim::Disconnect,
                    None,
                    20.,
                    color("--text-subtle"),
                ))
                .on_hover(lottie_hover(disconnect_id.clone(), cx))
                .on_click(cx.listener(|this, _, _, cx| this.disconnect_from_call(cx))),
            cx,
        );
        let camera = self.with_tip(
            "voice-camera-tip",
            Tip::text(
                dc_intl::tr(if camera_on {
                    "Turn Off Camera"
                } else {
                    "Turn On Camera"
                }),
                Side::Top,
            ),
            if camera_on {
                wide_active("voice-camera", &camera_id, Anim::Camera, Some("disable"))
            } else {
                wide("voice-camera", &camera_id, Anim::Camera, Some("enable"))
            }
            .on_hover(lottie_hover(camera_id.clone(), cx))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.voice_camera_clicked(cx))),
            cx,
        );
        let screen = self.with_tip(
            "voice-screen-tip",
            Tip::text(dc_intl::tr("Share Your Screen"), Side::Top),
            if own_stream.is_some() {
                wide_active(
                    "voice-screen",
                    &screen_id,
                    Anim::ScreenShare,
                    Some("disable"),
                )
            } else {
                wide(
                    "voice-screen",
                    &screen_id,
                    Anim::ScreenShare,
                    Some("enable"),
                )
            }
            .on_hover(lottie_hover(screen_id.clone(), cx))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| this.voice_screen_clicked(cx))),
            cx,
        );
        let activity = self.with_tip(
            "voice-activity-tip",
            Tip::text(dc_intl::tr("Start An Activity"), Side::Top),
            if self.activities.running.is_some() {
                // `greyButtonActive`: green while this session is in an activity.
                wide_active("voice-activity", &activity_id, Anim::Activity, None)
            } else {
                wide("voice-activity", &activity_id, Anim::Activity, None)
            }
            .on_hover(lottie_hover(activity_id.clone(), cx))
            .cursor_pointer()
            .on_click({
                // `onClick: () => { A(), ... }`: the `click` marker.
                let id = activity_id.clone();
                cx.listener(move |this, _, window, cx| {
                    lottie_icon::click(&id);
                    this.voice_activity_clicked(window, cx);
                })
            }),
            cx,
        );
        // `greyButtonActive` while the picker is open, `disabled` (opacity .3) under a server mute, suppression or deafening.
        let soundboard_reason = self.soundboard_disabled();
        let soundboard_open = self.panel.popout == Some(Popout::Soundboard);
        // Module 596339's panel button plays the soundboard marker on every click and both hover halves always.
        let soundboard = wide("voice-soundboard", &soundboard_id, Anim::Soundboard, None)
            .on_hover(lottie_hover(soundboard_id.clone(), cx))
            .when(soundboard_open, |this| {
                this.bg(color("--background-mod-strong"))
            })
            .when(soundboard_reason.is_some(), |this| this.opacity(0.3))
            .when(soundboard_reason.is_none(), |this| {
                let id = soundboard_id.clone();
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        lottie_icon::click(&id);
                        this.toggle_soundboard(window, cx);
                    }))
            });
        let soundboard: AnyElement = if soundboard_open {
            soundboard.into_any_element()
        } else {
            self.with_tip(
                "voice-soundboard-tip",
                Tip::text(
                    dc_intl::tr(soundboard_reason.unwrap_or("Open Soundboard")),
                    Side::Top,
                ),
                soundboard,
                cx,
            )
            .into_any_element()
        };
        // Module 483364 chooses the game card before stream metadata; they are one panel.
        let stream_row =
            own_stream
                .as_ref()
                .filter(|_| self.panel_running_game().is_none())
                .map(|info| {
                    let name = info.name.clone().unwrap_or_default();
                    let stop = self.with_tip(
                        "voice-stream-stop-tip",
                        Tip::text(dc_intl::tr("Stop Streaming"), Side::Top),
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
                                .child(div().absolute().left(px(19.)).top(px(20.)).child(
                                    super::icon("voice_member_camera", 14., "--text-strong"),
                                )),
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
                                                // `onContextMenu` of the channel link: the `rtc-channel` menu
                                                // (module 576701).
                                                .on_mouse_down(
                                                    gpui::MouseButton::Right,
                                                    cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                                        this.open_rtc_channel_menu(
                                                            guild_id,
                                                            channel_id,
                                                            event.position,
                                                            cx,
                                                        );
                                                        cx.stop_propagation();
                                                    }),
                                                )
                                                .child(match guild {
                                                    Some(guild) => {
                                                        format!("{channel} / {}", guild.name)
                                                    }
                                                    None => channel.clone(),
                                                }),
                                        )
                                        // Not in the pinned client: CallTimer (`plugin_hooks/call_timer.rs`).
                                        .children(self.call_timer_connection()),
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
                        // Show Voice Users: the channel's people under the connection row (module 596339 `TB`).
                        .children(self.rtc_voice_users(guild_id, channel_id, cx))
                        // A Stage's panel has no camera, screen, activity or soundboard row (stage-ann/stage-live).
                        .children((!stage).then(|| {
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(div().flex_1().child(camera))
                                .child(div().flex_1().child(screen))
                                .child(div().flex_1().child(activity))
                                .child(div().flex_1().child(soundboard))
                        })),
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
                                        this.leave_activity_by_hand(cx);
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
        self.voice_members_in(guild, channel, false, cx)
    }

    /// The voice user list of module 588224, under the sidebar row or (`popout`) in the call header's Participants
    /// popout (module 195007), where the scroller's own padding replaces the row's indent.
    pub(super) fn voice_members_in(
        &self,
        guild: Id,
        channel: Id,
        popout: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let prefix = if popout {
            "voice-popout-member"
        } else {
            "voice-member"
        };
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
                // Not in the pinned client: CallTimer's time by the row (`plugin_hooks/call_timer.rs`).
                let (timer_under, timer_icon) = match self.call_timer_row(member, cx) {
                    Some((true, text)) => (Some(text), None),
                    Some((false, icon)) => (None, Some(icon)),
                    None => (None, None),
                };
                // Not in the pinned client: RoleColorEverywhere's `voiceUsers`
                // (`plugin_hooks/role_color_everywhere.rs`).
                let role_look =
                    self.role_color_everywhere(super::plugin_hooks::RolePlace::VoiceUsers, member);
                div()
                    .id(gpui::SharedString::from(format!("{prefix}-{member}")))
                    .on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            // The voice list's user menu carries the media items (module 340851).
                            this.context_voice_row();
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // A left click opens the person's profile popout to the right of the row.
                        let anchor = this
                            .tip_anchors
                            .borrow()
                            .get(&gpui::SharedString::from(format!("{prefix}-{member}")))
                            .copied();
                        if let Some(anchor) = anchor {
                            this.open_profile(member, anchor, false, cx);
                        }
                    }))
                    // Module 340851: the row opens the voice user popout (772800) on hover.
                    .when(!popout, |row| {
                        row.on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                            let target = VoiceUserTarget {
                                guild,
                                channel,
                                user: member,
                            };
                            this.voice_row_hover(target, *inside, cx);
                        }))
                    })
                    .cursor_pointer()
                    .relative()
                    .child({
                        let anchors = self.tip_anchors.clone();
                        let key = gpui::SharedString::from(format!("{prefix}-{member}"));
                        gpui::canvas(
                            move |bounds, _, _| {
                                anchors.borrow_mut().insert(key.clone(), bounds);
                            },
                            |_, _, _, _| {},
                        )
                        // The row element ends 9px before the sidebar's edge (capture: row 108 to 366).
                        .absolute()
                        .top_0()
                        .left_0()
                        .bottom_0()
                        .right(px(9.))
                    })
                    .flex_none()
                    .h(px(32.))
                    .pl(px(if popout { 0. } else { 44. }))
                    .pr(px(if popout { 0. } else { 17. }))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_full()
                            .relative()
                            .children(
                                avatar.map(|picture| img(picture).size(px(24.)).rounded_full()),
                            )
                            // The ring is drawn over the avatar: a 3px `--background-base-lower` inset shadow
                            // under a 2px `--status-speaking` one (module 556525), both under a 50ms transition.
                            .child({
                                let (inner, backing) =
                                    super::motion::messages::speaking_rings(member, speaking);
                                div()
                                    .absolute()
                                    .inset_0()
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded_full()
                                            .border(px(backing))
                                            .border_color(color("--background-base-lower")),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded_full()
                                            .border(px(inner))
                                            .border_color(color("--status-speaking")),
                                    )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(match role_look {
                                Some(look) => gpui::rgb(look.color),
                                None => color(if speaking {
                                    "--interactive-text-active"
                                } else {
                                    "--channels-default"
                                }),
                            })
                            .child(match role_look.and_then(|look| look.gradient) {
                                Some(colors) => super::role_gradient::name(
                                    user.display_name().to_owned(),
                                    colors,
                                )
                                .into_any_element(),
                                None => super::name_styles::name(
                                    format!("voice-member-name-{}", user.id),
                                    user.display_name().to_owned(),
                                    self.name_styles(&user, Some(guild)).as_ref(),
                                    super::name_styles::Display::Font,
                                )
                                .into_any_element(),
                            })
                            .children(timer_under),
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
                    .children(timer_icon)
                    .into_any_element()
            })
            .collect()
    }
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

#[cfg(test)]
mod session_tests {
    use super::{
        AfterFailedWait, AfterMoveWait, MAX_FAILED_RECONNECTS, after_failed_wait, after_move_wait,
        taken_elsewhere,
    };

    #[test]
    fn unexpected_failed_disconnect_retries_within_budget_then_ends() {
        assert_eq!(
            after_failed_wait(Some((true, false, 7, 0)), 7, true),
            AfterFailedWait::Restart { tell: false }
        );
        assert_eq!(
            after_failed_wait(Some((true, false, 7, 2)), 7, false),
            AfterFailedWait::Restart { tell: true }
        );
        assert_eq!(
            after_failed_wait(Some((true, false, 7, MAX_FAILED_RECONNECTS)), 7, true),
            AfterFailedWait::End
        );
        // Reconnected, replaced, or already ended: do nothing.
        assert_eq!(
            after_failed_wait(Some((true, true, 7, 0)), 7, true),
            AfterFailedWait::Nothing
        );
        assert_eq!(
            after_failed_wait(Some((true, false, 8, 0)), 7, true),
            AfterFailedWait::Nothing
        );
        assert_eq!(after_failed_wait(None, 7, true), AfterFailedWait::Nothing);
    }

    #[test]
    fn a_moved_call_without_a_new_server_restarts_or_ends_once_the_wait_ran_out() {
        // Still waiting on the connection the move closed: reuse the stored server, or end without one.
        assert_eq!(
            after_move_wait(Some((true, false, 4)), 4, true),
            AfterMoveWait::Restart
        );
        assert_eq!(
            after_move_wait(Some((true, false, 4)), 4, false),
            AfterMoveWait::End
        );
        // Reconnected, already restarted (a new generation or not started), or no call any more: nothing.
        assert_eq!(
            after_move_wait(Some((true, true, 4)), 4, true),
            AfterMoveWait::Nothing
        );
        assert_eq!(
            after_move_wait(Some((true, false, 5)), 4, true),
            AfterMoveWait::Nothing
        );
        assert_eq!(
            after_move_wait(Some((false, false, 4)), 4, false),
            AfterMoveWait::Nothing
        );
        assert_eq!(after_move_wait(None, 4, true), AfterMoveWait::Nothing);
    }

    #[test]
    fn another_session_in_the_same_call_takes_it() {
        assert!(taken_elsewhere("a", "b", (0, 500), (0, Some(500))));
        assert!(taken_elsewhere("a", "b", (7, 70), (7, Some(71))));
    }

    #[test]
    fn our_own_state_and_other_places_do_not() {
        assert!(!taken_elsewhere("a", "a", (0, 500), (0, Some(500))));
        assert!(!taken_elsewhere("a", "b", (0, 500), (0, Some(600))));
        assert!(!taken_elsewhere("a", "b", (7, 70), (8, Some(80))));
        // The other device leaving voice is not a move into our call.
        assert!(!taken_elsewhere("a", "b", (7, 70), (7, None)));
    }
}
