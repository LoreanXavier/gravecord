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

/// The canvas plot width plus right Y-axis label gutter, CSS pixels (`240 + 28 = 268`).
pub(super) const PLOT_WIDTH: f32 = 240.;
pub(super) const Y_AXIS_GUTTER: f32 = 28.;
pub(super) const WIDTH: f32 = PLOT_WIDTH + Y_AXIS_GUTTER;
pub(super) const HEIGHT: f32 = 80.;
/// Minimum horizontal clearance from either plot edge so centered bottom timestamps are never clipped.
const EDGE_LABEL_MARGIN: f32 = 24.;
/// `fontSize = 11`.
const FONT_SIZE: f64 = 11.;
/// `graphContainer_BlzJTq` around the inline canvas: the canvas, the line box's descent under it and `padding-bottom:
/// 2px`, 85 tall in a screenshot of the official popout.
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
#[cfg(test)]
pub(super) fn values(points: &[Sample], start: f64, step: f64, count: usize) -> Vec<f64> {
    values_with(points, start, step, count, None)
}

/// [`values`] after optionally mapping each raw sample value (for example `ea.audioLevel` in `950040`).
pub(super) fn values_with(
    points: &[Sample],
    start: f64,
    step: f64,
    count: usize,
    converter: Option<fn(f64) -> f64>,
) -> Vec<f64> {
    let convert = |value: f64| converter.map_or(value, |map| map(value));
    let mut held = 0.;
    if let [first, second, ..] = points {
        let spacing = second.time - first.time;
        if spacing > 0. && first.time - start < 1.5 * spacing {
            held = convert(first.value);
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
            held = convert(point.value);
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

/// Filters minute ticks so bottom labels near the left/right plot edges are omitted rather than clipped.
pub(super) fn visible_minute_ticks(
    ticks: &[(f64, f64)],
    dpr: f64,
    plot_css_width: f32,
) -> Vec<(f64, f64)> {
    let dpr = if dpr > 0. { dpr } else { 1. };
    ticks
        .iter()
        .copied()
        .filter(|&(x_dev, _)| {
            let x_css = (x_dev / dpr) as f32;
            x_css >= EDGE_LABEL_MARGIN && x_css <= plot_css_width - EDGE_LABEL_MARGIN
        })
        .collect()
}

/// One repaint, in device pixels.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Frame {
    pub dpr: f64,
    pub css_width: f32,
    pub css_height: f32,
    pub plot_css_width: f32,
    pub gutter_css_width: f32,
    /// `canvas.width` and `canvas.height` of the plot area.
    pub width: f64,
    pub height: f64,
    /// The plot above the time labels: `height - ceil(fontSize * dpr) - 4`.
    pub plot: f64,
    /// Milliseconds a device pixel spans (`scale_ = 1000 / devicePixelRatio`).
    pub scale: f64,
    /// The time at column 0.
    pub start: f64,
    pub values: Vec<f64>,
    pub samples: Vec<Sample>,
    pub axis: Axis,
    pub ticks: Vec<(f64, f64)>,
    pub hover_x: Option<f32>,
}

impl Frame {
    /// `updateEndDate()` then `repaint()` at `now` (ms since the epoch) for the connection popout's `240x80` plot
    /// plus `28px` right Y-axis label gutter (`268x80` total).
    pub fn at(points: &[Sample], now: f64, dpr: f64) -> Frame {
        Self::with_gutter(points, now, dpr, PLOT_WIDTH, HEIGHT, Y_AXIS_GUTTER, None)
    }

    /// `updateEndDate()` then `repaint()` at `now` (ms since the epoch) for an arbitrary canvas size and optional
    /// sample converter.
    pub fn at_size(
        points: &[Sample],
        now: f64,
        dpr: f64,
        css_width: f32,
        css_height: f32,
        converter: Option<fn(f64) -> f64>,
    ) -> Frame {
        Self::with_gutter(points, now, dpr, css_width, css_height, 0., converter)
    }

    fn with_gutter(
        points: &[Sample],
        now: f64,
        dpr: f64,
        plot_css_width: f32,
        css_height: f32,
        gutter_css_width: f32,
        converter: Option<fn(f64) -> f64>,
    ) -> Frame {
        let dpr = if dpr > 0. { dpr } else { 1. };
        // `canvas.width = parseInt(width) * devicePixelRatio`, truncated by the canvas.
        let width = (f64::from(plot_css_width.floor()) * dpr).floor();
        let height = (f64::from(css_height.floor()) * dpr).floor();
        let scale = 1000. / dpr;
        let length = (now / scale).floor();
        let range = (length - width).max(0.);
        let position = if range == 0. { length - width } else { range };
        let start = position * scale;
        let plot = height - (FONT_SIZE * dpr).ceil() - 4.;
        let values = values_with(points, start, scale, width as usize, converter);
        let samples = points
            .iter()
            .map(|sample| Sample {
                time: sample.time,
                value: converter.map_or(sample.value, |map| map(sample.value)),
            })
            .collect();
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
            css_width: plot_css_width + gutter_css_width,
            css_height,
            plot_css_width,
            gutter_css_width,
            width,
            height,
            plot,
            scale,
            start,
            values,
            samples,
            axis: layout_labels(low, high, plot, FONT_SIZE * dpr),
            ticks: minute_ticks(start, scale, width),
            hover_x: None,
        }
    }

    /// Sets the pointer X position relative to the plot area (`0.0..=plot_css_width`).
    pub fn with_hover(mut self, hover_x: Option<f32>) -> Self {
        self.hover_x = hover_x.filter(|x| x.is_finite() && *x >= 0. && *x <= self.plot_css_width);
        self
    }

    /// Finds the nearest visible sample in time for a plot-local CSS X coordinate, returning
    /// `(sample, device_x, device_y)`.
    pub fn nearest_sample(&self, plot_css_x: f32) -> Option<(Sample, f64, f64)> {
        if !plot_css_x.is_finite() || self.width <= 0. || self.plot <= 1. {
            return None;
        }
        let end = self.start + self.width * self.scale;
        let clamped_css = plot_css_x.clamp(0., self.plot_css_width);
        let target_time = self.start + f64::from(clamped_css) * self.dpr * self.scale;
        let sample = self
            .samples
            .iter()
            .copied()
            .filter(|s| s.time >= self.start && s.time <= end)
            .min_by(|a, b| {
                (a.time - target_time)
                    .abs()
                    .total_cmp(&(b.time - target_time).abs())
            })?;
        let dot_x = ((sample.time - self.start) / self.scale).clamp(0., self.width - 1.);
        let bottom = self.plot - 1.;
        let span = self.axis.max - self.axis.min;
        let per_unit = if span > 0. { bottom / span } else { 0. };
        let dot_y = (bottom - js_round((sample.value - self.axis.min) * per_unit)).clamp(1., bottom);
        Some((sample, dot_x, dot_y))
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

/// Hover timestamp including seconds (`timeStyle: "medium"`).
fn hover_time(ms: f64) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_millis_opt(ms as i64).single() {
        Some(local) => crate::ui::timestamps::long("LTS", &local),
        None => String::new(),
    }
}

/// The canvas with its labels, sized to `frame.css_width` by `frame.css_height`.
pub(super) fn graph(frame: Frame) -> AnyElement {
    let dpr = frame.dpr as f32;
    let css_width = frame.css_width;
    let css_height = frame.css_height;
    let plot_css_width = frame.plot_css_width;
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
        visible_minute_ticks(&frame.ticks, frame.dpr, plot_css_width)
            .into_iter()
            .map(|(x, time)| {
                // Centred on the column, standing on the canvas's bottom edge.
                text(short_time(time))
                    .left(css(x) - px(40.))
                    .w(px(80.))
                    .top(px(css_height - font))
                    .flex()
                    .justify_center()
                    .into_any_element()
            })
            .collect()
    } else {
        Vec::new()
    };
    let value_labels: Vec<AnyElement> = if drawable {
        frame
            .label_rows()
            .into_iter()
            .map(|(label, y, hangs)| {
                let top = if hangs { css(y) } else { css(y) - px(font) };
                if frame.gutter_css_width > 0. {
                    text(label)
                        .left(px(plot_css_width + 4.))
                        .top(top)
                        .into_any_element()
                } else {
                    let right = px(css_width) - css(frame.width - 3.);
                    text(label).right(right).top(top).into_any_element()
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let hovered = if drawable {
        frame.hover_x.and_then(|x| frame.nearest_sample(x))
    } else {
        None
    };
    let hover_dot = hovered.map(|(_, dot_x, dot_y)| {
        div()
            .absolute()
            .left(css(dot_x) - px(3.))
            .top(css(dot_y) - px(3.))
            .size(px(6.))
            .rounded_full()
            .bg(color("--brand-500"))
            .border_1()
            .border_color(color("--background-base-low"))
    });
    // `.hoverReadout_QbZ2-g` pill anchored at top-left of the plot (`top: 4px`, `left: 4px`).
    let hover_readout = hovered.map(|(sample, _, _)| {
        div()
            .absolute()
            .left(px(4.))
            .top(px(4.))
            .px(px(6.))
            .py(px(4.))
            .rounded(px(4.))
            .bg(color("--background-surface-highest"))
            .shadow_sm()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .text_size(px(10.))
                    .line_height(px(12.))
                    .text_color(color("--text-muted"))
                    .whitespace_nowrap()
                    .child(hover_time(sample.time)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded(px(2.))
                            .flex_none()
                            .bg(color("--brand-500")),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(13.))
                            .font_weight(gpui::FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .whitespace_nowrap()
                            .child(format!("{:.0} ms", sample.value)),
                    ),
            )
    });
    let crosshair_x = hovered.map(|(_, dot_x, _)| dot_x);
    let paint_frame = frame;
    div()
        .relative()
        .w(px(css_width))
        .h(px(css_height))
        .flex_none()
        .overflow_hidden()
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
                        if let Some(hover_dev_x) = crosshair_x {
                            let mut guide = color("--text-default");
                            guide.a = 0.45;
                            let line_bounds = gpui::Bounds::new(
                                point(origin.x + css(hover_dev_x), origin.y + css(1.)),
                                gpui::size(px(1.), css(frame.plot - 1.)),
                            );
                            window.paint_quad(gpui::fill(line_bounds, guide));
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
        .children(hover_dot)
        .children(hover_readout)
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
        // The official popout's screenshot: spikes to about 700 ms on a 65 pixel plot read 1000, 500, 0.
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
        assert_eq!((frame.width, frame.height, frame.plot), (240., 80., 65.));
        assert_eq!(frame.css_width, 268.);
        assert_eq!(frame.plot_css_width, 240.);
        assert_eq!(frame.gutter_css_width, 28.);
        assert_eq!(frame.start, (now / 1000.).floor() * 1000. - 240_000.);
        assert_eq!(frame.values.len(), 240);
        assert!(frame.axis.labels.is_empty());
        // Every column reads 0: a flat line on the plot's last row.
        assert!(frame.line().iter().all(|&(_, y)| y == 64.));
        // At 1.25 the canvas is truncated to whole device pixels and the font grows with it.
        let scaled = Frame::at(&[], now, 1.25);
        assert_eq!(
            (scaled.width, scaled.height, scaled.plot),
            (300., 100., 82.)
        );
        assert_eq!(scaled.scale, 800.);
        assert!(frame.drawable() && scaled.drawable());
    }

    #[test]
    fn edge_clipping_timestamp_filter_drops_edge_labels() {
        let ticks = [
            (0., 1. * MINUTE_MS),
            (23.5, 2. * MINUTE_MS),
            (24., 3. * MINUTE_MS),
            (120., 4. * MINUTE_MS),
            (216., 5. * MINUTE_MS),
            (216.5, 6. * MINUTE_MS),
            (239., 7. * MINUTE_MS),
        ];
        assert_eq!(
            visible_minute_ticks(&ticks, 1., 240.),
            [
                (24., 3. * MINUTE_MS),
                (120., 4. * MINUTE_MS),
                (216., 5. * MINUTE_MS),
            ]
        );
        // Device pixel positions are scaled by dpr before comparing to CSS margins.
        let hidpi = [
            (29., 1. * MINUTE_MS),
            (60., 2. * MINUTE_MS),
            (270., 3. * MINUTE_MS),
            (271., 4. * MINUTE_MS),
        ];
        assert_eq!(
            visible_minute_ticks(&hidpi, 1.25, 240.),
            [(60., 2. * MINUTE_MS), (270., 3. * MINUTE_MS)]
        );
    }

    #[test]
    fn nearest_sample_snaps_to_closest_visible_ping() {
        let now = 1_700_000_000_000.;
        let points = [
            sample(now - 300_000., 999.),
            sample(now - 180_000., 42.),
            sample(now - 120_000., 180.),
            sample(now - 20_000., 75.),
        ];
        let frame = Frame::at(&points, now, 1.);
        // Window starts at now - 240_000: sample at -180s sits at x = 60, -120s at x = 120, -20s at x = 220.
        let (first, dx1, dy1) = frame.nearest_sample(75.).expect("near first sample");
        assert_eq!(first.value, 42.);
        assert_eq!(dx1, 60.);
        assert!(dy1 < 64.);

        let (mid, dx2, _) = frame.nearest_sample(115.).expect("near middle sample");
        assert_eq!(mid.value, 180.);
        assert_eq!(dx2, 120.);

        // Hovering far left still ignores out-of-window -300s sample and snaps to -180s.
        let (leftmost, _, _) = frame.nearest_sample(0.).expect("left clamp");
        assert_eq!(leftmost.value, 42.);
        assert!(frame.nearest_sample(f32::NAN).is_none());
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

    #[test]
    fn custom_size_and_converter_support_rtc_debug_graphs() {
        let now = 1_700_000_000_000.;
        let points = [
            sample(now - 20_000., 0.),
            sample(now - 10_000., 1.),
            sample(now - 1_000., 1.),
        ];
        let converter = |v: f64| {
            if v <= 0. {
                0.
            } else {
                (20. * v.ln()).max(-100.) + 100.
            }
        };
        let frame = Frame::at_size(&points, now, 1., 300., 100., Some(converter));
        assert_eq!((frame.css_width, frame.css_height), (300., 100.));
        assert_eq!((frame.width, frame.height, frame.plot), (300., 100., 85.));
        assert_eq!(frame.values.len(), 300);
        assert_eq!(frame.values[280], 0.);
        assert_eq!(frame.values[299], 100.);
    }
}
