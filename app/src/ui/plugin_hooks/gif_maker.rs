//! GifMaker (Equicord `src/equicordplugins/gifMaker`, GPL-3.0-or-later): "Make GIF" at the end of a message's menu
//! (a message with a picture, a video or a GIF) and of the Image Actions menu, and "Edit GIF" in the GIF picker's
//! menu, open "Make GIF": the GIF as it will be sent, Captions (None, Caption with its text and font size, Bubble
//! with its tip placed by a click on the preview and its Tip Base), and Dimensions (Width, Height, 32 to 1024).
//! Send puts `export.gif` in the open channel's message box as an upload (`promptToUpload`) and closes; Export saves
//! it into Downloads. Each change is stored as the original's hidden settings (`lastWidth`, ...). Not part of the
//! pinned client.
//!
//! The GIF is made off the UI thread (`plugins/gif_maker.rs`) 300ms after the last change, as the original's
//! debounced `createGif`; a newer change stops the one running. The media is the full file when it loads, else the
//! picture the chat already shows. The caption is set in the app's own font (gg sans) in place of the original's
//! Google fonts.

use super::super::{Root, color, modal};
use crate::editor::{Editor, EditorEvent};
use crate::plugins::gif_maker::{self as maker, Media, Mode, Options};
use dc_state::Message;
use gpui::{
    AnyElement, Bounds, Context, Entity, FontWeight, IntoElement, MouseButton, ParentElement,
    Pixels, SharedString, Styled, Subscription, canvas, div, img, prelude::*, px,
};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const ID: &str = "GifMaker";
const FILE_NAME: &str = "export.gif";
/// The original's 300ms wait after the last change.
const SETTLE_MS: u64 = 300;

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

fn number_of(state: &crate::plugins::State, key: &str) -> f64 {
    state.value(ID, key).as_f64().unwrap_or_default()
}

/// The media a dialog is opened on: what to fetch and the picture already loaded to use meanwhile.
#[derive(Clone, Debug)]
struct Opened {
    /// The full file's picture key.
    full: String,
    /// The chat's or the picker's picture of it, already loaded.
    shown: Option<String>,
}

struct Dialog {
    opened: Opened,
    options: Options,
    text: Entity<Editor>,
    width: Entity<Editor>,
    height: Entity<Editor>,
    _typing: Vec<Subscription>,
    /// The size is still the stored one, to be fitted to the media once its size is known (the original's
    /// auto-detect).
    fit_pending: bool,
    /// The GIF as last made: the file and its preview.
    file: Option<(Arc<Vec<u8>>, Arc<gpui::Image>)>,
    error: Option<String>,
    /// What the last GIF was asked with, so a frame asks again only when something changed.
    asked: Option<(Options, String)>,
    generating: bool,
}

/// A GIF picker result: its picture's address, its picture key and its size.
type Picked = (String, String, (u32, u32));

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
    /// The GIF picker result whose menu is open: its picture's address, key and size.
    static PICKER: RefCell<Option<Picked>> = const { RefCell::new(None) };
    /// Where the preview was painted, for the bubble's tip.
    static PREVIEW: Cell<Option<Bounds<Pixels>>> = const { Cell::new(None) };
}

/// The latest request; a running one whose number is older stops.
static LATEST: AtomicU64 = AtomicU64::new(0);

/// `folder/name`, or `name (1)`, `name (2)`, ... when that is taken.
fn free_name(folder: &std::path::Path, name: &str) -> std::path::PathBuf {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut target = folder.join(name);
    let mut copy = 1;
    while target.exists() {
        target = folder.join(format!("{stem} ({copy}).{extension}"));
        copy += 1;
    }
    target
}

/// The GIF as a file in a fresh temporary folder, for the upload to read.
fn scratch_file(bytes: &[u8]) -> Option<std::path::PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let dir = std::env::temp_dir()
        .join("gravecord-gifmaker")
        .join(nanos.to_string());
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// The picture key the chat drew a message's media under, when it is the message's first attachment.
fn shown_key(message: &Message, media: &Media) -> Option<String> {
    message
        .attachments
        .iter()
        .find(|attachment| attachment.proxy_url == media.url)
        .map(|attachment| format!("attachment:{}", attachment.id))
}

