//! The geometry of the image crop modal (module 142630) and its helpers: `Qx`, `Qp`, `l$` and `F3` of module 424632,
//! the maximum output sizes `P` of 142630, and the canvas draw of `R7` in module 830917. Everything is in CSS pixels
//! of the editing area unless named natural (the decoded picture's own pixels), in `f64` as the bundle computes.

/// All upload types of module 339984 `HL` handled by module 142630 `P`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum UploadType {
    Avatar,
    AvatarDecoration,
    Banner,
    GuildBanner,
    HomeHeader,
    GuildIcon,
    VideoBackground,
    ScheduledEventImage,
    PersonalWidgetField,
    PersonalWidgetCover,
}

/// `wL` of module 339984: the profile banner's width over its height.
pub(crate) const BANNER_RATIO: f64 = 17. / 6.;
/// `AB`: the server banner's ratio.
const GUILD_BANNER_RATIO: f64 = 16. / 9.;
/// `SCHEDULED_EVENT_IMAGE` ratio (2400 / 960 = 2.5).
const SCHEDULED_EVENT_RATIO: f64 = 2.5;
/// The widest the crop area gets (`432` throughout module 424632): the modal's content width.
pub(crate) const MAX_WIDTH: f64 = 432.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub(crate) const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

/// How far the picture may move from the centre (`dragBoundaries`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Bounds {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// `P(type)` of module 142630: the largest picture the cropper hands on (the canvas is never bigger).
pub(crate) fn max_dimensions(kind: UploadType) -> Size {
    match kind {
        UploadType::Banner => Size::new(2400., 848.),
        UploadType::Avatar
        | UploadType::AvatarDecoration
        | UploadType::GuildIcon
        | UploadType::PersonalWidgetField => Size::new(1024., 1024.),
        UploadType::GuildBanner | UploadType::PersonalWidgetCover => Size::new(2400., 1350.),
        UploadType::ScheduledEventImage => Size::new(2400., 960.),
        UploadType::HomeHeader => Size::new(2400., 600.),
        UploadType::VideoBackground => Size::new(1280., 720.),
    }
}

/// `P(type, cropAspectRatio)` of module 142630: fits an optional custom aspect ratio inside `max_dimensions`.
#[allow(dead_code)]
pub(crate) fn max_dimensions_with_aspect(kind: UploadType, aspect_ratio: Option<f64>) -> Size {
    let base = max_dimensions(kind);
    let Some(ratio) = aspect_ratio.filter(|r| r.is_finite() && *r > 0.) else {
        return base;
    };
    if ratio >= base.width / base.height {
        Size::new(base.width, base.width / ratio)
    } else {
        Size::new(base.height * ratio, base.height)
    }
}

/// Whether the output is not square (`e0` of module 142630).
pub(crate) fn wide_output(kind: UploadType) -> bool {
    let max = max_dimensions(kind);
    max.width != max.height
}

/// `o` of module 424632: a picture wider than 432 is first narrowed to 432; one narrower than `ratio` keeps that
/// size, a wider one is as tall as the crop area (`max_height`).
fn fit(ratio: f64, max_height: f64, width: f64, height: f64) -> Size {
    let (mut w, mut h) = (width, height);
    if width > MAX_WIDTH {
        w = MAX_WIDTH;
        h = MAX_WIDTH / width * height;
    }
    if w / h < ratio {
        Size::new(w, h)
    } else {
        Size::new(max_height / h * w, max_height)
    }
}

/// `Qx`: the size the picture is drawn at for a type, from its size at the editing area's full height.
pub(crate) fn image_dimensions(kind: UploadType, width: f64, height: f64) -> Size {
    match kind {
        UploadType::Avatar
        | UploadType::AvatarDecoration
        | UploadType::GuildIcon
        | UploadType::PersonalWidgetField => Size::new(width, height),
        // `pj = 432 / wL`.
        UploadType::Banner => fit(BANNER_RATIO, MAX_WIDTH / BANNER_RATIO, width, height),
        // `Jz = 432 / AB`.
        UploadType::GuildBanner | UploadType::VideoBackground | UploadType::PersonalWidgetCover => {
            fit(
                GUILD_BANNER_RATIO,
                MAX_WIDTH / GUILD_BANNER_RATIO,
                width,
                height,
            )
        }
        UploadType::ScheduledEventImage => fit(
            SCHEDULED_EVENT_RATIO,
            MAX_WIDTH / SCHEDULED_EVENT_RATIO,
            width,
            height,
        ),
        UploadType::HomeHeader => fit(4., 108., width, height),
    }
}

