//! VoiceMessageTranscriber (Equicord `src/equicordplugins/voiceMessageTranscriber.desktop`, GPL-3.0-or-later): a
//! "Transcribe Voice Message" button after a voice message's controls. Without `embed` it opens "Transcription
//! Options" (Audio Language: Auto Detect or one of Whisper's 99, searchable; Action: Transcribe or Translate to
//! English; Start), then "Voice Message Transcription" (or "…Translation"): Downloading Audio..., Processing Audio...,
//! Loading Model..., Transcribing..., the text as it grows, Show/Hide Timestamps and Copy Text, or "Transcription
//! Failed" with Retry. With `embed` the button opens a box under the player instead (with `maintainHorizontal` it
//! keeps the player's width), its status dot and title, the timestamps, copy and close buttons and Retry. The audio is
//! the player's own decoded file, taken to 16 kHz mono off the UI thread, and the transcription runs off it too. The
//! card's section deletes the model folder ("Delete all cached files (N MB)"). Not part of the pinned client.
//!
//! GAP: no speech model runner yet (`plugins/voice_message_transcriber.rs` says what remains): every transcription
//! ends in "Transcription Failed" with that reason, and `selectedModel`, `useGpu` and `quantized` are kept for the
//! runner; nothing downloads a model.

use super::super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::voice_message_transcriber::{self as api, Options, Transcript};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const ID: &str = "VoiceMessageTranscriber";
/// The modal's job, apart from the embedded boxes' (which go by their message and attachment).
const MODAL: &str = "modal";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    DownloadingAudio,
    ProcessingAudio,
    Loading,
    Transcribing,
    Complete,
    Error,
}

impl Status {
    fn heading(self) -> &'static str {
        match self {
            Status::DownloadingAudio => "Downloading Audio...",
            Status::ProcessingAudio => "Processing Audio...",
            Status::Loading => "Loading Model...",
            Status::Transcribing => "Transcribing...",
            Status::Complete => "Transcription",
            Status::Error => "Transcription Error",
        }
    }

    fn working(self) -> bool {
        !matches!(self, Status::Complete | Status::Error)
    }
}

struct Job {
    /// Bumped by each start: an older run's answers are dropped.
    run: u64,
    status: Status,
    transcript: Option<Transcript>,
    error: Option<String>,
    timestamps: bool,
}

/// What a click on a pill asked for; done on the next frame, where the view can act.
enum Action {
    Button { key: String, url: String },
    Retry(String),
    Close(String),
    Timestamps(String),
}

enum Dialog {
    Options {
        key: String,
        url: String,
        language: String,
        translate: bool,
        search: Entity<Editor>,
        _subs: Vec<Subscription>,
    },
    Transcription {
        key: String,
        url: String,
        language: String,
        translate: bool,
    },
}

#[derive(Default)]
struct State {
    jobs: HashMap<String, Job>,
    /// The embedded boxes that are open.
    open: HashSet<String>,
    pending: Vec<Action>,
    dialog: Option<Dialog>,
    runs: u64,
    /// The card's figure for the model folder, once read.
    cache: Option<u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<R>(read: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| read(&mut state.borrow_mut()))
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("VoiceMessageTranscriber")
}

fn flag(key: &str) -> bool {
    crate::plugins::with(|state| match key {
        "embed" => state.value(ID, "embed"),
        "maintainHorizontal" => state.value(ID, "maintainHorizontal"),
        _ => Value::Null,
    }) == Value::Bool(true)
}

fn options(language: Option<String>, translate: bool) -> Options {
    crate::plugins::with(|state| {
        let model = state.value(ID, "selectedModel");
        // `migrateLegacyModel`: an old model name reads as its new one.
        let model = model.as_str().unwrap_or("onnx-community/whisper-base");
        let model = api::LEGACY_MODELS
            .iter()
            .find(|(old, _)| *old == model)
            .map_or(model, |(_, new)| *new)
            .to_owned();
        Options {
            model,
            quantized: state.value(ID, "quantized") != Value::Bool(false),
            gpu: state.value(ID, "useGpu") != Value::Bool(false),
            language,
            translate,
        }
    })
}

