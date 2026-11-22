//! Inline layout: text runs and inline boxes on shared lines, with wrapping and hit testing. Message content, embeds and
//! the composer render through this. `StyledText` cannot draw a box inside a run.

use gpui::{
    App, Background, BorderStyle, Bounds, Corners, CursorStyle, DispatchPhase, Edges, Element,
    ElementId, Font, GlobalElementId, Hitbox, HitboxBehavior, Hsla, Image, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ShapedLine, SharedString, Size, Style, TextAlign, TextRun, TransformationMatrix, Window,
    fill, point, px, quad, size,
};
use std::cell::{Cell, RefCell};
use std::cmp::{max, min};
use std::collections::BTreeMap;
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

impl Piece {
    fn is_interactive(&self) -> bool {
        self.hover.is_some()
            || self.hover_underline
            || self.image.is_some()
            || self.boxed.as_ref().is_some_and(|box_style| {
                box_style.icon.is_some() || box_style.background == self.color
            })
    }
}

/// A place in the document: a message id, a block inside it, and a byte offset in that block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub key: (u64, u32),
    pub offset: usize,
}

#[derive(Clone)]
struct SelectableBlock {
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    laid: Rc<RefCell<Laid>>,
    text: Rc<String>,
    epoch: u64,
}

#[derive(Default)]
pub struct Selection {
    anchor: Option<Position>,
    head: Option<Position>,
    dragging: bool,
    blocks: BTreeMap<(u64, u32), SelectableBlock>,
    epoch: u64,
}

impl std::fmt::Debug for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Selection")
            .field("anchor", &self.anchor)
            .field("head", &self.head)
            .field("dragging", &self.dragging)
            .field("blocks", &self.blocks.len())
            .finish()
    }
}

pub type SharedSelection = Rc<RefCell<Selection>>;

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut at = index.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Word,
    Space,
    Punct,
}

fn char_kind(ch: char) -> CharKind {
    if ch.is_alphanumeric() || ch == '_' {
        CharKind::Word
    } else if ch.is_whitespace() {
        CharKind::Space
    } else {
        CharKind::Punct
    }
}

impl Selection {
    pub fn begin_frame(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        let epoch = self.epoch;
        self.blocks
            .retain(|_, block| block.epoch.saturating_add(1) >= epoch);
    }

    fn register_block(
        &mut self,
        key: (u64, u32),
        bounds: Bounds<Pixels>,
        line_height: Pixels,
        laid: Rc<RefCell<Laid>>,
        text: Rc<String>,
    ) {
        self.blocks.insert(
            key,
            SelectableBlock {
                bounds,
                line_height,
                laid,
                text,
                epoch: self.epoch,
            },
        );
    }

    fn block_text(&self, key: (u64, u32)) -> Option<Rc<String>> {
        self.blocks.get(&key).map(|block| block.text.clone())
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

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

    /// Shift+click extends an existing anchor (or starts from `to` if none yet).
    pub fn extend_click(&mut self, to: Position) {
        if self.anchor.is_none() {
            self.anchor = Some(to);
        }
        self.dragging = true;
        self.extend(to);
    }

    pub fn select_range(&mut self, from: Position, to: Position) {
        self.anchor = Some(from);
        self.head = Some(to);
        self.dragging = false;
    }

    pub fn end(&mut self) {
        self.dragging = false;
    }

    /// Clears only the active selection range (preserving registered selectable blocks across same-frame mouse events).
    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragging = false;
    }

    /// Clears both the active selection and all registered block geometry (e.g. when switching channels).
    pub fn clear_all(&mut self) {
        *self = Self::default();
    }

