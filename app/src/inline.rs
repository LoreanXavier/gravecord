//! Inline layout: text runs and inline boxes on shared lines, with wrapping and hit testing. Message content, embeds and
//! the composer render through this. `StyledText` cannot draw a box inside a run.

use gpui::{
    App, Background, BorderStyle, Bounds, Corners, DispatchPhase, Edges, Element, ElementId, Font,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, SharedString, Size,
    StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, quad,
    size,
};
use std::cell::RefCell;
use std::cmp::{max, min};
use std::rc::Rc;

#[derive(Clone)]
pub struct BoxStyle {
    pub padding_x: Pixels,
    pub height: Pixels,
    pub top: Pixels,
    pub background: Hsla,
    pub border: Hsla,
    pub radius: Pixels,
    pub text_line_height: Pixels,
    pub text_top: Pixels,
}

#[derive(Clone)]
pub struct Piece {
    pub text: String,
    pub font: Font,
    pub size: Pixels,
    pub color: Hsla,
    pub underline: bool,
    pub strike: bool,
    pub boxed: Option<BoxStyle>,
}

/// A place in the document: a message id, a block inside it, and a byte offset in that block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub key: (u64, u32),
    pub offset: usize,
}

#[derive(Debug, Default)]
pub struct Selection {
    anchor: Option<Position>,
    head: Option<Position>,
    dragging: bool,
}

pub type SharedSelection = Rc<RefCell<Selection>>;

impl Selection {
    pub fn begin(&mut self, at: Position) {
        self.anchor = Some(at);
        self.head = Some(at);
        self.dragging = true;
    }

    pub fn extend(&mut self, to: Position) {
        if self.dragging {
            self.head = Some(to);
        }
    }

    pub fn end(&mut self) {
        self.dragging = false;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Start and end in document order, or nothing when the selection is empty.
    pub fn range(&self) -> Option<(Position, Position)> {
        let (anchor, head) = (self.anchor?, self.head?);
        (anchor != head).then(|| (min(anchor, head), max(anchor, head)))
    }

    /// The selected bytes of one block of `length` bytes.
    pub fn range_in(&self, key: (u64, u32), length: usize) -> Option<(usize, usize)> {
        let (start, end) = self.range()?;
        if key < start.key || key > end.key {
            return None;
        }
        let from = if key == start.key { start.offset } else { 0 };
        let to = if key == end.key { end.offset } else { length };
        (from < to).then_some((from, to.min(length)))
    }
}

struct Fragment {
    shaped: ShapedLine,
    x: Pixels,
    width: Pixels,
    line: usize,
    piece: usize,
    /// Byte offset of this fragment in the whole text.
    start: usize,
}

#[derive(Default)]
pub struct Laid {
    fragments: Vec<Fragment>,
    lines: usize,
    width: Pixels,
}

/// Splits after each run of whitespace, so a fragment is a word with its trailing spaces.
pub fn words(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (at, character) in text.char_indices() {
        let space = character.is_whitespace();
        if in_space && !space {
            out.push((start, &text[start..at]));
            start = at;
        }
        in_space = space;
    }
    if start < text.len() {
        out.push((start, &text[start..]));
    }
    out
}

fn run_for(piece: &Piece, length: usize) -> TextRun {
    TextRun {
        len: length,
        font: piece.font.clone(),
        color: piece.color,
        background_color: None,
        underline: piece.underline.then_some(UnderlineStyle {
            thickness: px(1.),
            color: Some(piece.color),
            wavy: false,
        }),
        strikethrough: piece.strike.then_some(StrikethroughStyle {
            thickness: px(1.),
            color: Some(piece.color),
        }),
    }
}

fn shape(piece: &Piece, text: &str, window: &Window) -> ShapedLine {
    let run = run_for(piece, text.len());
    window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        piece.size,
        &[run],
        None,
    )
}

fn place(
    laid: &mut Laid,
    x: &mut Pixels,
    max_width: Option<Pixels>,
    fragment: (ShapedLine, usize, usize),
    padding: Pixels,
) {
    let (shaped, piece, start) = fragment;
    let width = shaped.width + padding * 2.;
    let overflow = max_width.is_some_and(|limit| *x + width > limit);
    if overflow && *x > px(0.) {
        laid.lines += 1;
        *x = px(0.);
    }
    laid.fragments.push(Fragment {
        shaped,
        x: *x,
        width,
        line: laid.lines - 1,
        piece,
        start,
    });
    *x += width;
    laid.width = laid.width.max(*x);
}