/// The button after the player's controls (`button(src)`); `None` when the plugin is off.
pub(in crate::ui) fn transcriber_button(key: &str, url: &str, salt: usize) -> Option<AnyElement> {
    if !on() {
        return None;
    }
    let (key, url) = (key.to_owned(), url.to_owned());
    Some(
        div()
            .id(("voice-transcribe", salt))
            .size(px(24.))
            .flex_none()
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .child(icon("list_bullets", 20., "--interactive-icon-default"))
            .hover(|style| style.opacity(0.8))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                with(|state| {
                    state.pending.push(Action::Button {
                        key: key.clone(),
                        url: url.clone(),
                    })
                });
                window.refresh();
            })
            .into_any_element(),
    )
}

/// The embedded box under the player (`embed`), while open; `None` keeps the pinned player as it is.
pub(in crate::ui) fn transcriber_embed(key: &str, salt: usize) -> Option<AnyElement> {
    if !on() || !with(|state| state.open.contains(key)) || !flag("embed") {
        return None;
    }
    let (status, text, error, timestamps) = with(|state| {
        let job = state.jobs.get(key)?;
        Some((
            job.status,
            job.transcript
                .as_ref()
                .map(|transcript| api::format(transcript, job.timestamps)),
            job.error.clone(),
            job.timestamps,
        ))
    })?;
    let action = |id: &'static str, glyph: &'static str, make: fn(String) -> Action| {
        let key = key.to_owned();
        div()
            .id((id, salt))
            .size(px(24.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(color("--background-mod-normal")))
            .child(icon(glyph, 14., "--interactive-icon-default"))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                with(|state| state.pending.push(make(key.clone())));
                window.refresh();
            })
    };
    let copy_text = text.clone().filter(|text| !text.is_empty());
    let dot = color(match status {
        Status::Error => "--status-danger",
        status if status.working() => "--status-warning",
        _ => "--status-positive",
    });
    let body = if let Some(error) = error {
        let key = key.to_owned();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(color("--text-feedback-critical"))
                    .child(error),
            )
            .child(
                div()
                    .id(("voice-transcribe-retry", salt))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(color("--control-primary-background-default"))
                    .text_size(px(14.))
                    .text_color(color("--white"))
                    .child("Retry")
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        with(|state| state.pending.push(Action::Retry(key.clone())));
                        window.refresh();
                    }),
            )
            .into_any_element()
    } else if let Some(text) = text.filter(|text| !text.is_empty()) {
        div()
            .id(("voice-transcribe-text", salt))
            .max_h(px(200.))
            .overflow_y_scroll()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-default"))
            .child(text)
            .into_any_element()
    } else {
        div()
            .text_size(px(12.))
            .text_color(color("--text-muted"))
            .child(if status.working() {
                "Transcribing in progress..."
            } else {
                "Initializing..."
            })
            .into_any_element()
    };
    Some(
        div()
            .id(("voice-transcribe-embed", salt))
            .mt(px(8.))
            .when(flag("maintainHorizontal"), |this| this.max_w(px(400.)))
            .p(px(12.))
            .rounded(px(8.))
            .bg(color("--background-mod-subtle"))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(div().size(px(8.)).rounded_full().bg(dot))
                            .child(status.heading()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .when(copy_text.is_some(), |this| {
                                let copy = copy_text.clone().unwrap_or_default();
                                this.child(action(
                                    "voice-transcribe-times",
                                    if timestamps {
                                        "clips_check"
                                    } else {
                                        "list_bullets"
                                    },
                                    Action::Timestamps,
                                ))
                                .child(
                                    div()
                                        .id(("voice-transcribe-copy", salt))
                                        .size(px(24.))
                                        .rounded(px(4.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .hover(|style| style.bg(color("--background-mod-normal")))
                                        .child(icon(
                                            "clips_copy",
                                            14.,
                                            "--interactive-icon-default",
                                        ))
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                                copy.clone(),
                                            ));
                                        }),
                                )
                            })
                            .child(action("voice-transcribe-close", "x_small", Action::Close)),
                    ),
            )
            .child(body)
            .into_any_element(),
    )
}