    /// Updates the drag head against currently painted blocks in the chat viewport.
    pub fn drag_to_point(&mut self, point: Point<Pixels>) -> bool {
        if !self.dragging || self.blocks.is_empty() {
            return false;
        }
        let has_current = self.blocks.values().any(|block| block.epoch == self.epoch);
        let mut active: Vec<((u64, u32), &SelectableBlock)> = self
            .blocks
            .iter()
            .filter(|(_, block)| !has_current || block.epoch == self.epoch)
            .map(|(key, block)| (*key, block))
            .collect();
        if active.is_empty() {
            return false;
        }
        active.sort_by(|(ka, a), (kb, b)| {
            a.bounds
                .origin
                .y
                .partial_cmp(&b.bounds.origin.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| ka.cmp(kb))
        });
        let target = if let Some((key, block)) = active
            .iter()
            .find(|(_, block)| point.y < block.bounds.bottom())
        {
            if point.y < block.bounds.top() {
                Position {
                    key: *key,
                    offset: 0,
                }
            } else {
                let offset = block
                    .laid
                    .borrow()
                    .index_at(point - block.bounds.origin, block.line_height)
                    .min(block.text.len());
                Position { key: *key, offset }
            }
        } else {
            let (last_key, last_block) = active.last().copied().expect("non-empty active blocks");
            Position {
                key: last_key,
                offset: last_block.text.len(),
            }
        };
        if self.head == Some(target) {
            false
        } else {
            self.head = Some(target);
            true
        }
    }

    pub fn word_range_at(text: &str, offset: usize) -> (usize, usize) {
        if text.is_empty() {
            return (0, 0);
        }
        let mut cursor = floor_char_boundary(text, offset);
        if cursor == text.len() {
            cursor = text.char_indices().last().map_or(0, |(idx, _)| idx);
        }
        let Some(pivot) = text[cursor..].chars().next() else {
            return (0, 0);
        };
        let kind = char_kind(pivot);
        let start = text[..cursor]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| char_kind(*ch) == kind)
            .last()
            .map_or(cursor, |(idx, _)| idx);
        let end = text[cursor..]
            .char_indices()
            .find(|(_, ch)| char_kind(*ch) != kind)
            .map_or(text.len(), |(rel, _)| cursor + rel);
        (start, end)
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
    /// A line clamp left lines out.
    cut: bool,
}

