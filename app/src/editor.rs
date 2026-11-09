//! The text field on gpui's input handler, so the OS input method composes straight into it (Turkish, CJK).
//! Used by the composer; search and every other field follow.

use crate::text_buffer::TextBuffer;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, SharedString, Size, Style, TextAlign,
    TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, div, fill, point, prelude::*, px,
    size,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// The composer's text: 16px on 22px lines. Other fields (the quick switcher's 22px) set their own.
pub const LINE_HEIGHT: f32 = 22.;
pub const FONT_SIZE: f32 = 16.;

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

#[derive(Default)]
struct Laid {
    line_height: Pixels,
    /// Each paragraph with the byte offset it starts at.
    lines: Vec<(WrappedLine, usize)>,
    bounds: Option<Bounds<Pixels>>,
}

impl Laid {
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
                let position = line.position_for_index(offset - start, self.line_height())?;
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
        start + found.unwrap_or_else(|nearest| nearest)
    }
}

pub struct Editor {
    focus: FocusHandle,
    buffer: TextBuffer,
    placeholder: SharedString,
    style: Rc<EditorStyle>,
    laid: Rc<RefCell<Laid>>,
    dragging: bool,
    /// A text area: Enter starts a new line instead of submitting.
    enter_inserts_newline: bool,
    /// `Edited` also follows deleting, pasting, cutting and undoing, not only typing.
    reports_all_edits: bool,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Editor {
    pub fn new(style: EditorStyle, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            buffer: TextBuffer::default(),
            placeholder: SharedString::default(),
            style: Rc::new(style),
            laid: Rc::default(),
            dragging: false,
            enter_inserts_newline: false,
            reports_all_edits: false,
        }
    }

    pub fn set_enter_inserts_newline(&mut self, on: bool) {
        self.enter_inserts_newline = on;
    }

    pub fn set_reports_all_edits(&mut self, on: bool) {
        self.reports_all_edits = on;
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer.clear();
        self.buffer.insert(text);
    }

    pub fn set_placeholder(&mut self, placeholder: String) {
        self.placeholder = placeholder.into();
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    /// Types `text` at the caret, replacing any selection, as a paste would.
    pub fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buffer.insert(text);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// Enter. An empty field still submits, because the composer may have files to send; listeners decide.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.buffer.text().trim().to_owned();
        self.buffer.clear();
        cx.emit(EditorEvent::Submit(text));
    }

    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let stroke = &event.keystroke;
        let (control, shift) = (stroke.modifiers.control, stroke.modifiers.shift);
        // While the input method composes, it owns these keys.
        if self.buffer.marked().is_some() {
            return;
        }
        let before = self
            .reports_all_edits
            .then(|| self.buffer.text().to_owned());
        let enter_inserts_newline = self.enter_inserts_newline;
        let buffer = &mut self.buffer;
        let move_to = |buffer: &mut TextBuffer, at: usize| {
            if shift {
                buffer.select_to(at);
            } else {
                buffer.set_caret(at);
            }
        };
        match stroke.key.as_str() {
            "enter" if shift || enter_inserts_newline => buffer.insert("\n"),
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
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
        laid.index_at(position - origin)
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

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.buffer.commit_composition();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

fn shape(editor: &Editor, width: Option<Pixels>, window: &Window) -> Vec<(WrappedLine, usize)> {
    let system = window.text_system();
    let mut lines = Vec::new();
    let mut offset = 0;
    let text = editor.buffer.text();
    let marked = editor.buffer.marked();
    for paragraph in text.split('\n') {
        let runs = runs_for(offset, paragraph.len(), marked.as_ref(), &editor.style);
        let shaped = system.shape_text(
            SharedString::from(paragraph.to_owned()),
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
        let (focus, style, laid, placeholder, selection, caret, multiline) = {
            let editor = self.editor.read(cx);
            (
                editor.focus.clone(),
                editor.style.clone(),
                editor.laid.clone(),
                editor.buffer.is_empty().then(|| editor.placeholder.clone()),
                editor.buffer.selection(),
                editor.buffer.caret(),
                editor.enter_inserts_newline,
            )
        };
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        let lines = shape(self.editor.read(cx), Some(bounds.size.width), window);
        {
            let mut laid = laid.borrow_mut();
            laid.lines = lines;
            laid.bounds = Some(bounds);
            laid.line_height = px(style.line_height);
        }
        let laid = laid.borrow();
        if !selection.is_empty()
            && let (Some(from), Some(to)) =
                (laid.position(selection.start), laid.position(selection.end))
            && from.y == to.y
        {
            window.paint_quad(fill(
                Bounds::new(
                    bounds.origin + from,
                    size(to.x - from.x, px(style.line_height)),
                ),
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
            // A text area's hint wraps inside the box as typed text would; a one-line field's runs on.
            if multiline {
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
                let shaped =
                    window
                        .text_system()
                        .shape_line(placeholder, px(style.size), &runs, None);
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
        if focus.is_focused(window)
            && let Some(position) = laid.position(caret)
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
