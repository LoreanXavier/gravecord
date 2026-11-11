//! The clip editor (modules 923904 the provider, 423699 the timeline, 701266 and 657766 the panes, 590146 the menus).
//! It plays the file silently on the player thread, trims by two handles, keeps the title and the audio switches and
//! writes them into the clip's document when it closes; the original video is not rewritten, as in the official client
//! (`editMetadata`, module 489496 `Yy`). The drawing is in `editor_view.rs`.
use super::super::{Root, editor_style};
use super::modal_size;
use super::model::{self, Edit};
use super::player::{self, Command, Player};
use crate::editor::{Editor as Field, EditorEvent};
use dc_clips::Clip;
use gpui::{
    AppContext as _, Bounds, Context, Entity, Pixels, Point, RenderImage, Subscription, Window,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// What a pointer drag moves (module 423699: the playhead, or the crop handle `start` / `end`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Drag {
    Playhead,
    Start,
    End,
}

/// The widths and heights the editor's boxes get at a modal size (stylesheets YPRx7I, wUYaqe, RHyl8o, wRblFq).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Layout {
    pub left: f32,
    pub video: (f32, f32),
    pub strip: f32,
}

/// The sidebar `clipForm` is 340 wide.
pub(super) const SIDEBAR: f32 = 340.;
/// Header 16 + 24 + 6 + 16 (title `heading-lg` line, gap, meta row).
pub(super) const HEADER: f32 = 62.;
/// Timeline card: 46 header, 16 padding, 24 ruler, 56 strip, 16 padding, 1 px borders.
pub(super) const CARD: f32 = 46. + 16. + 24. + 56. + 16. + 2.;
pub(super) const STRIP_HEIGHT: f32 = 56.;

pub(super) fn layout(modal: (f32, f32), aspect: f32) -> Layout {
    let left = modal.0 - SIDEBAR;
    let room = (left - 32., modal.1 - HEADER - CARD - 16. - 32.);
    let aspect = if aspect > 0. { aspect } else { 16. / 9. };
    let (width, height) = if room.0 / room.1.max(1.) > aspect {
        (room.1 * aspect, room.1)
    } else {
        (room.0, room.0 / aspect)
    };
    Layout {
        left,
        video: (width.max(1.).floor(), height.max(1.).floor()),
        // Card margin 16 a side, border 1, padding 16.
        strip: left - 32. - 2. - 32.,
    }
}

