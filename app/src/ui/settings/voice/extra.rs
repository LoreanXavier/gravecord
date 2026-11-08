//! The Voice & Video switches, slider and buttons that act on a call or on the machine, as module 158935 builds them:
//! No Audio Detected Warning (with the notice in the app frame), Switch Voice Channel Warning (the "You sure?" modal),
//! Audio Device Suggestions (the "New Audio Device Detected" modal), Global Attenuation, Quality Of Service, Diagnostic
//! Audio Recording, Debug Logging with Upload Logs and Show Folder, and Reset all Voice & Video settings.
//!
//! Where the official client keeps each value: the media engine store (`MediaEngineStore`) holds silenceWarning,
//! attenuation, qos and aecDumpEnabled, so Reset clears them; the switch channel alert is `UnsyncedUserSettingsStore`'s
//! `disableVoiceChannelChangeAlert`; the suggestions are `ConnectedDeviceStore`'s `neverShowModal` and `ignoredDevices`;
//! debug logging is native. Reset touches only the first group (module 754712 `AUDIO_RESET`).

use super::{Root, color};
use crate::storage;
use crate::ui::audio_menu::Side;
use crate::ui::modal::{self, Tone};
use crate::ui::settings::model::Choice;
use dc_state::Id;
use dc_voice::devices::{ConnectedDevices, InUse, Kind, Suggestion};
use gpui::{
    AnyElement, Context, FontWeight, HighlightStyle, IntoElement, SharedString, StyledText, div,
    prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub(in crate::ui) const SILENCE: &str = "voice_silence_warning_setting";
pub(in crate::ui) const SWITCH_ALERT: &str = "voice_switch_channel_alert_setting";
pub(in crate::ui) const SUGGESTIONS: &str = "voice_audio_device_suggestions_setting";
pub(in crate::ui) const ATTENUATION: &str = "voice_global_attenuation_slider";
pub(in crate::ui) const ATTENUATION_SET: &str = "voice_global_attenuation_field_set";
pub(in crate::ui) const ATTEN_SELF: &str = "voice_global_attenuation_for_self_setting";
pub(in crate::ui) const ATTEN_OTHERS: &str = "voice_global_attenuation_for_others_setting";
pub(in crate::ui) const QOS: &str = "voice_quality_of_service_setting";
pub(in crate::ui) const RECORDING: &str = "voice_and_video_audio_recording";
pub(in crate::ui) const DEBUG_LOGGING: &str = "voice_and_video_debug_logging";
pub(in crate::ui) const RESET: &str = "voice_and_video_reset_all_settings";

/// `MediaEngineStore`'s part of these settings, in storage.json next to the other `audio.*` keys.
const SILENCE_STORED: &str = "audio.silence_warning";
const ATTENUATION_STORED: &str = "audio.attenuation";
const ATTEN_SELF_STORED: &str = "audio.attenuation_self";
const ATTEN_OTHERS_STORED: &str = "audio.attenuation_others";
const QOS_STORED: &str = "audio.qos";
const RECORDING_STORED: &str = "audio.recording";
/// Native in the official client, so no store resets it; kept in storage.json here.
const DEBUG_STORED: &str = "audio.debug_logging";
/// The keys `AUDIO_RESET` removes along with the rest of `MediaEngineStore`: everything here and `Stored`'s own.
const RESET_KEYS: &[&str] = &[
    "audio.input_profile",
    "audio.push_to_talk",
    "audio.threshold",
    "audio.auto_threshold",
    "audio.advanced_voice_activity",
    "audio.push_to_talk_delay",
    "audio.push_to_talk_shortcut",
    "audio.echo_cancellation",
    "audio.noise_suppression",
    "audio.room_echo",
    "audio.automatic_gain_control",
    "audio.input_device",
    "audio.output_device",
    "audio.input_volume",
    "audio.output_volume",
    SILENCE_STORED,
    ATTENUATION_STORED,
    ATTEN_SELF_STORED,
    ATTEN_OTHERS_STORED,
    QOS_STORED,
    RECORDING_STORED,
];
const UNSYNCED_STORE: &str = "UnsyncedUserSettingsStore";
const DEVICE_STORE: &str = "ConnectedDeviceStore";

/// `getArticleURL(AV_ERROR_CODES)` and `NO_INPUT_DETECTED`.
const ERROR_CODES_ARTICLE: &str = "https://support.discord.com/hc/en-us/articles/30952914470807";

/// `AVError` codes (module 487329): no audio input detected, and the debug log upload failing.
const NO_AUDIO_INPUT_CODE: u32 = 3002;
const UPLOAD_FAILED_CODE: u32 = 4001;
/// `readLogFiles(0xe00000, true)`: the most the log files may add up to.
const LOG_BYTES: u64 = 0xe0_0000;

/// The voice folder: debug logs, the attenuation journal and recordings.
pub(in crate::ui) fn voice_folder() -> Option<PathBuf> {
    storage::data_dir().map(|dir| dir.join("voice"))
}

#[derive(Clone)]
pub(super) enum Dialog {
    /// 24814: joining another voice channel while in one.
    SwitchChannel {
        guild: Id,
        channel: Id,
        name: String,
    },
    /// `dU`: what the notice's close button asks.
    Silence {
        dont_show: bool,
    },
    /// 121025: a device was connected during the call.
    Device {
        suggestion: Suggestion,
        choice: Kind,
    },
    Reset,
    DebugLogging {
        enabled: bool,
    },
    UploadDone,
    UploadFailed {
        message: String,
        code: u32,
    },
}

pub(super) struct Extra {
    /// `silenceWarning`.
    pub silence_warning: bool,
    /// `disableVoiceChannelChangeAlert`.
    pub switch_alert_disabled: bool,
    /// `attenuation` (percent), `attenuateWhileSpeakingSelf`, `attenuateWhileSpeakingOthers`.
    pub attenuation: u8,
    pub attenuate_self: bool,
    pub attenuate_others: bool,
    pub qos: bool,
    /// `aecDumpEnabled`.
    pub recording: bool,
    pub debug_logging: bool,
    pub devices: ConnectedDevices,
    /// Counts connections, so the notice dismissed in one comes back in the next (`mediaSessionId`).
    serial: u64,
    /// The connection and microphone the notice was dismissed for.
    dismissed: Option<(u64, Option<String>)>,
    pub dialog: Option<Dialog>,
    /// Upload Logs: in flight (the button's `loading`), and spent for the session (`isDisabled`).
    uploading: bool,
    upload_spent: bool,
    upload_parts: usize,
    polling: bool,
}

fn flag(key: &str, default: bool) -> bool {
    storage::get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

impl Extra {
    /// At start: the stored values, given to the voice engine, the attenuation journal of a killed run put right.
    pub fn restore() -> Self {
        let unsynced = storage::get(UNSYNCED_STORE);
        let device_store = storage::get(DEVICE_STORE);
        let mut devices = ConnectedDevices::new(cfg!(windows));
        devices.restore(
            device_store
                .as_ref()
                .and_then(|store| store["neverShowModal"].as_bool())
                .unwrap_or(false),
            device_store
                .as_ref()
                .and_then(|store| store["ignoredDevices"].as_object())
                .map(|names| {
                    names
                        .iter()
                        .filter(|(_, ignored)| ignored.as_bool() == Some(true))
                        .map(|(name, _)| name.clone())
                        .collect::<BTreeSet<String>>()
                })
                .unwrap_or_default(),
        );
        let extra = Self {
            silence_warning: flag(SILENCE_STORED, true),
            switch_alert_disabled: unsynced
                .as_ref()
                .and_then(|store| store["disableVoiceChannelChangeAlert"].as_bool())
                .unwrap_or(false),
            attenuation: storage::get(ATTENUATION_STORED)
                .and_then(|value| value.as_u64())
                .map_or(0, |percent| percent.min(100) as u8),
            attenuate_self: flag(ATTEN_SELF_STORED, false),
            attenuate_others: flag(ATTEN_OTHERS_STORED, true),
            qos: flag(QOS_STORED, false),
            recording: flag(RECORDING_STORED, false),
            // The captured account has it on: the official client logs by default.
            debug_logging: flag(DEBUG_STORED, true),
            devices,
            serial: 0,
            dismissed: None,
            dialog: None,
            uploading: false,
            upload_spent: false,
            upload_parts: 0,
            polling: false,
        };
        if let Some(folder) = voice_folder() {
            let journal = folder.join("attenuation.journal");
            let restored = dc_voice::recover_attenuation(&journal);
            if restored > 0 {
                tracing::info!(
                    restored,
                    "audio sessions left lowered by a killed run put back"
                );
            }
            dc_voice::prefs().set_attenuation_journal(Some(journal));
        }
        extra.push_all();
        extra
    }

    /// Gives every value to the voice engine.
    fn push_all(&self) {
        let prefs = dc_voice::prefs();
        prefs.set_qos(self.qos);
        prefs.set_attenuation(self.attenuation, self.attenuate_self, self.attenuate_others);
        prefs.set_recording(self.recording.then(voice_folder).flatten());
        self.apply_debug_logging();
    }

    fn apply_debug_logging(&self) {
        match (self.debug_logging, voice_folder()) {
            (true, Some(folder)) => {
                if !crate::logging::file_open() {
                    crate::logging::start_file(&folder);
                }
            }
            _ => crate::logging::stop_file(),
        }
    }

    /// What the generated page's rows show for these settings.
    pub fn choices(&self) -> Vec<(&'static str, Choice)> {
        vec![
            (SILENCE, Choice::On(self.silence_warning)),
            (SWITCH_ALERT, Choice::On(!self.switch_alert_disabled)),
            (SUGGESTIONS, Choice::On(!self.devices.never_show())),
            (ATTEN_SELF, Choice::On(self.attenuate_self)),
            (ATTEN_OTHERS, Choice::On(self.attenuate_others)),
            (QOS, Choice::On(self.qos)),
            (RECORDING, Choice::On(self.recording)),
            (DEBUG_LOGGING, Choice::On(self.debug_logging)),
        ]
    }

    fn save_unsynced(&self) {
        let mut store = storage::get(UNSYNCED_STORE).unwrap_or_else(|| json!({}));
        store["disableVoiceChannelChangeAlert"] = json!(self.switch_alert_disabled);
        storage::set(UNSYNCED_STORE, store);
    }

    fn save_devices(&self) {
        let ignored: serde_json::Map<String, Value> = self
            .devices
            .ignored()
            .iter()
            .map(|name| (name.clone(), json!(true)))
            .collect();
        storage::set(
            DEVICE_STORE,
            json!({"ignoredDevices": ignored, "neverShowModal": self.devices.never_show()}),
        );
    }

    /// The error context of the notice (`makeErrorContextKey`): this connection and the microphone in it.
    fn context(&self) -> (u64, Option<String>) {
        let device =
            storage::get("audio.input_device").and_then(|value| value.as_str().map(str::to_owned));
        (self.serial, device)
    }
}

/// The setting keys this module owns and shows as switches or the slider.
pub(super) fn owns(key: &str) -> bool {
    matches!(
        key,
        SILENCE
            | SWITCH_ALERT
            | SUGGESTIONS
            | ATTEN_SELF
            | ATTEN_OTHERS
            | QOS
            | RECORDING
            | DEBUG_LOGGING
    )
}

/// Global Attenuation exists on Windows only (`supports(ATTENUATION)`).
pub(in crate::ui) fn attenuation_shown() -> bool {
    dc_voice::attenuation_supported()
}

fn handler(
    run: impl Fn(&mut Root, &mut gpui::Window, &mut Context<Root>) + 'static,
) -> modal::Handler {
    Box::new(run)
}

fn bold_run(before: &str, bold: &str, after: &str) -> AnyElement {
    let text = format!("{before}{bold}{after}");
    let range = before.len()..before.len() + bold.len();
    StyledText::new(text)
        .with_highlights([(
            range,
            HighlightStyle {
                font_weight: Some(FontWeight(600.)),
                ..Default::default()
            },
        )])
        .into_any_element()
}

fn plain(text: &str) -> AnyElement {
    div()
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

impl Root {
    /// A generated switch of this module changed (`setting_choices` already holds the new value).
    pub(in crate::ui) fn extra_setting_changed(&mut self, key: &'static str, on: bool) -> bool {
        if !owns(key) {
            return false;
        }
        let extra = &mut self.voice.extra;
        match key {
            SILENCE => {
                extra.silence_warning = on;
                storage::set(SILENCE_STORED, json!(on));
            }
            SWITCH_ALERT => {
                extra.switch_alert_disabled = !on;
                extra.save_unsynced();
            }
            SUGGESTIONS => {
                extra.devices.set_never_show(!on);
                extra.save_devices();
                if !on && matches!(extra.dialog, Some(Dialog::Device { .. })) {
                    extra.dialog = None;
                }
            }
            ATTEN_SELF | ATTEN_OTHERS => {
                if key == ATTEN_SELF {
                    extra.attenuate_self = on;
                    storage::set(ATTEN_SELF_STORED, json!(on));
                } else {
                    extra.attenuate_others = on;
                    storage::set(ATTEN_OTHERS_STORED, json!(on));
                }
                dc_voice::prefs().set_attenuation(
                    extra.attenuation,
                    extra.attenuate_self,
                    extra.attenuate_others,
                );
            }
            QOS => {
                extra.qos = on;
                storage::set(QOS_STORED, json!(on));
                dc_voice::prefs().set_qos(on);
            }
            RECORDING => {
                extra.recording = on;
                storage::set(RECORDING_STORED, json!(on));
                dc_voice::prefs().set_recording(on.then(voice_folder).flatten());
            }
            DEBUG_LOGGING => {
                // `re(e)`: the switch asks first; it moves once the question is answered.
                let current = extra.debug_logging;
                self.setting_choices.insert(key, Choice::On(current));
                extra.dialog = Some(Dialog::DebugLogging { enabled: on });
            }
            _ => {}
        }
        true
    }

    /// The attenuation slider let go: `setAttenuation(percent, self, others)`.
    pub(super) fn commit_attenuation(&mut self, fraction: f32) {
        let percent = (fraction * 100.).round().clamp(0., 100.) as u8;
        let extra = &mut self.voice.extra;
        extra.attenuation = percent;
        storage::set(ATTENUATION_STORED, json!(percent));
        dc_voice::prefs().set_attenuation(percent, extra.attenuate_self, extra.attenuate_others);
    }

    pub(super) fn attenuation_fraction(&self) -> f32 {
        f32::from(self.voice.extra.attenuation) / 100.
    }

    /// Reset all Voice & Video settings, after the question: the stored voice settings go back to the defaults of
    /// `e5()` without a restart (`AUDIO_RESET` removes `MediaEngineStore` and reloads).
    pub(in crate::ui) fn voice_reset_all(&mut self, cx: &mut Context<Self>) {
        for key in RESET_KEYS {
            storage::remove(key);
        }
        let prefs = dc_voice::prefs();
        prefs.set_input_device(None);
        prefs.set_output_device(None);
        prefs.set_input_gain(1.);
        prefs.set_output_gain(1.);
        let extra = &mut self.voice.extra;
        extra.silence_warning = true;
        extra.attenuation = 0;
        extra.attenuate_self = false;
        extra.attenuate_others = true;
        extra.qos = false;
        extra.recording = false;
        extra.dismissed = None;
        extra.dialog = None;
        self.voice.extra.push_all();
        self.voice.reset_stored();
        self.sync_ptt_hook();
        let choices = self.voice.choices();
        self.setting_choices.extend(choices);
        cx.notify();
    }

    // ---- the notice ----

    /// `NO_AUDIO_INPUT_DETECTED` is an active error (822209 `q`): in a connected call in voice activity mode, with the
    /// warning on, the microphone silent for good, not heard once since the connection began, and not muted.
    fn silence_notice_shown(&self) -> bool {
        let extra = &self.voice.extra;
        let prefs = dc_voice::prefs();
        self.call.as_ref().is_some_and(|call| call.connected)
            && extra.silence_warning
            && !self.voice.stored.effective().push_to_talk
            && prefs.input_detected() == Some(false)
            && !prefs.input_detected_this_connection()
            && !(self.self_mute || self.self_deaf)
            && extra.dismissed.as_ref() != Some(&extra.context())
    }

    /// The danger notice (`ut`): the sentence, "Error: 3002" opening the article, "Visit Settings", and the close
    /// button that dismisses it and asks (`dU`). `notice_Q02kKo` metrics: 14/36 medium, 4px in at the start and 28
    /// at the end, a 1px frame border on top and at the start, the top-left corner rounded 8.
    fn voice_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.silence_notice_shown() {
            return None;
        }
        let text = color("--notice-text-critical");
        Some(
            div()
                .id("voice-notice")
                .relative()
                .flex_none()
                .h(px(37.))
                .pl(px(4.))
                .pr(px(28.))
                .border_t_1()
                .border_l_1()
                .border_color(color("--app-frame-border"))
                .rounded_tl(px(8.))
                .bg(color("--notice-background-critical"))
                .text_color(text)
                .text_size(px(14.))
                .line_height(px(36.))
                .font_weight(FontWeight(500.))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    "Well, it looks like Discord is not detecting any input from your mic. Let's fix that!",
                )
                .child(
                    div()
                        .id("voice-notice-error")
                        .ml(px(6.))
                        .cursor_pointer()
                        .font_weight(FontWeight(600.))
                        .hover(|style| style.underline())
                        .on_click(cx.listener(|_, _, _, cx| cx.open_url(ERROR_CODES_ARTICLE)))
                        .child(format!("Error: {NO_AUDIO_INPUT_CODE}")),
                )
                .child(
                    div()
                        .id("voice-notice-settings")
                        .ml(px(10.))
                        .h(px(24.))
                        .px(px(10.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(text)
                        .line_height(px(22.))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_settings_section("Voice & Video", cx);
                        }))
                        .child("Visit Settings"),
                )
                .child(
                    div()
                        .id("voice-notice-close")
                        .absolute()
                        .top_0()
                        .right_0()
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            // `oT.w()` dismisses the error for this connection, then the question opens.
                            let context = this.voice.extra.context();
                            this.voice.extra.dismissed = Some(context);
                            this.voice.extra.dialog = Some(Dialog::Silence { dont_show: false });
                            cx.notify();
                        }))
                        .child(crate::ui::icon("close", 16., "--notice-text-critical")),
                )
                .into_any_element(),
        )
    }

    /// The row of the sidebar and the content, under the notice when there is one (`grid-area: notice`).
    pub(in crate::ui) fn main_row(
        &self,
        rail: AnyElement,
        sidebar: AnyElement,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = div().flex().flex_1().min_h(px(0.)).min_w(px(0.));
        match self.voice_notice(cx) {
            None => div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .child(rail)
                .child(sidebar)
                .child(content)
                .into_any_element(),
            Some(notice) => div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .child(rail)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(notice)
                        .child(row.child(sidebar).child(content)),
                )
                .into_any_element(),
        }
    }

    // ---- joining another channel ----

    /// `selectVoiceChannel` with the confirmation (389234): in a call, a different channel asks first, unless the
    /// warning is off or the call is in the target server's AFK channel.
    pub(in crate::ui) fn request_join_voice(
        &mut self,
        guild: Id,
        channel: Id,
        cx: &mut Context<Self>,
    ) {
        if self.switch_channel_asks(guild, channel) {
            let name = self
                .stores
                .guilds
                .iter()
                .filter(|candidate| candidate.id == guild)
                .flat_map(|guild| guild.channels.iter())
                .find(|candidate| candidate.id == channel)
                .map(|channel| channel.name.clone());
            // `null == k ? null`: a channel without a name shows no modal and joins.
            if let Some(name) = name {
                self.voice.extra.dialog = Some(Dialog::SwitchChannel {
                    guild,
                    channel,
                    name,
                });
                cx.notify();
                return;
            }
        }
        self.join_voice(guild, channel, cx);
    }

    fn switch_channel_asks(&self, guild: Id, channel: Id) -> bool {
        if self.voice.extra.switch_alert_disabled {
            return false;
        }
        let Some(call) = &self.call else {
            return false;
        };
        if call.channel == channel {
            return false;
        }
        // In the target server's AFK channel already: moving out of it needs no question.
        !(self.stores.afk_channels.get(&guild) == Some(&call.channel))
    }

    // ---- devices ----

    /// The call ended: no device is suggested after it, and the notice starts afresh.
    pub(in crate::ui) fn voice_call_ended(&mut self) {
        self.voice.extra.devices.disconnected();
        if matches!(self.voice.extra.dialog, Some(Dialog::Device { .. })) {
            self.voice.extra.dialog = None;
        }
    }

    /// A connection came up: the notice may show again for it.
    pub(in crate::ui) fn voice_connection_started(&mut self, cx: &mut Context<Self>) {
        self.voice.extra.serial += 1;
        self.start_voice_polling(cx);
    }

    /// A device was picked by hand: whatever was suggested is forgotten (`AUDIO_SET_INPUT_DEVICE`).
    pub(in crate::ui) fn voice_device_chosen(&mut self) {
        self.voice.extra.devices.dont_switch();
    }

    /// Lists the devices every two seconds from the first connection on, for as long as the app runs.
    fn start_voice_polling(&mut self, cx: &mut Context<Self>) {
        if self.voice.extra.polling {
            return;
        }
        self.voice.extra.polling = true;
        // Starts the listing thread and records what is plugged in now.
        let first = dc_voice::devices::watch().latest();
        self.voice.extra.devices.update(&first, false);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(dc_voice::devices::POLL)
                    .await;
                if this
                    .update(cx, |this, cx| this.poll_voice_devices(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// `MEDIA_ENGINE_DEVICES`, then `Hs()`: the first device connected during the call opens the suggestion.
    fn poll_voice_devices(&mut self, cx: &mut Context<Self>) {
        let connected = self.call.as_ref().is_some_and(|call| call.connected);
        let list = dc_voice::devices::watch().latest();
        self.voice.extra.devices.update(&list, connected);
        if self.voice.extra.dialog.is_some() {
            return;
        }
        let read =
            |key: &str| storage::get(key).and_then(|value| value.as_str().map(str::to_owned));
        let in_use = InUse {
            chosen_input: read("audio.input_device"),
            chosen_output: read("audio.output_device"),
            connected,
        };
        if let Some(suggestion) = self.voice.extra.devices.suggestion(&in_use) {
            let choice = suggestion.kind;
            self.voice.extra.dialog = Some(Dialog::Device { suggestion, choice });
            cx.notify();
        }
    }

    // ---- the buttons of Debug Logging ----

    /// `rt()`: Upload Logs. The files of the voice folder and the voice settings go to Discord Support.
    pub(in crate::ui) fn voice_upload_logs(&mut self, cx: &mut Context<Self>) {
        let extra = &mut self.voice.extra;
        if extra.uploading || extra.upload_spent {
            return;
        }
        extra.uploading = true;
        let folder = voice_folder().unwrap_or_default();
        let files = pick_log_files(&crate::logging::log_files(&folder));
        let state = json!({"extraInfo": Value::Null, "mediaEngineState": self.voice_state_json()});
        let _ = self
            .requests
            .send(crate::net::Request::UploadLogs { files, state });
        cx.notify();
    }

    /// The stored voice settings, the `mediaEngineState` of the upload.
    fn voice_state_json(&self) -> Value {
        let extra = &self.voice.extra;
        json!({
            "settings": self.voice.stored_json(),
            "silenceWarning": extra.silence_warning,
            "attenuation": extra.attenuation,
            "attenuateWhileSpeakingSelf": extra.attenuate_self,
            "attenuateWhileSpeakingOthers": extra.attenuate_others,
            "qos": extra.qos,
            "aecDumpEnabled": extra.recording,
            "debugLogging": extra.debug_logging,
        })
    }

    /// The upload finished (`Update::Fetched` under `voice-logs-upload`).
    pub(in crate::ui) fn voice_logs_uploaded(&mut self, body: &Value, cx: &mut Context<Self>) {
        self.panel_logs_uploaded(cx);
        let extra = &mut self.voice.extra;
        extra.uploading = false;
        // `onUploadFinish`: spent for the session whatever happened.
        extra.upload_spent = true;
        extra.upload_parts = body["count"].as_u64().unwrap_or(0) as usize;
        extra.dialog = Some(if upload_succeeded(body, extra.upload_parts) {
            Dialog::UploadDone
        } else {
            Dialog::UploadFailed {
                message: "Something went wrong and we were unable to upload your debug files. Please try again."
                    .to_owned(),
                code: UPLOAD_FAILED_CODE,
            }
        });
        cx.notify();
    }

    /// Show Folder: the voice folder in the file manager (`showItemInFolder(getLogPath())`).
    pub(in crate::ui) fn voice_show_folder(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = voice_folder() else {
            return;
        };
        let _ = std::fs::create_dir_all(&folder);
        cx.reveal_path(&folder);
    }

    /// Escape closes a dialog of this module. True when there was one.
    pub(in crate::ui) fn voice_dialog_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.voice.extra.dialog.is_none() {
            return false;
        }
        self.close_voice_dialog();
        cx.notify();
        true
    }

    /// Closing without an answer: a device suggestion counts as "Don't Switch" (`_.mk()`).
    fn close_voice_dialog(&mut self) {
        if matches!(self.voice.extra.dialog, Some(Dialog::Device { .. })) {
            self.voice.extra.devices.dont_switch();
        }
        self.voice.extra.dialog = None;
    }

    /// Whether Upload Logs and Show Folder are in use: (uploading, spent).
    pub(in crate::ui) fn upload_state(&self) -> (bool, bool) {
        (self.voice.extra.uploading, self.voice.extra.upload_spent)
    }

    // ---- the dialogs ----

    pub(in crate::ui) fn voice_dialog_layer(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.voice.extra.dialog.clone()?;
        let close = || -> modal::Handler { handler(|this, _, _| this.close_voice_dialog()) };
        let cancel = || {
            modal::button(
                "voice-dialog-cancel",
                "Cancel",
                Tone::Secondary,
                |this, _, _| this.close_voice_dialog(),
            )
        };
        let okay = |id: &'static str, tone: Tone, run: modal::Handler| modal::Button {
            id,
            label: "Okay".into(),
            tone,
            run,
        };
        Some(match dialog {
            Dialog::SwitchChannel {
                guild,
                channel,
                name,
            } => {
                let checked = self.voice.extra.switch_alert_disabled;
                let input = self.dialog_checkbox(
                    "voice-dont-ask",
                    "Don't ask again",
                    checked,
                    cx,
                    |this| {
                        // Checked at once: the setting is written even if the question is then cancelled.
                        let extra = &mut this.voice.extra;
                        extra.switch_alert_disabled = !extra.switch_alert_disabled;
                        extra.save_unsynced();
                        let on = !extra.switch_alert_disabled;
                        this.setting_choices.insert(SWITCH_ALERT, Choice::On(on));
                    },
                );
                modal::render_with_input(
                    modal::Spec {
                        id: "voice-switch-channel",
                        title: "You sure?".into(),
                        subtitle: Some(bold_run(
                            "Looks like you're in another voice channel. Are you sure you want to switch to ",
                            &name,
                            "?",
                        )),
                        body: None,
                        buttons: vec![
                            cancel(),
                            modal::button(
                                "voice-dialog-confirm",
                                "Confirm",
                                Tone::Primary,
                                move |this, _, cx| {
                                    this.close_voice_dialog();
                                    this.join_voice(guild, channel, cx);
                                },
                            ),
                        ],
                        close: close(),
                    },
                    input,
                    cx,
                )
            }
            Dialog::Silence { dont_show } => {
                let input = self.dialog_checkbox(
                    "voice-dont-show",
                    "Don't show me the warning again",
                    dont_show,
                    cx,
                    |this| {
                        if let Some(Dialog::Silence { dont_show }) = &mut this.voice.extra.dialog {
                            *dont_show = !*dont_show;
                        }
                    },
                );
                modal::render_with_input(
                    modal::Spec {
                        id: "voice-silence-dialog",
                        title: "We can't hear you!".into(),
                        subtitle: Some(plain(
                            "Is your mic extra fancy and silencing itself? In that case you can turn off the red warning by clicking the\nteeny tiny small text below.",
                        )),
                        body: None,
                        buttons: vec![okay(
                            "voice-dialog-okay",
                            Tone::Primary,
                            handler(move |this, _, _| {
                                if matches!(
                                    this.voice.extra.dialog,
                                    Some(Dialog::Silence { dont_show: true })
                                ) {
                                    this.voice.extra.silence_warning = false;
                                    storage::set(SILENCE_STORED, json!(false));
                                    this.setting_choices.insert(SILENCE, Choice::On(false));
                                }
                                this.voice.extra.dialog = None;
                            }),
                        )],
                        close: close(),
                    },
                    input,
                    cx,
                )
            }
            Dialog::Device { suggestion, choice } => self.device_dialog(suggestion, choice, cx),
            Dialog::Reset => modal::render(
                modal::Spec {
                    id: "voice-reset-dialog",
                    title: "Reset Voice and Video Settings".into(),
                    subtitle: Some(plain(
                        "Resetting voice settings will clear all local mutes and local volumes.\nAre you sure you want to do this?",
                    )),
                    body: None,
                    buttons: vec![
                        cancel(),
                        okay(
                            "voice-dialog-okay",
                            Tone::Critical,
                            handler(|this, _, cx| this.voice_reset_all(cx)),
                        ),
                    ],
                    close: close(),
                },
                cx,
            ),
            Dialog::DebugLogging { enabled } => modal::render(
                modal::Spec {
                    id: "voice-debug-dialog",
                    title: "Change Debug Logging".into(),
                    subtitle: Some(plain(
                        "Changing debug logging will quit and re-launch Discord.\nAre you sure you want to do this?",
                    )),
                    body: None,
                    buttons: vec![
                        cancel(),
                        okay(
                            "voice-dialog-okay",
                            Tone::Critical,
                            handler(move |this, _, _| {
                                let extra = &mut this.voice.extra;
                                extra.debug_logging = enabled;
                                storage::set(DEBUG_STORED, json!(enabled));
                                extra.apply_debug_logging();
                                extra.dialog = None;
                                this.setting_choices
                                    .insert(DEBUG_LOGGING, Choice::On(enabled));
                            }),
                        ),
                    ],
                    close: close(),
                },
                cx,
            ),
            Dialog::UploadDone => modal::render(
                modal::Spec {
                    id: "voice-upload-done",
                    title: "Upload Successful".into(),
                    subtitle: Some(plain(
                        "Success! Your debug logs have been uploaded to Discord Support.",
                    )),
                    body: None,
                    buttons: vec![okay("voice-dialog-okay", Tone::Primary, close())],
                    close: close(),
                },
                cx,
            ),
            Dialog::UploadFailed { message, code } => modal::render(
                modal::Spec {
                    id: "voice-upload-failed",
                    title: "Upload Failed".into(),
                    subtitle: Some(plain(&message)),
                    body: Some(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(format!("Error: {code}"))
                            .into_any_element(),
                    ),
                    buttons: vec![okay("voice-dialog-okay", Tone::Primary, close())],
                    close: close(),
                },
                cx,
            ),
        })
    }

    /// The action bar's checkbox with its label.
    fn dialog_checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Root) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                toggle(this);
                cx.notify();
            }))
            .child(crate::ui::server_games::checkbox(checked))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(label),
            )
            .into_any_element()
    }

    /// 121025: the suggestion, the three choices when the device is both, and "Don't suggest it again".
    fn device_dialog(
        &mut self,
        suggestion: Suggestion,
        choice: Kind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = suggestion.display_name.clone();
        let (before, after) = match suggestion.kind {
            Kind::Input => (
                "Discord has detected a new audio input device named ",
                "! Do you want to switch to it?",
            ),
            Kind::Output => (
                "Discord has detected a new audio output device named ",
                "! Do you want to switch to it?",
            ),
            Kind::InputAndOutput => (
                "Discord has detected a new audio device named ",
                "! Do you want to switch to it?",
            ),
        };
        let both = suggestion.kind == Kind::InputAndOutput;
        let body = both.then(|| {
            let option = |label: &'static str, value: Kind, index: usize| {
                div()
                    .id(SharedString::from(format!("voice-device-choice-{index}")))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(40.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(Dialog::Device { choice, .. }) = &mut this.voice.extra.dialog {
                            *choice = value;
                        }
                        cx.notify();
                    }))
                    .child(crate::ui::settings::render::radio_dot(choice == value))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(label),
                    )
            };
            div()
                .flex()
                .flex_col()
                .child(option(
                    "Switch both input and output",
                    Kind::InputAndOutput,
                    0,
                ))
                .child(option("Switch input only", Kind::Input, 1))
                .child(option("Switch output only", Kind::Output, 2))
                .into_any_element()
        });
        let ignore_name = name.clone();
        let input = div()
            .id("voice-device-ignore")
            .cursor_pointer()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-link"))
            .hover(|style| style.underline())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.voice.extra.devices.ignore(&ignore_name);
                this.voice.extra.save_devices();
                this.voice.extra.dialog = None;
                cx.notify();
            }))
            .child(format!("Don't suggest {name} again"))
            .into_any_element();
        let switch_name = name.clone();
        modal::render_with_input(
            modal::Spec {
                id: "voice-device-dialog",
                title: "New Audio Device Detected".into(),
                subtitle: Some(bold_run(before, &name, after)),
                body,
                buttons: vec![
                    modal::button(
                        "voice-dialog-dont-switch",
                        "Don't Switch",
                        Tone::Secondary,
                        |this, _, _| this.close_voice_dialog(),
                    ),
                    modal::button(
                        "voice-dialog-switch",
                        "Switch Device",
                        Tone::Primary,
                        move |this, _, cx| {
                            // `handleSwitch(choice)`: `CONNECTED_DEVICE_SWITCH` sets the devices and forgets the rest.
                            let picked = match &this.voice.extra.dialog {
                                Some(Dialog::Device { choice, .. }) => *choice,
                                _ => choice,
                            };
                            let (input, output) =
                                this.voice.extra.devices.switch(&switch_name, picked);
                            this.voice.extra.dialog = None;
                            if let Some(device) = input {
                                this.set_device(Side::Input, Some(device), cx);
                            }
                            if let Some(device) = output {
                                this.set_device(Side::Output, Some(device), cx);
                            }
                        },
                    ),
                ],
                close: handler(|this, _, _| this.close_voice_dialog()),
            },
            input,
            cx,
        )
    }
}

