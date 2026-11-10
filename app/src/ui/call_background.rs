//! 715989/139327: the call's five blurred ellipses and their 4.5-second CSS animation.
use super::Root;
use dc_tokens::Theme;
use gpui::{Context, DevicePixels, RenderImage, SvgSize, size};
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

const RASTER_DIVISOR: u32 = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Spec {
    width: u32,
    height: u32,
    animate: bool,
}

impl Spec {
    fn raster_size(self) -> gpui::Size<DevicePixels> {
        // The 200px blur has no fine detail. Compare the composited result against a full-size raster below.
        size(
            DevicePixels(self.width.div_ceil(RASTER_DIVISOR) as i32),
            DevicePixels(self.height.div_ceil(RASTER_DIVISOR) as i32),
        )
    }
}

#[derive(Default)]
pub(super) struct State {
    desired: Option<Spec>,
    settled: Option<Spec>,
    started: Option<Instant>,
    generation: u64,
    pending: bool,
    seen: bool,
    failed: bool,
    frame: Option<Arc<RenderImage>>,
    #[cfg(feature = "capture")]
    completed_jobs: u64,
    #[cfg(feature = "capture")]
    retired_textures: u64,
    #[cfg(feature = "capture")]
    render_ms: f64,
}

impl State {
    fn request(&mut self, spec: Spec, now: Instant) -> Option<(u64, String)> {
        self.seen = true;
        if self.desired != Some(spec) {
            if self.desired.is_none_or(|old| old.animate != spec.animate) {
                self.started = Some(now);
            }
            self.desired = Some(spec);
            self.generation += 1;
            self.failed = false;
        }
        if self.pending || self.failed || (!spec.animate && self.settled == Some(spec)) {
            return None;
        }
        self.pending = true;
        let seconds = spec.animate.then(|| {
            now.duration_since(self.started.unwrap_or(now))
                .as_secs_f64()
        });
        Some((self.generation, svg(spec.width, spec.height, seconds)))
    }

    fn finish(
        &mut self,
        generation: u64,
        image: Option<Arc<RenderImage>>,
    ) -> Option<Arc<RenderImage>> {
        self.pending = false;
        if self.desired.is_none() || generation != self.generation {
            return None;
        }
        self.settled = self.desired;
        self.failed = image.is_none();
        let image = image?;
        self.frame.replace(image)
    }

    fn end_frame(&mut self) -> Option<Arc<RenderImage>> {
        if std::mem::take(&mut self.seen) || self.desired.is_none() {
            return None;
        }
        self.desired = None;
        self.settled = None;
        self.started = None;
        self.generation += 1;
        self.frame.take()
    }
}