/// `-webkit-line-clamp`: the lines past `lines` are left out and the last kept line ends in an ellipsis, cut further
/// until the ellipsis fits. An emoji or a boxed piece at the end of that line goes, since the ellipsis follows text.
fn clamp_lines(
    laid: &mut Laid,
    lines: usize,
    pieces: &[Piece],
    max_width: Option<Pixels>,
    window: &Window,
) {
    if lines == 0 || laid.lines <= lines {
        return;
    }
    laid.cut = true;
    laid.fragments.retain(|fragment| fragment.line < lines);
    laid.lines = lines;
    let limit = max_width.unwrap_or(laid.width);
    while let Some(last) = laid
        .fragments
        .iter()
        .rposition(|fragment| fragment.line == lines - 1)
    {
        let fragment = &laid.fragments[last];
        let piece = &pieces[fragment.piece];
        if piece.image.is_some() || piece.boxed.is_some() {
            laid.fragments.remove(last);
            continue;
        }
        let text = fragment.shaped.text.trim_end().to_owned();
        let ellipsis = shape(piece, "\u{2026}", window).width;
        let room = limit - fragment.x;
        let shaped = if fragment.shaped.x_for_index(text.len()) + ellipsis <= room {
            shape(piece, &format!("{text}\u{2026}"), window)
        } else {
            shorten_text(piece, &text, &fragment.shaped, room, window)
        };
        let fragment = &mut laid.fragments[last];
        fragment.width = shaped.width;
        fragment.shaped = shaped;
        break;
    }
    laid.width = laid
        .fragments
        .iter()
        .map(|fragment| fragment.x + fragment.width)
        .fold(px(0.), Pixels::max);
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

/// Splits an oversized word into scalar-aligned slices whose measured width stays within `limit`
/// (taking at least one Unicode scalar per slice so progress is guaranteed).
fn split_long_word_slices(
    word: &str,
    limit: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<(usize, &str)> {
    if word.is_empty() || limit <= 0. || measure(word) <= limit {
        return vec![(0, word)];
    }
    let mut ends: Vec<usize> = word.char_indices().skip(1).map(|(i, _)| i).collect();
    ends.push(word.len());
    let mut slices = Vec::new();
    let mut start = 0;
    let mut end_ix = 0;
    while start < word.len() {
        if measure(&word[start..]) <= limit {
            slices.push((start, &word[start..]));
            break;
        }
        let candidates = &ends[end_ix..ends.len() - 1];
        if candidates.is_empty() {
            slices.push((start, &word[start..]));
            break;
        }
        let fit = candidates.partition_point(|&end| measure(&word[start..end]) <= limit);
        let pick = fit.saturating_sub(1);
        let cut = candidates[pick];
        slices.push((start, &word[start..cut]));
        start = cut;
        end_ix += pick + 1;
    }
    slices
}

struct SegmentBox {
    max_width: Option<Pixels>,
    truncate: bool,
    padding: Pixels,
    icon: Pixels,
}

fn lay_out_segment(
    laid: &mut Laid,
    x: &mut Pixels,
    piece: &Piece,
    index: usize,
    seg: (usize, &str),
    cfg: &SegmentBox,
    window: &Window,
) -> bool {
    let (seg_offset, seg_text) = seg;
    let whole = shape(piece, seg_text, window);
    let fits = cfg
        .max_width
        .is_none_or(|limit| *x + whole.width + cfg.padding * 2. + cfg.icon <= limit);
    if cfg.truncate && !fits {
        let limit = cfg.max_width.unwrap_or(px(0.));
        let shortened = shorten_text(
            piece,
            seg_text,
            &whole,
            limit - *x - cfg.padding * 2.,
            window,
        );
        place(
            laid,
            x,
            None,
            (shortened, index, seg_offset),
            cfg.padding,
            cfg.icon,
            false,
        );
        return true;
    }
    if fits || piece.boxed.is_some() || seg_text.is_empty() {
        place(
            laid,
            x,
            cfg.max_width,
            (whole, index, seg_offset),
            cfg.padding,
            cfg.icon,
            piece.boxed.is_none(),
        );
        return false;
    }
    for (at, word) in words(seg_text) {
        let shaped = shape(piece, word, window);
        let trimmed_len = word.trim_end_matches([' ', '\t']).len();
        let word_visible = shaped.x_for_index(trimmed_len) + cfg.padding * 2. + cfg.icon;
        if let Some(limit) = cfg.max_width
            && word_visible > limit
        {
            if *x > px(0.) {
                laid.lines += 1;
                *x = px(0.);
            }
            let inner_limit = f32::from((limit - cfg.padding * 2. - cfg.icon).max(px(1.)));
            let slices = split_long_word_slices(word, inner_limit, |sub| {
                let trimmed = sub.trim_end_matches([' ', '\t']);
                if trimmed.is_empty() {
                    0.
                } else {
                    f32::from(shape(piece, trimmed, window).width)
                }
            });
            for (sub_at, sub) in slices {
                let sub_shaped = shape(piece, sub, window);
                place(
                    laid,
                    x,
                    cfg.max_width,
                    (sub_shaped, index, seg_offset + at + sub_at),
                    cfg.padding,
                    cfg.icon,
                    true,
                );
            }
        } else {
            place(
                laid,
                x,
                cfg.max_width,
                (shaped, index, seg_offset + at),
                cfg.padding,
                cfg.icon,
                true,
            );
        }
    }
    false
}

fn lay_out(
    pieces: &[Piece],
    max_width: Option<Pixels>,
    truncate: bool,
    indent: Pixels,
    window: &Window,
) -> Laid {
    let mut laid = Laid {
        lines: 1,
        ..Laid::default()
    };
    // `text-indent`: the first line starts this far in (the header of a compact message sits before it).
    let mut x = indent;
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
        let cfg = SegmentBox {
            max_width,
            truncate,
            padding,
            icon,
        };
        if !truncate && piece.text.contains('\n') {
            let mut seg_offset = offset;
            let split_cfg = SegmentBox {
                truncate: false,
                ..cfg
            };
            for (seg_idx, seg) in piece.text.split('\n').enumerate() {
                if seg_idx > 0 {
                    laid.lines += 1;
                    x = px(0.);
                }
                let _ = lay_out_segment(
                    &mut laid,
                    &mut x,
                    piece,
                    index,
                    (seg_offset, seg),
                    &split_cfg,
                    window,
                );
                seg_offset += seg.len() + 1;
            }
        } else if lay_out_segment(
            &mut laid,
            &mut x,
            piece,
            index,
            (offset, &piece.text),
            &cfg,
            window,
        ) {
            break;
        }
        offset += piece.text.len();
    }
    laid
}

/// The longest prefix that fits with an ellipsis after it.
fn shorten_text(
    piece: &Piece,
    source: &str,
    whole: &ShapedLine,
    width: Pixels,
    window: &Window,
) -> ShapedLine {
    let ellipsis = shape(piece, "\u{2026}", window).width;
    let room = (width - ellipsis).max(px(0.));
    let cut = caret_positions(whole)
        .into_iter()
        .take_while(|(_, x)| *x <= room)
        .last()
        .map_or(0, |(at, _)| at)
        .min(source.len());
    let cut = floor_char_boundary(source, cut);
    let text = format!("{}\u{2026}", source[..cut].trim_end());
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
    fill_width: bool,
    truncate: bool,
    /// `-webkit-line-clamp`, and where to say whether lines were left out.
    clamp: Option<(usize, Rc<std::cell::Cell<bool>>)>,
    on_piece_click: Option<PieceClick>,
    on_piece_click_at: Option<PieceClickAt>,
    on_piece_hover: Option<PieceHover>,
    on_piece_context: Option<PieceContext>,
    /// The pieces that are links, and what a middle click on one of them does (`onAuxClick` of MaskedLink).
    on_link_aux_click: Option<(Rc<Vec<bool>>, PieceClick)>,
    /// How far in the first line starts, measured when laid out.
    first_line_indent: Option<FirstLineIndent>,
}

/// The first line's indent, measured with the window's text system when the text is laid out.
pub type FirstLineIndent = Rc<dyn Fn(&Window) -> Pixels>;

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
    /// The link a middle press went down on: the element's press target and the piece.
    static AUX_PRESS: std::cell::Cell<Option<PressState>> = const { std::cell::Cell::new(None) };
    /// Where the link under the pointer is drawn, in window coordinates, as the last pointer move found it.
    static LINK_UNDER_POINTER: std::cell::Cell<Option<Bounds<Pixels>>> = const { std::cell::Cell::new(None) };
}

