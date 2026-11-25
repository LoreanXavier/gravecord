//! A paragraph of the editor as laid out lines: plain text through gpui's wrapped line, or, when the paragraph holds
//! voids (`crate::editor_atoms`), text pieces and void boxes side by side, since a text run cannot carry a box's
//! padding, radius, icon or picture.
//!
//! A void box is module 787564's `inlineElement_T14dfz inlineVoid_T14dfz` span (`display:inline-block`) holding the
//! mention of module 332173 (`.wrapper_NDaLCQ`: `--mention-background`, 3px radius, `--mention-foreground`,
//! `--font-weight-medium`, `padding:0 2px`), drawn as the message pills are, or a custom emoji (`.emoji`: 1.375em,
//! `vertical-align:bottom`). It never breaks across rows.

use super::EditorStyle;
use gpui::{
    App, Bounds, Corners, Hsla, Pixels, Point, ShapedLine, SharedString, TextAlign, TextRun,
    TransformationMatrix, Window, WrappedLine, fill, point, px, size,
};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

/// How the composer draws one void, decided by the view that owns the data (names, roles, channels, pictures).
#[derive(Clone, Debug)]
pub struct AtomLook {
    /// What the box shows (`@Name`, `#channel`, or the raw token of someone the client does not know).
    pub label: String,
    pub color: Hsla,
    pub weight: f32,
    /// A pill: its background, horizontal padding, height, top and radius (the message pill's `BoxStyle`).
    pub pill: Option<Pill>,
    /// An icon before the label (a channel's), with its size and the room it takes.
    pub icon: Option<(SharedString, Pixels, Pixels)>,
    /// A picture in place of the label (a custom emoji), with its side.
    pub image: Option<(Arc<gpui::Image>, Pixels)>,
    /// What Copy writes for it (`mode: "plain"` of module 323350).
    pub copy: String,
    /// `preventSelection` of module 787564: a press on the box neither moves the caret nor starts a selection.
    pub prevent_selection: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Pill {
    pub background: Hsla,
    pub padding_x: Pixels,
    pub height: Pixels,
    pub top: Pixels,
    pub radius: Pixels,
}

pub(super) enum Line {
    Plain(WrappedLine),
    Rich(RichLine),
}

pub(super) struct RichLine {
    rows: Vec<Row>,
}

struct Row {
    start: usize,
    end: usize,
    width: Pixels,
    pieces: Vec<Piece>,
}

struct Piece {
    range: Range<usize>,
    x: Pixels,
    width: Pixels,
    body: Body,
}

enum Body {
    Text(ShapedLine),
    Atom {
        look: Rc<AtomLook>,
        label: Option<ShapedLine>,
    },
}

impl Line {
    pub(super) fn height(&self, line_height: Pixels) -> Pixels {
        match self {
            Line::Plain(line) => line.size(line_height).height.max(line_height),
            Line::Rich(rich) => (line_height * rich.rows.len() as f32).max(line_height),
        }
    }

    pub(super) fn width(&self, line_height: Pixels) -> Pixels {
        match self {
            Line::Plain(line) => line.size(line_height).width,
            Line::Rich(rich) => rich
                .rows
                .iter()
                .map(|row| row.width)
                .fold(px(0.), Pixels::max),
        }
    }

    pub(super) fn position(&self, local: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        match self {
            Line::Plain(line) => line.position_for_index(local, line_height),
            Line::Rich(rich) => {
                let index = rich.rows.iter().rposition(|row| row.start <= local)?;
                Some(point(
                    rich.rows[index].x_at(local),
                    line_height * index as f32,
                ))
            }
        }
    }

    pub(super) fn closest(&self, at: Point<Pixels>, line_height: Pixels) -> usize {
        match self {
            Line::Plain(line) => line
                .closest_index_for_position(at, line_height)
                .unwrap_or_else(|nearest| nearest),
            Line::Rich(rich) => {
                let Some(row) = rich.row_at(at.y, line_height) else {
                    return 0;
                };
                for piece in &row.pieces {
                    if at.x < piece.x + piece.width {
                        return match &piece.body {
                            Body::Text(shaped) => {
                                piece.range.start
                                    + shaped.closest_index_for_x((at.x - piece.x).max(px(0.)))
                            }
                            Body::Atom { .. } if at.x < piece.x + piece.width / 2. => {
                                piece.range.start
                            }
                            Body::Atom { .. } => piece.range.end,
                        };
                    }
                }
                row.end
            }
        }
    }

