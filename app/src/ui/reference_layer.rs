//! A popout placed against its target in the app's layer container, as the official `ReferencePositionLayer`
//! (class `Q` of module 623646) places one for `Popout` (module 453903): above or below the target, centred on it,
//! inverted to the side with more room (`autoInvert`), and pushed back inside the container (`nudgeAlignIntoViewport`).
//! The layer container (`layerContainer_VVADeK`: `position: absolute; top: 0; right: 0; bottom: 0; left: 0`) is the
//! whole window here. Only the vertical positions with `align: "center"` are ported: the ones the call controls use.

use gpui::{
    AnyElement, App, Bounds, Display, Edges, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, Position, Size, Style, Window,
    deferred, point, px,
};

/// The side of its target a popout opens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Top,
    Bottom,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Side::Top => Side::Bottom,
            Side::Bottom => Side::Top,
        }
    }
}

/// The props of `Popout` (453903) this layer honours.
#[derive(Clone, Copy, Debug)]
pub(super) struct Props {
    pub position: Side,
    pub spacing: f32,
    pub auto_invert: bool,
    pub nudge_align_into_viewport: bool,
}

/// The `style` the layer computes: an inset from the container's top or its bottom, and one from its left.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LayerStyle {
    top: Option<f32>,
    bottom: Option<f32>,
    left: f32,
}

impl LayerStyle {
    /// The popout's top edge in the container.
    fn top_edge(&self, height: f32, container: f32) -> f32 {
        match (self.top, self.bottom) {
            (Some(top), _) => top,
            (None, Some(bottom)) => container - bottom - height,
            (None, None) => 0.,
        }
    }
}

/// `nudgeLeftAlignment`: a left edge kept 12px inside the container on both sides.
fn nudge_left(left: f32, width: f32, container: f32, nudge: bool) -> f32 {
    if !nudge {
        return left;
    }
    let over = left + width - container + 12.;
    if over > 0. {
        (left - over).max(12.)
    } else {
        left.max(12.)
    }
}

/// `calculatePositionStyle` for `top`, `bottom` and `overlap_vertical` with `align: "center"` and no offset. The
/// left edge is rounded up (`formatDimension`, `Math.ceil`).
fn position_style(
    side: Side,
    overlap: bool,
    props: Props,
    target: Bounds<f32>,
    popout: Size<f32>,
    container: Size<f32>,
) -> LayerStyle {
    let centred = target.origin.x + (target.size.width - popout.width) / 2.;
    let left = nudge_left(
        centred,
        popout.width,
        container.width,
        props.nudge_align_into_viewport,
    )
    .ceil();
    let (top, bottom) = match (side, overlap) {
        // `overlap_vertical` pins the popout to the container edge on its own side.
        (Side::Top, true) => (Some(0.), None),
        (Side::Bottom, true) => (None, Some(0.)),
        (Side::Top, false) => (
            None,
            Some(container.height - target.origin.y + props.spacing),
        ),
        (Side::Bottom, false) => (
            Some(target.origin.y + target.size.height + props.spacing),
            None,
        ),
    };
    LayerStyle { top, bottom, left }
}

/// `g` of module 623646: the room left between the popout and the container edge it grows toward.
fn room(side: Side, style: LayerStyle, popout: Size<f32>, container: Size<f32>) -> f32 {
    match side {
        Side::Top => container.height - (style.bottom.unwrap_or(0.) + popout.height),
        Side::Bottom => container.height - (style.top.unwrap_or(0.) + popout.height),
    }
}

/// `calculateState` of module 623646: the side the popout opens on and its top left corner in the container.
pub(super) fn place(
    props: Props,
    target: Bounds<f32>,
    popout: Size<f32>,
    container: Size<f32>,
) -> (Side, Point<f32>) {
    let wanted = props.position;
    let mut side = wanted;
    let mut style = position_style(wanted, false, props, target, popout, container);
    let first = room(wanted, style, popout, container);
    let mut best = first;
    if props.auto_invert && first < 0. {
        let inverted = wanted.opposite();
        let other = position_style(inverted, false, props, target, popout, container);
        let other_room = room(inverted, other, popout, container);
        if other_room > first {
            side = inverted;
            style = other;
            best = other_room;
        }
        if other_room < 0. {
            // Neither side fits: `overlap_vertical` against the side kept, measured from the opposite edge.
            let overlap = position_style(side, true, props, target, popout, container);
            let overlap_room = room(side.opposite(), overlap, popout, container);
            if overlap_room > best {
                style = overlap;
                best = overlap_room;
            }
        }
    }
    // A top or bottom popout still short of room by less than its height is shifted back by what it lacks.
    if best < 0. && best.abs() < popout.height {
        match side {
            Side::Top => style.bottom = Some(style.bottom.unwrap_or(0.) + best),
            Side::Bottom => style.top = Some(style.top.unwrap_or(0.) + best),
        }
    }
    let top = style.top_edge(popout.height, container.height);
    (side, point(style.left, top))
}