/// Makes the GIF with the app's own font; a newer request than `generation` stops it.
fn make_gif(image: &gpui::Image, options: &Options, generation: u64) -> Result<Vec<u8>, String> {
    let faces = crate::fonts::load_cached().ok_or("Font not loaded")?;
    let face = crate::plugins::quoter::Face::parse(faces.first().ok_or("Font not loaded")?)
        .ok_or("Font not loaded")?;
    let stale = || LATEST.load(Ordering::Relaxed) != generation;
    maker::make(image.bytes(), &face, options, &stale)
}

/// A GIF picker result's menu opened: what Edit GIF works on.
pub(in crate::ui) fn gif_maker_picker(url: &str, key: String, size: (f32, f32)) {
    PICKER.with(|picker| {
        *picker.borrow_mut() = Some((url.to_owned(), key, (size.0 as u32, size.1 as u32)));
    });
}

impl Root {
    /// The message menu's "Make GIF", in a group of its own at the end (`children.push`), for a message with media.
    pub(in crate::ui) fn gif_maker_items(
        &self,
        items: &mut Vec<super::super::menu::Item>,
        message: dc_state::Id,
    ) {
        use super::super::menu::{Action, Item};
        if !on() {
            return;
        }
        if self
            .menu_message(message)
            .and_then(maker::media_of_message)
            .is_none()
        {
            return;
        }
        crate::plugins::note(ID);
        if !items.is_empty() && !matches!(items.last(), Some(Item::Separator)) {
            items.push(Item::Separator);
        }
        items.push(Item::Entry("Make GIF", "gif", Action::PluginMakeGif, false));
    }

    /// The rows of the Image Actions menu ("Make GIF", for a picture or video link) and of the GIF picker's ("Edit
    /// GIF").
    pub(in crate::ui) fn with_gif_maker_row(
        &self,
        mut items: Vec<super::super::context_menu::Entry>,
        target: super::super::context_menu::Target,
    ) -> Vec<super::super::context_menu::Entry> {
        use super::super::context_menu::{Act, Entry, Row, Target};
        if !on() {
            return items;
        }
        let label = match target {
            Target::Image => {
                let linked = self
                    .context
                    .image_target
                    .as_ref()
                    .and_then(|(url, _)| maker::media_of_link(url));
                if linked.is_none() {
                    return items;
                }
                "Make GIF"
            }
            Target::PickerGif => "Edit GIF",
            _ => return items,
        };
        crate::plugins::note(ID);
        if !items.is_empty() && !matches!(items.last(), Some(Entry::Separator)) {
            items.push(Entry::Separator);
        }
        items.push(Entry::Row(Row::new(Act::PluginMakeGif, label).icon("gif")));
        items
    }

    /// "Make GIF" of the message menu.
    pub(in crate::ui) fn gif_maker_open_message(
        &mut self,
        message: &Message,
        cx: &mut Context<Self>,
    ) {
        let Some(media) = maker::media_of_message(message) else {
            return;
        };
        let shown = shown_key(message, &media);
        self.gif_maker_open(media, shown, cx);
    }

    /// "Make GIF" of the Image Actions menu, or "Edit GIF" of the GIF picker's.
    pub(in crate::ui) fn gif_maker_open_target(
        &mut self,
        target: super::super::context_menu::Target,
        cx: &mut Context<Self>,
    ) {
        use super::super::context_menu::Target;
        match target {
            Target::Image => {
                let Some((url, key)) = self.context.image_target.clone() else {
                    return;
                };
                if let Some(media) = maker::media_of_link(&url) {
                    self.gif_maker_open(media, key, cx);
                }
            }
            Target::PickerGif => {
                let Some((url, key, size)) = PICKER.with(|picker| picker.borrow().clone()) else {
                    return;
                };
                let media = Media {
                    video: maker::media_of_link(&url).is_some_and(|media| media.video),
                    url,
                    size: Some(size),
                };
                // The dialog takes the picker's place, as the original's modal does.
                self.picker = None;
                self.gif_maker_open(media, Some(key), cx);
            }
            _ => {}
        }
    }