/// The content moved under the pointer (a wheel scroll, other rows): the link found by the last move is no longer
/// known to be there.
pub fn forget_link_under_pointer() {
    LINK_UNDER_POINTER.set(None);
}

/// Whether a middle press at `position` lands on a link. Blink starts no middle-click autoscroll on a live link
/// (`MouseEventManager`: `!hit.IsLiveLink()`): the press is the link's, whose `auxclick` opens it.
pub fn middle_press_on_link(position: Point<Pixels>) -> bool {
    LINK_UNDER_POINTER
        .get()
        .is_some_and(|bounds| bounds.contains(&position))
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
        && pressed
            .is_some_and(|press| press.target == current_target && press.piece == release_piece)
}

pub fn inline(pieces: Vec<Piece>, line_height: Pixels) -> Inline {
    Inline {
        pieces: Rc::new(pieces),
        line_height,
        laid: Rc::default(),
        selectable: None,
        fill_width: false,
        truncate: false,
        clamp: None,
        on_piece_click: None,
        on_piece_click_at: None,
        on_piece_hover: None,
        on_piece_context: None,
        on_link_aux_click: None,
        first_line_indent: None,
    }
}

impl Inline {
    /// Starts the first line `indent` in (`text-indent`); the lines after it start at the left edge.
    pub fn first_line_indent(mut self, indent: FirstLineIndent) -> Self {
        self.first_line_indent = Some(indent);
        self
    }

    /// Reports clicks on pieces, for spoilers that open where they are clicked.
    pub fn on_piece_click(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_piece_click = Some(Rc::new(handler));
        self
    }

