//! The text field on gpui's input handler, so the OS input method composes straight into it (Turkish, CJK).
//! Used by the composer; search and every other field follow.

use crate::spellcheck;
use crate::text_buffer::TextBuffer;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, Render, SharedString, Size, Style,
    Task, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, div, fill,
    point, prelude::*, px, size,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// The composer's text: 16px on 22px lines. Other fields (the quick switcher's 22px) set their own.
pub const LINE_HEIGHT: f32 = 22.;
pub const FONT_SIZE: f32 = 16.;

/// How long the text rests before it goes to the checker: the 250 ms the page's own language detector waits
/// (`debounce(..., 250)` in module 334604).
const SPELL_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(250);

/// What a right click on the field found: the word it landed on when that word is misspelled (its byte range and
/// text), and the selected text for Cut and Copy.
pub struct ContextClick {
    pub word: Option<(Range<usize>, String)>,
    pub selected: String,
}

/// The squiggle's red, wavelength and amplitude: Blink's `DocumentMarkerPainter` constants (the bundle has none).
const SQUIGGLE_RED: u32 = 0xff0000;
const SQUIGGLE_STEP: f32 = 2.;
const SQUIGGLE_AMPLITUDE: f32 = 2.;
const SQUIGGLE_THICKNESS: f32 = 1.;

/// The misspellings of the text, kept while the text changes.
#[derive(Default)]
struct Spell {
    /// Byte ranges of misspelled words in `text`.
    raw: Vec<Range<usize>>,
    /// The text `raw` belongs to.
    text: String,
    /// The text last sent to the checker.
    requested: String,
    /// The text changed by typing and the caret has not moved by hand since: the word at the caret is left unmarked.
    typing_hold: bool,
    task: Option<Task<()>>,
}

pub enum EditorEvent {
    Submit(String),
    Cancel,
    /// The text changed through typing or the input method.
    Edited,
}

pub struct EditorStyle {
    pub font: Font,
    pub size: f32,
    pub line_height: f32,
    pub text: Hsla,
    pub placeholder: Hsla,
    /// The placeholder in italics (the profile note's "Click to add a note").
    pub placeholder_italic: bool,
    pub selection: Hsla,
    pub caret: Hsla,
}

/// Blink's caret rhythm (`LayoutTheme::CaretBlinkInterval`, which on Windows is the system's `GetCaretBlinkTime`, 530 ms
/// by default): shown for one interval from the last edit or selection move, hidden for the next, and so on. The
/// bundle has no part in it, and `prefers-reduced-motion` does not touch the caret in Chromium.
pub const CARET_BLINK_MS: u64 = 530;

/// Whether the caret is drawn `since_activity_ms` after the last edit, selection move or focus. `interval_ms` 0
/// means the system turned blinking off: the caret stays solid.
pub fn caret_visible(since_activity_ms: u64, interval_ms: u64) -> bool {
    interval_ms == 0 || (since_activity_ms / interval_ms).is_multiple_of(2)
}

/// Milliseconds until the caret next changes between shown and hidden.
pub fn caret_next_toggle_ms(since_activity_ms: u64, interval_ms: u64) -> u64 {
    interval_ms.max(1) - since_activity_ms % interval_ms.max(1)
}

/// What the caret's drawing depends on.
#[derive(Clone, Copy)]
pub struct CaretFacts {
    pub active: bool,
    /// The animation clock is stopped (parity frames): the caret stays solid.
    pub frozen: bool,
    pub selection_empty: bool,
    pub interval_ms: u64,
    pub elapsed_ms: u64,
}

/// Whether the caret of a focused field is drawn, and after how many ms the field must be drawn again (`None`: never,
/// the state cannot change by itself). An inactive window and a range selection draw no caret and wake nothing.
#[derive(Debug, PartialEq, Eq)]
pub struct CaretPlan {
    pub visible: bool,
    pub wake_ms: Option<u64>,
}

pub fn caret_plan(facts: CaretFacts) -> CaretPlan {
    if !facts.active || !facts.selection_empty {
        return CaretPlan {
            visible: false,
            wake_ms: None,
        };
    }
    if facts.frozen || facts.interval_ms == 0 {
        return CaretPlan {
            visible: true,
            wake_ms: None,
        };
    }
    CaretPlan {
        visible: caret_visible(facts.elapsed_ms, facts.interval_ms),
        wake_ms: Some(caret_next_toggle_ms(facts.elapsed_ms, facts.interval_ms)),
    }
}

/// The system's caret blink time in ms; 0 when the user turned blinking off (`INFINITE`).
#[cfg(windows)]
fn system_blink_ms() -> u64 {
    // SAFETY: a plain query with no arguments.
    match unsafe { windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() } {
        0 => CARET_BLINK_MS,
        u32::MAX => 0,
        ms => u64::from(ms),
    }
}

#[cfg(not(windows))]
fn system_blink_ms() -> u64 {
    CARET_BLINK_MS
}

/// Draws that the blink asked for, over the process's life (the harness prints it with `motion idle`).
pub static BLINK_FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where the blink stands: what the field looked like at the last paint, so a change restarts the caret solid.
#[derive(Default)]
struct Blink {
    signature: u64,
    focused: bool,
    since: Option<std::time::Instant>,
    task: Option<Task<()>>,
}

#[derive(Default)]
struct Laid {
    blink: Blink,
    line_height: Pixels,
    /// Each paragraph with the byte offset it starts at.
    lines: Vec<(WrappedLine, usize)>,
    bounds: Option<Bounds<Pixels>>,
    /// A masked field (a password) shows one bullet per character: the real text, to map offsets into the bullets.
    masked_text: Option<String>,
}

