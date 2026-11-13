//! The connection popout's ping graph, shown with Developer Mode on (module 926641 draws module 229659's `A` with
//! `dataPoints: pings, width: 258, height: 80` only while `developerMode` of module 253932 is set).
//!
//! Module 229659 hands a canvas to Chromium's webrtc-internals `TimelineGraphView` (module 235002) with one data
//! series (`u`, sampling `RTCConnection.getPings()`), `timeOptions: {timeStyle: "short"}`, gg sans 11px, the
//! background `BACKGROUND_BASE_LOW`, text `TEXT_DEFAULT`, grid `BACKGROUND_MOD_MUTED` and the line `BRAND_500`. No
//! `windowMs`, markers or hover source are passed by the popout, so the scale stays one second per CSS pixel and the
//! graph has no crosshair and no tooltip. The view works in device pixels; this port does the same arithmetic and
//! divides by the scale factor only when it paints.

use crate::ui::color;
use gpui::{AnyElement, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px};

/// The canvas, CSS pixels.
pub(super) const WIDTH: f32 = 258.;
pub(super) const HEIGHT: f32 = 80.;
/// `fontSize = 11`.
const FONT_SIZE: f64 = 11.;
/// `graphContainer_BlzJTq` around the inline canvas: the canvas, the line box's descent under it and `padding-bottom:
/// 2px`, 85 tall in the owner's screenshot of the official popout.
pub(super) const CONTAINER_HEIGHT: f32 = 85.;
/// `drawTimeLabels` steps by a minute.
const MINUTE_MS: f64 = 60_000.;

/// One `_pings` entry: `{time: Date.now(), value}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub time: f64,
    pub value: f64,
}

/// `Math.round`.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `getValues` of module 229659's series: the value held at each column, starting from the first point when the
/// second follows within one and a half spacings of the window's start, else from 0.
pub(super) fn values(points: &[Sample], start: f64, step: f64, count: usize) -> Vec<f64> {
    let mut held = 0.;
    if let [first, second, ..] = points {
        let spacing = second.time - first.time;
        if spacing > 0. && first.time - start < 1.5 * spacing {
            held = first.value;
        }
    }
    let mut out = Vec::with_capacity(count);
    let mut index = 0;
    let mut time = start;
    for _ in 0..count {
        while let Some(point) = points.get(index) {
            if point.time >= time {
                break;
            }
            held = point.value;
            index += 1;
        }
        out.push(held);
        time += step;
    }
    out
}

/// The value axis: `min_`, `max_` and `labels_` of the graph after `layoutLabels_`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Axis {
    pub min: f64,
    pub max: f64,
    pub labels: Vec<String>,
}

/// `layoutLabels_`: below a range of 1024 the plain labels with two decimals to start from, otherwise the values in
/// steps of 1024 with a k, M, G, T or P suffix.
pub(super) fn layout_labels(min: f64, max: f64, height: f64, font_height: f64) -> Axis {
    if max - min < 1024. {
        return layout_basic(min, max, 2, height, font_height);
    }
    const SUFFIXES: [&str; 6] = ["", "k", "M", "G", "T", "P"];
    let mut unit = 1;
    let (mut low, mut high) = (min / 1024., max / 1024.);
    while unit + 1 < SUFFIXES.len() && high - low >= 1024. {
        low /= 1024.;
        high /= 1024.;
        unit += 1;
    }
    let mut axis = layout_basic(low, high, 2, height, font_height);
    for label in &mut axis.labels {
        label.push(' ');
        label.push_str(SUFFIXES[unit]);
    }
    let factor = 1024f64.powi(unit as i32);
    axis.min *= factor;
    axis.max *= factor;
    axis
}

