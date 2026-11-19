//! Where a web surface goes on screen: the box the app drew for it and the part of that box that may show.
//!
//! The official client's frames are part of the page: an `<iframe>` scrolled half out of the chat shows its visible
//! half and nothing over the header, it moves with the window, and it is gone with the window. A web view here is a
//! window of its own laid over the app's, so the same has to be made by hand: the surface's window covers only the
//! visible part of the box (the box cut to the view that holds it and to the app window's client area), and the web
//! view inside it keeps the whole box's size at an offset, so the page never re-lays out while it is clipped.

/// A rectangle in device pixels of the app window's client area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    /// A rectangle in the app's logical pixels (`(x, y, width, height)`), at `scale` device pixels each.
    pub fn logical(rect: (f32, f32, f32, f32), scale: f32) -> Self {
        Self {
            x: rect.0 * scale,
            y: rect.1 * scale,
            width: rect.2 * scale,
            height: rect.3 * scale,
        }
    }

    /// Snapped to whole device pixels: each edge to its nearest pixel, as a browser lays an iframe out.
    pub(crate) fn snapped(self) -> Px {
        Px {
            left: self.x.round() as i32,
            top: self.y.round() as i32,
            right: (self.x + self.width.max(0.)).round() as i32,
            bottom: (self.y + self.height.max(0.)).round() as i32,
        }
    }
}

/// Where a surface goes: the whole box, and the part of it that may show (`None`: none of it, the box is out of view).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub frame: Area,
    pub shown: Option<Area>,
}

impl Placement {
    /// A box shown whole (as far as the window's client area goes).
    pub fn whole(frame: Area) -> Self {
        Self {
            frame,
            shown: Some(frame),
        }
    }

    /// A box out of view.
    pub fn hidden(frame: Area) -> Self {
        Self { frame, shown: None }
    }
}

/// Whole device pixels, edges inclusive of `left`/`top` and exclusive of `right`/`bottom`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Px {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Px {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }

    fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    fn intersect(self, other: Px) -> Px {
        Px {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}

/// What the surface's window and the web view in it are set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The surface's window, in the client area: the visible part of the box.
    pub window: Px,
    /// The web view, in the surface's window: the whole box, offset so its visible part lands in the window.
    pub view: Px,
}

/// Lays a placement out in a client area of `client` device pixels; `None` when nothing of the box can show.
pub fn layout(placement: Placement, client: (i32, i32)) -> Option<Layout> {
    let frame = placement.frame.snapped();
    if frame.is_empty() {
        return None;
    }
    let client = Px {
        left: 0,
        top: 0,
        right: client.0,
        bottom: client.1,
    };
    let window = placement
        .shown?
        .snapped()
        .intersect(frame)
        .intersect(client);
    if window.is_empty() {
        return None;
    }
    Some(Layout {
        window,
        view: Px {
            left: frame.left - window.left,
            top: frame.top - window.top,
            right: frame.right - window.left,
            bottom: frame.bottom - window.top,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f32, y: f32, width: f32, height: f32) -> Area {
        Area {
            x,
            y,
            width,
            height,
        }
    }

    fn px(left: i32, top: i32, right: i32, bottom: i32) -> Px {
        Px {
            left,
            top,
            right,
            bottom,
        }
    }

    const CLIENT: (i32, i32) = (1600, 900);

    #[test]
    fn a_box_in_full_view_shows_whole() {
        let placement = Placement::whole(area(400., 300., 400., 352.));
        assert_eq!(
            layout(placement, CLIENT),
            Some(Layout {
                window: px(400, 300, 800, 652),
                view: px(0, 0, 400, 352),
            })
        );
    }

    #[test]
    fn a_box_scrolled_half_above_the_chat_shows_its_lower_part_only() {
        // The chat runs from y 49 to 800; the album box starts 100px above it.
        let placement = Placement {
            frame: area(400., -51., 400., 352.),
            shown: Some(area(300., 49., 900., 751.)),
        };
        let laid = layout(placement, CLIENT).expect("partly visible");
        assert_eq!(laid.window, px(400, 49, 800, 301));
        // The page keeps its whole size, 100px of it above the window's top edge.
        assert_eq!(laid.view, px(0, -100, 400, 252));
        assert_eq!(laid.view.width(), 400);
        assert_eq!(laid.view.height(), 352);
    }

    #[test]
    fn a_box_past_the_bottom_of_the_chat_is_cut_there() {
        let placement = Placement {
            frame: area(400., 700., 400., 352.),
            shown: Some(area(300., 49., 900., 751.)),
        };
        let laid = layout(placement, CLIENT).expect("partly visible");
        assert_eq!(laid.window, px(400, 700, 800, 800));
        assert_eq!(laid.view, px(0, 0, 400, 352));
    }

    #[test]
    fn a_box_out_of_the_chat_or_hidden_shows_nothing() {
        let chat = Some(area(300., 49., 900., 751.));
        let above = Placement {
            frame: area(400., -400., 400., 352.),
            shown: chat,
        };
        assert_eq!(layout(above, CLIENT), None);
        // Touching the edge is not showing.
        let touching = Placement {
            frame: area(400., -303., 400., 352.),
            shown: chat,
        };
        assert_eq!(layout(touching, CLIENT), None);
        assert_eq!(
            layout(Placement::hidden(area(400., 300., 400., 352.)), CLIENT),
            None
        );
        assert_eq!(
            layout(Placement::whole(area(400., 300., 0., 352.)), CLIENT),
            None
        );
    }

    #[test]
    fn nothing_shows_outside_the_window_s_client_area() {
        // Even with no view cutting it, a box past the window's edge stops at it (62.png: the album over another app).
        let placement = Placement::whole(area(1400., -60., 400., 352.));
        let laid = layout(placement, CLIENT).expect("partly visible");
        assert_eq!(laid.window, px(1400, 0, 1600, 292));
        assert_eq!(laid.view, px(0, -60, 400, 292));
        assert_eq!(
            layout(Placement::whole(area(1700., 100., 400., 80.)), CLIENT),
            None
        );
        assert_eq!(
            layout(Placement::whole(area(400., 100., 400., 80.)), (0, 0)),
            None
        );
    }

    #[test]
    fn logical_boxes_scale_and_snap_to_device_pixels() {
        // 125% scale (150% DPI at 0.833 zoom and the like): edges round to the nearest pixel.
        let frame = Area::logical((321.3, 100.1, 300., 80.), 1.25);
        let laid = layout(Placement::whole(frame), CLIENT).expect("visible");
        assert_eq!(laid.window, px(402, 125, 777, 225));
        assert_eq!(laid.view, px(0, 0, 375, 100));
        // A clip that ends mid-pixel still lines the window and the page up on the same pixel.
        let placement = Placement {
            frame,
            shown: Some(Area::logical((0., 140.3, 1000., 500.), 1.25)),
        };
        let laid = layout(placement, CLIENT).expect("visible");
        assert_eq!(laid.window, px(402, 175, 777, 225));
        assert_eq!(laid.view, px(0, -50, 375, 50));
    }
}