impl Root {
    /// The plugin's dialogs, and what the pills asked for since the last frame.
    pub(in crate::ui) fn transcriber_layer(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !on() {
            return None;
        }
        let pending = with(|state| std::mem::take(&mut state.pending));
        for action in pending {
            match action {
                Action::Button { key, url } => {
                    if flag("embed") {
                        let open = with(|state| state.open.contains(&key));
                        if open {
                            with(|state| state.open.remove(&key));
                        } else {
                            with(|state| state.open.insert(key.clone()));
                            let idle = with(|state| {
                                state
                                    .jobs
                                    .get(&key)
                                    .is_none_or(|job| job.status == Status::Error)
                            });
                            if idle {
                                self.transcriber_start(key.clone(), url, options(None, false), cx);
                            }
                        }
                    } else {
                        self.transcriber_open_options(key, url, cx);
                    }
                }
                Action::Retry(key) => {
                    if let Some(url) = self.transcriber_url(&key) {
                        self.transcriber_start(key, url, options(None, false), cx);
                    }
                }
                Action::Close(key) => {
                    with(|state| state.open.remove(&key));
                }
                Action::Timestamps(key) => with(|state| {
                    if let Some(job) = state.jobs.get_mut(&key) {
                        job.timestamps = !job.timestamps;
                    }
                }),
            }
            cx.notify();
        }
        self.transcriber_dialog(window, cx)
    }

    /// The address of the voice message a key names (`<message>:<attachment>`), from the loaded messages.
    fn transcriber_url(&self, key: &str) -> Option<String> {
        let (message, attachment) = key.split_once(':')?;
        let (message, attachment) = (
            message.parse::<u64>().ok()?,
            attachment.parse::<u64>().ok()?,
        );
        self.stores
            .messages
            .values()
            .flatten()
            .find(|known| known.id == message)
            .and_then(|known| {
                known
                    .attachments
                    .iter()
                    .find(|file| file.id == attachment)
                    .map(|file| file.url.clone())
            })
    }