/// `Qp`: the crop area for a picture drawn at `width` x `height`; `bound_height` is the height the picture had before
/// `Qx` (the fourth argument).
pub(crate) fn crop_dimensions(
    kind: UploadType,
    width: f64,
    height: f64,
    bound_height: f64,
) -> Size {
    match kind {
        UploadType::Avatar
        | UploadType::AvatarDecoration
        | UploadType::GuildIcon
        | UploadType::PersonalWidgetField => {
            let side = width.min(height);
            Size::new(side, side)
        }
        UploadType::Banner => {
            let w = width.min(MAX_WIDTH);
            Size::new(w, w * (1. / BANNER_RATIO))
        }
        UploadType::GuildBanner | UploadType::VideoBackground | UploadType::PersonalWidgetCover => {
            let w = width.min(MAX_WIDTH);
            Size::new(w, (9. / 16. * w).min(bound_height))
        }
        UploadType::ScheduledEventImage => {
            let w = width.min(MAX_WIDTH);
            Size::new(w, (w / SCHEDULED_EVENT_RATIO).min(bound_height))
        }
        UploadType::HomeHeader => {
            let w = width.min(MAX_WIDTH);
            Size::new(w, 0.25 * w)
        }
    }
}

/// `l$`: how far a picture of `width` x `height` may move before it uncovers the crop area.
pub(crate) fn drag_bounds(width: f64, height: f64, crop: Size) -> Bounds {
    let mut bounds = Bounds::default();
    let dx = width - crop.width;
    let dy = height - crop.height;
    if dx != 0. {
        bounds.left = -(dx / 2.).abs();
        bounds.right = dx / 2.;
    }
    if dy != 0. {
        bounds.bottom = -(dy / 2.).abs();
        bounds.top = dy / 2.;
    }
    bounds
}

/// lodash `clamp(number, lower, upper)`: the upper bound first, then the lower.
fn clamp(value: f64, lower: f64, upper: f64) -> f64 {
    let value = if value <= upper { value } else { upper };
    if value >= lower { value } else { lower }
}

/// `F3`: the offset kept inside the bounds.
pub(crate) fn clamp_offset(x: f64, y: f64, bounds: Bounds) -> Point {
    Point {
        x: clamp(x, bounds.left, bounds.right),
        y: clamp(y, bounds.bottom, bounds.top),
    }
}

/// The editor's state (the reducer `U` of module 142630 and the live offset `J`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Editor {
    pub kind: UploadType,
    /// The decoded picture's own size (`naturalWidth`, `naturalHeight`), upright.
    pub natural: Size,
    /// `imageDimensions`: the picture's drawn size at zoom 1, as it looks (rotated).
    pub image: Size,
    pub crop: Size,
    pub bounds: Bounds,
    pub offset: Point,
    /// 0, 90, 180 or 270, clockwise.
    pub rotation: u32,
    pub zoom: f64,
    pub has_edits: bool,
}

/// `V`: anything other than the starting transform is an edit.
fn edited(zoom: f64, rotation: u32, offset: Point) -> bool {
    zoom != 1. || rotation != 0 || offset.x != 0. || offset.y != 0.
}

impl Editor {
    /// The picture has loaded (`eK`): it is drawn at the editing area's full height (`.image{height:100%}`), and the
    /// type turns that size into the drawn size and the crop area.
    pub(crate) fn new(kind: UploadType, natural: Size, area_height: f64) -> Self {
        let width = area_height * natural.width / natural.height;
        let image = image_dimensions(kind, width, area_height);
        let crop = crop_dimensions(kind, image.width, image.height, area_height);
        let bounds = drag_bounds(image.width, image.height, crop);
        Self {
            kind,
            natural,
            image,
            crop,
            bounds,
            offset: Point::default(),
            rotation: 0,
            zoom: 1.,
            has_edits: false,
        }
    }