    /// Adds a click handler beside the one already set (a plugin's piece next to the app's own).
    pub fn also_on_piece_click(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        let previous = self.on_piece_click.take();
        self.on_piece_click = Some(Rc::new(move |piece, window, cx| {
            if let Some(previous) = &previous {
                previous(piece, window, cx);
            }
            handler(piece, window, cx);
        }));
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

    /// At most `lines` lines, the last one ending in an ellipsis when more text follows (`-webkit-line-clamp`).
    /// `cut` is set after each layout to whether lines were left out.
    pub fn line_clamp(mut self, lines: usize, cut: Rc<std::cell::Cell<bool>>) -> Self {
        self.clamp = Some((lines, cut));
        self
    }

    /// Reports a middle click (press and release on the same piece) on one of the pieces `links` marks: MaskedLink's
    /// `onAuxClick: e => { 1 === e.button && x(e) }` (module 9578), the same handler as a left click.
    pub fn on_link_aux_click(
        mut self,
        links: Vec<bool>,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_link_aux_click = Some((Rc::new(links), Rc::new(handler)));
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
        self.fill_width = true;
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
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
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
        if let Some((links, handler)) = self.on_link_aux_click.clone() {
            let laid = self.laid.clone();
            let line_height = self.line_height;
            let target = press_target(self.selectable.as_ref(), bounds);
            let is_link = move |piece: usize| links.get(piece).copied().unwrap_or(false);
            let moved_laid = laid.clone();
            let moved_hitbox = hitbox.clone();
            let moved_link = is_link.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let over = moved_hitbox
                    .is_hovered(window)
                    .then(|| {
                        moved_laid
                            .borrow()
                            .fragment_at(event.position - bounds.origin, line_height)
                    })
                    .flatten()
                    .filter(|(piece, _, _)| moved_link(*piece))
                    .map(|(_, _, rect)| Bounds::new(bounds.origin + rect.origin, rect.size));
                match over {
                    Some(rect) => LINK_UNDER_POINTER.set(Some(rect)),
                    None => {
                        if LINK_UNDER_POINTER
                            .get()
                            .is_some_and(|rect| !rect.contains(&event.position))
                        {
                            LINK_UNDER_POINTER.set(None);
                        }
                    }
                }
            });
            let down_laid = laid.clone();
            let down_hitbox = hitbox.clone();
            let down_link = is_link.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Middle
                    && down_hitbox.is_hovered(window)
                {
                    let press = down_laid
                        .borrow()
                        .piece_at(event.position - bounds.origin, line_height)
                        .filter(|piece| down_link(*piece))
                        .map(|piece| PressState { target, piece });
                    if press.is_some() {
                        AUX_PRESS.set(press);
                    }
                }
            });
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Middle {
                    return;
                }
                let Some(pressed) = AUX_PRESS.get().filter(|press| press.target == target) else {
                    return;
                };
                AUX_PRESS.set(None);
                if !hitbox.is_hovered(window) {
                    return;
                }
                if let Some((piece, _, _)) = laid
                    .borrow()
                    .fragment_at(event.position - bounds.origin, line_height)
                    && piece == pressed.piece
                    && is_link(piece)
                {
                    handler(piece, window, cx);
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
                    if ACTIVE_PRESS
                        .get()
                        .is_some_and(|press| press.target == target)
                    {
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
        let hitbox_down = hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox_down.is_hovered(window)
            {
                let at = position_at(event.position, &down_laid.borrow(), down.key);
                let mut selection = down.selection.borrow_mut();
                if event.click_count >= 3 {
                    let len = selection.block_text(down.key).map_or(0, |text| text.len());
                    selection.select_range(
                        Position {
                            key: down.key,
                            offset: 0,
                        },
                        Position {
                            key: down.key,
                            offset: len,
                        },
                    );
                } else if event.click_count == 2 {
                    let (from, to) = selection
                        .block_text(down.key)
                        .map_or((at.offset, at.offset), |text| {
                            Selection::word_range_at(&text, at.offset)
                        });
                    selection.select_range(
                        Position {
                            key: down.key,
                            offset: from,
                        },
                        Position {
                            key: down.key,
                            offset: to,
                        },
                    );
                } else if event.modifiers.shift {
                    selection.extend_click(at);
                } else {
                    selection.begin(at);
                }
                window.refresh();
            }
        });
        let initial_interactive = hitbox.is_hovered(window)
            && laid
                .borrow()
                .piece_at(window.mouse_position() - bounds.origin, line_height)
                .is_some_and(|ix| self.pieces.get(ix).is_some_and(Piece::is_interactive));
        let last_interactive = Rc::new(Cell::new(initial_interactive));
        let cursor_pieces = self.pieces.clone();
        let cursor_laid = laid.clone();
        let cursor_hitbox = hitbox.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let interactive = cursor_hitbox.is_hovered(window)
                && cursor_laid
                    .borrow()
                    .piece_at(event.position - bounds.origin, line_height)
                    .is_some_and(|ix| cursor_pieces.get(ix).is_some_and(Piece::is_interactive));
            if interactive != last_interactive.get() {
                last_interactive.set(interactive);
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
        let fill_width = self.fill_width;
        let truncate = self.truncate;
        let clamp = self.clamp.clone();
        let indent = self.first_line_indent.clone();
        let id =
            window.request_measured_layout(Style::default(), move |known, available, window, _| {
                let limit = known.width.or(match available.width {
                    gpui::AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let indent = indent
                    .as_ref()
                    .map_or(px(0.), |indent| indent(window).max(px(0.)));
                let mut result = lay_out(&pieces, limit, truncate, indent, window);
                if let Some((lines, cut)) = &clamp {
                    clamp_lines(&mut result, *lines, &pieces, limit, window);
                    cut.set(result.cut);
                }
                let width = if fill_width {
                    limit.map_or(result.width, |w| result.width.max(w))
                } else {
                    result.width
                };
                let measured: Size<Pixels> = size(width, line_height * result.lines as f32);
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
        if let Some(selectable) = &self.selectable {
            let text = Rc::new(
                self.pieces
                    .iter()
                    .map(|piece| piece.text.as_str())
                    .collect::<String>(),
            );
            selectable.selection.borrow_mut().register_block(
                selectable.key,
                bounds,
                self.line_height,
                self.laid.clone(),
                text,
            );
        }
        if let Some(hitbox) = hitbox.as_ref() {
            if self.selectable.is_some() {
                let interactive = hitbox.is_hovered(window)
                    && self
                        .laid
                        .borrow()
                        .piece_at(window.mouse_position() - bounds.origin, self.line_height)
                        .is_some_and(|ix| self.pieces.get(ix).is_some_and(Piece::is_interactive));
                window.set_cursor_style(
                    if interactive {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::IBeam
                    },
                    hitbox,
                );
            }
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

    /// A box with an icon and no text (Translate+'s icon before its translation) is placed like any box: the
    /// segment with no text skips the word split (`seg_text.is_empty()`), and the fragment takes the icon's room.
    #[test]
    fn an_empty_boxed_piece_keeps_its_room() {
        use super::{Laid, place};
        let mut laid = Laid {
            lines: 1,
            ..Default::default()
        };
        let mut x = px(0.);
        place(
            &mut laid,
            &mut x,
            Some(px(200.)),
            (gpui::ShapedLine::default(), 0, 0),
            px(0.),
            px(20.),
            false,
        );
        assert_eq!(laid.fragments.len(), 1);
        assert_eq!(laid.fragments[0].width, px(20.));
        assert_eq!(laid.fragments[0].text_offset, px(20.));
        assert_eq!(x, px(20.));
        assert!(super::words("").is_empty());
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

    #[test]
    fn split_long_word_slices_respects_ascii_and_turkish_utf8_boundaries() {
        let ascii = "abcdefghijklmnopqrstuvwxyz0123456789";
        let slices = super::split_long_word_slices(ascii, 50., |s| s.chars().count() as f32 * 10.);
        assert!(slices.len() > 1);
        assert!(slices.iter().all(|(_, s)| s.chars().count() <= 5));
        assert_eq!(slices.iter().map(|(_, s)| *s).collect::<String>(), ascii);

        let turkish = "ğüşiöçĞÜŞİÖÇğüşiöç";
        let slices =
            super::split_long_word_slices(turkish, 25., |s| s.chars().count() as f32 * 10.);
        assert!(slices.iter().all(|(_, s)| s.chars().count() <= 2));
        assert_eq!(slices.iter().map(|(_, s)| *s).collect::<String>(), turkish);
    }

    #[test]
    fn word_range_at_handles_ascii_and_multibyte_words() {
        let text = "merhaba dünya!!";
        let (a, b) = Selection::word_range_at(text, 2);
        assert_eq!(&text[a..b], "merhaba");
        let mid = "merhaba ".len() + 2;
        let (c, d) = Selection::word_range_at(text, mid);
        assert_eq!(&text[c..d], "dünya");
        let (e, f) = Selection::word_range_at(text, text.len());
        assert_eq!(&text[e..f], "!!");
    }

    #[test]
    fn shift_click_extends_selection_and_clear_preserves_registered_blocks() {
        use super::Laid;
        use gpui::{Bounds, point, size};
        use std::{cell::RefCell, rc::Rc};

        let mut selection = Selection::default();
        selection.begin(at(1, 0, 1));
        selection.extend_click(at(1, 0, 5));
        assert_eq!(selection.range(), Some((at(1, 0, 1), at(1, 0, 5))));
        selection.end();
        selection.extend_click(at(2, 0, 3));
        assert_eq!(selection.range(), Some((at(1, 0, 1), at(2, 0, 3))));

        let laid = Rc::new(RefCell::new(Laid::default()));
        selection.register_block(
            (1, 0),
            Bounds::new(point(px(0.), px(0.)), size(px(100.), px(20.))),
            px(20.),
            laid,
            Rc::new("hello".to_owned()),
        );
        selection.clear();
        assert_eq!(selection.range(), None);
        assert!(selection.block_text((1, 0)).is_some());
        selection.clear_all();
        assert!(selection.block_text((1, 0)).is_none());
    }

    #[test]
    fn drag_to_point_selects_across_blocks_gaps_and_viewport_edges() {
        use super::{Fragment, Laid};
        use gpui::{
            Bounds, FontId, GlyphId, LineLayout, ShapedGlyph, ShapedLine, ShapedRun, point, size,
        };
        use std::{cell::RefCell, rc::Rc, sync::Arc};

        let make_laid = |text: &str| {
            let mut line = ShapedLine::default();
            line.text = text.to_owned().into();
            *line = Arc::new(LineLayout {
                width: px(text.len() as f32 * 10.),
                len: text.len(),
                runs: vec![ShapedRun {
                    font_id: FontId(0),
                    glyphs: text
                        .char_indices()
                        .enumerate()
                        .map(|(i, (index, _))| ShapedGlyph {
                            id: GlyphId(0),
                            position: point(px(i as f32 * 10.), px(0.)),
                            index,
                            is_emoji: false,
                        })
                        .collect(),
                }],
                ..Default::default()
            });
            Rc::new(RefCell::new(Laid {
                fragments: vec![Fragment {
                    shaped: line,
                    x: px(0.),
                    width: px(text.len() as f32 * 10.),
                    text_offset: px(0.),
                    line: 0,
                    piece: 0,
                    start: 0,
                }],
                lines: 1,
                width: px(text.len() as f32 * 10.),
                cut: false,
            }))
        };

        let mut selection = Selection::default();
        selection.register_block(
            (10, 0),
            Bounds::new(point(px(10.), px(100.)), size(px(100.), px(20.))),
            px(20.),
            make_laid("first"),
            Rc::new("first".to_owned()),
        );
        selection.register_block(
            (20, 0),
            Bounds::new(point(px(10.), px(140.)), size(px(100.), px(20.))),
            px(20.),
            make_laid("second"),
            Rc::new("second".to_owned()),
        );

        selection.begin(at(10, 0, 2));
        // Inside gap above second block selects up to start of second block.
        assert!(selection.drag_to_point(point(px(20.), px(130.))));
        assert_eq!(selection.range(), Some((at(10, 0, 2), at(20, 0, 0))));
        // Below all blocks selects through end of last block.
        assert!(selection.drag_to_point(point(px(20.), px(200.))));
        assert_eq!(selection.range(), Some((at(10, 0, 2), at(20, 0, 6))));
        // Backwards above first block clamps to start of first block.
        assert!(selection.drag_to_point(point(px(20.), px(40.))));
        assert_eq!(selection.range(), Some((at(10, 0, 0), at(10, 0, 2))));
    }
}