    /// The void under `at`, if any.
    pub(super) fn atom_at(
        &self,
        at: Point<Pixels>,
        line_height: Pixels,
    ) -> Option<(Range<usize>, Rc<AtomLook>)> {
        let Line::Rich(rich) = self else {
            return None;
        };
        let row = rich.row_at(at.y, line_height)?;
        row.pieces.iter().find_map(|piece| match &piece.body {
            Body::Atom { look, .. } if piece.x <= at.x && at.x < piece.x + piece.width => {
                Some((piece.range.clone(), look.clone()))
            }
            _ => None,
        })
    }

    /// The boxes that cover `from..to` (offsets in the paragraph), row by row, from the paragraph's top left.
    pub(super) fn rects(&self, from: usize, to: usize, line_height: Pixels) -> Vec<Bounds<Pixels>> {
        let mut result = Vec::new();
        match self {
            Line::Plain(line) => {
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
                            point(left.min(right), line_height * row as f32),
                            size((right - left).abs(), line_height),
                        ));
                    }
                    row_start = row_end;
                }
            }
            Line::Rich(rich) => {
                for (index, row) in rich.rows.iter().enumerate() {
                    let first = from.max(row.start);
                    let last = to.min(row.end);
                    if first < last {
                        let left = row.x_at(first);
                        let right = row.x_at(last);
                        result.push(Bounds::new(
                            point(left, line_height * index as f32),
                            size(right - left, line_height),
                        ));
                    }
                }
            }
        }
        result
    }

    pub(super) fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        match self {
            Line::Plain(line) => {
                let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
            }
            Line::Rich(rich) => {
                for (index, row) in rich.rows.iter().enumerate() {
                    let top = origin.y + line_height * index as f32;
                    for piece in &row.pieces {
                        let left = origin.x + piece.x;
                        match &piece.body {
                            Body::Text(shaped) => {
                                let _ = shaped.paint(
                                    point(left, top),
                                    line_height,
                                    TextAlign::Left,
                                    None,
                                    window,
                                    cx,
                                );
                            }
                            Body::Atom { look, label } => {
                                paint_atom(
                                    look,
                                    label.as_ref(),
                                    left,
                                    top,
                                    piece.width,
                                    line_height,
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_atom(
    look: &AtomLook,
    label: Option<&ShapedLine>,
    left: Pixels,
    top: Pixels,
    width: Pixels,
    line_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some((image, side)) = &look.image {
        if let Some(data) = image.clone().use_render_image(window, cx) {
            // `vertical-align: bottom` on the row; Chromium snaps replaced images to whole pixels.
            let at = point(left.round(), (top + line_height - *side).round());
            let area = Bounds::new(at, size(*side, *side));
            let frame = crate::ui::frame_clock::frame_index(&data, window);
            let _ = window.paint_image(area, area, Corners::default(), data, frame, false);
        }
        return;
    }
    let mut text_left = left;
    if let Some(pill) = &look.pill {
        window.paint_quad(
            fill(
                Bounds::new(point(left, top + pill.top), size(width, pill.height)),
                pill.background,
            )
            .corner_radii(pill.radius),
        );
        text_left += pill.padding_x;
        if let Some((path, icon_size, room)) = &look.icon {
            let at = point(text_left, top + pill.top + (pill.height - *icon_size) / 2.);
            let _ = window.paint_svg(
                Bounds::new(at, size(*icon_size, *icon_size)),
                path.clone(),
                None,
                TransformationMatrix::unit(),
                look.color,
                cx,
            );
            text_left += *room;
        }
    }
    if let Some(label) = label {
        let _ = label.paint(
            point(text_left, top),
            line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}

impl RichLine {
    fn row_at(&self, y: Pixels, line_height: Pixels) -> Option<&Row> {
        let index = (f32::from(y) / f32::from(line_height)).floor().max(0.) as usize;
        self.rows.get(index.min(self.rows.len().checked_sub(1)?))
    }
}

impl Row {
    fn x_at(&self, local: usize) -> Pixels {
        for piece in &self.pieces {
            if piece.range.start <= local && local <= piece.range.end {
                return match &piece.body {
                    Body::Text(shaped) => piece.x + shaped.x_for_index(local - piece.range.start),
                    Body::Atom { .. } if local == piece.range.start => piece.x,
                    Body::Atom { .. } => piece.x + piece.width,
                };
            }
        }
        self.width
    }
}

/// The runs of `range` out of a paragraph's runs.
fn slice_runs(runs: &[TextRun], range: &Range<usize>) -> Vec<TextRun> {
    let mut out = Vec::new();
    let mut at = 0;
    for run in runs {
        let (from, to) = (at.max(range.start), (at + run.len).min(range.end));
        if from < to {
            let mut part = run.clone();
            part.len = to - from;
            out.push(part);
        }
        at += run.len;
    }
    out
}

/// A paragraph with voids: `atoms` are byte ranges of `paragraph` with their looks, `runs` style the whole paragraph.
pub(super) fn rich_line(
    paragraph: &str,
    atoms: &[(Range<usize>, Rc<AtomLook>)],
    runs: &[TextRun],
    style: &EditorStyle,
    width: Option<Pixels>,
    window: &Window,
) -> RichLine {
    let system = window.text_system();
    let font_size = px(style.size);
    // The paragraph as text stretches and voids, in order.
    enum Item {
        Text(Range<usize>),
        Atom(Range<usize>, Rc<AtomLook>, Option<Box<ShapedLine>>, Pixels),
    }
    let mut items = Vec::new();
    let mut at = 0;
    for (range, look) in atoms {
        if at < range.start {
            items.push(Item::Text(at..range.start));
        }
        let (label, box_width) = match &look.image {
            Some((_, side)) => (None, *side),
            None => {
                let mut font = style.font.clone();
                font.weight = gpui::FontWeight(look.weight);
                let run = TextRun {
                    len: look.label.len(),
                    font,
                    color: look.color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = system.shape_line(
                    SharedString::from(look.label.clone()),
                    font_size,
                    &[run],
                    None,
                );
                let padding = look.pill.map_or(px(0.), |pill| pill.padding_x);
                let room = look.icon.as_ref().map_or(px(0.), |(_, _, room)| *room);
                let width = shaped.width + padding * 2. + room;
                (Some(Box::new(shaped)), width)
            }
        };
        items.push(Item::Atom(range.clone(), look.clone(), label, box_width));
        at = range.end;
    }
    if at < paragraph.len() {
        items.push(Item::Text(at..paragraph.len()));
    }
    let breaks: Vec<usize> = match width {
        Some(width) => {
            let fragments: Vec<gpui::LineFragment> = items
                .iter()
                .map(|item| match item {
                    Item::Text(range) => gpui::LineFragment::text(&paragraph[range.clone()]),
                    Item::Atom(range, _, _, width) => {
                        gpui::LineFragment::element(*width, range.len())
                    }
                })
                .collect();
            let mut wrapper = system.line_wrapper(style.font.clone(), font_size);
            wrapper
                .wrap_line(&fragments, width, gpui::IndentAdjustment::NoIndent)
                .map(|boundary| boundary.ix)
                .collect()
        }
        None => Vec::new(),
    };
    let mut edges = vec![0];
    edges.extend(breaks);
    edges.push(paragraph.len());
    let mut rows = Vec::new();
    for pair in edges.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let mut pieces = Vec::new();
        let mut x = px(0.);
        for item in &items {
            match item {
                Item::Text(range) => {
                    let (from, to) = (range.start.max(start), range.end.min(end));
                    if from >= to {
                        continue;
                    }
                    let piece_runs = slice_runs(runs, &(from..to));
                    let shaped = system.shape_line(
                        SharedString::from(paragraph[from..to].to_owned()),
                        font_size,
                        &piece_runs,
                        None,
                    );
                    let width = shaped.width;
                    pieces.push(Piece {
                        range: from..to,
                        x,
                        width,
                        body: Body::Text(shaped),
                    });
                    x += width;
                }
                Item::Atom(range, look, label, width) => {
                    if range.start < start || range.start >= end {
                        continue;
                    }
                    pieces.push(Piece {
                        range: range.clone(),
                        x,
                        width: *width,
                        body: Body::Atom {
                            look: look.clone(),
                            label: label.as_deref().cloned(),
                        },
                    });
                    x += *width;
                }
            }
        }
        rows.push(Row {
            start,
            end,
            width: x,
            pieces,
        });
    }
    RichLine { rows }
}