    /// The editing area changed height (`resize` runs `eK` again while the zoom is 1): new sizes, the offset kept.
    pub(crate) fn resized(&mut self, area_height: f64) {
        if self.zoom > 1. {
            return;
        }
        let mut next = Self::new(self.kind, self.natural, area_height);
        // `eK` measures the element as it is laid out, rotated by the transform; a quarter turn swaps its sides.
        for _ in (0..self.rotation).step_by(90) {
            if next.image.width == next.image.height {
                continue;
            }
            let (width, height) = (next.image.height, next.image.width);
            next.image = image_dimensions(self.kind, width, height);
            next.crop = crop_dimensions(self.kind, next.image.width, next.image.height, height);
            next.bounds = drag_bounds(next.image.width, next.image.height, next.crop);
        }
        self.image = next.image;
        self.crop = next.crop;
        self.bounds = next.bounds;
        self.offset = clamp_offset(self.offset.x, self.offset.y, self.bounds);
    }

    /// Whether the picture can move sideways (`eH`) and up and down (`ev`).
    pub(crate) fn movable(&self) -> (bool, bool) {
        (self.bounds.right > 0., self.bounds.top > 0.)
    }

    /// `e$`: moves the picture, kept inside the bounds.
    pub(crate) fn move_to(&mut self, x: f64, y: f64) {
        self.offset = clamp_offset(x, y, self.bounds);
    }

    /// `STOP_DRAGGING_IMAGE`: the offset is committed and the edits counted again.
    pub(crate) fn commit(&mut self) {
        self.has_edits = edited(self.zoom, self.rotation, self.offset);
    }

    /// `eX`: the zoom slider moved.
    pub(crate) fn set_zoom(&mut self, zoom: f64) {
        self.bounds = drag_bounds(self.image.width * zoom, self.image.height * zoom, self.crop);
        self.offset = clamp_offset(self.offset.x, self.offset.y, self.bounds);
        self.zoom = zoom;
        self.has_edits = edited(zoom, self.rotation, self.offset);
    }

    /// `eJ`: the sizes of the picture turned a quarter, at `zoom`.
    fn turned(&self, zoom: f64) -> (Size, Size, Bounds) {
        let image = image_dimensions(self.kind, self.image.height, self.image.width);
        let crop = crop_dimensions(self.kind, image.width, image.height, self.image.width);
        let bounds = drag_bounds(image.width * zoom, image.height * zoom, crop);
        (image, crop, bounds)
    }

    /// `eZ`: a quarter turn clockwise; the offset turns with the picture. Returns the new angle, which the bundle
    /// announces ("Rotated to {degrees} degrees").
    pub(crate) fn rotate(&mut self) -> u32 {
        let rotation = (self.rotation + 90) % 360;
        let (x, y) = (-self.offset.y, self.offset.x);
        if self.image.width != self.image.height {
            let (image, crop, bounds) = self.turned(self.zoom);
            self.image = image;
            self.crop = crop;
            self.bounds = bounds;
        }
        self.offset = clamp_offset(x, y, self.bounds);
        self.rotation = rotation;
        self.has_edits = edited(self.zoom, rotation, self.offset);
        rotation
    }

    /// `eq`: Reset puts the picture back upright, centred and unzoomed.
    pub(crate) fn reset(&mut self) {
        if self.image.width != self.image.height && !self.rotation.is_multiple_of(180) {
            let (image, crop, bounds) = self.turned(1.);
            self.image = image;
            self.crop = crop;
            self.bounds = bounds;
        } else {
            self.bounds = drag_bounds(self.image.width, self.image.height, self.crop);
        }
        self.offset = Point::default();
        self.rotation = 0;
        self.zoom = 1.;
        self.has_edits = false;
    }