#[derive(Clone, Copy)]
struct VerticalMove {
    x: Pixels,
    caret: usize,
    position: Point<Pixels>,
    width: Pixels,
}

/// The bullet a masked field shows for each character.
const MASK: char = '\u{2022}';

impl Laid {
    fn caret_position(
        &self,
        caret: usize,
        vertical: Option<VerticalMove>,
    ) -> Option<Point<Pixels>> {
        vertical
            .filter(|motion| {
                motion.caret == caret
                    && self
                        .bounds
                        .is_some_and(|bounds| bounds.size.width == motion.width)
            })
            .map(|motion| motion.position)
            .or_else(|| self.position(caret))
    }

    /// An offset in the real text as an offset in the shown text.
    fn shown_offset(&self, offset: usize) -> usize {
        match &self.masked_text {
            Some(text) => text[..offset.min(text.len())].chars().count() * MASK.len_utf8(),
            None => offset,
        }
    }

    /// An offset in the shown text as an offset in the real text.
    fn real_offset(&self, shown: usize) -> usize {
        match &self.masked_text {
            Some(text) => text
                .char_indices()
                .nth(shown / MASK.len_utf8())
                .map_or(text.len(), |(at, _)| at),
            None => shown,
        }
    }

    fn line_height(&self) -> Pixels {
        if self.line_height > px(0.) {
            self.line_height
        } else {
            px(LINE_HEIGHT)
        }
    }

    fn paragraph_tops(&self) -> Vec<Pixels> {
        let mut top = px(0.);
        self.lines
            .iter()
            .map(|(line, _)| {
                let current = top;
                top += line.size(self.line_height()).height.max(self.line_height());
                current
            })
            .collect()
    }

    fn position(&self, offset: usize) -> Option<Point<Pixels>> {
        let tops = self.paragraph_tops();
        for (index, (line, start)) in self.lines.iter().enumerate().rev() {
            if offset >= *start {
                let local = self.shown_offset(offset) - self.shown_offset(*start);
                let position = line.position_for_index(local, self.line_height())?;
                return Some(point(position.x, tops[index] + position.y));
            }
        }
        None
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        let tops = self.paragraph_tops();
        let index = tops.iter().rposition(|top| position.y >= *top).unwrap_or(0);
        let Some((line, start)) = self.lines.get(index) else {
            return 0;
        };
        let local = point(position.x, position.y - tops[index]);
        let found = line.closest_index_for_position(local, self.line_height());
        let shown = self.shown_offset(*start) + found.unwrap_or_else(|nearest| nearest);
        self.real_offset(shown)
    }