/// The strip's cells: each `ceil(56 * aspect)` wide, enough to fill the strip (module 923904).
pub(super) fn thumb_cells(strip: f32, aspect: f32) -> (u32, usize) {
    let width = (STRIP_HEIGHT * aspect).ceil().max(1.) as u32;
    (width, (strip / width as f32).ceil().max(1.) as usize)
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

pub(in crate::ui) struct Editor {
    pub clip: Clip,
    pub edit: Edit,
    pub preset: String,
    pub player: Player,
    /// Tells a polling task which editor it belongs to.
    pub generation: u64,
    serial: u64,
    pub position: f64,
    pub playing: bool,
    resume: bool,
    drag: Option<Drag>,
    pub focus: Option<Drag>,
    loaded: bool,
    pub failed: Option<String>,
    pub frame: Option<Arc<RenderImage>>,
    pub thumbs: Vec<Option<Arc<RenderImage>>>,
    thumbs_asked: Option<(u32, usize, u32)>,
    sent_size: (u32, u32),
    pub strip: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub scale: Cell<f32>,
    pub title: Entity<Field>,
    pub title_editing: bool,
    /// The modal's own focus, so keys reach it once the title field is done with them (a focused field takes
    /// Escape and Enter as its own actions before any key listener).
    pub shell: gpui::FocusHandle,
    pub refocus: Cell<bool>,
    _title_events: Subscription,
}

impl Editor {
    pub(super) fn open(clip: &Clip, cx: &mut Context<Root>) -> Self {
        let duration = clip.meta.length as f64 / 1000.;
        let (edit, position) = Edit::of(&clip.meta, duration);
        let preset = clip
            .meta
            .edit_metadata
            .as_ref()
            .and_then(|edit| edit.get("crop"))
            .and_then(|crop| crop.get("preset"))
            .and_then(|preset| preset.as_str())
            .unwrap_or("original")
            .to_owned();
        let title = cx.new(|cx| {
            let mut field = Field::new(editor_style(), cx);
            field.set_keeps_text(true);
            field.set_reports_all_edits(true);
            field.set_single_line(true);
            field.set_max_utf16(200);
            field
        });
        let events = cx.subscribe(&title, |this, field, event: &EditorEvent, cx| {
            this.clips_title_event(&field, event, cx);
        });
        Self {
            player: Player::open(&clip.filepath, (1280, 720)),
            clip: clip.clone(),
            edit,
            preset,
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            serial: 1,
            position,
            playing: false,
            resume: false,
            drag: None,
            focus: None,
            loaded: false,
            failed: None,
            frame: None,
            thumbs: Vec::new(),
            thumbs_asked: None,
            sent_size: (0, 0),
            strip: Rc::new(Cell::new(None)),
            scale: Cell::new(1.),
            title,
            title_editing: false,
            shell: cx.focus_handle(),
            refocus: Cell::new(true),
            _title_events: events,
        }
    }

    pub(super) fn duration(&self) -> f64 {
        self.edit.crop.duration
    }

    pub(super) fn aspect(&self) -> f32 {
        match self.player.shared.loaded.get() {
            Some(loaded) if loaded.height > 0 => loaded.width as f32 / loaded.height as f32,
            _ => 16. / 9.,
        }
    }

    /// Shows the picture at `seconds`: the playhead moves and the player is asked for it.
    pub(super) fn seek(&mut self, seconds: f64) {
        self.position = self.edit.crop.keep_inside(seconds);
        self.serial += 1;
        self.player.send(Command::Seek {
            ms: (self.position * 1000.).round() as u64,
            serial: self.serial,
        });
    }

    pub(super) fn play(&mut self) {
        let crop = self.edit.crop;
        // Playing outside the range, or from its very end, starts at the start (module 923904 `play`).
        if self.position < crop.start || self.position >= crop.end - 0.001 {
            self.position = crop.start;
        }
        self.serial += 1;
        self.playing = true;
        self.player.send(Command::Play {
            from: (self.position * 1000.).round() as u64,
            to: (crop.end * 1000.).round() as u64,
            serial: self.serial,
        });
    }

    pub(super) fn pause(&mut self) {
        if self.playing {
            self.playing = false;
            self.player.send(Command::Pause);
        }
    }

    pub(super) fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// The skip buttons: ten seconds back or forward inside the range.
    pub(super) fn skip(&mut self, seconds: f64) {
        let was = self.playing;
        self.pause();
        self.seek(self.position + seconds);
        if was {
            self.play();
        }
    }

    /// A drag, or a click, moved the pointer to `x`.
    pub(super) fn drag_to(&mut self, x: f32) {
        let (Some(drag), Some(bounds)) = (self.drag, self.strip.get()) else {
            return;
        };
        let time = model::time_at(
            x,
            f32::from(bounds.origin.x),
            f32::from(bounds.size.width),
            self.duration(),
        );
        match drag {
            Drag::Playhead => self.seek(time),
            Drag::Start => {
                let start = self.edit.crop.set_start(time);
                self.seek(start);
            }
            Drag::End => {
                let end = self.edit.crop.set_end(time);
                self.seek(end);
            }
        }
    }

    pub(super) fn begin_drag(&mut self, drag: Drag, x: f32) {
        self.resume = self.playing;
        self.pause();
        self.drag = Some(drag);
        self.focus = (drag != Drag::Playhead).then_some(drag);
        self.drag_to(x);
    }

    /// An arrow key: the focused handle or the playhead moves by a step (Shift: a tenth of it).
    fn nudge(&mut self, direction: f64, shift: bool) {
        let step = direction * model::key_step(self.duration(), shift);
        match self.focus {
            Some(Drag::Start) => {
                let start = self.edit.crop.set_start(self.edit.crop.start + step);
                self.seek(start);
            }
            Some(Drag::End) => {
                let end = self.edit.crop.set_end(self.edit.crop.end + step);
                self.seek(end);
            }
            _ => self.skip(step),
        }
    }

    /// Takes what the player thread has produced. True when something changed.
    fn poll(&mut self, cx: &mut Context<Root>) -> bool {
        let mut changed = false;
        let shared = self.player.shared.clone();
        if !self.loaded {
            if let Some(loaded) = shared.loaded.get() {
                self.loaded = true;
                let duration = loaded.duration_ms as f64 / 1000.;
                if (duration - self.duration()).abs() > 0.001 {
                    let (edit, position) = Edit::of(&self.clip.meta, duration);
                    self.edit.crop = edit.crop;
                    self.position = position;
                }
                self.seek(self.position);
                changed = true;
            } else if let Some(error) = shared.failed.lock().ok().and_then(|failed| failed.clone())
                && self.failed.as_deref() != Some(error.as_str())
            {
                self.failed = Some(error);
                changed = true;
            }
        }
        let frame = shared.frame.lock().ok().and_then(|mut frame| frame.take());
        if let Some(frame) = frame
            && frame.serial == self.serial
            && let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
        {
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
            if let Some(old) = self.frame.replace(image) {
                cx.drop_image(old, None);
            }
            if self.playing {
                self.position = frame.ms as f64 / 1000.;
            }
            changed = true;
        }
        let thumbs: Vec<player::Thumb> = shared
            .thumbs
            .lock()
            .map(|mut thumbs| std::mem::take(&mut *thumbs))
            .unwrap_or_default();
        for thumb in thumbs {
            if thumb.slot < self.thumbs.len()
                && let Some(buffer) =
                    image::RgbaImage::from_raw(thumb.width, thumb.height, thumb.bgra)
            {
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                if let Some(old) = self.thumbs[thumb.slot].replace(image) {
                    cx.drop_image(old, None);
                }
                changed = true;
            }
        }
        if shared.ended.swap(false, Ordering::AcqRel) && self.playing {
            self.playing = false;
            self.position = self.edit.crop.end;
            changed = true;
        }
        changed
    }

    /// Tells the player the size pictures are drawn at and asks for the strip's cells once both are known.
    fn sizes(&mut self, modal: (f32, f32)) {
        if !self.loaded {
            return;
        }
        let scale = self.scale.get();
        let aspect = self.aspect();
        let layout = layout(modal, aspect);
        let size = (
            (layout.video.0 * scale).ceil() as u32,
            (layout.video.1 * scale).ceil() as u32,
        );
        if size != self.sent_size {
            self.sent_size = size;
            self.player.send(Command::Size {
                width: size.0,
                height: size.1,
            });
            // The picture on screen is of the old size: ask for it again.
            if !self.playing {
                self.seek(self.position);
            }
        }
        let (width, count) = thumb_cells(layout.strip, aspect);
        let height = (STRIP_HEIGHT * scale).ceil() as u32;
        if self.thumbs_asked != Some((width, count, height)) {
            self.thumbs_asked = Some((width, count, height));
            self.thumbs.resize(count, None);
            let duration_ms = self.duration() * 1000.;
            let times = (0..count)
                .map(|index| {
                    ((index as f64 + 0.5) * f64::from(width) / f64::from(layout.strip)
                        * duration_ms)
                        .clamp(0., duration_ms) as u64
                })
                .collect();
            self.player.send(Command::Thumbs { times, height });
        }
    }
}

impl Root {
    pub(super) fn clips_editor_mut(&mut self) -> Option<&mut Editor> {
        self.clips
            .engine
            .ui
            .modal
            .as_mut()
            .and_then(|modal| modal.editor.as_mut())
    }

    /// One line for the harness: where the playhead is, the range, whether it plays and how much was decoded.
    #[cfg(feature = "capture")]
    pub(super) fn clips_editor_status(&self) -> String {
        let Some(editor) = self
            .clips
            .engine
            .ui
            .modal
            .as_ref()
            .and_then(|modal| modal.editor.as_ref())
        else {
            return "no editor".to_owned();
        };
        format!(
            "position {:.3} start {:.3} end {:.3} duration {:.3} playing {} loaded {} decoded {} thumbs {}/{} title {:?} editing {} drag {:?} focus {:?}",
            editor.position,
            editor.edit.crop.start,
            editor.edit.crop.end,
            editor.edit.crop.duration,
            editor.playing,
            editor.loaded,
            editor.player.shared.decoded.load(Ordering::Relaxed),
            editor.thumbs.iter().flatten().count(),
            editor.thumbs.len(),
            editor.edit.name,
            editor.title_editing,
            editor.drag,
            editor.focus,
        )
    }

    /// Polls the open editor every frame or so until it closes or another one replaces it.
    pub(super) fn clips_start_polling(&mut self, cx: &mut Context<Self>) {
        let Some(generation) = self.clips_editor_mut().map(|editor| editor.generation) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        let modal = {
                            let (width, height, _) = modal_size(this.viewport);
                            (width, height)
                        };
                        let Some(editor) = this.clips_editor_mut() else {
                            return false;
                        };
                        if editor.generation != generation {
                            return false;
                        }
                        let changed = editor.poll(cx);
                        editor.sizes(modal);
                        if changed {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Writes an edit that changed something into the clip and drops the editor.
    pub(super) fn clips_close_editor(&mut self, cx: &mut Context<Self>) {
        let Some(mut editor) = self
            .clips
            .engine
            .ui
            .modal
            .as_mut()
            .and_then(|modal| modal.editor.take())
        else {
            return;
        };
        editor.pause();
        if let Some(old) = editor.frame.take() {
            cx.drop_image(old, None);
        }
        for old in editor.thumbs.drain(..).flatten() {
            cx.drop_image(old, None);
        }
        if !editor.edit.differs_from(&editor.clip.meta, &editor.preset) {
            return;
        }
        let units = editor
            .player
            .shared
            .loaded
            .get()
            .map(|loaded| loaded.units.clone());
        let edit = editor.edit.clone();
        let meta = editor.clip.meta.clone();
        let preset = editor.preset.clone();
        let start_ms = (edit.crop.start * 1000.).round() as u64;
        self.clips_write(
            editor.clip.filepath.clone(),
            move || {
                // A new picture at the crop start, 640 x 360 at most (module 956050).
                let thumbnail = units
                    .as_ref()
                    .and_then(|units| player::still_data_url(units, start_ms, (640, 360)));
                edit.apply(&meta, &preset, thumbnail)
            },
            cx,
        );
    }

    /// Builds a clip's new document off the interface thread and writes it into the file (`replace_metadata`), then
    /// updates the library entry and forgets the cached thumbnail.
    pub(super) fn clips_write(
        &mut self,
        path: PathBuf,
        build: impl FnOnce() -> dc_clips::ClipMetadata + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let meta = build();
                    dc_clips::mp4::replace_metadata(&path, meta.to_json().as_bytes()).map(|()| meta)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(meta) => {
                    let id = meta.id.clone();
                    this.clips.engine.ui.last_write = format!("ok {id}");
                    if let Some(clip) = this.clips.engine.clips.get_mut(&id) {
                        clip.meta = meta;
                    }
                    if let Some(Some(old)) = this.clips.engine.ui.pictures.borrow_mut().remove(&id)
                    {
                        cx.drop_image(old, None);
                    }
                    cx.notify();
                }
                Err(error) => {
                    this.clips.engine.ui.last_write = format!("failed {error}");
                    tracing::warn!("clips: writing the document failed: {error}");
                }
            });
        })
        .detach();
    }

    /// The title field of the Clip Info card (module 701266 `ec`): it holds the clip's own name, shows the default
    /// title as its placeholder, writes each edit into the name at once (an empty field is no name) and ends on Enter,
    /// Escape or a press outside, without reverting and without closing the editor.
    fn clips_title_event(
        &mut self,
        field: &Entity<Field>,
        event: &EditorEvent,
        cx: &mut Context<Self>,
    ) {
        let text = field.read(cx).text().to_owned();
        let Some(editor) = self.clips_editor_mut() else {
            return;
        };
        match event {
            EditorEvent::Edited => {
                editor.edit.name = (!text.is_empty()).then_some(text);
            }
            EditorEvent::Submit(_) | EditorEvent::Cancel => {
                editor.title_editing = false;
                editor.refocus.set(true);
            }
        }
        cx.notify();
    }

    /// A press outside the field ends the edit (the field's `onBlur`).
    pub(super) fn clips_end_title_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.clips_editor_mut()
            && editor.title_editing
        {
            editor.title_editing = false;
            editor.refocus.set(true);
            cx.notify();
        }
    }

    /// The title button was pressed: the field takes its place holding the clip's name, with the default title as
    /// its placeholder.
    pub(super) fn clips_edit_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.clips_editor_mut() else {
            return;
        };
        let name = editor.edit.name.clone().unwrap_or_default();
        let mut unnamed = editor.clip.meta.clone();
        unnamed.name = None;
        let placeholder = model::title(&unnamed);
        editor.title_editing = true;
        let field = editor.title.clone();
        field.update(cx, |field, _| {
            field.set_text(&name);
            field.set_placeholder(placeholder);
        });
        window.focus(&gpui::Focusable::focus_handle(field.read(cx), cx), cx);
        cx.notify();
    }

    /// Keys while the modal is open. True when the editor took the key.
    pub(super) fn clips_editor_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = event.keystroke.key.as_str();
        let shift = event.keystroke.modifiers.shift;
        let editing = self
            .clips_editor_mut()
            .is_some_and(|editor| editor.title_editing);
        if editing {
            // The field takes every key but Escape, which ends the edit here (module 701266 `ec`: Enter and Escape
            // blur the input and stop the event, so the editor stays open); an earlier layer may take Escape first.
            if key == "escape" {
                self.clips_end_title_edit(cx);
                return true;
            }
            return false;
        }
        match key {
            "escape" => self.clips_editor_closed(cx),
            "space" => {
                if let Some(editor) = self.clips_editor_mut() {
                    editor.toggle();
                }
            }
            "left" | "right" => {
                if let Some(editor) = self.clips_editor_mut() {
                    editor.nudge(if key == "left" { -1. } else { 1. }, shift);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The pointer moved. A move that carries no pressed button while a drag is under way means the button was let go
    /// where this window did not see it (outside it): the drag ends here, so no handle stays stuck to the pointer.
    pub(super) fn clips_editor_pointer(
        &mut self,
        position: Point<Pixels>,
        pressed: Option<gpui::MouseButton>,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.clips_editor_mut()
            && editor.drag.is_some()
        {
            if pressed == Some(gpui::MouseButton::Left) {
                editor.drag_to(f32::from(position.x));
            } else {
                editor.drag = None;
                if std::mem::take(&mut editor.resume) {
                    editor.play();
                }
            }
            cx.notify();
        }
    }

    pub(super) fn clips_editor_release(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = self.clips_editor_mut()
            && editor.drag.take().is_some()
        {
            if std::mem::take(&mut editor.resume) {
                editor.play();
            }
            cx.notify();
        }
    }

    /// Switches the favourite flag and writes it at once (module 590146 `Favorite` item).
    pub(super) fn clips_toggle_favourite(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.clips_editor_mut() else {
            return;
        };
        let on = !editor.clip.meta.is_favorite.unwrap_or(false);
        editor.clip.meta.is_favorite = Some(on);
        let meta = editor.clip.meta.clone();
        let path = editor.clip.filepath.clone();
        self.clips_write(path, move || meta, cx);
        cx.notify();
    }

    pub(super) fn clips_toggle_audio(&mut self, track: &'static str, cx: &mut Context<Self>) {
        if let Some(editor) = self.clips_editor_mut() {
            match track {
                "application" => editor.edit.application_audio ^= true,
                _ => editor.edit.voice_audio ^= true,
            }
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_fits_the_pane_and_the_strip_fills_the_card() {
        let at = layout((1200., 640.), 16. / 9.);
        assert_eq!(at.left, 860.);
        assert_eq!(at.strip, 860. - 66.);
        assert!(at.video.0 <= 828. && at.video.1 <= 370.);
        assert!((at.video.0 / at.video.1 - 16. / 9.).abs() < 0.02);
        // A tall clip is limited by the height, a wide one by the width.
        assert!(layout((1200., 640.), 0.5).video.0 < 200.);
        assert_eq!(layout((1200., 640.), 4.).video.0, 828.);
    }

    #[test]
    fn the_strip_gets_enough_cells() {
        let (width, count) = thumb_cells(794., 16. / 9.);
        assert_eq!(width, 100);
        assert_eq!(count, 8);
    }
}
