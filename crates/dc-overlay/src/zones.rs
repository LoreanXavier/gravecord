//! Clickable regions of a locked overlay (module 302614, the `data-click-zone` elements).
//!
//! A locked overlay lets every click through to the game, except inside the click zones the page registers with the
//! native module (`setClickZones`): the top notification, the soundboard, the empty state. Each zone is an element's
//! bounding box, every side rounded up (`Math.ceil`) and divided by the window's size. "Enable overlay clickable
//! regions" off (`OverlayStoreV2.disableClickableRegions`) registers none, so a locked overlay is click-through
//! everywhere. Unlocked, the overlay takes every click.

/// One zone as fractions of the overlay window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// `v` then `updateZones`: an element's bounds in window pixels as a zone of a `width` by `height` window.
pub fn zone_of(left: f32, top: f32, right: f32, bottom: f32, width: f32, height: f32) -> Zone {
    if width <= 0. || height <= 0. {
        return Zone {
            left: 0.,
            top: 0.,
            right: 0.,
            bottom: 0.,
        };
    }
    Zone {
        left: left.ceil() / width,
        top: top.ceil() / height,
        right: right.ceil() / width,
        bottom: bottom.ceil() / height,
    }
}

/// Who gets a click at a point of the overlay window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The overlay takes it.
    Overlay,
    /// It goes through to the window under the overlay.
    Through,
}

/// Whether a window point (pixels) is inside a zone. The right and bottom edges are outside.
pub fn inside(zones: &[Zone], x: f32, y: f32, width: f32, height: f32) -> bool {
    zones.iter().any(|zone| {
        x >= zone.left * width
            && x < zone.right * width
            && y >= zone.top * height
            && y < zone.bottom * height
    })
}

/// The hit test of the overlay window at a point.
pub fn hit(
    locked: bool,
    regions_disabled: bool,
    zones: &[Zone],
    point: (f32, f32),
    size: (f32, f32),
) -> Hit {
    if !locked {
        return Hit::Overlay;
    }
    if !regions_disabled && inside(zones, point.0, point.1, size.0, size.1) {
        Hit::Overlay
    } else {
        Hit::Through
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_is_the_rounded_up_bounds_over_the_window() {
        let zone = zone_of(12.2, 14.5, 332.1, 114.9, 1280., 720.);
        assert_eq!(zone.left, 13. / 1280.);
        assert_eq!(zone.top, 15. / 720.);
        assert_eq!(zone.right, 333. / 1280.);
        assert_eq!(zone.bottom, 115. / 720.);
        assert_eq!(zone_of(1., 1., 2., 2., 0., 0.).right, 0.);
    }

    #[test]
    fn a_locked_overlay_takes_clicks_only_in_its_zones() {
        let size = (1280., 720.);
        let zones = [zone_of(10., 10., 330., 110., size.0, size.1)];
        assert_eq!(hit(false, false, &[], (5., 5.), size), Hit::Overlay);
        assert_eq!(hit(true, false, &zones, (20., 20.), size), Hit::Overlay);
        assert_eq!(hit(true, false, &zones, (5., 5.), size), Hit::Through);
        // The right and bottom edges belong to what is beyond.
        assert_eq!(hit(true, false, &zones, (330., 50.), size), Hit::Through);
        assert_eq!(hit(true, false, &zones, (329.9, 109.9), size), Hit::Overlay);
        // Clickable regions off: nothing is clickable while locked.
        assert_eq!(hit(true, true, &zones, (20., 20.), size), Hit::Through);
        assert_eq!(hit(true, false, &[], (20., 20.), size), Hit::Through);
    }
}