    fn selection_bounds(&self, selection: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut result = Vec::new();
        for ((line, start), top) in self.lines.iter().zip(self.paragraph_tops()) {
            let base = self.shown_offset(*start);
            let from = self.shown_offset(selection.start).saturating_sub(base);
            let to = self.shown_offset(selection.end).saturating_sub(base);
            let layout = &line.unwrapped_layout;
            let ends = line
                .wrap_boundaries
                .iter()
                .map(|boundary| layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
                .chain([line.len()]);
            let mut row_start = 0;
            for (row, row_end) in ends.enumerate() {
                let first = from.max(row_start);
                let last = to.min(row_end);
                if first < last {
                    let origin = layout.x_for_index(row_start);
                    let left = layout.x_for_index(first) - origin;
                    let right = layout.x_for_index(last) - origin;
                    result.push(Bounds::new(
                        point(left.min(right), top + self.line_height() * row as f32),
                        size((right - left).abs(), self.line_height()),
                    ));
                }
                row_start = row_end;
            }
        }
        result
    }
}

pub struct Editor {
    focus: FocusHandle,
    buffer: TextBuffer,
    placeholder: SharedString,
    /// An en-US message shown as the placeholder in the current interface language, read at each paint.
    placeholder_message: Option<&'static str>,
    ellipsis_placeholder: bool,
    style: Rc<EditorStyle>,
    laid: Rc<RefCell<Laid>>,
    dragging: bool,
    /// A text area: Enter starts a new line instead of submitting.
    enter_inserts_newline: bool,
    /// `Edited` also follows deleting, pasting, cutting and undoing, not only typing.
    reports_all_edits: bool,
    /// A password field: bullets on screen, no copy or cut.
    masked: bool,
    /// A form field: Enter reports `Submit` with no text and leaves the field as it is.
    keeps_text: bool,
    /// Underlines misspelled words (the composer).
    spell: Option<Spell>,
    /// Styles the typed markdown while the preview setting is on (the composer).
    markdown: bool,
    /// A one-line field whose text ends at the right edge (`text-align: end`).
    align_end: bool,
    /// Keeps the visual column across short lines and the side of a soft-wrap boundary.
    vertical: Option<VerticalMove>,
}

impl EventEmitter<EditorEvent> for Editor {}

thread_local! {
    /// The focus handles of the text fields that exist now, so the keyboard layer can tell whether typing is going to
    /// a field (the official global handler skips keys while an input has focus).
    static FIELDS: RefCell<Vec<gpui::WeakFocusHandle>> = RefCell::default();
}

/// Whether `handle` is the focus handle of a live text field.
pub fn is_field(handle: &FocusHandle) -> bool {
    FIELDS.with(|fields| fields.borrow().iter().any(|field| handle == field))
}

impl Drop for Editor {
    fn drop(&mut self) {
        FIELDS.with(|fields| fields.borrow_mut().retain(|field| self.focus != *field));
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Editor {
    pub fn new(style: EditorStyle, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        FIELDS.with(|fields| fields.borrow_mut().push(focus.downgrade()));
        Self {
            focus,
            buffer: TextBuffer::default(),
            placeholder: SharedString::default(),
            placeholder_message: None,
            ellipsis_placeholder: false,
            style: Rc::new(style),
            laid: Rc::default(),
            dragging: false,
            enter_inserts_newline: false,
            reports_all_edits: false,
            masked: false,
            keeps_text: false,
            align_end: false,
            spell: None,
            markdown: false,
            vertical: None,
        }
    }

    /// The field styles typed markdown as the message will read while "Preview emoji, mentions, and markdown syntax
    /// as you type" is on (`crate::markup_preview::enabled`).
    pub fn set_markdown_preview(&mut self, on: bool) {
        self.markdown = on;
    }

    /// Marks misspelled words with the system spell checker, as the official composer's text area does.
    pub fn set_spellcheck(&mut self, on: bool) {
        self.spell = on.then(Spell::default);
    }

    /// Brings the misspellings up to date with the text: ranges move with the edit, and a check of the new text
    /// starts once the text has rested (a newer edit drops the waiting one).
    fn sync_spell(&mut self, cx: &mut Context<Self>) {
        let Some(spell) = self.spell.as_mut() else {
            return;
        };
        let text = self.buffer.text();
        if spell.text != text {
            spell.raw = spellcheck::shift_ranges(&spell.text, text, &spell.raw);
            spell.text = text.to_owned();
            spell.typing_hold = true;
        }
        if !spellcheck::is_enabled() || spell.requested == text {
            return;
        }
        spell.requested = text.to_owned();
        if text.trim().is_empty() {
            spell.raw.clear();
            spell.task = None;
            return;
        }
        let checked = text.to_owned();
        spell.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SPELL_DEBOUNCE).await;
            let sent = checked.clone();
            let answer = cx
                .background_executor()
                .spawn(async move {
                    spellcheck::ensure_languages();
                    spellcheck::service().check(sent).recv().ok()
                })
                .await;
            if let Some(ranges) = answer {
                let _ = this.update(cx, |this, cx| {
                    if let Some(spell) = this.spell.as_mut()
                        && spell.text == checked
                    {
                        spell.raw = ranges;
                        cx.notify();
                    }
                });
            }
        }));
    }

    /// The misspellings to draw or act on: none while the switch is off, none for a learned word, none for the word
    /// being typed (Blink marks it once the caret leaves) or one inside the input method's composition.
    fn spell_marks(&self, focused: bool) -> Vec<Range<usize>> {
        let Some(spell) = self.spell.as_ref() else {
            return Vec::new();
        };
        if !spellcheck::is_enabled() {
            return Vec::new();
        }
        let text = self.buffer.text();
        let selection = self.buffer.selection();
        let marked = self.buffer.marked();
        spell
            .raw
            .iter()
            .filter(|range| {
                text.get((*range).clone())
                    .is_some_and(|word| !spellcheck::has_learned_word(word))
            })
            .filter(|range| {
                !(focused
                    && spell.typing_hold
                    && selection.is_empty()
                    && range.start <= selection.start
                    && selection.start <= range.end)
            })
            .filter(|range| {
                marked
                    .as_ref()
                    .is_none_or(|marked| marked.end <= range.start || range.end <= marked.start)
            })
            .cloned()
            .collect()
    }

    /// A right click: the caret goes to the click unless it landed in the selection; a misspelled word under the
    /// click gets selected (Blink's `SelectClosestMisspellingFromHitTestResult`). Returns what the menu needs.
    pub fn context_click(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ContextClick {
        self.vertical = None;
        window.focus(&self.focus, cx);
        let at = self.index_for_window_point(position);
        let selection = self.buffer.selection();
        if selection.is_empty() || at < selection.start || at > selection.end {
            let word = self
                .spell_marks(false)
                .into_iter()
                .find(|range| range.start <= at && at <= range.end);
            match word {
                Some(range) => {
                    self.buffer.set_caret(range.start);
                    self.buffer.select_to(range.end);
                }
                None => self.buffer.set_caret(at),
            }
            if let Some(spell) = self.spell.as_mut() {
                spell.typing_hold = false;
            }
        }
        let selection = self.buffer.selection();
        let word = self
            .spell_marks(false)
            .into_iter()
            .find(|range| *range == selection)
            .map(|range| (range.clone(), self.buffer.text()[range].to_owned()));
        cx.notify();
        ContextClick {
            word,
            selected: self.buffer.selected_text().to_owned(),
        }
    }

    pub fn copy_selection(&self, cx: &mut Context<Self>) {
        if !self.masked && !self.buffer.selection().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.selected_text().to_owned(),
            ));
        }
    }

    pub fn cut_selection(&mut self, cx: &mut Context<Self>) {
        if self.masked || self.buffer.selection().is_empty() {
            return;
        }
        self.copy_selection(cx);
        self.vertical = None;
        self.buffer.backspace();
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.insert(&text, cx);
        }
    }

    /// Shows one bullet per character and refuses copy and cut.
    pub fn set_masked(&mut self, on: bool) {
        self.masked = on;
    }

    /// Enter in a form field submits the form, not the text: the field keeps what was typed (a failed login keeps the
    /// password), and the text does not travel through the event.
    /// Right-aligns the text of a one-line field (`text-align: end`).
    pub fn set_align_end(&mut self, on: bool) {
        self.align_end = on;
    }

    pub fn set_keeps_text(&mut self, on: bool) {
        self.keeps_text = on;
    }

    pub fn set_enter_inserts_newline(&mut self, on: bool) {
        self.enter_inserts_newline = on;
    }

    pub fn set_reports_all_edits(&mut self, on: bool) {
        self.reports_all_edits = on;
    }

    pub fn set_max_utf16(&mut self, limit: usize) {
        self.buffer.set_max_utf16(limit);
    }

    /// A new look, for an editor that outlives a palette change.
    pub fn set_style(&mut self, style: EditorStyle) {
        self.vertical = None;
        self.style = Rc::new(style);
    }

    pub fn set_single_line(&mut self, on: bool) {
        self.buffer.set_single_line(on);
    }

    pub fn set_text(&mut self, text: &str) {
        self.vertical = None;
        self.buffer.clear();
        self.buffer.insert(text);
    }

    pub fn set_placeholder(&mut self, placeholder: String) {
        self.placeholder = placeholder.into();
        self.placeholder_message = None;
    }

    /// The placeholder is an en-US message (`dc_intl::tr`), so it follows the interface language while shown.
    pub fn set_placeholder_message(&mut self, message: &'static str) {
        self.placeholder = message.into();
        self.placeholder_message = Some(message);
    }

    pub fn set_ellipsis_placeholder(&mut self, on: bool) {
        self.ellipsis_placeholder = on;
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    /// Types `text` at the caret, replacing any selection, as a paste would.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.vertical = None;
        self.buffer.insert(text);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// A digest of what an edit or a selection move changes: the blink restarts solid when it differs from the last paint.
    fn blink_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.buffer.text().hash(&mut hasher);
        let selection = self.buffer.selection();
        (selection.start, selection.end, self.buffer.marked()).hash(&mut hasher);
        hasher.finish()
    }

    /// The caret's byte offset, when nothing is selected (autocomplete reads the word before it).
    pub fn collapsed_caret(&self) -> Option<usize> {
        let selection = self.buffer.selection();
        (selection.is_empty() && self.buffer.marked().is_none()).then_some(selection.start)
    }

    /// Replaces the bytes `range` by `text` with the caret after it, as accepting a suggestion does.
    pub fn replace_range(
        &mut self,
        range: std::ops::Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.vertical = None;
        self.buffer.replace(Some(range), text);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// Enter. An empty field still submits, because the composer may have files to send; listeners decide.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        self.vertical = None;
        if self.keeps_text {
            cx.emit(EditorEvent::Submit(String::new()));
            return;
        }
        let text = self.buffer.text().trim().to_owned();
        self.buffer.clear();
        cx.emit(EditorEvent::Submit(text));
    }

    fn move_vertical(&mut self, down: bool, shift: bool, window: &Window) {
        let width = self.laid.borrow().bounds.map(|bounds| bounds.size.width);
        let Some(width) = width else { return };
        // Input can change between paints. Navigate the current text at the last measured width.
        let lines = shape(self, Some(width), window);
        let mut laid = self.laid.borrow_mut();
        laid.lines = lines;
        laid.line_height = px(self.style.line_height);
        laid.masked_text = self.masked.then(|| self.buffer.text().to_owned());
        let Some(position) = laid.caret_position(self.buffer.caret(), self.vertical) else {
            return;
        };
        let x = self
            .vertical
            .filter(|motion| motion.width == width)
            .map_or(position.x, |motion| motion.x);
        let height: Pixels = laid
            .lines
            .iter()
            .map(|(line, _)| line.size(laid.line_height()).height.max(laid.line_height()))
            .sum();
        let y = position.y
            + if down {
                laid.line_height()
            } else {
                -laid.line_height()
            };
        let outside = y < px(0.) || y >= height;
        let at = if outside {
            if down { self.buffer.text().len() } else { 0 }
        } else {
            laid.index_at(point(x, y + laid.line_height() / 2.))
        };
        let Some(mut target) = laid.position(at) else {
            return;
        };
        if !outside && target.y != y {
            // A soft-wrap boundary belongs to both rows; index lookup prefers the preceding row.
            target = point(px(0.), y);
        }
        self.vertical = Some(VerticalMove {
            x: if outside { target.x } else { x },
            caret: at,
            position: target,
            width,
        });
        if shift {
            self.buffer.select_to(at);
        } else {
            self.buffer.set_caret(at);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let (control, shift) = (stroke.modifiers.control, stroke.modifiers.shift);
        // While the input method composes, it owns these keys.
        if self.buffer.marked().is_some() {
            return;
        }
        let vertical = matches!(stroke.key.as_str(), "up" | "down")
            && !control
            && !stroke.modifiers.alt
            && !stroke.modifiers.platform;
        if !vertical {
            self.vertical = None;
        }
        if matches!(
            stroke.key.as_str(),
            "left" | "right" | "up" | "down" | "home" | "end"
        ) && let Some(spell) = self.spell.as_mut()
        {
            spell.typing_hold = false;
        }
        if vertical && !self.buffer.is_single_line() {
            self.move_vertical(stroke.key == "down", shift, window);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let before = self
            .reports_all_edits
            .then(|| self.buffer.text().to_owned());
        let enter_inserts_newline = self.enter_inserts_newline;
        let masked = self.masked;
        let buffer = &mut self.buffer;
        let move_to = |buffer: &mut TextBuffer, at: usize| {
            if shift {
                buffer.select_to(at);
            } else {
                buffer.set_caret(at);
            }
        };
        match stroke.key.as_str() {
            "enter" if !buffer.is_single_line() && (shift || enter_inserts_newline) => {
                buffer.insert("\n");
            }
            "enter" => self.submit(cx),
            "escape" => cx.emit(EditorEvent::Cancel),
            "backspace" => buffer.backspace(),
            "delete" => buffer.delete(),
            "left" => {
                let at = match (control, shift) {
                    (true, _) => buffer.word_left(),
                    (false, true) => buffer.char_left(),
                    (false, false) => buffer.left(),
                };
                move_to(buffer, at);
            }
            "right" => {
                let at = match (control, shift) {
                    (true, _) => buffer.word_right(),
                    (false, true) => buffer.char_right(),
                    (false, false) => buffer.right(),
                };
                move_to(buffer, at);
            }
            "home" => {
                let at = buffer.line_start();
                move_to(buffer, at);
            }
            "end" => {
                let at = buffer.line_end();
                move_to(buffer, at);
            }
            "a" if control => buffer.select_all(),
            "z" if control && shift => buffer.redo(),
            "z" if control => buffer.undo(),
            "y" if control => buffer.redo(),
            "c" | "x" if control && masked => {}
            "c" if control && !buffer.selection().is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(buffer.selected_text().to_owned()));
            }
            "x" if control && !buffer.selection().is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(buffer.selected_text().to_owned()));
                buffer.backspace();
            }
            "v" if control => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    buffer.insert(&text);
                }
            }
            _ => {
                cx.propagate();
                return;
            }
        }
        if before.is_some_and(|before| before != self.buffer.text()) {
            cx.emit(EditorEvent::Edited);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// A press on the editor or on the padding of the box that holds it: focus, and the caret at the nearest place
    /// in the text.
    pub fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vertical = None;
        window.prevent_default();
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        if let Some(spell) = self.spell.as_mut() {
            spell.typing_hold = false;
        }
        let at = self.index_for_window_point(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(at);
        } else {
            self.buffer.set_caret(at);
        }
        self.dragging = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging {
            let at = self.index_for_window_point(event.position);
            self.buffer.select_to(at);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.dragging = false;
    }

    fn index_for_window_point(&self, position: Point<Pixels>) -> usize {
        let laid = self.laid.borrow();
        let origin = laid.bounds.map_or(Point::default(), |bounds| bounds.origin);
        // A point above or under the text (the padding of its box) reads as one on the first or the last line.
        let mut at = position - origin;
        if let Some(bounds) = laid.bounds {
            at.y =
                at.y.clamp(px(0.), (bounds.size.height - px(1.)).max(px(0.)));
        }
        laid.index_at(at)
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.buffer.range_from_utf16(&range);
        *actual = Some(self.buffer.range_to_utf16(&bytes));
        Some(self.buffer.text()[bytes].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.range_to_utf16(&self.buffer.selection()),
            reversed: self.buffer.is_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked()
            .map(|range| self.buffer.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let length = self.buffer.text().len();
        self.buffer.commit_composition();
        if self.buffer.text().len() != length {
            cx.emit(EditorEvent::Edited);
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vertical = None;
        let range = range.map(|range| self.buffer.range_from_utf16(&range));
        self.buffer.replace(range, text);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vertical = None;
        let range = range.map(|range| self.buffer.range_from_utf16(&range));
        let caret = selection.map(|inner| TextBuffer::utf16_range_in(text, &inner));
        self.buffer.compose(range, text, caret);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let bytes = self.buffer.range_from_utf16(&range);
        let position = self.laid.borrow().position(bytes.start)?;
        Some(Bounds::new(
            element_bounds.origin + position,
            size(px(2.), px(self.style.line_height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let at = self.index_for_window_point(position);
        Some(self.buffer.offset_to_utf16(at))
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vertical = None;
        let bytes = self.buffer.range_from_utf16(&range);
        self.buffer.set_caret(bytes.start);
        self.buffer.select_to(bytes.end);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.buffer.offset_to_utf16(self.buffer.text().len()))
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_spell(cx);
        div()
            .id("editor")
            .w_full()
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(EditorElement {
                editor: cx.entity(),
            })
    }
}

struct EditorElement {
    editor: Entity<Editor>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn run(text_length: usize, font: &Font, color: Hsla, underline: bool) -> TextRun {
    TextRun {
        len: text_length,
        font: font.clone(),
        color,
        background_color: None,
        underline: underline.then_some(UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    }
}

/// One run per marked or unmarked stretch of a paragraph, so the composition is underlined as the OS expects.
fn runs_for(
    start: usize,
    length: usize,
    marked: Option<&Range<usize>>,
    style: &EditorStyle,
) -> Vec<TextRun> {
    let end = start + length;
    let Some(marked) = marked.filter(|marked| marked.start < end && marked.end > start) else {
        return vec![run(length, &style.font, style.text, false)];
    };
    let (from, to) = (marked.start.max(start), marked.end.min(end));
    let parts = [(start, from, false), (from, to, true), (to, end, false)];
    parts
        .iter()
        .filter(|(left, right, _)| right > left)
        .map(|(left, right, underline)| run(right - left, &style.font, style.text, *underline))
        .collect()
}

/// The runs of a paragraph under the markdown preview: each stretch with the weight, slant, colour, decoration and
/// font of its flags (`.syntaxBefore`, `.bold`, `.italics`, `.underline`, `.strikethrough`, `.inlineCode`,
/// `.fakeLink` of CSS 358931), and the composition underlined as `runs_for` does.
fn styled_runs(
    start: usize,
    paragraph: &str,
    marked: Option<&Range<usize>>,
    style: &EditorStyle,
) -> Vec<TextRun> {
    use crate::markup_preview as mp;
    let flags = mp::flags(paragraph);
    let composing = |at: usize| marked.is_some_and(|m| m.contains(&(start + at)));
    // Split where the flags or the composition change.
    let mut parts: Vec<(usize, u16, bool)> = Vec::new();
    for (at, flag) in flags.iter().enumerate() {
        let under = composing(at);
        match parts.last_mut() {
            Some((length, last, was)) if last == flag && *was == under => *length += 1,
            _ => parts.push((1, *flag, under)),
        }
    }
    parts
        .into_iter()
        .map(|(length, flag, under)| {
            let mut font = style.font.clone();
            let mut tint = style.text;
            let mut strike = false;
            let mut underline = under;
            if flag & mp::BOLD != 0 {
                font.weight = gpui::FontWeight(700.);
            }
            if flag & mp::ITALIC != 0 {
                font.style = gpui::FontStyle::Italic;
            }
            if flag & mp::CODE != 0 {
                font.family = "gg sans mono".into();
            }
            if flag & mp::UNDERLINE != 0 {
                underline = true;
            }
            if flag & mp::STRIKE != 0 {
                strike = true;
            }
            if flag & mp::LINK != 0 {
                tint = crate::ui::color("--text-link").into();
            }
            if flag & mp::SYNTAX != 0 {
                tint = crate::ui::color("--textbox-markdown-syntax").into();
                font.weight = gpui::FontWeight(if flag & mp::LIGHT_SYNTAX != 0 {
                    400.
                } else {
                    600.
                });
            }
            let mut out = run(length, &font, tint, underline);
            if strike {
                out.strikethrough = Some(gpui::StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(tint),
                });
            }
            if flag & mp::CODE != 0 {
                out.background_color = Some(crate::ui::color("--background-code").into());
            }
            out
        })
        .collect()
}

fn shape(editor: &Editor, width: Option<Pixels>, window: &Window) -> Vec<(WrappedLine, usize)> {
    let system = window.text_system();
    let mut lines = Vec::new();
    let mut offset = 0;
    let text = editor.buffer.text();
    let marked = editor.buffer.marked();
    for paragraph in text.split('\n') {
        let shown = if editor.masked {
            paragraph.chars().map(|_| MASK).collect::<String>()
        } else {
            paragraph.to_owned()
        };
        let runs = if editor.masked {
            runs_for(0, shown.len(), None, &editor.style)
        } else if editor.markdown && crate::markup_preview::enabled() {
            styled_runs(offset, paragraph, marked.as_ref(), &editor.style)
        } else {
            runs_for(offset, paragraph.len(), marked.as_ref(), &editor.style)
        };
        let shaped = system.shape_text(
            SharedString::from(shown),
            px(editor.style.size),
            &runs,
            width,
            None,
        );
        if let Some(line) = shaped.ok().and_then(|mut lines| lines.pop()) {
            lines.push((line, offset));
        }
        offset += paragraph.len() + 1;
    }
    lines
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.).into();
        let editor = self.editor.clone();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let line_height = px(editor.read(cx).style.line_height);
            let lines = shape(editor.read(cx), width, window);
            let height: Pixels = lines
                .iter()
                .map(|(line, _)| line.size(line_height).height.max(line_height))
                .fold(px(0.), |sum, height| sum + height);
            let measured: Size<Pixels> = size(width.unwrap_or(px(0.)), height.max(line_height));
            measured
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (
            focus,
            style,
            laid,
            placeholder,
            selection,
            caret,
            multiline,
            masked,
            ellipsis,
            marks,
            align_end,
            vertical,
        ) = {
            let editor = self.editor.read(cx);
            (
                editor.focus.clone(),
                editor.style.clone(),
                editor.laid.clone(),
                editor
                    .buffer
                    .is_empty()
                    .then(|| match editor.placeholder_message {
                        Some(message) => SharedString::from(dc_intl::tr(message)),
                        None => editor.placeholder.clone(),
                    }),
                editor.buffer.selection(),
                editor.buffer.caret(),
                editor.enter_inserts_newline,
                editor.masked,
                editor.ellipsis_placeholder,
                editor.spell_marks(editor.focus.is_focused(window)),
                editor.align_end,
                editor.vertical,
            )
        };
        let signature = self.editor.read(cx).blink_signature();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        let lines = shape(self.editor.read(cx), Some(bounds.size.width), window);
        // `text-align: end`: everything below is drawn from an origin moved right by the free width.
        let bounds = if align_end && !multiline {
            let width = lines
                .iter()
                .map(|(line, _)| line.size(px(style.line_height)).width)
                .fold(px(0.), Pixels::max);
            let shift = (bounds.size.width - width).max(px(0.));
            Bounds::new(
                bounds.origin + point(shift, px(0.)),
                size(bounds.size.width - shift, bounds.size.height),
            )
        } else {
            bounds
        };
        {
            let mut laid = laid.borrow_mut();
            laid.lines = lines;
            laid.masked_text = masked.then(|| self.editor.read(cx).buffer.text().to_owned());
            laid.bounds = Some(bounds);
            laid.line_height = px(style.line_height);
        }
        let focused = focus.is_focused(window);
        let active = window.is_window_active();
        let caret_shown = {
            let mut state = laid.borrow_mut();
            let blink = &mut state.blink;
            if !focused || !active {
                // An unfocused field, or an inactive or minimised window: Chromium draws no caret and nothing wakes.
                blink.focused = false;
                blink.since = None;
                blink.task = None;
                !focused
            } else {
                let now = std::time::Instant::now();
                if !blink.focused || blink.signature != signature || blink.since.is_none() {
                    blink.since = Some(now);
                }
                blink.focused = true;
                blink.signature = signature;
                let elapsed = now.duration_since(blink.since.unwrap_or(now)).as_millis() as u64;
                let plan = caret_plan(CaretFacts {
                    active,
                    frozen: crate::ui::motion::is_frozen(),
                    selection_empty: selection.is_empty(),
                    interval_ms: system_blink_ms(),
                    elapsed_ms: elapsed,
                });
                match plan.wake_ms {
                    Some(wake) => {
                        let wait = std::time::Duration::from_millis(wake);
                        let editor = self.editor.downgrade();
                        // One pending wake: a newer paint replaces (and so cancels) the one before it.
                        blink.task = Some(cx.spawn(async move |cx| {
                            cx.background_executor().timer(wait).await;
                            BLINK_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let _ = editor.update(cx, |_, cx| cx.notify());
                        }));
                    }
                    None => blink.task = None,
                }
                plan.visible
            }
        };
        let laid = laid.borrow();
        for selected in laid.selection_bounds(selection.clone()) {
            window.paint_quad(fill(
                Bounds::new(bounds.origin + selected.origin, selected.size),
                style.selection,
            ));
        }

        if let Some(placeholder) = placeholder {
            let mut placeholder_font = style.font.clone();
            if style.placeholder_italic {
                placeholder_font.style = gpui::FontStyle::Italic;
            }
            let runs = [run(
                placeholder.len(),
                &placeholder_font,
                style.placeholder,
                false,
            )];
            // A text area's hint wraps; fields such as header search truncate only their hint. The composer's
            // hint (`.placeholder_SHcSVi`: `white-space:nowrap`, `text-overflow:ellipsis`) stays on one line.
            if multiline && !ellipsis {
                if let Ok(mut lines) = window.text_system().shape_text(
                    placeholder,
                    px(style.size),
                    &runs,
                    Some(bounds.size.width),
                    None,
                ) && let Some(line) = lines.pop()
                {
                    let _ = line.paint(
                        bounds.origin,
                        px(style.line_height),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            } else {
                let mut shaped = window.text_system().shape_line(
                    placeholder.clone(),
                    px(style.size),
                    &runs,
                    None,
                );
                if ellipsis && shaped.width > bounds.size.width {
                    let (text, runs) = window
                        .text_system()
                        .line_wrapper(placeholder_font, px(style.size))
                        .truncate_line(
                            placeholder,
                            bounds.size.width,
                            "…",
                            &runs,
                            gpui::TruncateFrom::End,
                        );
                    shaped = window
                        .text_system()
                        .shape_line(text, px(style.size), &runs, None);
                }
                // A box too narrow even for the ellipsis shows no hint (the official clips it).
                if !ellipsis || shaped.width <= bounds.size.width {
                    let _ = shaped.paint(
                        bounds.origin,
                        px(style.line_height),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
        }
        let tops = laid.paragraph_tops();
        for (index, (line, _)) in laid.lines.iter().enumerate() {
            let origin = bounds.origin + point(px(0.), tops[index]);
            let _ = line.paint(
                origin,
                px(style.line_height),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        paint_squiggles(&laid, &marks, bounds.origin, style.line_height, window);
        // Blink paints no caret over a range selection.
        if focused
            && selection.is_empty()
            && caret_shown
            && let Some(position) = laid.caret_position(caret, vertical)
        {
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + position,
                    size(px(1.), px(style.line_height)),
                ),
                style.caret,
            ));
        }
    }
}

/// The red wave under each misspelled range, row by row (a word that wraps gets a piece on each row). The wave sits one
/// pixel under the baseline, as Blink places it; the baseline rounds like gpui's own line painting.
fn paint_squiggles(
    laid: &Laid,
    marks: &[Range<usize>],
    origin: Point<Pixels>,
    line_height: f32,
    window: &mut Window,
) {
    if marks.is_empty() || laid.masked_text.is_some() {
        return;
    }
    let tops = laid.paragraph_tops();
    let height = px(line_height);
    let color: Hsla = gpui::rgb(SQUIGGLE_RED).into();
    for mark in marks {
        let Some(index) = laid
            .lines
            .iter()
            .rposition(|(_, start)| *start <= mark.start)
        else {
            continue;
        };
        let (line, start) = &laid.lines[index];
        let (from, to) = (mark.start - start, mark.end - start);
        let Some(word) = line.text.get(from..to) else {
            continue;
        };
        let layout = &line.unwrapped_layout;
        let ascent = layout.ascent.round();
        let descent = layout.descent.round();
        let padding_top = ((height - ascent - descent) / 2.).floor();
        // (row top, x start, x end) pieces, joined while they stay on one row.
        let mut pieces: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
        for (at, c) in word.char_indices() {
            let first = from + at;
            let Some(position) = line.position_for_index(first, height) else {
                continue;
            };
            let width = layout.x_for_index(first + c.len_utf8()) - layout.x_for_index(first);
            match pieces.last_mut() {
                Some(piece) if piece.0 == position.y => piece.2 = position.x + width,
                _ => pieces.push((position.y, position.x, position.x + width)),
            }
        }
        for (row, x0, x1) in pieces {
            let top = origin.y + tops[index] + row + padding_top + ascent + px(1.);
            let mut path = PathBuilder::stroke(px(SQUIGGLE_THICKNESS));
            let mut x = origin.x + x0;
            let end = origin.x + x1;
            let mut low = false;
            path.move_to(point(x, top));
            while x < end {
                x = (x + px(SQUIGGLE_STEP)).min(end);
                low = !low;
                path.line_to(point(
                    x,
                    top + if low { px(SQUIGGLE_AMPLITUDE) } else { px(0.) },
                ));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
    }
}

#[cfg(test)]
mod blink_tests {
    use super::*;

    #[test]
    fn the_caret_is_solid_for_one_interval_then_alternates() {
        assert!(caret_visible(0, 530));
        assert!(caret_visible(529, 530));
        assert!(!caret_visible(530, 530));
        assert!(!caret_visible(1059, 530));
        assert!(caret_visible(1060, 530));
        assert!(!caret_visible(1590, 530));
    }

    fn facts() -> CaretFacts {
        CaretFacts {
            active: true,
            frozen: false,
            selection_empty: true,
            interval_ms: 530,
            elapsed_ms: 600,
        }
    }

    #[test]
    fn an_active_window_blinks_and_asks_for_the_next_toggle() {
        assert_eq!(
            caret_plan(facts()),
            CaretPlan {
                visible: false,
                wake_ms: Some(460)
            }
        );
    }

    #[test]
    fn an_inactive_window_a_selection_and_a_frozen_clock_wake_nothing() {
        let off = CaretPlan {
            visible: false,
            wake_ms: None,
        };
        assert_eq!(
            caret_plan(CaretFacts {
                active: false,
                ..facts()
            }),
            off
        );
        assert_eq!(
            caret_plan(CaretFacts {
                selection_empty: false,
                ..facts()
            }),
            off
        );
        assert_eq!(
            caret_plan(CaretFacts {
                frozen: true,
                ..facts()
            }),
            CaretPlan {
                visible: true,
                wake_ms: None
            }
        );
    }

    #[test]
    fn a_zero_interval_never_blinks() {
        assert!(caret_visible(12345, 0));
    }

    #[test]
    fn the_next_toggle_is_the_rest_of_the_interval() {
        assert_eq!(caret_next_toggle_ms(0, 530), 530);
        assert_eq!(caret_next_toggle_ms(100, 530), 430);
        assert_eq!(caret_next_toggle_ms(530, 530), 530);
        assert_eq!(caret_next_toggle_ms(1000, 530), 60);
    }
}

#[cfg(all(test, feature = "capture"))]
mod navigation_tests {
    use super::*;

    #[gpui::test]
    fn vertical_keys_keep_column_selection_and_wrapped_row(cx: &mut gpui::TestAppContext) {
        let window = cx.open_window(size(px(180.), px(200.)), |_, cx| {
            Editor::new(
                EditorStyle {
                    font: gpui::font("Arial"),
                    size: 16.,
                    line_height: 22.,
                    text: gpui::white(),
                    placeholder: gpui::white(),
                    placeholder_italic: false,
                    selection: gpui::black(),
                    caret: gpui::white(),
                },
                cx,
            )
        });
        cx.run_until_parked();
        window
            .update(cx, |editor, window, cx| {
                fn press(
                    editor: &mut Editor,
                    key: &'static str,
                    window: &mut Window,
                    cx: &mut Context<Editor>,
                ) {
                    editor.on_key(
                        &KeyDownEvent {
                            keystroke: gpui::Keystroke::parse(key).unwrap(),
                            is_held: false,
                            prefer_character_input: false,
                        },
                        window,
                        cx,
                    );
                }
                editor.set_text("abcdefghij\nx\nabcdefghij");
                editor.buffer.set_caret(20);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), 12);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), 7);
                press(editor, "shift-down", window, cx);
                assert_eq!(editor.buffer.selection(), 7..12);
                press(editor, "shift-down", window, cx);
                assert_eq!(editor.buffer.selection(), 7..20);
                assert_eq!(editor.laid.borrow().selection_bounds(7..20).len(), 3);
                press(editor, "shift-up", window, cx);
                assert_eq!(editor.buffer.selection(), 7..12);
                press(editor, "left", window, cx);
                assert_eq!(editor.buffer.caret(), 7);
                press(editor, "home", window, cx);
                press(editor, "down", window, cx);
                assert_eq!(editor.buffer.caret(), 11);

                let line = "ab😀éğxyz";
                editor.set_text(&format!("{line}\nx\n{line}"));
                editor.buffer.set_caret(line.len() + 3 + "ab😀é".len());
                press(editor, "up", window, cx);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), "ab😀é".len());
                assert!(editor.text().is_char_boundary(editor.buffer.caret()));

                editor.set_text(&"m".repeat(100));
                editor.buffer.set_caret(0);
                press(editor, "down", window, cx);
                let first = editor.buffer.caret();
                assert!(first > 0 && first < 100);
                press(editor, "down", window, cx);
                assert!(editor.buffer.caret() > first);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), first);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), 0);
                assert!(editor.laid.borrow().selection_bounds(0..100).len() > 2);
                press(editor, "up", window, cx);
                assert_eq!(editor.buffer.caret(), 0);
            })
            .unwrap();
    }
}