fn lay_out(pieces: &[Piece], max_width: Option<Pixels>, window: &Window) -> Laid {
    let mut laid = Laid {
        lines: 1,
        ..Laid::default()
    };
    let mut x = px(0.);
    let mut offset = 0;
    for (index, piece) in pieces.iter().enumerate() {
        let padding = piece.boxed.as_ref().map_or(px(0.), |style| style.padding_x);
        // Shaped whole when it fits, which keeps kerning across spaces. Split only to wrap.
        let whole = shape(piece, &piece.text, window);
        let fits = max_width.is_none_or(|limit| x + whole.width + padding * 2. <= limit);
        if fits || piece.boxed.is_some() {
            place(
                &mut laid,
                &mut x,
                max_width,
                (whole, index, offset),
                padding,
            );
        } else {
            for (at, word) in words(&piece.text) {
                let shaped = shape(piece, word, window);
                place(
                    &mut laid,
                    &mut x,
                    max_width,
                    (shaped, index, offset + at),
                    padding,
                );
            }
        }
        offset += piece.text.len();
    }
    laid
}

pub struct Inline {
    pieces: Rc<Vec<Piece>>,
    line_height: Pixels,
    laid: Rc<RefCell<Laid>>,
    selectable: Option<Selectable>,
}

#[derive(Clone)]
struct Selectable {
    key: (u64, u32),
    selection: SharedSelection,
    highlight: Hsla,
}

pub fn inline(pieces: Vec<Piece>, line_height: Pixels) -> Inline {
    Inline {
        pieces: Rc::new(pieces),
        line_height,
        laid: Rc::default(),
        selectable: None,
    }
}

impl Inline {
    pub fn selectable(
        mut self,
        key: (u64, u32),
        selection: SharedSelection,
        highlight: Hsla,
    ) -> Self {
        self.selectable = Some(Selectable {
            key,
            selection,
            highlight,
        });
        self
    }

    fn paint_selection(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let Some(selectable) = &self.selectable else {
            return;
        };
        let laid = self.laid.borrow();
        let length: usize = self.pieces.iter().map(|piece| piece.text.len()).sum();
        let selection = selectable.selection.borrow();
        let Some((from, to)) = selection.range_in(selectable.key, length) else {
            return;
        };
        for fragment in &laid.fragments {
            let end = fragment.start + fragment.shaped.text.len();
            if to <= fragment.start || from >= end {
                continue;
            }
            let padding = (fragment.width - fragment.shaped.width) / 2.;
            let left = fragment
                .shaped
                .x_for_index(from.max(fragment.start) - fragment.start);
            let right = fragment.shaped.x_for_index(to.min(end) - fragment.start);
            let origin = point(
                bounds.origin.x + fragment.x + padding + left,
                bounds.origin.y + self.line_height * fragment.line as f32,
            );
            window.paint_quad(fill(
                Bounds::new(origin, size(right - left, self.line_height)),
                selectable.highlight,
            ));
        }
    }

    fn listen(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let Some(selectable) = self.selectable.clone() else {
            return;
        };
        let line_height = self.line_height;
        let laid = self.laid.clone();
        let position_at = move |point: Point<Pixels>, laid: &Laid, key| Position {
            key,
            offset: laid.index_at(point - bounds.origin, line_height),
        };
        let down = selectable.clone();
        let down_laid = laid.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && bounds.contains(&event.position)
            {
                let at = position_at(event.position, &down_laid.borrow(), down.key);
                down.selection.borrow_mut().begin(at);
                window.refresh();
            }
        });
        let moved = selectable.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.pressed_button == Some(MouseButton::Left)
                && bounds.contains(&event.position)
            {
                let at = position_at(event.position, &laid.borrow(), moved.key);
                moved.selection.borrow_mut().extend(at);
                window.refresh();
            }
        });
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
            if phase == DispatchPhase::Bubble {
                selectable.selection.borrow_mut().end();
            }
        });
    }
}