/// Draws `child` in a top layer (`deferred`, never clipped by the content it is declared in) placed against the
/// element it is put in: that parent is the target, and must be positioned (`relative`).
pub(super) fn reference_layer(props: Props, priority: usize, child: AnyElement) -> AnyElement {
    deferred(ReferenceLayer { props, child })
        .with_priority(priority)
        .into_any_element()
}

struct ReferenceLayer {
    props: Props,
    child: AnyElement,
}

impl IntoElement for ReferenceLayer {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ReferenceLayer {
    type RequestLayoutState = LayoutId;
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
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child = self.child.request_layout(window, cx);
        // The layer fills its parent, the target; the popout inside it takes its own size.
        let style = Style {
            position: Position::Absolute,
            display: Display::Block,
            inset: Edges::all(px(0.).into()),
            ..Style::default()
        };
        let layout = window.request_layout(style, [child], cx);
        (layout, child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let laid = window.layout_bounds(*child);
        let viewport = window.viewport_size();
        let f = |value: Pixels| f32::from(value);
        let target = Bounds {
            origin: point(f(bounds.origin.x), f(bounds.origin.y)),
            size: Size {
                width: f(bounds.size.width),
                height: f(bounds.size.height),
            },
        };
        let (_, at) = place(
            self.props,
            target,
            Size {
                width: f(laid.size.width),
                height: f(laid.size.height),
            },
            Size {
                width: f(viewport.width),
                height: f(viewport.height),
            },
        );
        let origin = point(px(at.x), px(at.y));
        window.with_element_offset(origin - laid.origin, |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Popout` as the soundboard tip (module 195118 for 987933) uses it.
    const TIP: Props = Props {
        position: Side::Top,
        spacing: 14.,
        auto_invert: true,
        nudge_align_into_viewport: true,
    };

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<f32> {
        Bounds {
            origin: point(x, y),
            size: Size {
                width: w,
                height: h,
            },
        }
    }

    fn size(width: f32, height: f32) -> Size<f32> {
        Size { width, height }
    }

    #[test]
    fn it_opens_over_the_target_centred_when_there_is_room() {
        // A 40px button at (600, 700) in a 1280 x 900 window, a 240 x 200 popout: 14px over it, centred.
        let (side, at) = place(
            TIP,
            bounds(600., 700., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(side, Side::Top);
        assert_eq!(at, point(500., 486.));
    }

    #[test]
    fn it_inverts_under_the_target_when_only_that_side_has_room() {
        // The button 210px from the top: 210 - 14 - 200 < 0 over it, room under it.
        let (side, at) = place(
            TIP,
            bounds(600., 210., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(side, Side::Bottom);
        assert_eq!(at, point(500., 264.));
    }

    #[test]
    fn it_keeps_the_side_with_more_room_and_overlaps_when_neither_fits() {
        // 300px high window, the button in the middle: 120 - 14 - 200 = -94 over, 300 - (160 + 14 + 200) = -74
        // under. Under has more room; neither fits, so it pins to the bottom edge (overlap_vertical).
        let (side, at) = place(
            TIP,
            bounds(600., 120., 40., 40.),
            size(240., 200.),
            size(1280., 300.),
        );
        assert_eq!(side, Side::Bottom);
        assert_eq!(at, point(500., 100.));
        // Over has more room: pinned to the top edge.
        let (side, at) = place(
            TIP,
            bounds(600., 150., 40., 40.),
            size(240., 200.),
            size(1280., 300.),
        );
        assert_eq!(side, Side::Top);
        assert_eq!(at, point(500., 0.));
    }

    #[test]
    fn it_is_nudged_12px_inside_the_container_horizontally() {
        // Centred, it would start at -80: pushed to 12.
        let (_, at) = place(
            TIP,
            bounds(20., 700., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(at.x, 12.);
        // Within 12px of the right edge: pushed so its right edge is 12px in.
        let (_, at) = place(
            TIP,
            bounds(1150., 700., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(at.x, 1280. - 12. - 240.);
        // The left edge rounds up.
        let (_, at) = place(
            TIP,
            bounds(600.5, 700., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(at.x, 501.);
    }

    #[test]
    fn without_auto_invert_a_short_popout_is_shifted_back_to_the_edge() {
        let props = Props {
            auto_invert: false,
            ..TIP
        };
        let (side, at) = place(
            props,
            bounds(600., 150., 40., 40.),
            size(240., 200.),
            size(1280., 900.),
        );
        assert_eq!(side, Side::Top);
        assert_eq!(at.y, 0.);
    }
}