    fn gif_maker_open(&mut self, media: Media, shown: Option<String>, cx: &mut Context<Self>) {
        if !on() {
            return;
        }
        let max = (
            crate::plugins::with(|state| number_of(state, "maxWidth")) as u32,
            crate::plugins::with(|state| number_of(state, "maxHeight")) as u32,
        );
        let stored = (
            crate::plugins::with(|state| number_of(state, "lastWidth")) as u32,
            crate::plugins::with(|state| number_of(state, "lastHeight")) as u32,
        );
        let (width, height) = maker::initial_size(max, media.size, stored);
        let options = Options {
            width: maker::clamp_side(f64::from(width)),
            height: maker::clamp_side(f64::from(height)),
            mode: Mode::from_key(&crate::plugins::with(|state| {
                state
                    .value(ID, "lastCaptionMode")
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })),
            text: crate::plugins::with(|state| {
                state
                    .value(ID, "lastCaptionText")
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            }),
            size: crate::plugins::with(|state| number_of(state, "lastCaptionSize")) as f32,
            tip: (80., 80.),
            tip_base: crate::plugins::with(|state| number_of(state, "lastBubbleTipBase")) as f32,
        };
        let line = |text: String, placeholder: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut editor = Editor::new(super::super::editor_style(), cx);
                editor.set_single_line(true);
                editor.set_placeholder(placeholder.to_owned());
                editor.set_text(&text);
                editor
            })
        };
        let text = line(options.text.clone(), "Enter caption...", cx);
        let width_box = line(options.width.to_string(), "", cx);
        let height_box = line(options.height.to_string(), "", cx);
        let typing = vec![
            cx.subscribe(&text, |_, editor, _: &EditorEvent, cx| {
                let value = editor.read(cx).text().to_owned();
                Self::gif_maker_patch(|options| options.text = value);
                cx.notify();
            }),
            cx.subscribe(&width_box, |_, editor, event: &EditorEvent, cx| {
                Self::gif_maker_side(editor, event, true, cx);
            }),
            cx.subscribe(&height_box, |_, editor, event: &EditorEvent, cx| {
                Self::gif_maker_side(editor, event, false, cx);
            }),
        ];
        let full = format!("gifmaker:{}", media.url);
        self.want_image(full.clone(), media.url.clone(), cx);
        let fit_pending = media.size.is_none();
        crate::plugins::note(ID);
        DIALOG.with(|dialog| {
            *dialog.borrow_mut() = Some(Dialog {
                opened: Opened { full, shown },
                options,
                text,
                width: width_box,
                height: height_box,
                _typing: typing,
                fit_pending,
                file: None,
                error: None,
                asked: None,
                generating: false,
            })
        });
        cx.notify();
    }

    /// A Width or Height box changed (`onChange`), or Enter was pressed in it (`onBlur`'s clamp).
    fn gif_maker_side(
        editor: Entity<Editor>,
        event: &EditorEvent,
        width: bool,
        cx: &mut Context<Self>,
    ) {
        let digits: String = editor
            .read(cx)
            .text()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let side = maker::clamp_side(digits.parse::<f64>().unwrap_or(0.));
        if matches!(event, EditorEvent::Submit(_)) || digits != editor.read(cx).text() {
            editor.update(cx, |editor, _| editor.set_text(&side.to_string()));
        }
        Self::gif_maker_patch(|options| {
            if width {
                options.width = side;
            } else {
                options.height = side;
            }
        });
        cx.notify();
    }

    /// `patch`: changes the options and stores them as the original's hidden settings.
    fn gif_maker_patch(change: impl FnOnce(&mut Options)) {
        let Some(options) = DIALOG.with(|dialog| {
            let mut dialog = dialog.borrow_mut();
            let dialog = dialog.as_mut()?;
            change(&mut dialog.options);
            Some(dialog.options.clone())
        }) else {
            return;
        };
        crate::plugins::update(|state| {
            state.set_value(ID, "lastWidth", &Value::from(options.width));
            state.set_value(ID, "lastHeight", &Value::from(options.height));
            state.set_value(ID, "lastCaptionMode", &Value::from(options.mode.key()));
            state.set_value(ID, "lastCaptionText", &Value::from(options.text.clone()));
            state.set_value(ID, "lastCaptionSize", &Value::from(f64::from(options.size)));
            state.set_value(
                ID,
                "lastBubbleTipBase",
                &Value::from(f64::from(options.tip_base)),
            );
        });
    }

    fn gif_maker_close(cx: &mut Context<Self>) {
        LATEST.fetch_add(1, Ordering::Relaxed);
        DIALOG.with(|dialog| *dialog.borrow_mut() = None);
        cx.notify();
    }

    /// The bytes to make the GIF from: the full file once loaded, else the picture already shown.
    fn gif_maker_source(&self, opened: &Opened) -> Option<(String, Arc<gpui::Image>)> {
        std::iter::once(&opened.full)
            .chain(opened.shown.as_ref())
            .find_map(|key| {
                self.images
                    .get(key)
                    .map(|image| (key.clone(), image.clone()))
            })
    }

    /// Asks for a GIF when the options or the source changed since the last one.
    fn gif_maker_make(&self, cx: &mut Context<Self>) {
        let Some((opened, mut options, asked, fit_pending)) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            Some((
                dialog.opened.clone(),
                dialog.options.clone(),
                dialog.asked.clone(),
                dialog.fit_pending,
            ))
        }) else {
            return;
        };
        let Some((key, image)) = self.gif_maker_source(&opened) else {
            return;
        };
        if fit_pending {
            // The original's auto-detect: a picture's own size, fitted to the maximum.
            let detected = if maker::sniff(image.bytes()) == maker::Source::Mp4 {
                crate::video_clip::mp4_first_frame_rgba(image.bytes())
                    .map(|frame| frame.dimensions())
            } else {
                image::ImageReader::new(std::io::Cursor::new(image.bytes()))
                    .with_guessed_format()
                    .ok()
                    .and_then(|reader| reader.into_dimensions().ok())
            };
            if let Some(size) = detected {
                let max = (
                    crate::plugins::with(|state| number_of(state, "maxWidth")) as u32,
                    crate::plugins::with(|state| number_of(state, "maxHeight")) as u32,
                );
                let (width, height) =
                    maker::initial_size(max, Some(size), (options.width, options.height));
                options.width = maker::clamp_side(f64::from(width));
                options.height = maker::clamp_side(f64::from(height));
            }
            DIALOG.with(|dialog| {
                if let Some(dialog) = dialog.borrow_mut().as_mut() {
                    dialog.fit_pending = false;
                    dialog.options.width = options.width;
                    dialog.options.height = options.height;
                    let (w, h) = (options.width.to_string(), options.height.to_string());
                    dialog.width.update(cx, |editor, _| editor.set_text(&w));
                    dialog.height.update(cx, |editor, _| editor.set_text(&h));
                }
            });
        }
        let wanted = (options.clone(), key);
        if asked.as_ref() == Some(&wanted) {
            return;
        }
        let generation = LATEST.fetch_add(1, Ordering::Relaxed) + 1;
        DIALOG.with(|dialog| {
            if let Some(dialog) = dialog.borrow_mut().as_mut() {
                dialog.asked = Some(wanted);
                dialog.generating = true;
            }
        });
        let bytes = image.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(SETTLE_MS))
                .await;
            if LATEST.load(Ordering::Relaxed) != generation {
                return;
            }
            let made = cx
                .background_executor()
                .spawn(async move { make_gif(&bytes, &options, generation) })
                .await;
            let _ = this.update(cx, |_, cx| {
                if LATEST.load(Ordering::Relaxed) != generation {
                    return;
                }
                DIALOG.with(|dialog| {
                    let mut dialog = dialog.borrow_mut();
                    let Some(dialog) = dialog.as_mut() else {
                        return;
                    };
                    dialog.generating = false;
                    match made {
                        Ok(gif) => {
                            dialog.error = None;
                            let preview = Arc::new(gpui::Image::from_bytes(
                                gpui::ImageFormat::Gif,
                                gif.clone(),
                            ));
                            dialog.file = Some((Arc::new(gif), preview));
                        }
                        Err(error) if maker::is_cancel(&error) => {}
                        Err(error) => dialog.error = Some(error),
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn gif_maker_file() -> Option<Arc<Vec<u8>>> {
        DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            dialog
                .as_ref()?
                .file
                .as_ref()
                .map(|(bytes, _)| bytes.clone())
        })
    }

    /// Export: the GIF into Downloads under a free name (a capture run that may not write keeps it in the log).
    fn gif_maker_export(&mut self, cx: &mut Context<Self>) {
        let Some(bytes) = Self::gif_maker_file() else {
            return;
        };
        super::super::actions::log_effect("GifMaker Export", FILE_NAME, bytes.len());
        if crate::inert::on() {
            return;
        }
        let saved = std::env::var_os("USERPROFILE").and_then(|home| {
            let folder = std::path::PathBuf::from(home).join("Downloads");
            std::fs::create_dir_all(&folder).ok()?;
            let target = free_name(&folder, FILE_NAME);
            std::fs::write(&target, &bytes[..]).ok()?;
            Some(target.file_name()?.to_string_lossy().into_owned())
        });
        match saved {
            Some(saved) => self.show_success_toast(format!("Saved {saved} to Downloads"), cx),
            None => self.put_app_toast(format!("Unable to download {FILE_NAME}"), false, cx),
        }
    }

    /// Send: the GIF becomes an upload of the open channel's message box, and the dialog closes.
    fn gif_maker_send(&mut self, cx: &mut Context<Self>) {
        let Some(bytes) = Self::gif_maker_file() else {
            return;
        };
        if self.channel.is_none() {
            return;
        }
        super::super::actions::log_effect("GifMaker Send", FILE_NAME, bytes.len());
        let Some(path) = scratch_file(&bytes) else {
            return;
        };
        self.attach_files(vec![path], cx);
        Self::gif_maker_close(cx);
    }

    /// A small button of the Captions row (`size="min"`, primary when chosen).
    fn gif_maker_mode_button(mode: Mode, chosen: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id(SharedString::from(format!("gifmaker-mode-{}", mode.key())))
            .h(px(32.))
            .px(px(12.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .font_weight(FontWeight(500.))
            .when(chosen, |this| {
                this.bg(color("--control-primary-background-default"))
                    .text_color(color("--control-primary-text-default"))
                    .hover(|style| style.bg(color("--control-primary-background-hover")))
            })
            .when(!chosen, |this| {
                this.bg(color("--control-secondary-background-default"))
                    .border_1()
                    .border_color(color("--control-secondary-border-default"))
                    .text_color(color("--control-secondary-text-default"))
                    .hover(|style| style.bg(color("--control-secondary-background-hover")))
            })
            .on_click(cx.listener(move |_, _, _, cx| {
                Self::gif_maker_patch(|options| options.mode = mode);
                cx.notify();
            }))
            .child(mode.name())
            .into_any_element()
    }

    /// The dialog, while it is open.
    pub(in crate::ui) fn gif_maker_dialog(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = DIALOG.with(|dialog| dialog.borrow().is_some());
        if !open {
            return None;
        }
        if !on() {
            Self::gif_maker_close(cx);
            return None;
        }
        self.gif_maker_make(cx);
        let (options, text, width, height, preview, generating, error) = DIALOG.with(|dialog| {
            let dialog = dialog.borrow();
            let dialog = dialog.as_ref()?;
            Some((
                dialog.options.clone(),
                dialog.text.clone(),
                dialog.width.clone(),
                dialog.height.clone(),
                dialog.file.as_ref().map(|(_, preview)| preview.clone()),
                dialog.generating,
                dialog.error.clone(),
            ))
        })?;
        let input = |editor: &Entity<Editor>, cx: &mut Context<Self>| {
            let focus = gpui::Focusable::focus_handle(editor.read(cx), cx);
            div()
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&focus, cx);
                })
                .min_h(px(40.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .text_color(color("--text-default"))
                .text_size(px(16.))
                .child(editor.clone())
        };
        let heading = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-subtle"))
                .child(text.to_uppercase())
        };
        let label = |text: &'static str| {
            div()
                .mb(px(4.))
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let bubble = options.mode == Mode::Bubble;
        let gif_size = (options.width as f32, options.height as f32);
        // `preview-section`: the GIF at its own size, scaled down to fit, with its overlays.
        let mut frame = div()
            .id("gifmaker-preview")
            .relative()
            .flex_none()
            .min_h(px(120.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .overflow_hidden()
            .bg(color("--background-base-lowest"));
        if let Some(preview) = preview {
            let ratio = gif_size.1 / gif_size.0.max(1.);
            let shown_width = gif_size.0.min(360.);
            frame = frame.child(
                div()
                    .relative()
                    .w(px(shown_width))
                    .h(px(shown_width * ratio))
                    .when(bubble, |this| this.cursor_crosshair())
                    .child(
                        img(preview)
                            .size_full()
                            .opacity(if generating { 0.5 } else { 1. }),
                    )
                    .child(
                        canvas(
                            |bounds: Bounds<Pixels>, _, _| {
                                PREVIEW.with(|cell| cell.set(Some(bounds)))
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    ),
            );
        }
        if bubble {
            // `handlePreviewClick`: the tip goes where the preview was clicked, in the GIF's pixels.
            frame = frame.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, event: &gpui::MouseDownEvent, _, cx| {
                    let Some(bounds) = PREVIEW.with(Cell::get) else {
                        return;
                    };
                    if !bounds.contains(&event.position) {
                        return;
                    }
                    let x = f32::from(event.position.x - bounds.origin.x)
                        / f32::from(bounds.size.width);
                    let y = f32::from(event.position.y - bounds.origin.y)
                        / f32::from(bounds.size.height);
                    Self::gif_maker_patch(|options| {
                        options.tip = (x * options.width as f32, y * options.height as f32);
                    });
                    cx.notify();
                }),
            );
        }
        let overlay = |text: String, tint: &'static str| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(16.))
                .text_size(px(14.))
                .font_weight(FontWeight(600.))
                .text_color(color(tint))
                .child(text)
        };
        if generating {
            frame = frame.child(overlay("Generating GIF...".into(), "--text-strong"));
        } else if let Some(error) = error {
            frame = frame.child(overlay(error, "--text-feedback-critical"));
        }
        let mut controls = div().flex().flex_col().child(heading("Captions")).child(
            div().flex().gap(px(8.)).mb(px(16.)).children(
                Mode::ALL.map(|mode| Self::gif_maker_mode_button(mode, mode == options.mode, cx)),
            ),
        );
        let slider = |this: &Self,
                      id: &'static str,
                      label: &'static str,
                      min: u32,
                      max: u32,
                      value: u32,
                      format: std::rc::Rc<dyn Fn(u32) -> String>,
                      apply: fn(&mut Options, u32),
                      cx: &mut Context<Self>| {
            this.menu_slider_row(
                super::super::menu_slider::MenuSliderConfig {
                    id: id.into(),
                    label: label.to_owned(),
                    min,
                    max,
                    value,
                    palette: super::super::menu_slider::ThemePalette::Default,
                    format,
                    on_change: std::rc::Rc::new(move |_, value, cx| {
                        Self::gif_maker_patch(|options| apply(options, value));
                        cx.notify();
                    }),
                },
                cx,
            )
        };
        match options.mode {
            Mode::Caption => {
                controls = controls
                    .child(label("Text"))
                    .child(div().mb(px(16.)).child(input(&text, cx)))
                    .child(slider(
                        self,
                        "gifmaker-font-size",
                        "Font size",
                        10,
                        100,
                        options.size.round() as u32,
                        std::rc::Rc::new(|value| format!("{value}.00px")),
                        |options, value| options.size = value as f32,
                        cx,
                    ));
            }
            Mode::Bubble => {
                controls = controls
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(14.))
                            .text_color(color("--text-subtle"))
                            .child("Click on the preview to position the bubble tip"),
                    )
                    .child(slider(
                        self,
                        "gifmaker-tip-base",
                        "Tip Base",
                        0,
                        80,
                        (options.tip_base * 100.).round() as u32,
                        std::rc::Rc::new(|value| format!("{value}%")),
                        |options, value| options.tip_base = value as f32 / 100.,
                        cx,
                    ));
            }
            Mode::None => {}
        }
        controls = controls
            .child(div().mt(px(16.)).child(heading("Dimensions")))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .child(label("Width"))
                            .child(input(&width, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(label("Height"))
                            .child(input(&height, cx)),
                    ),
            );
        let tall = (f32::from(window.viewport_size().height) - 220.).max(200.);
        let body = div()
            .id("gifmaker-body")
            .max_h(px(tall))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(frame)
            .child(controls)
            .into_any_element();
        let spec = modal::Spec {
            id: "gifmaker-dialog",
            title: SharedString::from("Make GIF"),
            subtitle: None,
            body: Some(body),
            buttons: vec![
                modal::button(
                    "gifmaker-send",
                    "Send",
                    modal::Tone::Primary,
                    |this, _, cx| this.gif_maker_send(cx),
                ),
                modal::button(
                    "gifmaker-export",
                    "Export",
                    modal::Tone::Primary,
                    |this, _, cx| this.gif_maker_export(cx),
                ),
            ],
            close: Box::new(|_, _, cx| Self::gif_maker_close(cx)),
        };
        Some(modal::render_large(spec, cx))
    }
}