    /// `e5`: an arrow key moves the picture 4px, 40px with Shift, along an axis it can move on. Returns whether
    /// the key was an arrow the editor took.
    pub(crate) fn nudge(&mut self, key: &str, shift: bool) -> bool {
        let step = if shift { 40. } else { 4. };
        let (dx, dy) = match key {
            "left" => (-step, 0.),
            "right" => (step, 0.),
            "up" => (0., -step),
            "down" => (0., step),
            _ => return false,
        };
        let (sideways, upright) = self.movable();
        if (dx != 0. && sideways) || (dy != 0. && upright) {
            self.move_to(self.offset.x + dx, self.offset.y + dy);
            self.commit();
        }
        true
    }

    /// `e1`'s factor: a wide output type stretches a picture wider than the banner ratio to the crop's height.
    fn stretch(&self) -> f64 {
        let ratio = self.image.width / self.image.height;
        if wide_output(self.kind) && ratio > BANNER_RATIO {
            self.crop.height / self.image.height
        } else {
            1.
        }
    }

    /// `e1`: the element's own (unrotated) layout size, zoom included.
    pub(crate) fn element(&self) -> Size {
        let scale = self.zoom * self.stretch();
        let (width, height) = if !self.rotation.is_multiple_of(180) {
            (self.image.height, self.image.width)
        } else {
            (self.image.width, self.image.height)
        };
        Size::new(width * scale, height * scale)
    }

    /// The picture as it looks: the element turned by the rotation.
    pub(crate) fn drawn(&self) -> Size {
        let element = self.element();
        if !self.rotation.is_multiple_of(180) {
            Size::new(element.height, element.width)
        } else {
            element
        }
    }

    /// `R7` of module 830917 for this transform.
    pub(crate) fn draw(&self) -> Draw {
        draw(
            self.natural,
            self.element(),
            self.crop,
            self.offset,
            max_dimensions(self.kind),
            self.rotation,
        )
    }
}

/// What the canvas of module 830917 draws: the source rectangle of the natural picture, and the canvas size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Draw {
    pub source_x: f64,
    pub source_y: f64,
    pub source_width: f64,
    pub source_height: f64,
    /// `canvas.width` and `canvas.height` (assigned from fractions, so truncated).
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub rotation: u32,
}