/// `readLogFiles(0xe00000, true)`: the newest log files, as many as fit in the limit.
pub(super) fn pick_log_files(newest_first: &[PathBuf]) -> Vec<PathBuf> {
    let mut total = 0u64;
    let mut picked = Vec::new();
    for path in newest_first {
        let size = std::fs::metadata(path).map_or(0, |meta| meta.len());
        if total + size > LOG_BYTES {
            break;
        }
        total += size;
        picked.push(path.clone());
    }
    picked
}

/// The upload's answer is good (`uploadDebugFiles`): the request went through, every part was stored, and none of
/// the flags the server may add says otherwise.
pub(super) fn upload_succeeded(report: &Value, parts: usize) -> bool {
    if report["ok"].as_bool() != Some(true) || parts < 2 {
        return false;
    }
    let body = &report["body"];
    if let Some(count) = body["success_count"].as_u64()
        && count as usize != parts
    {
        return false;
    }
    ["store_success", "id_match", "all_success"]
        .iter()
        .all(|flag| body[*flag].as_bool() != Some(false))
}

impl super::VoiceState {
    /// Opens Reset all Voice & Video settings' question.
    pub fn ask_reset(&mut self) {
        self.extra.dialog = Some(Dialog::Reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upload_is_good_only_when_every_part_was_stored() {
        let good = json!({"ok": true, "body": {"success_count": 3, "store_success": true}});
        assert!(upload_succeeded(&good, 3));
        assert!(!upload_succeeded(&good, 4), "a part went missing");
        assert!(!upload_succeeded(&json!({"ok": false, "status": 429}), 3));
        assert!(!upload_succeeded(
            &json!({"ok": true, "body": {"all_success": false}}),
            3
        ));
        assert!(!upload_succeeded(
            &json!({"ok": true, "body": {"id_match": false}}),
            3
        ));
        assert!(
            !upload_succeeded(&good, 1),
            "the settings file alone is no upload (NO_FILE)"
        );
        assert!(upload_succeeded(&json!({"ok": true, "body": null}), 2));
    }

    #[test]
    fn log_files_fill_the_limit_newest_first() {
        let folder = std::env::temp_dir().join(format!("gravecord-logs-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let big = folder.join("big.log");
        let small = folder.join("small.log");
        std::fs::write(&big, vec![0u8; (LOG_BYTES as usize) - 10]).unwrap();
        std::fs::write(&small, vec![0u8; 100]).unwrap();
        // The small one is newer and fits; the big one then would not.
        let picked = pick_log_files(&[small.clone(), big.clone()]);
        assert_eq!(picked, vec![small.clone()]);
        assert_eq!(pick_log_files(std::slice::from_ref(&big)), vec![big]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_defaults_are_the_official_ones() {
        // `e5()`: silenceWarning true, attenuation 0, self false, others true, qos false, aecDumpEnabled false.
        assert!(RESET_KEYS.contains(&SILENCE_STORED));
        assert!(RESET_KEYS.contains(&ATTENUATION_STORED));
        assert!(RESET_KEYS.contains(&QOS_STORED));
        assert!(RESET_KEYS.contains(&RECORDING_STORED));
        // Not part of `MediaEngineStore`: the debug log switch and the two other stores stay.
        assert!(!RESET_KEYS.contains(&DEBUG_STORED));
        assert!(!RESET_KEYS.contains(&UNSYNCED_STORE));
        assert!(!RESET_KEYS.contains(&DEVICE_STORE));
    }
}