impl IntoElement for Inline {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Inline {
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
        let pieces = self.pieces.clone();
        let laid = self.laid.clone();
        let line_height = self.line_height;
        let id =
            window.request_measured_layout(Style::default(), move |known, available, window, _| {
                let limit = known.width.or(match available.width {
                    gpui::AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let result = lay_out(&pieces, limit, window);
                let measured: Size<Pixels> = size(result.width, line_height * result.lines as f32);
                *laid.borrow_mut() = result;
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
        self.listen(bounds, window);
        self.paint_selection(bounds, window);
        let laid = self.laid.borrow();
        for fragment in &laid.fragments {
            let piece = &self.pieces[fragment.piece];
            let top = bounds.origin.y + self.line_height * fragment.line as f32;
            let left = bounds.origin.x + fragment.x;
            let (text_origin, text_height): (Point<Pixels>, Pixels) = match &piece.boxed {
                Some(style) => {
                    window.paint_quad(quad(
                        Bounds::new(
                            point(left, top + style.top),
                            size(fragment.width, style.height),
                        ),
                        Corners::all(style.radius),
                        Background::from(style.background),
                        Edges::all(px(1.)),
                        style.border,
                        BorderStyle::Solid,
                    ));
                    (
                        point(left + style.padding_x, top + style.text_top),
                        style.text_line_height,
                    )
                }
                None => (point(left, top), self.line_height),
            };
            let _ =
                fragment
                    .shaped
                    .paint(text_origin, text_height, TextAlign::Left, None, window, cx);
        }
    }
}

/// The caret position nearest to `x`, as a browser picks it: the closer of a character's two edges.
fn nearest_caret(line: &ShapedLine, x: Pixels) -> usize {
    let text = line.text.as_ref();
    let edges = text.char_indices().map(|(at, _)| at).chain([text.len()]);
    edges
        .min_by(|a, b| {
            let distance = |index: usize| (line.x_for_index(index) - x).abs();
            distance(*a)
                .partial_cmp(&distance(*b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0)
}

impl Laid {
    /// The byte offset closest to a point relative to the element's origin.
    pub fn index_at(&self, position: Point<Pixels>, line_height: Pixels) -> usize {
        let line = (position.y / line_height).floor().max(0.) as usize;
        let line = line.min(self.lines.saturating_sub(1));
        let on_line = self
            .fragments
            .iter()
            .filter(|fragment| fragment.line == line);
        let mut last = 0;
        for fragment in on_line {
            let end = fragment.start + fragment.shaped.text.len();
            if position.x < fragment.x + fragment.width {
                let padding = (fragment.width - fragment.shaped.width) / 2.;
                let inside = (position.x - fragment.x - padding).max(px(0.));
                return fragment.start + nearest_caret(&fragment.shaped, inside);
            }
            last = end;
        }
        last
    }
}

#[cfg(test)]
mod tests {
    use super::{Position, Selection, words};

    fn at(message: u64, block: u32, offset: usize) -> Position {
        Position {
            key: (message, block),
            offset,
        }
    }

    #[test]
    fn empty_selection_has_no_range() {
        let mut selection = Selection::default();
        assert_eq!(selection.range(), None);
        selection.begin(at(1, 0, 3));
        assert_eq!(selection.range(), None);
    }

    #[test]
    fn backwards_drag_is_normalized() {
        let mut selection = Selection::default();
        selection.begin(at(5, 0, 2));
        selection.extend(at(3, 1, 4));
        assert_eq!(selection.range(), Some((at(3, 1, 4), at(5, 0, 2))));
    }

    #[test]
    fn range_in_covers_whole_middle_blocks_and_partial_ends() {
        let mut selection = Selection::default();
        selection.begin(at(1, 0, 2));
        selection.extend(at(3, 0, 4));
        assert_eq!(selection.range_in((1, 0), 10), Some((2, 10)));
        assert_eq!(selection.range_in((2, 0), 7), Some((0, 7)));
        assert_eq!(selection.range_in((3, 0), 10), Some((0, 4)));
        assert_eq!(selection.range_in((4, 0), 10), None);
    }

    #[test]
    fn extending_stops_after_the_mouse_is_released() {
        let mut selection = Selection::default();
        selection.begin(at(1, 0, 0));
        selection.extend(at(1, 0, 2));
        selection.end();
        selection.extend(at(1, 0, 9));
        assert_eq!(selection.range(), Some((at(1, 0, 0), at(1, 0, 2))));
    }

    #[test]
    fn words_keep_their_trailing_spaces() {
        assert_eq!(words("a bc  d"), [(0, "a "), (2, "bc  "), (6, "d")]);
    }

    #[test]
    fn words_cover_every_byte_of_multibyte_text() {
        let text = "çığ öş  ü";
        let parts = words(text);
        assert_eq!(
            parts.iter().map(|(_, word)| *word).collect::<String>(),
            text
        );
        assert_eq!(parts[1].0, "çığ ".len());
    }

    #[test]
    fn leading_space_stays_with_the_first_fragment() {
        assert_eq!(words("  a"), [(0, "  "), (2, "a")]);
    }

    #[test]
    fn empty_text_has_no_words() {
        assert!(words("").is_empty());
    }
}