fn ease_in_out(progress: f64) -> f64 {
    let (mut low, mut high) = (0., 1.);
    for _ in 0..24 {
        let t = (low + high) / 2.;
        let x = 3. * (1. - t) * (1. - t) * t * 0.42 + 3. * (1. - t) * t * t * 0.58 + t * t * t;
        if x < progress {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) / 2.;
    3. * (1. - t) * t * t + t * t * t
}

fn phase(seconds: f64) -> f64 {
    let t = seconds.rem_euclid(4.5) / 4.5;
    if t <= 0.55 {
        ease_in_out(t / 0.55)
    } else {
        1. - ease_in_out((t - 0.55) / 0.45)
    }
}

#[derive(Clone, Copy, Debug)]
struct Ellipse {
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    opacity: f64,
    light: bool,
}

fn ellipses(width: f64, height: f64, seconds: Option<f64>) -> [Ellipse; 5] {
    ellipses_at(width, height, seconds.map_or(0., phase), seconds.is_some())
}

fn ellipses_at(width: f64, height: f64, t: f64, animate: bool) -> [Ellipse; 5] {
    let mix = |a: f64, b: f64| a + (b - a) * t;
    // CSS matches scale functions by their primitive and pads the shorter transform list with identity.
    // Interpolate each function, then multiply; interpolating the final matrices loses the Y overshoot.
    let left = (mix(1., 0.82), mix(1., 0.82) * mix(1.25, 1.));
    let right = (mix(0.82, 1.), mix(0.82, 1.) * mix(1., 1.25));
    [
        Ellipse {
            cx: 0.,
            cy: height * 0.0005,
            rx: width * 0.495 * left.0,
            ry: height * 0.4425 * left.1,
            opacity: mix(1., 0.75),
            light: false,
        },
        Ellipse {
            cx: 0.,
            cy: height * -0.0005,
            rx: width * 0.225 * left.0,
            ry: height * 0.1175 * left.1,
            opacity: if animate { mix(0.6, 0.45) } else { 1. },
            light: true,
        },
        Ellipse {
            cx: width * 1.005,
            cy: height * 0.0005,
            rx: width * 0.495 * right.0,
            ry: height * 0.4425 * right.1,
            opacity: mix(0.75, 0.7),
            light: false,
        },
        Ellipse {
            cx: width,
            cy: height * 0.0005,
            rx: width * 0.225 * right.0,
            ry: height * 0.1175 * right.1,
            opacity: mix(0.45, 0.7),
            light: true,
        },
        // The nonanimated declaration translates by 50%; animation keyframes replace that transform.
        Ellipse {
            cx: width * if animate { 0.5 } else { 1. },
            cy: 0.,
            rx: width * 0.5 * mix(0.36, 0.7),
            ry: height * 0.5 * mix(0.36, 1.) * mix(1., 0.3),
            opacity: mix(0.45, 0.6),
            light: true,
        },
    ]
}

fn svg(width: u32, height: u32, seconds: Option<f64>) -> String {
    let color = |token| Theme::Midnight.color(token) >> 8;
    let base = color("--bg-animated-gradient-background-not-black");
    let dark = color("--bg-animated-gradient-background-indigo-2");
    let light = color("--blurple-49");
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><defs><filter id="blur" x="-100%" y="-100%" width="300%" height="300%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="200"/></filter></defs><g filter="url(#blur)"><rect width="{width}" height="{height}" fill="#{base:06x}"/>"##
    );
    for e in ellipses(width as f64, height as f64, seconds) {
        let fill = if e.light { light } else { dark };
        let _ = write!(
            out,
            r##"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="#{fill:06x}" opacity="{}"/>"##,
            e.cx, e.cy, e.rx, e.ry, e.opacity
        );
    }
    out.push_str("</g></svg>");
    out
}

impl Root {
    pub(super) fn release_call_background(&mut self, cx: &mut gpui::App) {
        if let Some(image) = self.vv.background.frame.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }

    pub(super) fn call_empty_background(
        &mut self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let spec = Spec {
            width: width.round().max(1.) as u32,
            height: height.round().max(1.) as u32,
            animate: !self.pref_on("reduced_motion", false),
        };
        if let Some((generation, svg)) = self.vv.background.request(spec, Instant::now()) {
            let renderer = cx.svg_renderer();
            cx.spawn(async move |this, cx| {
                #[cfg(feature = "capture")]
                let started = Instant::now();
                let image = cx
                    .background_executor()
                    .spawn(async move {
                        let parsed = renderer.parse_svg(svg.as_bytes()).ok()?;
                        renderer
                            .render_parsed(&parsed, SvgSize::ExactSize(spec.raster_size()))
                            .ok()
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let old = this.vv.background.finish(generation, image);
                    #[cfg(feature = "capture")]
                    {
                        this.vv.background.completed_jobs += 1;
                        this.vv.background.render_ms = started.elapsed().as_secs_f64() * 1000.;
                    }
                    if let Some(old) = old {
                        #[cfg(feature = "capture")]
                        {
                            this.vv.background.retired_textures += 1;
                        }
                        cx.defer(move |cx| cx.drop_image(old, None));
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        self.vv.background.frame.clone()
    }

    pub(super) fn end_call_background_frame(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.vv.background.end_frame() {
            #[cfg(feature = "capture")]
            {
                self.vv.background.retired_textures += 1;
            }
            cx.defer(move |cx| cx.drop_image(old, None));
        }
    }

    #[cfg(feature = "capture")]
    pub(crate) fn call_background_status(&self) -> String {
        let s = &self.vv.background;
        format!(
            "active={} pending={} has_frame={} completed_jobs={} retired_textures={} last_render_ms={:.2}",
            s.desired.is_some(),
            s.pending,
            s.frame.is_some(),
            s.completed_jobs,
            s.retired_textures,
            s.render_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_keyframes_loop_and_reduced_motion_keeps_the_static_transform() {
        assert!(phase(0.).abs() < 0.000001);
        assert!((phase(4.5 * 0.55) - 1.).abs() < 0.000001);
        assert!((phase(1.) - phase(5.5)).abs() < 0.000001);
        let a = ellipses(1000., 1000., Some(0.));
        let b = ellipses(1000., 1000., Some(4.5 * 0.55));
        assert!((a[0].rx - 495.).abs() < 0.001);
        assert!((b[0].rx - 405.9).abs() < 0.001);
        assert!((b[4].ry - 150.).abs() < 0.001);
        assert_eq!(a[4].cx, 500.);
        assert_eq!(ellipses(1000., 1000., None)[4].cx, 1000.);
        // Actual computed matrices in voice-view-hidden-darker-cozy at the captured intermediate opacity.
        let captured = ellipses_at(905., 688., 0.312052, true);
        assert!((captured[0].ry / (688. * 0.4425) - 1.10616).abs() < 0.00001);
        assert!((captured[2].ry / (688. * 0.4425) - 0.944521).abs() < 0.00001);
        assert!((captured[4].ry / (688. * 0.5) - 0.437451).abs() < 0.00001);
    }

    #[test]
    fn pending_work_is_bounded_and_closed_views_discard_its_result() {
        let now = Instant::now();
        let spec = Spec {
            width: 905,
            height: 688,
            animate: true,
        };
        let mut state = State::default();
        let (generation, _) = state.request(spec, now).unwrap();
        assert!(state.request(Spec { width: 950, ..spec }, now).is_none());
        assert!(state.end_frame().is_none());
        assert!(state.end_frame().is_none());
        assert!(state.desired.is_none());
        assert!(state.pending);
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(1, 1),
        )]));
        assert!(state.finish(generation, Some(image)).is_none());
        assert!(state.frame.is_none());
        assert!(!state.pending);
        assert!(state.request(spec, now).is_some());
    }

    #[test]
    fn rasterized_background_keeps_the_glow_above_the_call() {
        let renderer = gpui::SvgRenderer::new(Arc::new(()));
        for phase in [Some(0.), Some(4.5 * 0.55), None] {
            let text = svg(905, 688, phase);
            let parsed = renderer.parse_svg(text.as_bytes()).unwrap();
            let start = Instant::now();
            let frame = renderer
                .render_parsed(
                    &parsed,
                    SvgSize::ExactSize(size(DevicePixels(905), DevicePixels(688))),
                )
                .unwrap();
            eprintln!("call background raster: {:?}", start.elapsed());
            let bytes = frame.as_bytes(0).unwrap();
            assert_eq!(bytes.len(), 905 * 688 * 4);
            let top_blue = bytes[(150 * 905 + 452) * 4];
            let bottom_blue = bytes[(600 * 905 + 452) * 4];
            assert!(
                top_blue > bottom_blue + 10,
                "top={top_blue}, bottom={bottom_blue}"
            );
            let spec = Spec {
                width: 905,
                height: 688,
                animate: true,
            };
            let start = Instant::now();
            let low = renderer
                .render_parsed(&parsed, SvgSize::ExactSize(spec.raster_size()))
                .unwrap();
            eprintln!("call background reduced raster: {:?}", start.elapsed());
            let pixels = image::RgbaImage::from_raw(
                spec.width.div_ceil(RASTER_DIVISOR),
                spec.height.div_ceil(RASTER_DIVISOR),
                low.as_bytes(0).unwrap().to_vec(),
            )
            .unwrap();
            let scaled = image::imageops::resize(
                &pixels,
                spec.width,
                spec.height,
                image::imageops::FilterType::Triangle,
            );
            let mut maximum = 0.;
            let mut total = 0.;
            for (full, reduced) in bytes
                .as_chunks::<4>()
                .0
                .iter()
                .zip(scaled.as_raw().as_chunks::<4>().0)
            {
                for channel in 0..3 {
                    let a = f64::from(full[channel]) * f64::from(full[3]) / 255.;
                    let b = f64::from(reduced[channel]) * f64::from(reduced[3]) / 255.;
                    let delta = (a - b).abs();
                    if delta > maximum {
                        maximum = delta;
                    }
                    total += delta;
                }
            }
            eprintln!(
                "reduced composite difference: max={maximum:.3}, mean={:.3}",
                total / (905. * 688. * 3.)
            );
            assert!(
                maximum < 3.,
                "resampling changed the blurred color by {maximum}"
            );
        }
    }
}