/// `layoutLabelsBasic_`: as many labels as fit at two font heights plus 4 apart (2 to 6), on a step of 1, 2 or 5
/// times a power of ten, dropping a decimal for each power climbed.
pub(super) fn layout_basic(
    min: f64,
    max: f64,
    decimals: usize,
    height: f64,
    font_height: f64,
) -> Axis {
    let range = max - min;
    if range == 0. {
        return Axis {
            min: max,
            max,
            labels: Vec::new(),
        };
    }
    let spacing = 2. * font_height + 4.;
    let fit = (1. + height / spacing).clamp(2., 6.);
    let mut step = 10f64.powf(-(decimals as f64));
    let mut places = decimals;
    while (range / step).ceil() + 1. > fit {
        if (range / (2. * step)).ceil() + 1. <= fit {
            step *= 2.;
            break;
        }
        if (range / (5. * step)).ceil() + 1. <= fit {
            step *= 5.;
            break;
        }
        step *= 10.;
        places = places.saturating_sub(1);
    }
    let top_i = (max / step).ceil() as i64;
    let bot_i = (min / step).floor() as i64;
    let mut labels = Vec::new();
    for i in (bot_i..=top_i).rev() {
        let label = i as f64 * step;
        labels.push(format!("{label:.places$}"));
    }
    Axis {
        min: bot_i as f64 * step,
        max: top_i as f64 * step,
        labels,
    }
}

/// `drawTimeLabels`: every whole minute from the window's start, at its column, until the canvas ends.
pub(super) fn minute_ticks(start: f64, scale: f64, width: f64) -> Vec<(f64, f64)> {
    let mut ticks = Vec::new();
    let mut time = (start / MINUTE_MS).ceil() * MINUTE_MS;
    loop {
        let x = js_round((time - start) / scale);
        if x >= width {
            break;
        }
        ticks.push((x, time));
        time += MINUTE_MS;
    }
    ticks
}

/// One repaint, in device pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Frame {
    pub dpr: f64,
    /// `canvas.width` and `canvas.height`.
    pub width: f64,
    pub height: f64,
    /// The plot above the time labels: `height - ceil(fontSize * dpr) - 4`.
    pub plot: f64,
    /// Milliseconds a device pixel spans (`scale_ = 1000 / devicePixelRatio`).
    pub scale: f64,
    /// The time at column 0.
    pub start: f64,
    pub values: Vec<f64>,
    pub axis: Axis,
    pub ticks: Vec<(f64, f64)>,
}

impl Frame {
    /// `updateEndDate()` then `repaint()` at `now` (ms since the epoch). The view's start time stays 0 and its
    /// scrollbar at the right edge, so the window ends at the last whole device pixel of `now`.
    pub fn at(points: &[Sample], now: f64, dpr: f64) -> Frame {
        let dpr = if dpr > 0. { dpr } else { 1. };
        // `canvas.width = parseInt(width) * devicePixelRatio`, truncated by the canvas.
        let width = (f64::from(WIDTH) * dpr).floor();
        let height = (f64::from(HEIGHT) * dpr).floor();
        let scale = 1000. / dpr;
        let length = (now / scale).floor();
        let range = (length - width).max(0.);
        let position = if range == 0. { length - width } else { range };
        let start = position * scale;
        let plot = height - (FONT_SIZE * dpr).ceil() - 4.;
        let values = values(points, start, scale, width as usize);
        let (mut low, mut high) = (0f64, 0f64);
        for &value in &values {
            if value > high {
                high = value;
            } else if value < low {
                low = value;
            }
        }
        Frame {
            dpr,
            width,
            height,
            plot,
            scale,
            start,
            axis: layout_labels(low, high, plot, FONT_SIZE * dpr),
            ticks: minute_ticks(start, scale, width),
            values,
        }
    }

    /// `repaint` draws nothing but the background when the canvas is shorter than four font sizes or narrower than
    /// 50 device pixels.
    pub fn drawable(&self) -> bool {
        4. * FONT_SIZE <= self.height && self.width >= 50.
    }

    /// `drawLines`: one vertex per column, rounded to whole device pixels.
    pub fn line(&self) -> Vec<(f64, f64)> {
        let bottom = self.plot - 1.;
        let per_unit = if self.axis.max != 0. {
            bottom / (self.axis.max - self.axis.min)
        } else {
            0.
        };
        self.values
            .iter()
            .enumerate()
            .map(|(column, value)| {
                (
                    column as f64,
                    bottom - js_round((value - self.axis.min) * per_unit),
                )
            })
            .collect()
    }

    /// `drawTicks`: the inner labels' rows, 10 device pixels at the right edge.
    pub fn tick_rows(&self) -> Vec<f64> {
        let count = self.axis.labels.len();
        (1..count.saturating_sub(1))
            .map(|n| js_round(self.plot * n as f64 / (count - 1) as f64))
            .collect()
    }

