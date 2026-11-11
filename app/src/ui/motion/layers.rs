//! The base layer of the layer stack (`Layers`, module 596339, class `baseLayer_Fd0FEH` of stylesheet 555956).
//!
//! The app itself is the layer stack's base layer. While a full-window layer (Server Settings, Channel Settings) is
//! pushed, the base layer's mode is `HIDDEN` and `animateUnder` runs: its opacity springs to 0 on
//! `{tension: 100, friction: 10}` (`S8`) and springs back to 1 when the last layer pops (`animateIn`). Its scale
//! state moves too, but `getAnimatedStyle` leaves `transform` unset for the base layer, so the app only fades. The
//! base layer's own background is transparent (`.layer.baseLayer`), the window behind it is `.bg` and the layer on
//! top is opaque, so what shows is a cross fade. Reduced motion changes nothing here: it only removes the
//! transform of the layers above.

use super::{key, rn_spring_from};
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

/// `S8`: `{friction: 10, tension: 100}`.
pub const TENSION: f64 = 100.;
pub const FRICTION: f64 = 10.;

/// The opacity of the base layer this frame. `hidden` is whether a full-window layer is open.
pub fn base_opacity(hidden: bool) -> f32 {
    rn_spring_from(
        key("base-layer-opacity"),
        1.,
        if hidden { 0. } else { 1. },
        TENSION,
        FRICTION,
    )
    .clamp(0., 1.)
}

/// Draws `child` at `opacity` without adding a node to the layout or an id to the element path, so the tree under
/// it keeps its state and its place.
pub fn faded(child: impl IntoElement, opacity: f32) -> AnyElement {
    Faded {
        child: child.into_any_element(),
        opacity,
    }
    .into_any_element()
}

struct Faded {
    child: AnyElement,
    opacity: f32,
}

impl Element for Faded {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let opacity = (self.opacity < 1.).then_some(self.opacity);
        let child = &mut self.child;
        window.with_element_opacity(opacity, |window| child.paint(window, cx));
    }
}

impl IntoElement for Faded {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::motion::{begin_frame, freeze, reset_for_tests};

    fn at(ms: f64, hidden: bool) -> f32 {
        freeze(Some(ms));
        begin_frame(false);
        base_opacity(hidden)
    }

    #[test]
    fn the_base_layer_fades_out_and_back_on_the_layer_spring() {
        reset_for_tests();
        assert_eq!(at(0., false), 1.);
        // A layer opens: the opacity leaves 1 along the origami spring and ends at 0.
        let start = at(1000., true);
        assert!(start > 0.99);
        let middle = at(1150., true);
        assert!(middle < 0.9 && middle > 0.0, "{middle}");
        assert_eq!(at(4000., true), 0.);
        // It closes: back to 1, never outside 0..1.
        assert_eq!(at(4000., false), 0.);
        let back = at(4200., false);
        assert!(back > 0. && back <= 1., "{back}");
        assert_eq!(at(8000., false), 1.);
    }
}
