//! The editor's size rules: `UserProfileModalV2` (`root_ZhNJfE`, `layoutContainer_s3DB6Y`) sets its sizes through
//! the `--custom-modal-v2-*` variables and their media queries (CSS `f4f30bf79660bbe0.css` and `8e8eccd890e9820b.css`
//! of the pin), here as numbers of the window's size.
//!
//! - width: 962px plus the panel's 240 inner width, at most the window less 48px each side, from 1169px; the whole
//!   window from 929px to 1168px; below 929px the whole window with the panel over the profile's right edge.
//! - height: the whole window below 785px (the modal sits under the app's 32px title bar, which it pads around);
//!   from 785px `min(800px, 100vh - 96px)`, centred. Never under 550px.
//! - the profile card: `min(max(300px, 45.5vw - (panel + 96px) * .455), 400px)` from 1169px, `45.5vw - panel * .455`
//!   from 929px; compact (`compact-sm`/`compact-xs`) at 380/350px and less.

/// `--custom-app-top-bar-height` on Windows (module 419070's `:root`).
const SYSTEM_BAR: f32 = 32.;
/// `--custom-modal-v2-editing-panel-inner-width`.
const PANEL_INNER: f32 = 240.;
/// `--custom-modal-v2-tabpanel-scrollbar-offset`.
pub(super) const SCROLLBAR_OFFSET: f32 = 16.;
/// `--custom-modal-v2-gutter`.
pub(super) const GUTTER: f32 = 32.;

/// The profile card's compact steps (`u2` of module 927071, set by the card's own width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Compact {
    None,
    Small,
    ExtraSmall,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    /// The editor's box in the window.
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// `--custom-modal-v2-editing-panel-width`: the panel with its borders.
    pub panel_width: f32,
    /// Below 929px the panel lies over the profile, at its right edge, instead of beside it.
    pub panel_over: bool,
    /// `--custom-modal-v2-system-bar-padding`: the title bar's height the content keeps clear of.
    pub system_bar: f32,
    /// `--custom-modal-v2-padding` of the profile column.
    pub padding: f32,
    /// `--custom-modal-v2-profile-width`.
    pub profile_width: f32,
    pub compact: Compact,
    /// `border-radius` of the editor: `--radius-lg` from 785px high and 1169px wide.
    pub radius: f32,
    /// The profile column's 1px end border (`border-inline-end` of `profileContentOuter_s3DB6Y`).
    pub content_border: f32,
}

impl Geometry {
    pub(super) fn of(window_width: f32, window_height: f32) -> Self {
        let (w, h) = (window_width, window_height);
        let wide = w >= 1169.;
        let grid = w >= 929.;
        let tall = h >= 785.;
        let width = if wide {
            (962. + PANEL_INNER).min(w - 96.)
        } else {
            w
        };
        let height = if tall { 800_f32.min(h - 96.) } else { h };
        let panel_width = PANEL_INNER + if wide { 2. } else { 1. };
        let profile = if wide {
            0.455 * w - (panel_width + 96.) * 0.455
        } else if grid {
            0.455 * w - panel_width * 0.455
        } else {
            0.455 * w
        };
        let profile_width = profile.clamp(300., 400.);
        let compact = if profile_width <= 350. {
            Compact::ExtraSmall
        } else if profile_width <= 380. {
            Compact::Small
        } else {
            Compact::None
        };
        Self {
            left: ((w - width) / 2.).max(0.),
            top: if tall {
                ((h - height) / 2.).max(0.)
            } else {
                0.
            },
            width,
            height,
            panel_width,
            panel_over: !grid,
            system_bar: if tall { 0. } else { SYSTEM_BAR },
            padding: if w >= 1267. { 48. } else { 32. },
            profile_width,
            compact,
            radius: if tall && wide { 16. } else { 0. },
            content_border: if wide { 1. } else { 0. },
        }
    }

    /// The profile column (`inner_P1YEjQ`): the editor less the panel beside it.
    pub(super) fn content_width(&self) -> f32 {
        let panel = if self.panel_over {
            0.
        } else {
            self.panel_width
        };
        self.width - panel - self.content_border
    }

    /// `profileContentColumns_s3DB6Y`: the content less its start and end padding.
    pub(super) fn columns_width(&self) -> f32 {
        self.content_width() - self.padding - (self.padding - SCROLLBAR_OFFSET)
    }

    /// The top padding of the profile column, where the card starts.
    pub(super) fn content_top(&self) -> f32 {
        self.padding + self.system_bar
    }

    /// The card runs from the column's top padding to the editor's bottom edge.
    pub(super) fn card_height(&self) -> f32 {
        self.height - self.content_top()
    }

    /// `container_y4ZEM5`: the widgets column beside the card.
    pub(super) fn widgets_width(&self) -> f32 {
        (self.columns_width() - self.profile_width).max(0.)
    }