    /// `drawLabels`: the first label hangs from the top, the others stand on their rows.
    pub fn label_rows(&self) -> Vec<(String, f64, bool)> {
        let count = self.axis.labels.len();
        if count == 0 {
            return Vec::new();
        }
        let spacing = if count > 1 {
            (self.plot - 1.) / (count - 1) as f64
        } else {
            0.
        };
        self.axis
            .labels
            .iter()
            .enumerate()
            .map(|(n, label)| (label.clone(), spacing * n as f64, n == 0))
            .collect()
    }
}

/// `toLocaleTimeString(timeLocales, {timeStyle: "short"})`.
fn short_time(ms: f64) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_millis_opt(ms as i64).single() {
        Some(local) => crate::ui::timestamps::long("LT", &local),
        None => String::new(),
    }
}

/// The canvas with its labels, 258 by 80.
pub(super) fn graph(frame: Frame) -> AnyElement {
    let dpr = frame.dpr as f32;
    let font = FONT_SIZE as f32;
    let css = move |device: f64| px(device as f32 / dpr);
    let text = |label: String| {
        div()
            .absolute()
            .h(px(font))
            .text_size(px(font))
            .line_height(px(font))
            .font_weight(gpui::FontWeight::NORMAL)
            .text_color(color("--text-default"))
            .whitespace_nowrap()
            .child(label)
    };
    let drawable = frame.drawable();
    let time_labels: Vec<AnyElement> = if drawable {
        frame
            .ticks
            .iter()
            .map(|&(x, time)| {
                // Centred on the column, standing on the canvas's bottom edge.
                text(short_time(time))
                    .left(css(x) - px(40.))
                    .w(px(80.))
                    .top(px(HEIGHT - font))
                    .flex()
                    .justify_center()
                    .into_any_element()
            })
            .collect()
    } else {
        Vec::new()
    };
    let value_labels: Vec<AnyElement> = if drawable {
        let right = px(WIDTH) - css(frame.width - 3.);
        frame
            .label_rows()
            .into_iter()
            .map(|(label, y, hangs)| {
                let top = if hangs { css(y) } else { css(y) - px(font) };
                text(label).right(right).top(top).into_any_element()
            })
            .collect()
    } else {
        Vec::new()
    };
    let paint_frame = frame;
    div()
        .relative()
        .w(px(WIDTH))
        .h(px(HEIGHT))
        .flex_none()
        .bg(color("--background-base-low"))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let frame = paint_frame;
                    if !frame.drawable() {
                        return;
                    }
                    let origin = bounds.origin;
                    let at = |x: f64, y: f64| point(origin.x + css(x), origin.y + css(y));
                    let mask = gpui::ContentMask { bounds };
                    window.with_content_mask(Some(mask), |window| {
                        let grid = color("--background-mod-muted");
                        // The minute lines, the plot's frame and the value ticks share the grid colour.
                        let mut lines = PathBuilder::stroke(px(1.));
                        for &(x, _) in &frame.ticks {
                            lines.move_to(at(x, 1.));
                            lines.line_to(at(x, frame.plot));
                        }
                        let (right, bottom) = (frame.width, frame.plot);
                        lines.move_to(at(1., 1.));
                        lines.line_to(at(right, 1.));
                        lines.line_to(at(right, bottom));
                        lines.line_to(at(1., bottom));
                        lines.close();
                        for y in frame.tick_rows() {
                            lines.move_to(at(frame.width - 1., y));
                            lines.line_to(at(frame.width - 11., y));
                        }
                        if let Ok(path) = lines.build() {
                            window.paint_path(path, grid);
                        }
                        let mut series = PathBuilder::stroke(px(1.));
                        for (index, (x, y)) in frame.line().into_iter().enumerate() {
                            if index == 0 {
                                series.move_to(at(x, y));
                            } else {
                                series.line_to(at(x, y));
                            }
                        }
                        if let Ok(path) = series.build() {
                            window.paint_path(path, color("--brand-500"));
                        }
                    });
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .children(time_labels)
        .children(value_labels)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time: f64, value: f64) -> Sample {
        Sample { time, value }
    }

    #[test]
    fn values_hold_the_last_point_before_each_column() {
        let points = [sample(10., 5.), sample(20., 7.), sample(30., 9.)];
        // The first point lies 10 after the start, under one and a half spacings: the graph starts from it.
        assert_eq!(values(&points, 0., 10., 5), [5., 5., 5., 7., 9.]);
        // Far from the start the columns before the first point read 0.
        assert_eq!(values(&points, -100., 30., 5), [0., 0., 0., 0., 5.]);
        // A point exactly at a column's time counts from the next column.
        assert_eq!(values(&[sample(10., 3.)], 0., 10., 3), [0., 0., 3.]);
        assert_eq!(values(&[], 0., 1., 3), [0., 0., 0.]);
    }

    #[test]
    fn the_axis_matches_the_official_label_steps() {
        // The owner's screenshot: spikes to about 700 ms on a 65 pixel plot read 1000, 500, 0.
        let axis = layout_labels(0., 700., 65., 11.);
        assert_eq!(axis.labels, ["1000", "500", "0"]);
        assert_eq!((axis.min, axis.max), (0., 1000.));
        // A quiet call around 100 ms: 3.5 labels fit, so 50 is too fine and the step is 100.
        assert_eq!(
            layout_labels(0., 103., 65., 11.).labels,
            ["200", "100", "0"]
        );
        // Nothing but zeros: no labels and a flat axis.
        let empty = layout_labels(0., 0., 65., 11.);
        assert!(empty.labels.is_empty());
        assert_eq!((empty.min, empty.max), (0., 0.));
        // Small values keep their decimals.
        assert_eq!(
            layout_labels(0., 0.03, 65., 11.).labels,
            ["0.04", "0.02", "0.00"]
        );
        // Past 1024 the values are divided and suffixed.
        assert_eq!(
            layout_labels(0., 3000., 65., 11.).labels,
            ["4 k", "2 k", "0 k"]
        );
        // A taller plot fits more labels, at most six.
        assert_eq!(
            layout_labels(0., 700., 300., 11.).labels,
            ["800", "600", "400", "200", "0"]
        );
    }

    #[test]
    fn minute_ticks_fall_on_whole_minutes_inside_the_canvas() {
        let start = 3. * MINUTE_MS + 30_000.;
        let ticks = minute_ticks(start, 1000., 258.);
        assert_eq!(
            ticks,
            [
                (30., 4. * MINUTE_MS),
                (90., 5. * MINUTE_MS),
                (150., 6. * MINUTE_MS),
                (210., 7. * MINUTE_MS)
            ]
        );
        assert!(minute_ticks(MINUTE_MS, 1000., 258.).first() == Some(&(0., MINUTE_MS)));
    }

    #[test]
    fn a_frame_ends_at_now_and_scrolls_by_whole_pixels() {
        let now = 1_700_000_000_500.;
        let frame = Frame::at(&[], now, 1.);
        assert_eq!((frame.width, frame.height, frame.plot), (258., 80., 65.));
        assert_eq!(frame.start, (now / 1000.).floor() * 1000. - 258_000.);
        assert_eq!(frame.values.len(), 258);
        assert!(frame.axis.labels.is_empty());
        // Every column reads 0: a flat line on the plot's last row.
        assert!(frame.line().iter().all(|&(_, y)| y == 64.));
        // At 1.25 the canvas is truncated to whole device pixels and the font grows with it.
        let scaled = Frame::at(&[], now, 1.25);
        assert_eq!(
            (scaled.width, scaled.height, scaled.plot),
            (322., 100., 82.)
        );
        assert_eq!(scaled.scale, 800.);
        assert!(frame.drawable() && scaled.drawable());
    }

    #[test]
    fn the_line_and_ticks_follow_the_axis() {
        let now = 1_700_000_000_000.;
        let points: Vec<Sample> = (0..60)
            .map(|n| {
                sample(
                    now - 300_000. + 5_000. * f64::from(n),
                    if n == 50 { 700. } else { 100. },
                )
            })
            .collect();
        let frame = Frame::at(&points, now, 1.);
        assert_eq!(frame.axis.labels, ["1000", "500", "0"]);
        assert_eq!(frame.tick_rows(), [33.]);
        let rows = frame.label_rows();
        assert_eq!(rows[0], ("1000".to_owned(), 0., true));
        assert_eq!(rows[2], ("0".to_owned(), 64., false));
        let line = frame.line();
        // 100 ms sits round(100 * 64 / 1000) = 6 above the bottom row, 700 ms 45 above it.
        assert_eq!(line[0].1, 58.);
        assert!(line.iter().any(|&(_, y)| y == 19.));
    }
}
