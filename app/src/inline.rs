//! Inline layout: text runs and inline boxes on shared lines, with wrapping and hit testing. Message content, embeds and
//! the composer render through this. `StyledText` cannot draw a box inside a run.

use gpui::{
    App, Background, BorderStyle, Bounds, Corners, DispatchPhase, Edges, Element, ElementId, Font,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, Image, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine,
    SharedString, Size, Style, TextAlign, TextRun, TransformationMatrix, Window, fill, point, px,
    quad, size,
};
use std::cell::RefCell;
use std::cmp::{max, min};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct BoxStyle {
    /// An icon before the text, with the room it takes.
    pub icon: Option<(SharedString, Pixels, Pixels)>,
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
    /// An emoji drawn in place of text, with its size.
    pub image: Option<(Arc<Image>, Pixels)>,
    /// The background and text colours while the pointer is over the piece (a mention's `:hover`), reached over
    /// `.05s ease-out`.
    pub hover: Option<(Hsla, Hsla)>,
    /// The piece is underlined while the pointer is over it (`a:hover { text-decoration: underline }` of a link).
    pub hover_underline: bool,
    /// The text is drawn in a role's gradient colours (a role mention of the enhanced role styles, module 332173:
    /// the text sits in a `usernameGradient` span); the slide runs while the pointer is over the piece.
    pub gradient: Option<dc_state::RoleColors>,
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
    /// Where the text starts inside the fragment, after padding and any icon.
    text_offset: Pixels,
    line: usize,
    piece: usize,
    /// Byte offset of this fragment in the whole text.
    start: usize,
}

