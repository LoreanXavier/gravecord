//! `parity(id)`: records an element's final bounds for the layout-tree diff.
//! In normal builds it does nothing. Ids map to official class names in `tests/parity/map.json`.

use gpui::{ParentElement, Styled};

#[cfg(feature = "capture")]
mod registry {
    use gpui::{Bounds, Pixels};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static BOUNDS: RefCell<BTreeMap<String, [f32; 4]>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub fn record(id: &str, bounds: Bounds<Pixels>) {
        let value = [
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ];
        BOUNDS.with(|map| map.borrow_mut().insert(id.to_owned(), value));
    }

    pub fn clear() {
        BOUNDS.with(|map| map.borrow_mut().clear());
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
pub use registry::{clear, snapshot, write};

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
            move |bounds, _, _| registry::record(id, bounds),
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