    /// The board's grid: the widgets column less the gutter and the scrollbar offset.
    pub(super) fn board_width(&self) -> f32 {
        (self.widgets_width() - GUTTER - SCROLLBAR_OFFSET).max(0.)
    }

    /// `widgetGrid_jFcvcH`: four columns, each tile spans two, 16px gaps.
    pub(super) fn tile_width(&self) -> f32 {
        ((self.board_width() - 16.) / 2.).max(0.)
    }

    /// `--custom-modal-v2-header-button-top`: the close button's top.
    pub(super) fn close_top(&self) -> f32 {
        8. + self.system_bar
    }

    /// `--custom-modal-v2-profile-card-padding`.
    pub(super) fn card_padding(&self) -> f32 {
        if self.compact == Compact::None {
            32.
        } else {
            24.
        }
    }

    /// `avatarOffsetX` of `u2` (16 when compact), the avatar's ring 9px outside it.
    pub(super) fn avatar_left(&self) -> f32 {
        if self.compact == Compact::None {
            32.
        } else {
            16.
        }
    }

    /// `avatarSize`: 120, 96 at `compact-xs`.
    pub(super) fn avatar_size(&self) -> f32 {
        if self.compact == Compact::ExtraSmall {
            96.
        } else {
            120.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinned capture's window (`profile-editor-darker-cozy.json`): 1184 wide from x 48, 720 high, the panel 242,
    /// the content 941 with 48px padding and 80 at the top, the card 400x640, the widgets column 461.
    #[test]
    fn the_captured_window_gives_the_captured_boxes() {
        let geometry = Geometry::of(1280., 720.);
        assert_eq!(geometry.left, 48.);
        assert_eq!(geometry.width, 1184.);
        assert_eq!(geometry.height, 720.);
        assert_eq!(geometry.top, 0.);
        assert_eq!(geometry.panel_width, 242.);
        assert_eq!(geometry.content_width(), 941.);
        assert_eq!(geometry.columns_width(), 861.);
        assert_eq!(geometry.content_top(), 80.);
        assert_eq!(geometry.profile_width, 400.);
        assert_eq!(geometry.card_height(), 640.);
        assert_eq!(geometry.widgets_width(), 461.);
        assert_eq!(geometry.board_width(), 413.);
        assert_eq!(geometry.tile_width(), 198.5);
        assert_eq!(geometry.close_top(), 40.);
        assert_eq!(geometry.radius, 0.);
    }

    /// From 1169px the editor is 1202 wide, never closer than 48px to the window's sides.
    #[test]
    fn a_wide_window_caps_the_width() {
        assert_eq!(Geometry::of(1920., 1080.).width, 1202.);
        assert_eq!(Geometry::of(1200., 720.).width, 1104.);
        assert_eq!(Geometry::of(1920., 1080.).left, 359.);
    }

    /// Between 929 and 1168px the editor takes the whole window, the panel one pixel narrower.
    #[test]
    fn a_middle_window_is_filled() {
        let geometry = Geometry::of(1100., 700.);
        assert_eq!((geometry.left, geometry.width), (0., 1100.));
        assert_eq!(geometry.panel_width, 241.);
        assert_eq!(geometry.content_border, 0.);
        assert_eq!(geometry.padding, 32.);
        assert!((geometry.profile_width - (0.455 * 1100. - 241. * 0.455)).abs() < 0.001);
        assert_eq!(geometry.compact, Compact::None);
    }

    /// From 785px high the editor is at most 800 high, centred, with no title bar padding and round corners.
    #[test]
    fn a_tall_window_centres_a_bounded_height() {
        let geometry = Geometry::of(1920., 1080.);
        assert_eq!(geometry.height, 800.);
        assert_eq!(geometry.top, 140.);
        assert_eq!(geometry.system_bar, 0.);
        assert_eq!(geometry.close_top(), 8.);
        assert_eq!(geometry.radius, 16.);
        assert_eq!(geometry.card_height(), 752.);
        let short = Geometry::of(1600., 900.);
        assert_eq!(short.height, 800.);
        assert_eq!(short.top, 50.);
    }

    /// The profile card shrinks with the window and turns compact at 380 and 350px.
    #[test]
    fn the_card_turns_compact_in_small_windows() {
        let small = Geometry::of(1000., 600.);
        assert!((small.profile_width - 345.345).abs() < 0.01);
        assert_eq!(small.compact, Compact::ExtraSmall);
        assert_eq!(small.avatar_size(), 96.);
        assert_eq!(small.card_padding(), 24.);
        let narrow = Geometry::of(800., 600.);
        assert!(narrow.panel_over);
        assert_eq!(narrow.width, 800.);
        assert_eq!(narrow.content_width(), 800.);
        assert_eq!(Geometry::of(800., 400.).height, 400.);
    }
}