    fn transcriber_open_options(&self, key: String, url: String, cx: &mut Context<Self>) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        let sub = cx.subscribe(
            &search,
            |_: &mut Root, _: Entity<Editor>, event: &EditorEvent, cx| {
                if matches!(event, EditorEvent::Cancel) {
                    with(|state| state.dialog = None);
                }
                cx.notify();
            },
        );
        with(|state| {
            state.dialog = Some(Dialog::Options {
                key,
                url,
                language: "auto".to_owned(),
                translate: false,
                search,
                _subs: vec![sub],
            })
        });
    }

    /// `startTranscription`: the player's decoded file (loaded now if it is not yet), brought to 16 kHz and given to
    /// the engine, all off the UI thread; each step's status shows as it is reached.
    fn transcriber_start(
        &self,
        key: String,
        url: String,
        options: Options,
        cx: &mut Context<Self>,
    ) {
        let run = with(|state| {
            state.runs += 1;
            let run = state.runs;
            state.jobs.insert(
                key.clone(),
                Job {
                    run,
                    status: Status::DownloadingAudio,
                    transcript: None,
                    error: None,
                    timestamps: false,
                },
            );
            run
        });
        // The modal's job reads the clip of the pill it was opened from.
        let clip_key = match with(|state| match &state.dialog {
            Some(Dialog::Transcription { key: from, .. }) if key == MODAL => Some(from.clone()),
            _ => None,
        }) {
            Some(from) => from,
            None => key.clone(),
        };
        self.voice_hub.borrow_mut().request(&clip_key, &url);
        let hub = self.voice_hub.clone();
        cx.spawn(async move |this, cx| {
            let set = |cx: &mut gpui::AsyncApp, change: Box<dyn FnOnce(&mut Job)>| {
                let key = key.clone();
                let _ = this.update(cx, move |_, cx| {
                    with(|state| {
                        if let Some(job) = state.jobs.get_mut(&key).filter(|job| job.run == run) {
                            change(job);
                        }
                    });
                    cx.notify();
                });
            };
            // The player's loader runs on its own thread; this waits for it, a minute at most.
            let mut clip = None;
            for _ in 0..600 {
                match hub.borrow().load(&clip_key) {
                    Some(super::super::voice_message::Load::Ready(ready)) => {
                        clip = Some(ready);
                        break;
                    }
                    Some(super::super::voice_message::Load::Failed) => break,
                    _ => {}
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
            let Some(clip) = clip else {
                set(
                    cx,
                    Box::new(|job| {
                        job.status = Status::Error;
                        job.error = Some("Failed to download audio".to_owned());
                    }),
                );
                return;
            };
            set(cx, Box::new(|job| job.status = Status::ProcessingAudio));
            enum Step {
                Started,
                Partial(Transcript),
                Done(Result<Transcript, String>),
            }
            let (sender, mut steps) = tokio::sync::mpsc::unbounded_channel::<Step>();
            cx.background_executor()
                .spawn(async move {
                    let audio = api::to_model_rate(clip.samples());
                    let done = match api::engine() {
                        None => Err(api::NO_ENGINE.to_owned()),
                        Some(engine) => {
                            let _ = sender.send(Step::Started);
                            engine.transcribe(&audio, &options, &mut |partial| {
                                let _ = sender.send(Step::Partial(partial.clone()));
                            })
                        }
                    };
                    let _ = sender.send(Step::Done(done));
                })
                .detach();
            let mut result = Err("Transcription worker was stopped".to_owned());
            while let Some(step) = steps.recv().await {
                match step {
                    Step::Started => set(cx, Box::new(|job| job.status = Status::Loading)),
                    Step::Partial(partial) => set(
                        cx,
                        Box::new(move |job| {
                            job.status = Status::Transcribing;
                            job.transcript = Some(partial);
                        }),
                    ),
                    Step::Done(done) => {
                        result = done;
                        break;
                    }
                }
            }
            match result {
                Ok(transcript) => set(
                    cx,
                    Box::new(move |job| {
                        job.transcript = Some(transcript);
                        job.status = Status::Complete;
                    }),
                ),
                Err(error) => set(
                    cx,
                    Box::new(move |job| {
                        job.error = Some(error);
                        job.status = Status::Error;
                    }),
                ),
            }
        })
        .detach();
    }

    fn transcriber_dialog(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        enum Shown {
            Options(String, bool, Entity<Editor>),
            Transcription(bool),
        }
        let shown = with(|state| match state.dialog.as_ref()? {
            Dialog::Options {
                language,
                translate,
                search,
                ..
            } => Some(Shown::Options(language.clone(), *translate, search.clone())),
            Dialog::Transcription { translate, .. } => Some(Shown::Transcription(*translate)),
        })?;
        Some(match shown {
            Shown::Options(language, translate, search) => {
                self.transcriber_options_modal(&language, translate, search, window, cx)
            }
            Shown::Transcription(translate) => self.transcriber_modal(translate, cx),
        })
    }

    fn transcriber_options_modal(
        &self,
        language: &str,
        translate: bool,
        search: Entity<Editor>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = search.read(cx).text().trim().to_lowercase();
        let handle = gpui::Focusable::focus_handle(search.read(cx), cx);
        let codes = std::iter::once("auto").chain(api::LANGUAGES.iter().map(|(code, _)| *code));
        let rows: Vec<AnyElement> = codes
            .filter(|code| {
                query.is_empty() || api::language_label(code).to_lowercase().contains(&query)
            })
            .map(|code| {
                let chosen = code == language;
                let pick = code.to_owned();
                div()
                    .id(SharedString::from(format!("voice-transcribe-lang-{code}")))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(14.))
                    .text_color(color(if chosen {
                        "--text-strong"
                    } else {
                        "--text-default"
                    }))
                    .when(chosen, |this| this.bg(color("--background-mod-strong")))
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .child(api::language_label(code))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let pick = pick.clone();
                        with(|state| {
                            if let Some(Dialog::Options { language, .. }) = state.dialog.as_mut() {
                                *language = pick;
                            }
                        });
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let radio = |id: &'static str, label: &'static str, value: bool| {
            let selected = translate == value;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.))
                .p(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .bg(color("--background-mod-subtle"))
                .child(
                    div()
                        .size(px(20.))
                        .rounded_full()
                        .border_2()
                        .border_color(color(if selected {
                            "--control-brand-foreground"
                        } else {
                            "--interactive-text-default"
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |this| {
                            this.child(
                                div()
                                    .size(px(10.))
                                    .rounded_full()
                                    .bg(color("--control-brand-foreground")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .text_color(color("--text-default"))
                        .child(label),
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    with(|state| {
                        if let Some(Dialog::Options { translate, .. }) = state.dialog.as_mut() {
                            *translate = value;
                        }
                    });
                    cx.notify();
                }))
        };
        let heading = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .child(heading("Audio Language"))
                    .child(
                        div()
                            .cursor_text()
                            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                                window.focus(&handle, cx)
                            })
                            .h(px(40.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .child(div().flex_1().child(search)),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child(format!("Selected: {}", api::language_label(language))),
                    )
                    .child(
                        div()
                            .id("voice-transcribe-langs")
                            .mt(px(4.))
                            .h(px(160.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .child(
                div().child(heading("Action")).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(radio(
                            "voice-transcribe-task-transcribe",
                            "Transcribe",
                            false,
                        ))
                        .child(radio(
                            "voice-transcribe-task-translate",
                            "Translate to English",
                            true,
                        )),
                ),
            )
            .into_any_element();
        modal::render(
            modal::Spec {
                id: "voice-transcribe-options",
                title: SharedString::from("Transcription Options"),
                subtitle: None,
                body: Some(body),
                buttons: vec![modal::button(
                    "voice-transcribe-start",
                    "Start",
                    modal::Tone::Primary,
                    |this, _, cx| {
                        let Some((key, url, language, translate)) =
                            with(|state| match state.dialog.take() {
                                Some(Dialog::Options {
                                    key,
                                    url,
                                    language,
                                    translate,
                                    ..
                                }) => Some((key, url, language, translate)),
                                other => {
                                    state.dialog = other;
                                    None
                                }
                            })
                        else {
                            return;
                        };
                        with(|state| {
                            state.dialog = Some(Dialog::Transcription {
                                key,
                                url: url.clone(),
                                language: language.clone(),
                                translate,
                            })
                        });
                        let language = (language != "auto").then_some(language);
                        this.transcriber_start(
                            MODAL.to_owned(),
                            url,
                            options(language, translate),
                            cx,
                        );
                    },
                )],
                close: Box::new(|_, _, cx| {
                    with(|state| state.dialog = None);
                    cx.notify();
                }),
            },
            cx,
        )
    }

    fn transcriber_modal(&self, translate: bool, cx: &mut Context<Self>) -> AnyElement {
        let (status, text, error, timestamps) = with(|state| {
            state
                .jobs
                .get(MODAL)
                .map_or((Status::DownloadingAudio, None, None, false), |job| {
                    (
                        job.status,
                        job.transcript
                            .as_ref()
                            .map(|transcript| api::format(transcript, job.timestamps))
                            .filter(|text| !text.is_empty()),
                        job.error.clone(),
                        job.timestamps,
                    )
                })
        });
        let body = if let Some(error) = &error {
            div()
                .py(px(32.))
                .px(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-feedback-critical"))
                        .child("Transcription Failed"),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_center()
                        .text_color(color("--text-muted"))
                        .child(error.clone()),
                )
                .into_any_element()
        } else if let Some(text) = &text {
            div()
                .id("voice-transcribe-result")
                .max_h(px(320.))
                .overflow_y_scroll()
                .p(px(12.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-default"))
                .child(text.clone())
                .into_any_element()
        } else {
            div()
                .h(px(200.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(20.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(if status.working() {
                    status.heading()
                } else {
                    ""
                })
                .into_any_element()
        };
        let mut buttons = Vec::new();
        if error.is_some() {
            buttons.push(modal::button(
                "voice-transcribe-retry-modal",
                "Retry",
                modal::Tone::Primary,
                |this, _, cx| {
                    let Some((url, language, translate)) = with(|state| match &state.dialog {
                        Some(Dialog::Transcription {
                            url,
                            language,
                            translate,
                            ..
                        }) => Some((url.clone(), language.clone(), *translate)),
                        _ => None,
                    }) else {
                        return;
                    };
                    let language = (language != "auto").then_some(language);
                    this.transcriber_start(MODAL.to_owned(), url, options(language, translate), cx);
                },
            ));
        }
        if let Some(text) = text {
            buttons.push(modal::button(
                "voice-transcribe-timestamps",
                if timestamps {
                    "Hide Timestamps"
                } else {
                    "Show Timestamps"
                },
                modal::Tone::Secondary,
                |_, _, cx| {
                    with(|state| {
                        if let Some(job) = state.jobs.get_mut(MODAL) {
                            job.timestamps = !job.timestamps;
                        }
                    });
                    cx.notify();
                },
            ));
            buttons.push(modal::button(
                "voice-transcribe-copy-modal",
                "Copy Text",
                modal::Tone::Secondary,
                move |this, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                    this.show_success_toast("Copied!".to_owned(), cx);
                },
            ));
        }
        modal::render(
            modal::Spec {
                id: "voice-transcribe-modal",
                title: SharedString::from(if translate {
                    "Voice Message Translation"
                } else {
                    "Voice Message Transcription"
                }),
                subtitle: None,
                body: Some(body),
                buttons,
                close: Box::new(|_, _, cx| {
                    with(|state| {
                        state.dialog = None;
                        state.jobs.remove(MODAL);
                    });
                    cx.notify();
                }),
            },
            cx,
        )
    }

    /// The card's `deleteModalFiles`: "Delete all cached files (N MB)", read and emptied off the UI thread.
    pub(in crate::ui) fn transcriber_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let size = with(|state| state.cache);
        if size.is_none() {
            with(|state| state.cache = Some(0));
            cx.spawn(async move |this, cx| {
                let size = cx
                    .background_executor()
                    .spawn(async { api::cache_size() })
                    .await;
                let _ = this.update(cx, |_, cx| {
                    with(|state| state.cache = Some(size));
                    cx.notify();
                });
            })
            .detach();
        }
        let size = size.unwrap_or(0);
        let empty = size == 0;
        let model = options(None, false);
        let download = api::model_size(&model.model, model.quantized, model.gpu).unwrap_or("?");
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(format!("Model: {} ({download})", model.model)),
            )
            .child(
                div()
                    .id("voice-transcribe-delete-cache")
                    .h(px(38.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--white"))
                    .bg(color("--control-critical-primary-background-default"))
                    .when(empty, |this| this.opacity(0.5).cursor_not_allowed())
                    .when(!empty, |this| this.cursor_pointer())
                    .child(format!(
                        "Delete all cached files ({:.2} MB)",
                        size as f64 / 1024. / 1024.
                    ))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if empty {
                            return;
                        }
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .spawn(async { api::clear_cache() })
                                .await;
                            let _ = this.update(cx, |_, cx| {
                                with(|state| state.cache = Some(0));
                                cx.notify();
                            });
                        })
                        .detach();
                    })),
            )
            .into_any_element()
    }

    /// Capture only: `transcriber stand-in <text>` (a stand-in engine for the frames) and `transcriber state`.
    #[cfg(feature = "capture")]
    pub(crate) fn transcriber_script(&mut self, words: &[&str]) -> String {
        match words {
            ["stand-in", text @ ..] => {
                if let Ok(mut stand_in) = api::STAND_IN.lock() {
                    *stand_in = (!text.is_empty()).then(|| text.join(" "));
                }
                format!(
                    "transcriber: stand-in {}",
                    if text.is_empty() { "off" } else { "on" }
                )
            }
            ["state"] => with(|state| {
                let mut jobs: Vec<String> = state
                    .jobs
                    .iter()
                    .map(|(key, job)| {
                        format!(
                            "{key}={:?}:{:?}",
                            job.status,
                            job.transcript
                                .as_ref()
                                .map(|transcript| transcript.text.clone())
                                .or(job.error.clone())
                        )
                    })
                    .collect();
                jobs.sort();
                format!(
                    "transcriber: open={} jobs=[{}]",
                    state.open.len(),
                    jobs.join(", ")
                )
            }),
            _ => "transcriber: stand-in <text> | state".to_owned(),
        }
    }
}