/// `R7` (`h` of module 830917) without the flip: the crop area's centre in the element's own frame, turned back by
/// the rotation, scaled to natural pixels.
pub(crate) fn draw(
    natural: Size,
    element: Size,
    crop: Size,
    offset: Point,
    max: Size,
    rotation: u32,
) -> Draw {
    let scale = natural.width / element.width;
    let turned = match rotation {
        90 => Point {
            x: offset.y,
            y: -offset.x,
        },
        180 => Point {
            x: -offset.x,
            y: -offset.y,
        },
        270 => Point {
            x: -offset.y,
            y: offset.x,
        },
        _ => offset,
    };
    let sideways = !rotation.is_multiple_of(180);
    let crop_width = crop.width * scale;
    let crop_height = crop.height * scale;
    let canvas_width = crop_width.min(max.width);
    let canvas_height = crop_height.min(max.height);
    let half_width = (if sideways { crop.height } else { crop.width }) / 2.;
    let half_height = (if sideways { crop.width } else { crop.height }) / 2.;
    Draw {
        source_x: (element.width / 2. - half_width - turned.x) * scale,
        source_y: (element.height / 2. - half_height - turned.y) * scale,
        source_width: if sideways { crop_height } else { crop_width },
        source_height: if sideways { crop_width } else { crop_height },
        canvas_width: canvas_width as u32,
        canvas_height: canvas_height as u32,
        rotation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_largest_outputs_are_the_bundles() {
        assert_eq!(max_dimensions(UploadType::Avatar), Size::new(1024., 1024.));
        assert_eq!(
            max_dimensions(UploadType::GuildIcon),
            Size::new(1024., 1024.)
        );
        assert_eq!(max_dimensions(UploadType::Banner), Size::new(2400., 848.));
        assert_eq!(
            max_dimensions(UploadType::GuildBanner),
            Size::new(2400., 1350.)
        );
        assert_eq!(
            max_dimensions(UploadType::HomeHeader),
            Size::new(2400., 600.)
        );
        assert!(!wide_output(UploadType::Avatar));
        assert!(wide_output(UploadType::Banner));
    }

    #[test]
    fn an_avatar_crop_is_the_shorter_side_of_the_drawn_picture() {
        // A 1600 x 900 picture in the 200px high area is drawn 355.6 x 200; the circle is 200 across.
        let editor = Editor::new(UploadType::Avatar, Size::new(1600., 900.), 200.);
        assert!(close(editor.image.width, 200. * 16. / 9.));
        assert_eq!(editor.image.height, 200.);
        assert_eq!(editor.crop, Size::new(200., 200.));
        assert!(close(editor.bounds.right, (200. * 16. / 9. - 200.) / 2.));
        assert!(close(editor.bounds.left, -editor.bounds.right));
        assert_eq!((editor.bounds.top, editor.bounds.bottom), (0., 0.));
        assert_eq!(editor.movable(), (true, false));
    }

    #[test]
    fn a_wide_banner_is_drawn_as_tall_as_its_crop() {
        // 3000 x 600 at 200 high is 1000 x 200: narrowed to 432 x 86.4, wider than 17:6, so 152.47 high.
        let editor = Editor::new(UploadType::Banner, Size::new(3000., 600.), 200.);
        let crop_height = MAX_WIDTH / BANNER_RATIO;
        assert!(close(editor.image.height, crop_height));
        assert!(close(editor.image.width, crop_height / 86.4 * 432.));
        assert!(close(editor.crop.width, 432.));
        assert!(close(editor.crop.height, crop_height));
        // A narrow one keeps its size and the crop is as wide as the picture.
        let tall = Editor::new(UploadType::Banner, Size::new(500., 1000.), 200.);
        assert_eq!(tall.image, Size::new(100., 200.));
        assert!(close(tall.crop.width, 100.));
        assert!(close(tall.crop.height, 100. * 6. / 17.));
    }

    #[test]
    fn the_server_guide_banner_is_four_to_one() {
        let editor = Editor::new(UploadType::HomeHeader, Size::new(2000., 1000.), 350.);
        // 700 x 350, narrowed to 432 x 216 (2:1, under 4:1).
        assert_eq!(editor.image, Size::new(432., 216.));
        assert_eq!(editor.crop, Size::new(432., 108.));
        assert_eq!(editor.bounds.top, 54.);
        assert_eq!(editor.bounds.right, 0.);
    }

    #[test]
    fn the_guild_banner_crop_never_outgrows_the_picture() {
        let crop = crop_dimensions(UploadType::GuildBanner, 432., 100., 100.);
        assert_eq!(crop, Size::new(432., 100.));
        let crop = crop_dimensions(UploadType::GuildBanner, 432., 300., 300.);
        assert_eq!(crop, Size::new(432., 243.));
    }

    #[test]
    fn drags_stop_at_the_bounds() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(400., 200.), 200.);
        assert_eq!(editor.bounds.right, 100.);
        editor.move_to(500., 30.);
        assert_eq!(editor.offset, Point { x: 100., y: 0. });
        editor.move_to(-500., -30.);
        assert_eq!(editor.offset, Point { x: -100., y: 0. });
        assert!(!editor.has_edits);
        editor.commit();
        assert!(editor.has_edits);
    }

    #[test]
    fn zooming_widens_the_bounds_and_zooming_out_pulls_the_picture_back() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(200., 200.), 200.);
        assert_eq!(editor.movable(), (false, false));
        editor.set_zoom(2.);
        assert_eq!(editor.bounds.right, 100.);
        assert_eq!(editor.bounds.top, 100.);
        editor.move_to(90., -90.);
        editor.set_zoom(1.5);
        assert_eq!(editor.offset, Point { x: 50., y: -50. });
        assert!(editor.has_edits);
        assert_eq!(editor.element(), Size::new(300., 300.));
    }

    #[test]
    fn a_quarter_turn_swaps_the_sides_and_turns_the_offset() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(400., 200.), 200.);
        editor.move_to(60., 0.);
        assert_eq!(editor.rotate(), 90);
        // Drawn 400 x 200, turned it looks 200 x 400: the circle stays 200 across and may now move up and down.
        assert_eq!(editor.image, Size::new(200., 400.));
        assert_eq!(editor.crop, Size::new(200., 200.));
        assert_eq!(editor.offset, Point { x: 0., y: 60. });
        // The element keeps its own unrotated size.
        assert_eq!(editor.element(), Size::new(400., 200.));
        assert_eq!(editor.drawn(), Size::new(200., 400.));
        assert_eq!(editor.rotate(), 180);
        assert_eq!(editor.rotate(), 270);
        assert_eq!(editor.rotate(), 0);
        assert_eq!(editor.image, Size::new(400., 200.));
        assert_eq!(editor.offset, Point { x: 60., y: 0. });
    }

    #[test]
    fn reset_puts_everything_back() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(400., 200.), 200.);
        let start = editor.clone();
        editor.set_zoom(1.8);
        editor.move_to(10., 20.);
        editor.rotate();
        editor.reset();
        assert_eq!(editor, start);
    }

    #[test]
    fn arrow_keys_move_by_four_or_forty_on_a_movable_axis() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(400., 200.), 200.);
        assert!(editor.nudge("right", false));
        assert_eq!(editor.offset, Point { x: 4., y: 0. });
        assert!(editor.nudge("left", true));
        assert_eq!(editor.offset, Point { x: -36., y: 0. });
        // It cannot move up and down.
        assert!(editor.nudge("down", true));
        assert_eq!(editor.offset.y, 0.);
        assert!(!editor.nudge("home", false));
        assert!(editor.has_edits);
    }

    #[test]
    fn the_output_of_an_untouched_avatar_is_the_centre_square() {
        // 1600 x 900: drawn 355.6 x 200, the crop 200 square; scale 4.5.
        let editor = Editor::new(UploadType::Avatar, Size::new(1600., 900.), 200.);
        let draw = editor.draw();
        assert!(close(draw.source_x, 350.));
        assert!(close(draw.source_y, 0.));
        assert!(close(draw.source_width, 900.));
        assert!(close(draw.source_height, 900.));
        assert_eq!((draw.canvas_width, draw.canvas_height), (900, 900));
    }

    #[test]
    fn the_output_is_capped_at_the_types_largest_size() {
        let editor = Editor::new(UploadType::Avatar, Size::new(4000., 2000.), 200.);
        let draw = editor.draw();
        assert!(close(draw.source_width, 2000.));
        assert_eq!((draw.canvas_width, draw.canvas_height), (1024, 1024));
        let banner = Editor::new(UploadType::Banner, Size::new(6000., 1200.), 350.);
        let draw = banner.draw();
        assert_eq!(draw.canvas_width, 2400);
        assert_eq!(draw.canvas_height, 848);
    }

    #[test]
    fn a_moved_picture_crops_the_other_way_and_zoom_shrinks_the_source() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(400., 200.), 200.);
        // Moved 100px right, the crop sits over the picture's left end.
        editor.move_to(100., 0.);
        let draw = editor.draw();
        assert!(close(draw.source_x, 0.));
        editor.set_zoom(2.);
        editor.move_to(0., 0.);
        let draw = editor.draw();
        // Zoomed twice, the crop covers half as many natural pixels, around the centre.
        assert!(close(draw.source_width, 100.));
        assert!(close(draw.source_x, 150.));
        assert!(close(draw.source_y, 50.));
    }

    #[test]
    fn a_turned_crop_reads_the_offset_in_the_pictures_own_frame() {
        let mut editor = Editor::new(UploadType::Avatar, Size::new(200., 400.), 200.);
        // 100 x 200 drawn, the crop 100 across; turned it is 200 x 100 and may move sideways by 50.
        editor.rotate();
        assert_eq!(editor.image, Size::new(200., 100.));
        editor.move_to(50., 0.);
        let draw = editor.draw();
        // A clockwise turn puts the picture's bottom on the left; moved right, the crop shows that bottom half.
        assert!(close(draw.source_y, 200.));
        assert!(close(draw.source_x, 0.));
        assert!(close(draw.source_width, 200.));
    }
}