/// The motion key of a piece's hover state: its text and where it sits on its line, so it holds across frames
/// without growing when the list scrolls.
fn hover_key(piece: &Piece, fragment: &Fragment) -> u64 {
    crate::ui::motion::key(&(
        "mention",
        piece.text.as_str(),
        fragment.line,
        f32::from(fragment.x).round() as i32,
    ))
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

/// Decorations are painted by `paint_decorations`, not the run: gpui places them differently from Chromium.
fn run_for(piece: &Piece, length: usize) -> TextRun {
    TextRun {
        len: length,
        font: piece.font.clone(),
        color: piece.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// Blink's rules: thickness is a tenth of the font size, at least 1px; the underline sits 1px under the baseline and the
/// line-through two thirds of the way down the ascent.
fn paint_decorations(
    piece: &Piece,
    fragment: &Fragment,
    origin: Point<Pixels>,
    line_height: Pixels,
    window: &mut Window,
) {
    let ascent = fragment.shaped.ascent.round();
    let descent = fragment.shaped.descent.round();
    let top = origin.y + ((line_height - ascent - descent) / 2.).floor();
    let thickness = (piece.size / 10.).floor().max(px(1.));
    let bar = |y: Pixels| Bounds::new(point(origin.x, y), size(fragment.shaped.width, thickness));
    if piece.underline
        || (piece.hover_underline && crate::ui::motion::hovered(hover_key(piece, fragment)))
    {
        window.paint_quad(fill(bar(top + ascent + px(1.)), piece.color));
    }
    if piece.strike {
        window.paint_quad(fill(bar(top + (ascent * 2. / 3.).floor()), piece.color));
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
    icon: Pixels,
    hang_spaces: bool,
) {
    let (shaped, piece, start) = fragment;
    let width = shaped.width + padding * 2. + icon;
    // Collapsible spaces hang outside the line box; non-breaking spaces and inline boxes do not.
    let visible_width = if hang_spaces {
        shaped.x_for_index(shaped.text.trim_end_matches([' ', '\t']).len())
    } else {
        width
    };
    let overflow = max_width.is_some_and(|limit| *x + visible_width > limit);
    if overflow && *x > px(0.) {
        laid.lines += 1;
        *x = px(0.);
    }
    laid.fragments.push(Fragment {
        shaped,
        x: *x,
        width,
        text_offset: padding + icon,
        line: laid.lines - 1,
        piece,
        start,
    });
    laid.width = laid.width.max(*x + visible_width);
    *x += width;
}

fn lay_out(pieces: &[Piece], max_width: Option<Pixels>, truncate: bool, window: &Window) -> Laid {
    let mut laid = Laid {
        lines: 1,
        ..Laid::default()
    };
    let mut x = px(0.);
    let mut offset = 0;
    for (index, piece) in pieces.iter().enumerate() {
        if let Some((_, size)) = &piece.image {
            let empty = shape(piece, "", window);
            place(
                &mut laid,
                &mut x,
                max_width,
                (empty, index, offset),
                px(0.),
                *size,
                false,
            );
            offset += piece.text.len();
            continue;
        }
        let padding = piece.boxed.as_ref().map_or(px(0.), |style| style.padding_x);
        let icon = piece
            .boxed
            .as_ref()
            .and_then(|style| style.icon.as_ref())
            .map_or(px(0.), |(_, _, room)| *room);
        // Shaped whole when it fits, which keeps kerning across spaces. Split only to wrap.
        let whole = shape(piece, &piece.text, window);
        let fits = max_width.is_none_or(|limit| x + whole.width + padding * 2. + icon <= limit);
        if truncate && !fits {
            let limit = max_width.unwrap_or(px(0.));
            let shortened = shorten(piece, &whole, limit - x - padding * 2., window);
            place(
                &mut laid,
                &mut x,
                None,
                (shortened, index, offset),
                padding,
                icon,
                false,
            );
            break;
        }
        if fits || piece.boxed.is_some() {
            place(
                &mut laid,
                &mut x,
                max_width,
                (whole, index, offset),
                padding,
                icon,
                piece.boxed.is_none(),
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
                    icon,
                    true,
                );
            }
        }
        offset += piece.text.len();
    }
    laid
}

/// The longest prefix that fits with an ellipsis after it.
fn shorten(piece: &Piece, whole: &ShapedLine, width: Pixels, window: &Window) -> ShapedLine {
    let ellipsis = shape(piece, "\u{2026}", window).width;
    let room = (width - ellipsis).max(px(0.));
    let cut = caret_positions(whole)
        .into_iter()
        .take_while(|(_, x)| *x <= room)
        .last()
        .map_or(0, |(at, _)| at);
    let text = format!("{}\u{2026}", piece.text[..cut].trim_end());
    shape(piece, &text, window)
}

/// Called with the index of the piece a click landed on.
type PieceClick = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// Called with the index of the piece a click landed on and where that piece is drawn, in window coordinates.
/// A right press on a piece, with where the pointer is.
type PieceContext = Rc<dyn Fn(usize, Point<Pixels>, &mut Window, &mut App)>;

type PieceClickAt = Rc<dyn Fn(usize, Bounds<Pixels>, &mut Window, &mut App)>;

/// Called when the pointer moves onto another piece (with where it is drawn) or leaves them all.
type PieceHover = Rc<dyn Fn(Option<(usize, Bounds<Pixels>)>, &mut Window, &mut App)>;

pub struct Inline {
    pieces: Rc<Vec<Piece>>,
    line_height: Pixels,
    laid: Rc<RefCell<Laid>>,
    selectable: Option<Selectable>,
    truncate: bool,
    on_piece_click: Option<PieceClick>,
    on_piece_click_at: Option<PieceClickAt>,
    on_piece_hover: Option<PieceHover>,
    on_piece_context: Option<PieceContext>,
}

#[derive(Clone)]
struct Selectable {
    key: (u64, u32),
    selection: SharedSelection,
    highlight: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PressTarget {
    Selectable((u64, u32)),
    Position(i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PressState {
    target: PressTarget,
    piece: usize,
}

thread_local! {
    static ACTIVE_PRESS: std::cell::Cell<Option<PressState>> = const { std::cell::Cell::new(None) };
}

fn press_target(selectable: Option<&Selectable>, bounds: Bounds<Pixels>) -> PressTarget {
    match selectable {
        Some(selectable) => PressTarget::Selectable(selectable.key),
        None => PressTarget::Position(
            f32::from(bounds.origin.x).round() as i32,
            f32::from(bounds.origin.y).round() as i32,
        ),
    }
}

fn press_matches_release(
    pressed: Option<PressState>,
    current_target: PressTarget,
    release_piece: usize,
    has_selection: bool,
) -> bool {
    !has_selection
        && pressed.is_some_and(|press| press.target == current_target && press.piece == release_piece)
}

pub fn inline(pieces: Vec<Piece>, line_height: Pixels) -> Inline {
    Inline {
        pieces: Rc::new(pieces),
        line_height,
        laid: Rc::default(),
        selectable: None,
        truncate: false,
        on_piece_click: None,
        on_piece_click_at: None,
        on_piece_hover: None,
        on_piece_context: None,
    }
}

impl Inline {
    /// Reports clicks on pieces, for spoilers that open where they are clicked.
    pub fn on_piece_click(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_piece_click = Some(Rc::new(handler));
        self
    }

    /// As `on_piece_click`, with the bounds of the piece that was clicked (for popouts that open beside it).
    pub fn on_piece_click_at(
        mut self,
        handler: impl Fn(usize, Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_piece_click_at = Some(Rc::new(handler));
        self
    }

    /// Reports the piece under the pointer when it changes, with its bounds (for tooltips on pieces).
    pub fn on_piece_hover(
        mut self,
        handler: impl Fn(Option<(usize, Bounds<Pixels>)>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_piece_hover = Some(Rc::new(handler));
        self
    }

    /// Reports a right press on a piece with the pointer position (for the menu of a link). It does not stop the
    /// event, so the surrounding message menu still opens, and can read what this recorded.
    pub fn on_piece_context(
        mut self,
        handler: impl Fn(usize, Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_piece_context = Some(Rc::new(handler));
        self
    }

    /// One line, cut with an ellipsis.
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }

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
            let padding = fragment.text_offset;
            let left = caret_x(&fragment.shaped, from.max(fragment.start) - fragment.start);
            let right = caret_x(&fragment.shaped, to.min(end) - fragment.start);
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

    /// Whether the text answers the mouse at all: only then it gets a hitbox.
    fn listens(&self) -> bool {
        self.selectable.is_some()
            || self.on_piece_click.is_some()
            || self.on_piece_click_at.is_some()
            || self.on_piece_hover.is_some()
            || self.on_piece_context.is_some()
            || self
                .pieces
                .iter()
                .any(|piece| piece.hover.is_some() || piece.hover_underline)
    }

    /// The listeners ask the hitbox, not the bounds: a layer above the text (User Settings, a dialog, a menu) takes
    /// the mouse, and a link under it must not open or light up.
    fn listen(&self, bounds: Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window) {
        if self
            .pieces
            .iter()
            .any(|piece| piece.hover.is_some() || piece.hover_underline)
        {
            let laid = self.laid.clone();
            let pieces = self.pieces.clone();
            let line_height = self.line_height;
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let over = hitbox.is_hovered(window).then(|| {
                    laid.borrow()
                        .hover_piece_at(event.position - bounds.origin, line_height)
                });
                let laid = laid.borrow();
                for fragment in &laid.fragments {
                    if pieces[fragment.piece].hover.is_none()
                        && !pieces[fragment.piece].hover_underline
                    {
                        continue;
                    }
                    let key = hover_key(&pieces[fragment.piece], fragment);
                    let inside = over.flatten().is_some_and(|at| at == fragment.start);
                    if crate::ui::motion::hovered(key) != inside {
                        crate::ui::motion::set_hover(key, inside);
                        window.refresh();
                    }
                }
            });
        }
        if let Some(handler) = self.on_piece_hover.clone() {
            let laid = self.laid.clone();
            let line_height = self.line_height;
            let last: Rc<RefCell<Option<(usize, usize)>>> = Rc::default();
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let over = if hitbox.is_hovered(window) {
                    laid.borrow()
                        .fragment_at(event.position - bounds.origin, line_height)
                } else {
                    None
                };
                let now = over.map(|(piece, fragment, _)| (piece, fragment));
                if *last.borrow() == now {
                    return;
                }
                *last.borrow_mut() = now;
                handler(
                    over.map(|(piece, _, rect)| {
                        (piece, Bounds::new(bounds.origin + rect.origin, rect.size))
                    }),
                    window,
                    cx,
                );
            });
        }
        if let Some(handler) = self.on_piece_context.clone() {
            let laid = self.laid.clone();
            let line_height = self.line_height;
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Right
                    && hitbox.is_hovered(window)
                    && let Some(piece) = laid
                        .borrow()
                        .piece_at(event.position - bounds.origin, line_height)
                {
                    handler(piece, event.position, window, cx);
                }
            });
        }
        if self.on_piece_click_at.is_some() || self.on_piece_click.is_some() {
            let click_at = self.on_piece_click_at.clone();
            let click = self.on_piece_click.clone();
            let laid = self.laid.clone();
            let line_height = self.line_height;
            let hitbox = hitbox.clone();
            let down_laid = laid.clone();
            let down_hitbox = hitbox.clone();
            let target = press_target(self.selectable.as_ref(), bounds);
            let selection = self.selectable.as_ref().map(|s| s.selection.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && down_hitbox.is_hovered(window)
                {
                    let press = down_laid
                        .borrow()
                        .piece_at(event.position - bounds.origin, line_height)
                        .map(|piece| PressState { target, piece });
                    ACTIVE_PRESS.set(press);
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if !hitbox.is_hovered(window) {
                    if ACTIVE_PRESS.get().is_some_and(|press| press.target == target) {
                        ACTIVE_PRESS.set(None);
                    }
                    return;
                }
                let pressed = ACTIVE_PRESS.take();
                let has_selection = selection
                    .as_ref()
                    .is_some_and(|sel| sel.borrow().range().is_some());
                if let Some((piece, _, rect)) = laid
                    .borrow()
                    .fragment_at(event.position - bounds.origin, line_height)
                    && press_matches_release(pressed, target, piece, has_selection)
                {
                    if let Some(handler) = &click_at {
                        handler(
                            piece,
                            Bounds::new(bounds.origin + rect.origin, rect.size),
                            window,
                            cx,
                        );
                    }
                    if let Some(handler) = &click {
                        handler(piece, window, cx);
                    }
                }
            });
        }
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
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox.is_hovered(window)
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
    type PrepaintState = Option<Hitbox>;

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
        let truncate = self.truncate;
        let id =
            window.request_measured_layout(Style::default(), move |known, available, window, _| {
                let limit = known.width.or(match available.width {
                    gpui::AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let result = lay_out(&pieces, limit, truncate, window);
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
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Option<Hitbox> {
        self.listens()
            .then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(hitbox) = hitbox.as_ref() {
            self.listen(bounds, hitbox, window);
        }
        self.paint_selection(bounds, window);
        let laid = self.laid.borrow();
        // Plain text of different sizes shares a baseline, set by the line's largest text, as inline boxes do.
        let mut main: Vec<(Pixels, Pixels, Pixels)> = Vec::new();
        for fragment in &laid.fragments {
            let piece = &self.pieces[fragment.piece];
            if piece.boxed.is_some() || piece.image.is_some() {
                continue;
            }
            if main.len() <= fragment.line {
                main.resize(fragment.line + 1, (px(0.), px(0.), px(0.)));
            }
            if piece.size > main[fragment.line].0 {
                main[fragment.line] = (
                    piece.size,
                    fragment.shaped.ascent.round(),
                    fragment.shaped.descent.round(),
                );
            }
        }
        for fragment in &laid.fragments {
            let piece = &self.pieces[fragment.piece];
            let top = bounds.origin.y + self.line_height * fragment.line as f32;
            let left = bounds.origin.x + fragment.x;
            if let Some((image, size)) = &piece.image {
                if let Some(data) = image.clone().use_render_image(window, cx) {
                    gpui::note_painted_image(window, image, &data);
                    // Chromium snaps replaced images to whole pixels.
                    let area =
                        Bounds::new(point(left.round(), top.round()), gpui::size(*size, *size));
                    let frame = crate::ui::frame_clock::frame_index(&data, window);
                    let _ = window.paint_image(area, area, Corners::default(), data, frame, false);
                }
                continue;
            }
            let mut recolored: Option<Hsla> = None;
            let (text_origin, text_height): (Point<Pixels>, Pixels) = match &piece.boxed {
                Some(style) => {
                    if let Some((path, icon_size, _)) = &style.icon {
                        let at = point(
                            left + style.padding_x,
                            top + style.top + (style.height - *icon_size) / 2.,
                        );
                        let _ = window.paint_svg(
                            Bounds::new(at, size(*icon_size, *icon_size)),
                            path.clone(),
                            None,
                            TransformationMatrix::unit(),
                            piece.color,
                            cx,
                        );
                    }
                    let (background, text) = match piece.hover {
                        Some((hover_background, hover_text)) => {
                            let key = hover_key(piece, fragment);
                            let target = crate::ui::motion::hovered(key);
                            let ease =
                                crate::ui::motion::css(50., crate::ui::motion::Ease::EaseOut);
                            let shade = |base: Hsla, over: Hsla, tag: u64| {
                                let shown = crate::ui::motion::color(
                                    key ^ tag,
                                    gpui::Rgba::from(if target { over } else { base }),
                                    ease,
                                );
                                Hsla::from(shown)
                            };
                            (
                                shade(style.background, hover_background, 0x11),
                                shade(piece.color, hover_text, 0x12),
                            )
                        }
                        None => (style.background, piece.color),
                    };
                    if text != piece.color {
                        recolored = Some(text);
                    }
                    window.paint_quad(quad(
                        Bounds::new(
                            point(left, top + style.top),
                            size(fragment.width, style.height),
                        ),
                        Corners::all(style.radius),
                        Background::from(background),
                        Edges::all(px(1.)),
                        style.border,
                        BorderStyle::Solid,
                    ));
                    (
                        point(left + fragment.text_offset, top + style.text_top),
                        style.text_line_height,
                    )
                }
                None => match main.get(fragment.line) {
                    Some(&(largest, ascent, descent)) if piece.size < largest => {
                        let baseline =
                            top + ((self.line_height - ascent - descent) / 2.).floor() + ascent;
                        let own_ascent = fragment.shaped.ascent.round();
                        let own_height = own_ascent + fragment.shaped.descent.round();
                        (point(left, baseline - own_ascent), own_height)
                    }
                    _ => (point(left, top), self.line_height),
                },
            };
            let shaped = if let Some(colors) = &piece.gradient {
                // The gradient slides while the pointer is on the mention (`gradientUsernameAnimation`).
                let sliding = piece.hover.is_some()
                    && crate::ui::motion::hovered(hover_key(piece, fragment))
                    && !crate::ui::motion::reduced();
                if sliding {
                    window.request_animation_frame();
                }
                let text = fragment.shaped.text.as_ref();
                let offset = if sliding {
                    crate::ui::role_gradient::offset_now()
                } else {
                    0.
                };
                let runs = crate::ui::role_gradient::runs_for(
                    text,
                    &fragment.shaped,
                    &run_for(piece, 0),
                    colors,
                    offset,
                );
                Some(window.text_system().shape_line(
                    SharedString::from(text.to_owned()),
                    piece.size,
                    &runs,
                    None,
                ))
            } else {
                recolored.map(|text| {
                    let mut shown = piece.clone();
                    shown.color = text;
                    shape(&shown, fragment.shaped.text.as_ref(), window)
                })
            };
            let _ = shaped.as_ref().unwrap_or(&fragment.shaped).paint(
                text_origin,
                text_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            paint_decorations(piece, fragment, text_origin, text_height, window);
        }
    }
}

/// A ligature is one glyph over several characters. gpui reports a character inside one at the glyph's end, so it shares
/// its x with the next caret. Like Chromium, this divides the glyph evenly between its characters.
fn spread_ligatures(carets: &mut [(usize, Pixels)]) {
    let mut index = 1;
    while index + 1 < carets.len() {
        if carets[index].1 != carets[index + 1].1 {
            index += 1;
            continue;
        }
        let mut last = index + 1;
        while last + 1 < carets.len() && carets[last + 1].1 == carets[index].1 {
            last += 1;
        }
        let (from, to) = (carets[index - 1].1, carets[last].1);
        let steps = (last - index + 1) as f32;
        for (step, caret) in carets[index..last].iter_mut().enumerate() {
            caret.1 = from + (to - from) * ((step + 1) as f32 / steps);
        }
        index = last + 1;
    }
}

/// The x of every caret position.
fn caret_positions(line: &ShapedLine) -> Vec<(usize, Pixels)> {
    let text = line.text.as_ref();
    let mut carets: Vec<(usize, Pixels)> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .map(|at| (at, line.x_for_index(at)))
        .collect();
    spread_ligatures(&mut carets);
    carets
}

fn caret_x(line: &ShapedLine, index: usize) -> Pixels {
    caret_positions(line)
        .iter()
        .find(|(at, _)| *at >= index)
        .map_or(line.width, |(_, x)| *x)
}

/// The caret position nearest to `x`, as a browser picks it: the closer of a character's two edges.
fn nearest_caret(line: &ShapedLine, x: Pixels) -> usize {
    caret_positions(line)
        .into_iter()
        .min_by(|a, b| {
            let (left, right) = ((a.1 - x).abs(), (b.1 - x).abs());
            left.partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(at, _)| at)
}

impl Laid {
    /// The byte offset closest to a point relative to the element's origin.
    /// The piece drawn under `position`, if any.
    pub fn piece_at(&self, position: Point<Pixels>, line_height: Pixels) -> Option<usize> {
        let line = (position.y / line_height).floor().max(0.) as usize;
        self.fragments
            .iter()
            .find(|fragment| {
                fragment.line == line
                    && position.x >= fragment.x
                    && position.x < fragment.x + fragment.width
            })
            .map(|fragment| fragment.piece)
    }

    /// The piece under a point, the fragment's start, and the fragment's rectangle relative to the element's origin.
    fn fragment_at(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Option<(usize, usize, Bounds<Pixels>)> {
        let line = (position.y / line_height).floor().max(0.) as usize;
        self.fragments
            .iter()
            .find(|fragment| {
                fragment.line == line
                    && position.x >= fragment.x
                    && position.x < fragment.x + fragment.width
            })
            .map(|fragment| {
                (
                    fragment.piece,
                    fragment.start,
                    Bounds::new(
                        point(fragment.x, line_height * fragment.line as f32),
                        size(fragment.width, line_height),
                    ),
                )
            })
    }

    /// The start of the fragment under a point, when it is one with a hover look.
    fn hover_piece_at(&self, position: Point<Pixels>, line_height: Pixels) -> Option<usize> {
        let line = (position.y / line_height).floor().max(0.) as usize;
        self.fragments
            .iter()
            .find(|fragment| {
                fragment.line == line
                    && position.x >= fragment.x
                    && position.x < fragment.x + fragment.width
            })
            .map(|fragment| fragment.start)
    }

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
                let inside = (position.x - fragment.x - fragment.text_offset).max(px(0.));
                return fragment.start + nearest_caret(&fragment.shaped, inside);
            }
            last = end;
        }
        last
    }
}

#[cfg(test)]
mod tests {
    use super::{Position, Selection, spread_ligatures, words};
    use gpui::px;

    #[test]
    fn line_end_spaces_hang_but_nonbreaking_spaces_and_boxes_do_not() {
        use super::{Laid, place};
        use gpui::{FontId, GlyphId, LineLayout, ShapedGlyph, ShapedLine, ShapedRun, point};
        use std::sync::Arc;
        let shaped = |text: &str| {
            let mut line = ShapedLine::default();
            line.text = text.to_owned().into();
            *line = Arc::new(LineLayout {
                width: px(10.),
                len: text.len(),
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs: text
                        .char_indices()
                        .enumerate()
                        .map(|(i, (index, _))| ShapedGlyph {
                            id: GlyphId(0),
                            position: point(px(i as f32 * 3.), px(0.)),
                            index,
                            is_emoji: false,
                        })
                        .collect(),
                }],
                ..Default::default()
            });
            line
        };
        for (text, hang_spaces, expected_line) in [
            ("abc ", true, 0),
            ("abc\u{a0}", true, 1),
            ("abc ", false, 1),
        ] {
            let mut laid = Laid {
                lines: 1,
                ..Default::default()
            };
            let mut x = px(1.);
            place(
                &mut laid,
                &mut x,
                Some(px(10.)),
                (shaped(text), 0, 0),
                px(0.),
                px(0.),
                hang_spaces,
            );
            assert_eq!(laid.fragments[0].line, expected_line);
            if hang_spaces && text.ends_with(' ') {
                assert_eq!(laid.width, px(10.));
                assert_eq!(x, px(11.));
                place(
                    &mut laid,
                    &mut x,
                    Some(px(10.)),
                    (shaped("next"), 0, 4),
                    px(0.),
                    px(0.),
                    true,
                );
                assert_eq!(laid.fragments[1].line, 1);
                assert_eq!(laid.fragments[1].start, 4);
            }
        }
    }

    #[test]
    fn a_ligature_is_divided_evenly() {
        // "fi" as one glyph ending at 8, then "l" ending at 12.
        let mut carets = [(0, px(0.)), (1, px(8.)), (2, px(8.)), (3, px(12.))];
        spread_ligatures(&mut carets);
        assert_eq!(carets[1].1, px(4.));
        assert_eq!(carets[2].1, px(8.));
        assert_eq!(carets[3].1, px(12.));
    }

    #[test]
    fn a_three_character_ligature_gets_thirds() {
        let mut carets = [(0, px(3.)), (1, px(9.)), (2, px(9.)), (3, px(9.))];
        spread_ligatures(&mut carets);
        assert_eq!(carets[1].1, px(5.));
        assert_eq!(carets[2].1, px(7.));
        assert_eq!(carets[3].1, px(9.));
    }

    #[test]
    fn plain_text_is_untouched() {
        let mut carets = [(0, px(0.)), (1, px(4.)), (2, px(9.))];
        spread_ligatures(&mut carets);
        assert_eq!(carets, [(0, px(0.)), (1, px(4.)), (2, px(9.))]);
    }

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

    /// gpui's wrapped text (settings copy, tooltips, embeds) follows CSS `white-space: normal`: a space after the
    /// last word that fits hangs past the line box instead of pushing that word down. The no-op text system gives
    /// every character 9.6px at 16px.
    #[test]
    fn wrapped_text_lets_trailing_spaces_hang() {
        use gpui::{NoopTextSystem, TextRun, TextSystem, WindowTextSystem, font};
        use std::sync::Arc;
        let system =
            WindowTextSystem::new(Arc::new(TextSystem::new(Arc::new(NoopTextSystem::new()))));
        let breaks = |text: &str, width: f32| -> Vec<usize> {
            let run = TextRun {
                len: text.len(),
                font: font("Test"),
                color: Default::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let lines = system
                .shape_text(
                    text.to_owned().into(),
                    px(16.),
                    &[run],
                    Some(px(width)),
                    None,
                )
                .unwrap();
            lines[0]
                .wrap_boundaries()
                .iter()
                .map(|boundary| boundary.glyph_ix)
                .collect()
        };
        // "aa bbb" is 57.6px wide, so 58px holds it and the following space overflows.
        assert_eq!(breaks("aa bbb cc", 58.), [7]);
        assert_eq!(breaks("aa bbb  cc", 58.), [8]);
        assert_eq!(breaks("aa bbb\tcc", 58.), [7]);
        assert!(breaks("aaaa  ", 39.).is_empty());
        // One glyph over the width still wraps the whole word.
        assert_eq!(breaks("aa bbbb cc", 58.), [3, 8]);
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

    #[test]
    fn press_matches_release_survives_repaint_on_same_piece_and_rejects_selection_or_other_piece() {
        use super::{ACTIVE_PRESS, PressState, PressTarget, press_matches_release};
        let target = PressTarget::Selectable((42, 3));
        ACTIVE_PRESS.set(Some(PressState { target, piece: 1 }));
        assert!(press_matches_release(
            ACTIVE_PRESS.get(),
            PressTarget::Selectable((42, 3)),
            1,
            false
        ));
        assert!(!press_matches_release(
            ACTIVE_PRESS.get(),
            PressTarget::Selectable((42, 3)),
            2,
            false
        ));
        assert!(!press_matches_release(
            ACTIVE_PRESS.get(),
            PressTarget::Selectable((99, 0)),
            1,
            false
        ));
        assert!(!press_matches_release(
            ACTIVE_PRESS.get(),
            PressTarget::Selectable((42, 3)),
            1,
            true
        ));
        ACTIVE_PRESS.set(None);
    }
}
