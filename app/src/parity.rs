//! `parity(id)`: records an element's final bounds for the layout-tree diff.
//! In normal builds it does nothing. Ids map to official class names in `tests/parity/map.json`.

use gpui::{ParentElement, Styled};

#[cfg(feature = "capture")]
mod registry {
    use gpui::{Bounds, Pixels};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    /// A probe's id, its bounds and the content mask it was drawn under.
    type Probe = (String, [f32; 4], [f32; 4]);

    thread_local! {
        static BOUNDS: RefCell<BTreeMap<String, [f32; 4]>> = const { RefCell::new(BTreeMap::new()) };
        /// Each probe in the order it was laid out, with the content mask it was drawn under (the visible part of
        /// its overflow-hidden ancestors), for `cargo xtask drift`'s structural checks.
        static PROBES: RefCell<Vec<Probe>> = const { RefCell::new(Vec::new()) };
    }

    fn rect(bounds: Bounds<Pixels>) -> [f32; 4] {
        [
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ]
    }

    pub fn record(id: &str, bounds: Bounds<Pixels>, mask: Bounds<Pixels>) {
        let value = rect(bounds);
        BOUNDS.with(|map| map.borrow_mut().insert(id.to_owned(), value));
        PROBES.with(|probes| probes.borrow_mut().push((id.to_owned(), value, rect(mask))));
    }

    pub fn clear() {
        BOUNDS.with(|map| map.borrow_mut().clear());
        PROBES.with(|probes| probes.borrow_mut().clear());
    }

    /// `<frame>.scene.json`: the window, every probe with its mask in layout order, and the painted quads with theirs
    /// (logical pixels). Read by `cargo xtask drift`.
    pub fn write_scene(path: &std::path::Path, window: &gpui::Window) {
        let scale = window.scale_factor();
        let size = window.viewport_size();
        let mut body = format!(
            "{{\n  \"window\": [{}, {}],\n  \"scale\": {scale},\n  \"probes\": [\n",
            f32::from(size.width),
            f32::from(size.height)
        );
        PROBES.with(|probes| {
            let lines: Vec<String> = probes
                .borrow()
                .iter()
                .map(|(id, [x, y, w, h], [mx, my, mw, mh])| {
                    format!("    [\"{id}\", {x}, {y}, {w}, {h}, {mx}, {my}, {mw}, {mh}]")
                })
                .collect();
            body.push_str(&lines.join(",\n"));
        });
        body.push_str("\n  ],\n  \"quads\": [\n");
        let unscale = |value: gpui::ScaledPixels| value.0 / scale;
        let mut first = true;
        for quad in window.painted_quads() {
            let (b, m) = (quad.bounds, quad.content_mask.bounds);
            // [x, y, w, h, mask x, y, w, h, background alpha, border width, draw order]
            let alpha = quad.background.as_solid().map_or(
                if quad.background.is_transparent() {
                    0.
                } else {
                    1.
                },
                |c| c.a,
            );
            let border = unscale(quad.border_widths.top)
                .max(unscale(quad.border_widths.right))
                .max(unscale(quad.border_widths.bottom))
                .max(unscale(quad.border_widths.left));
            if !first {
                body.push_str(",\n");
            }
            first = false;
            let _ = write!(
                body,
                "    [{}, {}, {}, {}, {}, {}, {}, {}, {alpha}, {border}, {}]",
                unscale(b.origin.x),
                unscale(b.origin.y),
                unscale(b.size.width),
                unscale(b.size.height),
                unscale(m.origin.x),
                unscale(m.origin.y),
                unscale(m.size.width),
                unscale(m.size.height),
                quad.order
            );
        }
        body.push_str("\n  ]\n}\n");
        let _ = std::fs::write(path, body);
    }

    /// Every recorded id with its bounds.
    pub fn snapshot() -> Vec<(String, [f32; 4])> {
        BOUNDS.with(|map| {
            map.borrow()
                .iter()
                .map(|(id, bounds)| (id.clone(), *bounds))
                .collect()
        })
    }

    pub fn write(path: &std::path::Path) {
        let body = BOUNDS.with(|map| {
            let lines: Vec<String> = map
                .borrow()
                .iter()
                .map(|(id, [x, y, width, height])| {
                    format!("  \"{id}\": [{x}, {y}, {width}, {height}]")
                })
                .collect();
            format!("{{\n{}\n}}\n", lines.join(",\n"))
        });
        let _ = std::fs::write(path, body);
    }
}

#[cfg(feature = "capture")]
pub use registry::{clear, snapshot, write, write_scene};

pub trait Parity: ParentElement + Styled + Sized {
    /// Call before `.absolute()`: it makes the element a positioning parent for its probe.
    fn parity(self, id: &'static str) -> Self {
        self.probe(id, [0., 0., 0., 0.])
    }

    /// An absolute child is placed inside the border, so a bordered element gives its border width to be measured whole.
    fn parity_with_border(self, id: &'static str, border: f32) -> Self {
        self.probe(id, [border; 4])
    }

    /// As `parity_with_border`, for an element with a left border only.
    fn parity_left(self, id: &'static str, border: f32) -> Self {
        self.probe(id, [0., 0., 0., border])
    }

    /// As `parity_with_border`, for an element with a right border only.
    fn parity_right(self, id: &'static str, border: f32) -> Self {
        self.probe(id, [0., border, 0., 0.])
    }

    /// As `parity_with_border`, for an element with a bottom border only.
    fn parity_below(self, id: &'static str, border: f32) -> Self {
        self.probe(id, [0., 0., border, 0.])
    }

    /// Top, right, bottom and left distances from the padding box to the element's outer edge.
    #[cfg(feature = "capture")]
    fn probe(self, id: &'static str, [top, right, bottom, left]: [f32; 4]) -> Self {
        let probe = gpui::canvas(
            move |bounds, window, _| registry::record(id, bounds, window.content_mask().bounds),
            |_, (), _, _| {},
        )
        .absolute()
        .top(gpui::px(-top))
        .right(gpui::px(-right))
        .bottom(gpui::px(-bottom))
        .left(gpui::px(-left));
        self.relative().child(probe)
    }

    #[cfg(not(feature = "capture"))]
    fn probe(self, _: &'static str, _: [f32; 4]) -> Self {
        self
    }
}

impl<T: ParentElement + Styled + Sized> Parity for T {}
