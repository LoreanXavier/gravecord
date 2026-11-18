//! VoiceMessages (Vencord `src/plugins/voiceMessages`, GPL-3.0-or-later): the composer's "+" menu ends with "Send
//! Voice Message" ("Send Audio File as Voice Message" where the client's own Record Voice Message row is there), in a
//! server only with Send Messages and Send Voice Messages. It opens "Record Voice Message": "Start recording" / "Stop
//! recording" (the microphone with the plugin's `echoCancellation` and `noiseSuppression`, Ogg Opus) and "Upload File"
//! (any audio file, also dropped on the buttons), then "Preview": the recorded or chosen audio in the voice message
//! player, or a pill with a dot, the time and "RECORDING" / "----"; "Failed to parse selected audio file: …" when it
//! cannot be decoded, and the iOS warning for a file that is not Ogg Opus. Send uploads it as a voice message (the
//! waveform and length the plugin computes, the pending reply, SilentMessageToggle's flag) with the "Now sending voice
//! message... Please be patient" toast. Not part of the pinned client.
//!
//! GAP: the toast's clock icon (the app toast has none); the convertio link of the warning is drawn as its text.

use super::super::{Root, color, modal};
use dc_state::Id;
use gpui::{
    AnyElement, Context, ExternalPaths, FontWeight, InteractiveElement, IntoElement, ParentElement,
    PathPromptOptions, SharedString, StatefulInteractiveElement, Styled, div, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

const ID: &str = "VoiceMessages";
/// `VOICE_MESSAGE_FLAG`.
const DEFAULT_WAVEFORM: &str = "AAAAAAAAAAAA";
const DEFAULT_DURATION: f64 = 1.;
const MIN_BINS: usize = 32;
const MAX_BINS: usize = 256;
const BINS_PER_SECOND: f64 = 10.;
/// The preview player's message and attachment ids (never a real message).
const PREVIEW_MESSAGE: Id = u64::MAX - 77;

#[derive(Clone, Debug, PartialEq)]
enum Meta {
    Ready { waveform: String, duration: f64 },
    Pending,
    Failed(String),
}

struct Blob {
    bytes: Arc<Vec<u8>>,
    mime: String,
}

struct Dialog {
    channel: Id,
    recorder: Option<dc_voice::voice_recorder::Recorder>,
    recording_since: Option<Instant>,
    blob: Option<Blob>,
    meta: Meta,
    /// Which blob the preview's attachment id stands for.
    generation: u64,
    ticking: bool,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn processing() -> (bool, bool) {
    crate::plugins::with(|state| {
        let flag = |key: &str| state.value(ID, key) == Value::Bool(true);
        (flag("echoCancellation"), flag("noiseSuppression"))
    })
}

/// `generateWaveform` over the first channel: RMS bins (10 a second, 32 to 256), eased up towards full scale, base64.
/// The original's inner index adds the sample offset twice, so each bin reads every other sample from its start
/// (past the end a missing sample makes the bin 0); that is kept.
fn generate_waveform(samples: &[f32], duration: f64) -> String {
    use base64::Engine as _;
    let len = samples.len();
    let bins = ((duration * BINS_PER_SECOND).floor() as usize).clamp(MIN_BINS.min(len), MAX_BINS);
    if bins == 0 {
        return String::new();
    }
    let per_bin = len / bins;
    let mut out = vec![0u8; bins];
    for (bin, value) in out.iter_mut().enumerate() {
        let mut sum = 0f64;
        let mut missing = false;
        for sample in 0..per_bin {
            match samples.get(bin * per_bin + 2 * sample) {
                Some(level) => sum += f64::from(*level).powi(2),
                None => missing = true,
            }
        }
        *value = if missing || per_bin == 0 {
            0
        } else {
            ((sum / per_bin as f64).sqrt() * 255.)
                .floor()
                .clamp(0., 255.) as u8
        };
    }
    let max = out.iter().copied().max().unwrap_or(0);
    if max > 0 {
        let easing = (100. * (f64::from(max) / 255.).powi(3)).min(1.);
        let ratio = 1. + (255. / f64::from(max) - 1.) * easing;
        for value in &mut out {
            *value = (f64::from(*value) * ratio).floor().min(255.) as u8;
        }
    }
    base64::engine::general_purpose::STANDARD.encode(out)
}

/// `isUnsupportedFormat`: not `audio/ogg`, or a codec that is not Opus.
fn unsupported(mime: &str) -> bool {
    !mime.starts_with("audio/ogg") || (mime.contains("codecs") && !mime.contains("opus"))
}

/// The type a browser gives a chosen file, by its extension.
fn mime_of(path: &std::path::Path) -> String {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/x-m4a",
        "aac" => "audio/aac",
        "webm" => "audio/webm",
        _ => "",
    }
    .to_owned()
}

impl Root {
    /// The "+" menu's row while the plugin is on: its label, if the channel allows voice messages.
    pub(in crate::ui) fn voice_messages_plus_label(&self) -> Option<&'static str> {
        if !on() {
            return None;
        }
        if let Some(channel) = self.current_channel()
            && self.guild.is_some()
            && !(self.can_in_channel(channel, dc_state::permissions::SEND_VOICE_MESSAGES)
                && self.can_in_channel(channel, dc_state::permissions::SEND_MESSAGES))
        {
            return None;
        }
        Some(if self.record_voice_offered() {
            "Send Audio File as Voice Message"
        } else {
            "Send Voice Message"
        })
    }

    pub(in crate::ui) fn voice_messages_open(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel else {
            return;
        };
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                channel,
                recorder: None,
                recording_since: None,
                blob: None,
                meta: Meta::Ready {
                    waveform: DEFAULT_WAVEFORM.to_owned(),
                    duration: DEFAULT_DURATION,
                },
                generation: 0,
                ticking: false,
            });
        });
        cx.notify();
    }

    /// `onCloseCallback`: `VENCORD_STOP_RECORDING`, the recording thrown away.
    fn voice_messages_close(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = DIALOG.with(|dialog| dialog.borrow_mut().take())
            && let Some(recorder) = dialog.recorder
        {
            recorder.cancel();
        }
        cx.notify();
    }

    fn voice_messages_toggle_recording(&mut self, cx: &mut Context<Self>) {
        let recorder = DIALOG.with(|dialog| {
            dialog
                .borrow_mut()
                .as_mut()
                .and_then(|dialog| dialog.recorder.take())
        });
        match recorder {
            Some(recorder) => {
                DIALOG.with(|dialog| {
                    if let Some(dialog) = dialog.borrow_mut().as_mut() {
                        dialog.recording_since = None;
                    }
                });
                match recorder.stop() {
                    Ok(recording) => self.voice_messages_set_blob(
                        recording.file,
                        "audio/ogg; codecs=opus".to_owned(),
                        cx,
                    ),
                    Err(_) => {
                        self.put_app_toast("Failed to finish recording".to_owned(), false, cx)
                    }
                }
            }
            None => {
                let (echo, noise) = processing();
                match dc_voice::voice_recorder::Recorder::start_microphone_with(echo, noise) {
                    Ok(recorder) => {
                        DIALOG.with(|dialog| {
                            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                                dialog.recorder = Some(recorder);
                                dialog.recording_since = Some(Instant::now());
                            }
                        });
                        self.voice_messages_tick(cx);
                    }
                    Err(_) => self.put_app_toast("Failed to start recording".to_owned(), false, cx),
                }
            }
        }
        cx.notify();
    }

    /// The preview's timer moves while recording.
    fn voice_messages_tick(&mut self, cx: &mut Context<Self>) {
        let start = DIALOG.with(|dialog| {
            dialog.borrow_mut().as_mut().is_some_and(|dialog| {
                let start = !dialog.ticking;
                dialog.ticking = true;
                start
            })
        });
        if !start {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let recording = DIALOG.with(|dialog| {
                    dialog
                        .borrow()
                        .as_ref()
                        .is_some_and(|dialog| dialog.recording_since.is_some())
                });
                if this.update(cx, |_, cx| cx.notify()).is_err() || !recording {
                    DIALOG.with(|dialog| {
                        if let Some(dialog) = dialog.borrow_mut().as_mut() {
                            dialog.ticking = false;
                        }
                    });
                    break;
                }
            }
        })
        .detach();
    }

    /// `setAudioBlob`: the file is kept, then decoded off the UI thread for its waveform, its length and the player.
    fn voice_messages_set_blob(&mut self, bytes: Vec<u8>, mime: String, cx: &mut Context<Self>) {
        let bytes = Arc::new(bytes);
        let generation = DIALOG.with(|dialog| {
            let mut dialog = dialog.borrow_mut();
            let dialog = dialog.as_mut()?;
            dialog.generation += 1;
            dialog.blob = Some(Blob {
                bytes: bytes.clone(),
                mime,
            });
            dialog.meta = Meta::Pending;
            Some(dialog.generation)
        });
        let Some(generation) = generation else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            let decoded = dc_voice::sound::decode_attachment(bytes.to_vec());
            let clip = dc_voice::voice_player::Clip::from_file(bytes.to_vec()).ok();
            (decoded, clip)
        });
        cx.spawn(async move |this, cx| {
            let (decoded, clip) = task.await;
            let _ = this.update(cx, |this, cx| {
                let meta = match decoded {
                    Ok(decoded) => {
                        let channels = usize::from(decoded.channels.max(1));
                        let first: Vec<f32> =
                            decoded.samples.iter().step_by(channels).copied().collect();
                        let duration = first.len() as f64 / f64::from(decoded.rate.max(1));
                        Meta::Ready {
                            waveform: generate_waveform(&first, duration),
                            duration,
                        }
                    }
                    Err(_) => Meta::Failed("Unable to decode audio data".to_owned()),
                };
                let current = DIALOG.with(|dialog| {
                    let mut dialog = dialog.borrow_mut();
                    match dialog.as_mut() {
                        Some(dialog) if dialog.generation == generation => {
                            dialog.meta = meta;
                            true
                        }
                        _ => false,
                    }
                });
                if current && let Some(clip) = clip {
                    self_preload(this, generation, clip);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn voice_messages_choose(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let reading = path.clone();
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read(reading) })
                .await;
            let Ok(bytes) = read else {
                return;
            };
            let mime = mime_of(&path);
            let _ = this.update(cx, |this, cx| this.voice_messages_set_blob(bytes, mime, cx));
        })
        .detach();
    }

    /// A dropped file, read off the UI thread.
    fn voice_messages_read(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let reading = path.clone();
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read(reading) })
                .await;
            if let Ok(bytes) = read {
                let mime = mime_of(&path);
                let _ = this.update(cx, |this, cx| this.voice_messages_set_blob(bytes, mime, cx));
            }
        })
        .detach();
    }

    /// `sendAudio`.
    fn voice_messages_send(&mut self, cx: &mut Context<Self>) {
        let Some((channel, bytes, meta)) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            let blob = dialog.blob.as_ref()?;
            Some((dialog.channel, blob.bytes.clone(), dialog.meta.clone()))
        }) else {
            return;
        };
        let (waveform, duration) = match meta {
            Meta::Ready { waveform, duration } => (waveform, duration),
            _ => (DEFAULT_WAVEFORM.to_owned(), DEFAULT_DURATION),
        };
        let ping = self.reply_ping;
        let reply = self
            .replying
            .take()
            .filter(|_| self.channel == Some(channel))
            .map(|message| (self.guild, message.id, ping));
        let extra_flags = self.silent_message_take_flag();
        let _ = self.requests.send(crate::net::Request::SendVoice {
            key: format!("{}{channel}", super::super::voice_message::VOICE_SEND_KEY),
            channel,
            file: bytes.to_vec(),
            duration_secs: duration,
            waveform,
            reply,
            extra_flags,
        });
        self.voice_messages_close(cx);
        self.put_app_toast(
            "Now sending voice message... Please be patient".to_owned(),
            false,
            cx,
        );
    }

    /// `VoiceMessageModal`, while open.
    pub(in crate::ui) fn voice_messages_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (recording_since, has_blob, meta, mime, generation) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            Some((
                dialog.recording_since,
                dialog.blob.is_some(),
                dialog.meta.clone(),
                dialog.blob.as_ref().map(|blob| blob.mime.clone()),
                dialog.generation,
            ))
        })?;
        let recording = recording_since.is_some();
        let button = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .min_h(px(38.))
                .min_w(px(100.))
                .px(px(15.))
                .py(px(7.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-primary-border-default"))
                .bg(color("--control-primary-background-default"))
                .text_color(color("--control-primary-text-default"))
                .text_size(px(16.))
                .font_weight(FontWeight(500.))
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(color("--control-primary-background-hover"))
                        .text_color(color("--control-primary-text-hover"))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| match id {
                    "voice-messages-record" => this.voice_messages_toggle_recording(cx),
                    _ => this.voice_messages_choose(cx),
                }))
        };
        let buttons = div()
            .id("voice-messages-buttons")
            .flex()
            .gap(px(8.))
            .mb(px(16.))
            .child(div().flex_1().min_w(px(0.)).flex().child(button(
                "voice-messages-record",
                if recording {
                    "Stop recording"
                } else {
                    "Start recording"
                },
                cx,
            )))
            .child(div().flex_1().min_w(px(0.)).flex().child(button(
                "voice-messages-upload",
                "Upload File",
                cx,
            )))
            .child(div().flex_1())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first().cloned() {
                    this.voice_messages_read(path, cx);
                }
            }));
        let preview: AnyElement = match (&meta, has_blob && !recording) {
            (Meta::Failed(message), _) => div()
                .text_color(color("--text-feedback-critical"))
                .child(format!("Failed to parse selected audio file: {message}"))
                .into_any_element(),
            (Meta::Ready { waveform, duration }, true) => {
                let message = dc_state::Message::from_value(&json!({
                    "id": PREVIEW_MESSAGE.to_string(),
                    "channel_id": "0",
                    "type": 0,
                    "content": "",
                    "flags": 8192,
                    "author": {"id": "0", "username": "", "discriminator": "0"},
                    "timestamp": "2026-01-01T00:00:00.000000+00:00",
                    "attachments": [{
                        "id": generation.to_string(),
                        "filename": "voice-message.ogg",
                        "size": 0,
                        "url": "",
                        "proxy_url": "",
                        "duration_secs": duration,
                        "waveform": waveform,
                    }],
                    "embeds": [],
                    "mentions": [],
                    "mention_roles": [],
                    "pinned": false,
                    "mention_everyone": false,
                    "tts": false,
                }));
                match message
                    .as_ref()
                    .and_then(|message| Some((message, message.attachments.first()?)))
                {
                    Some((message, attachment)) => self.voice_pill(message, attachment),
                    None => div().into_any_element(),
                }
            }
            _ => {
                let seconds = recording_since.map_or(0, |since| since.elapsed().as_secs());
                div()
                    .h(px(48.))
                    .px(px(16.))
                    .rounded(px(24.))
                    .bg(color("--background-base-lower"))
                    .text_color(color("--text-default"))
                    .flex()
                    .items_center()
                    .child(div().size(px(16.)).rounded_full().bg(color(if recording {
                        "--status-danger"
                    } else {
                        "--control-secondary-background-default"
                    })))
                    .child(
                        div()
                            .mx(px(8.))
                            .opacity(0.8)
                            .text_size(px(12.8))
                            .font_family(super::super::MONO)
                            .child(format!("{}:{:02}", seconds / 60, seconds % 60)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_center()
                            .opacity(0.5)
                            .font_weight(FontWeight(600.))
                            .child(if recording { "RECORDING" } else { "----" }),
                    )
                    .into_any_element()
            }
        };
        let warning = mime.filter(|mime| unsupported(mime)).map(|mime| {
            div()
                .mt(px(16.))
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--background-feedback-warning"))
                .bg(color("--background-feedback-mod-warning"))
                .text_color(color("--text-feedback-warning"))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(format!(
                    "Voice Messages have to be OggOpus to be playable on iOS. This file is {mime} so it will not be \
                     playable on iOS."
                ))
                .child(
                    "To fix it, first convert it to OggOpus, for example using the convertio web converter \
                     (https://convertio.co/mp3-opus/)",
                )
        });
        let body = div()
            .flex()
            .flex_col()
            .text_size(px(16.))
            .child(buttons)
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Preview"),
            )
            .child(preview)
            .children(warning)
            .into_any_element();
        let spec = modal::Spec {
            id: "voice-messages-dialog",
            title: SharedString::from("Record Voice Message"),
            subtitle: None,
            body: Some(body),
            buttons: vec![modal::button(
                "voice-messages-send",
                "Send",
                if has_blob {
                    modal::Tone::Primary
                } else {
                    modal::Tone::PrimaryDisabled
                },
                |this, _, cx| this.voice_messages_send(cx),
            )],
            close: Box::new(|this, _, cx| this.voice_messages_close(cx)),
        };
        Some(modal::render(spec, cx))
    }
}

/// The decoded file goes into the voice player under the preview's key.
fn self_preload(root: &mut Root, generation: u64, clip: dc_voice::voice_player::Clip) {
    root.voice_hub
        .borrow_mut()
        .preload(&format!("{PREVIEW_MESSAGE}:{generation}"), Arc::new(clip));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waveform_follows_generate_waveform() {
        // Silence: every bin 0, at least 32 of them.
        let silent = vec![0f32; 48_000];
        let encoded = generate_waveform(&silent, 1.);
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(bytes.len(), 32);
        assert!(bytes.iter().all(|value| *value == 0));
        // A full-scale tone: the bins that read inside the samples are 255, the last ones run past the end.
        let loud = vec![1f32; 48_000 * 3];
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(generate_waveform(&loud, 3.))
            .unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 255);
        assert_eq!(*bytes.last().unwrap(), 0);
        assert!(
            unsupported("audio/mpeg")
                && !unsupported("audio/ogg; codecs=opus")
                && !unsupported("audio/ogg")
        );
        assert!(unsupported("audio/ogg; codecs=vorbis"));
    }
}
