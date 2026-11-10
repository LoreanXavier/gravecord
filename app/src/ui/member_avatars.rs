//! Avatar artwork with the source's transparent SVG masks and size table (97808/778712).
use super::{Root, color};
use crate::parity::Parity;
use dc_state::{Id, User, collectibles::AvatarDecoration};
use gpui::{Context, Corners, Image, RenderImage, Window, canvas, div, prelude::*, px};
use image::{Rgba, RgbaImage};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Size {
    Member,
    Profile,
}

impl Size {
    fn geometry(self) -> Geometry {
        let (base, status, stroke, offset) = match self {
            Self::Member => (32., 10., 3., 0.),
            Self::Profile => (80., 16., 6., 4.),
        };
        Geometry {
            base,
            art: base * 1.2,
            inset: base * 0.1,
            status,
            center: base - offset - status / 2.,
            hole: status / 2. + stroke,
        }
    }
}

struct Geometry {
    base: f32,
    art: f32,
    inset: f32,
    status: f32,
    center: f32,
    hole: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Online,
    Mobile,
    Idle,
    Dnd,
    Offline,
}

impl Status {
    fn parse(value: &str, mobile: bool) -> Self {
        match value {
            "online" if mobile => Self::Mobile,
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            "offline" | "invisible" => Self::Offline,
            _ => Self::Online,
        }
    }
    fn token(self) -> &'static str {
        match self {
            Self::Online | Self::Mobile => "--icon-status-online",
            Self::Idle => "--icon-status-idle",
            Self::Dnd => "--icon-status-dnd",
            Self::Offline => "--icon-status-offline",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Spec {
    size: Size,
    pixels: u32,
    status: Status,
    tint: [u8; 4],
}

struct CachedFrame {
    avatar: u64,
    decoration: Option<u64>,
    spec: Spec,
    image: Arc<RenderImage>,
}

type PaintedFrame = (Arc<RenderImage>, Option<Arc<RenderImage>>);

#[derive(Default)]
pub(super) struct Cache {
    decoded: HashMap<u64, Option<Arc<RgbaImage>>>,
    frames: HashMap<(Id, Size), CachedFrame>,
    expiry: Option<f64>,
    generation: u64,
}

impl Cache {
    fn decode(&mut self, source: &Image) -> Option<Arc<RgbaImage>> {
        self.decoded
            .entry(source.id())
            .or_insert_with(|| {
                image::load_from_memory(source.bytes())
                    .ok()
                    .map(|image| Arc::new(image.to_rgba8()))
            })
            .clone()
    }

    fn retain(&mut self, users: &HashSet<(Id, Size)>) -> Vec<Arc<RenderImage>> {
        let mut retired = Vec::new();
        self.frames.retain(|user, frame| {
            if users.contains(user) {
                true
            } else {
                retired.push(frame.image.clone());
                false
            }
        });
        let images: HashSet<_> = self
            .frames
            .values()
            .flat_map(|frame| std::iter::once(frame.avatar).chain(frame.decoration))
            .collect();
        self.decoded.retain(|id, _| images.contains(id));
        retired
    }

    fn frame(
        &mut self,
        user: Id,
        avatar: &Image,
        decoration: Option<&Image>,
        spec: Spec,
    ) -> Option<PaintedFrame> {
        let decoration_id = decoration.map(Image::id);
        if let Some(frame) = self.frames.get(&(user, spec.size))
            && frame.avatar == avatar.id()
            && frame.decoration == decoration_id
            && frame.spec == spec
        {
            return Some((frame.image.clone(), None));
        }
        let base = self.decode(avatar)?;
        let artwork = decoration.and_then(|source| self.decode(source));
        let mut buffer = compose(&base, artwork.as_deref(), spec);
        for pixel in buffer.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        let retired = self
            .frames
            .insert(
                (user, spec.size),
                CachedFrame {
                    avatar: avatar.id(),
                    decoration: decoration_id,
                    spec,
                    image: image.clone(),
                },
            )
            .map(|old| old.image);
        Some((image, retired))
    }
}

fn circle(x: f32, y: f32, center_x: f32, center_y: f32, radius: f32, feather: f32) -> f32 {
    ((radius - (x - center_x).hypot(y - center_y)) / feather + 0.5).clamp(0., 1.)
}

fn rounded_rect(
    x: f32,
    y: f32,
    [left, top, width, height]: [f32; 4],
    radius: f32,
    feather: f32,
) -> f32 {
    let dx = (x - left - width / 2.).abs() - (width / 2. - radius);
    let dy = (y - top - height / 2.).abs() - (height / 2. - radius);
    let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - radius;
    (0.5 - distance / feather).clamp(0., 1.)
}

fn status_cutout(x: f32, y: f32, spec: Spec, decoration: bool, feather: f32) -> f32 {
    let g = spec.size.geometry();
    if spec.status != Status::Mobile {
        return circle(x, y, g.center, g.center, g.hole, feather);
    }
    // 573435's avatar and decoration masks share the physical box but different corner radii.
    let stroke = g.hole - g.status / 2.;
    let radius = match spec.size {
        Size::Member => 4.2,
        Size::Profile => 7.2,
    } * if decoration { 1.2 } else { 1. };
    rounded_rect(
        x,
        y,
        [
            g.center - g.hole,
            g.center - g.status - stroke,
            g.status + 2. * stroke,
            g.status * 1.5 + 2. * stroke,
        ],
        radius,
        feather,
    )
}

fn status_mask(x: f32, y: f32, status: Status, feather: f32) -> f32 {
    // The mobile glyph is 10x15; the remaining masks use a 10x10 box.
    if status == Status::Mobile {
        let shell = rounded_rect(x, y, [0., 0., 10., 15.], 1.875, feather);
        let screen = rounded_rect(x, y, [1.25, 2.5, 7.5, 7.5], 0., feather);
        let button = circle(x, y, 5., 12.5, 1.25, feather);
        return shell * (1. - screen) * (1. - button);
    }
    let disk = circle(x, y, 5., 5., 5., feather);
    let hole = match status {
        Status::Online | Status::Mobile => 0.,
        Status::Offline => circle(x, y, 5., 5., 2.5, feather),
        Status::Idle => circle(x, y, 2.5, 2.5, 3.75, feather),
        Status::Dnd => {
            let closest_x = x.clamp(2.5, 7.5);
            circle(x, y, closest_x, 5., 1.25, feather)
        }
    };
    disk * (1. - hole)
}

fn over(base: &mut [f32; 4], pixel: Rgba<u8>, coverage: f32) {
    let alpha = pixel[3] as f32 / 255. * coverage;
    for channel in 0..3 {
        base[channel] = pixel[channel] as f32 / 255. * alpha + base[channel] * (1. - alpha);
    }
    base[3] = alpha + base[3] * (1. - alpha);
}

fn compose(avatar: &RgbaImage, decoration: Option<&RgbaImage>, spec: Spec) -> RgbaImage {
    let geometry = spec.size.geometry();
    let feather = geometry.art / spec.pixels as f32;
    RgbaImage::from_fn(spec.pixels, spec.pixels, |x, y| {
        let x = (x as f32 + 0.5) * feather;
        let y = (y as f32 + 0.5) * feather;
        let ax = x - geometry.inset;
        let ay = y - geometry.inset;
        let cutout = 1. - status_cutout(ax, ay, spec, false, feather);
        let mut output = [0.; 4];
        if let Some(pixel) =
            image::imageops::sample_bilinear(avatar, ax / geometry.base, ay / geometry.base)
        {
            over(
                &mut output,
                pixel,
                circle(
                    ax,
                    ay,
                    geometry.base / 2.,
                    geometry.base / 2.,
                    geometry.base / 2.,
                    feather,
                ) * cutout,
            );
        }
        if let Some(pixel) = decoration.and_then(|art| {
            image::imageops::sample_bilinear(art, x / geometry.art, y / geometry.art)
        }) {
            over(
                &mut output,
                pixel,
                1. - status_cutout(ax, ay, spec, true, feather),
            );
        }
        let origin = geometry.center - geometry.status / 2.;
        let origin_y = if spec.status == Status::Mobile {
            geometry.center - geometry.status
        } else {
            origin
        };
        let to_mask = 10. / geometry.status;
        over(
            &mut output,
            Rgba(spec.tint),
            status_mask(
                (ax - origin) * to_mask,
                (ay - origin_y) * to_mask,
                spec.status,
                feather * to_mask,
            ),
        );
        let alpha = output[3];
        if alpha > 0. {
            for channel in &mut output[..3] {
                *channel /= alpha;
            }
        }
        Rgba(output.map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8))
    })
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn mobile_member_target(
        &self,
        viewport: gpui::Size<gpui::Pixels>,
    ) -> Option<(gpui::Point<gpui::Pixels>, String)> {
        if !self.show_members || self.settings.is_some() || self.profile.is_some() {
            return None;
        }
        let guild = self.guild?;
        let anchors = self.tip_anchors.borrow();
        self.stores
            .member_lists
            .get(&guild)?
            .rows
            .iter()
            .find_map(|row| {
                let dc_state::member_list::Row::Member { user, .. } = row else {
                    return None;
                };
                let presence = self.stores.presence.get(user.id, Some(guild))?;
                if presence.status != "online" || !self.stores.presence.is_mobile_online(user.id) {
                    return None;
                }
                let key = gpui::SharedString::from(format!("member-{}", user.id));
                let bounds = anchors.get(&key)?;
                // The text-channel list starts below the captured 32px titlebar and 50px header.
                (bounds.origin.y >= px(82.)
                    && bounds.bottom() <= viewport.height
                    && bounds.origin.x >= px(0.)
                    && bounds.right() <= viewport.width)
                    .then(|| (bounds.center(), user.display_name().to_owned()))
            })
    }

    pub(super) fn equipped_decoration<'a>(
        &'a self,
        user: &'a User,
    ) -> Option<&'a AvatarDecoration> {
        let guild = self
            .guild
            .and_then(|guild| self.stores.members.get(guild, user.id));
        AvatarDecoration::equipped(
            user.avatar_decoration.as_deref(),
            guild.and_then(|member| member.avatar_decoration.as_ref()),
            chrono::Utc::now().timestamp_millis() as f64,
        )
    }

    pub(super) fn prepare_member_avatars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut members: Vec<_> = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild))
            .into_iter()
            .flat_map(|list| list.rows.iter())
            .filter_map(|row| match row {
                dc_state::member_list::Row::Member { user, .. } => {
                    Some((user.id, self.equipped_decoration(user).cloned()))
                }
                _ => None,
            })
            .collect();
        let mut users: HashSet<_> = members
            .iter()
            .map(|(user, _)| (*user, Size::Member))
            .collect();
        if let Some(profile) = &self.profile {
            users.insert((profile.user, Size::Profile));
            if let Some(user) = self.stores.users.get(&profile.user) {
                members.push((user.id, self.equipped_decoration(user).cloned()));
            }
        }
        for image in self.member_avatars.borrow_mut().retain(&users) {
            cx.drop_image(image, Some(window));
        }
        let expiry = members
            .iter()
            .filter_map(|(_, decoration)| decoration.as_ref()?.expires_at)
            .min_by(f64::total_cmp);
        for (_, decoration) in members {
            if let Some(decoration) = decoration {
                self.request_image(
                    format!("avatar-decoration:{}", decoration.sku_id),
                    decoration.static_url(),
                );
            }
        }
        let generation = {
            let mut cache = self.member_avatars.borrow_mut();
            if cache.expiry == expiry {
                return;
            }
            cache.expiry = expiry;
            cache.generation += 1;
            cache.generation
        };
        if let Some(expiry) = expiry {
            // 652215.mnr caps the source timer; +1 respects the strict expiry comparison.
            let delay = (expiry * 1000. - chrono::Utc::now().timestamp_millis() as f64 + 1.)
                .clamp(1., 2_147_483_647.) as u64;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(delay))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let mut cache = this.member_avatars.borrow_mut();
                    if cache.generation == generation {
                        cache.expiry = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    pub(super) fn release_member_avatars(&mut self, cx: &mut gpui::App) {
        let images = self.member_avatars.borrow_mut().retain(&HashSet::new());
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }

    pub(super) fn member_avatar(&self, user: &User, status: &str, probe: bool) -> gpui::Div {
        self.decorated_avatar(user, status, Size::Member, probe)
    }

    pub(super) fn profile_avatar(&self, user: &User, status: &str) -> gpui::Div {
        self.decorated_avatar(user, status, Size::Profile, false)
            .parity("profile-avatar")
    }

    fn decorated_avatar(&self, user: &User, status: &str, size: Size, probe: bool) -> gpui::Div {
        let avatar = self.images.get(&self.guild_avatar_source(user).0).cloned();
        let decoration = self
            .equipped_decoration(user)
            .and_then(|decoration| {
                self.images
                    .get(&format!("avatar-decoration:{}", decoration.sku_id))
            })
            .cloned();
        let cache = self.member_avatars.clone();
        let user = user.id;
        let status = Status::parse(status, self.stores.presence.is_mobile_online(user));
        let tint = color(status.token());
        let tint = [tint.r, tint.g, tint.b, tint.a].map(|value| (value * 255.).round() as u8);
        let geometry = size.geometry();
        div().relative().size(px(geometry.base)).flex_none().child(
            div()
                .when(probe && decoration.is_some(), |this| {
                    this.parity("member-avatar-decoration")
                })
                .absolute()
                .left(px(-geometry.inset))
                .top(px(-geometry.inset))
                .size(px(geometry.art))
                .child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, cx| {
                            let visible = bounds.intersect(&window.content_mask().bounds);
                            if visible.size.width <= px(0.) || visible.size.height <= px(0.) {
                                return;
                            }
                            let Some(avatar) = &avatar else {
                                return;
                            };
                            let spec = Spec {
                                size,
                                pixels: (geometry.art * window.scale_factor() * 2.).ceil() as u32,
                                status,
                                tint,
                            };
                            if let Some((image, retired)) =
                                cache
                                    .borrow_mut()
                                    .frame(user, avatar, decoration.as_deref(), spec)
                            {
                                if let Some(retired) = retired {
                                    cx.drop_image(retired, Some(window));
                                }
                                let _ = window.paint_image(
                                    bounds,
                                    bounds,
                                    Corners::all(px(0.)),
                                    image,
                                    0,
                                    false,
                                );
                            }
                        },
                    )
                    .size_full(),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_masks_preserve_screen_button_and_the_taller_transparent_ring() {
        let avatar = RgbaImage::from_pixel(32, 32, Rgba([255, 0, 0, 255]));
        let decoration = RgbaImage::from_pixel(32, 32, Rgba([0, 255, 0, 255]));
        assert!(Status::parse("online", true) == Status::Mobile);
        assert!(Status::parse("dnd", true) == Status::Dnd);
        for size in [Size::Member, Size::Profile] {
            let g = size.geometry();
            let spec = Spec {
                size,
                pixels: (g.art * 10.).round() as u32,
                status: Status::Mobile,
                tint: [0, 0, 255, 255],
            };
            let rendered = compose(&avatar, Some(&decoration), spec);
            let at = |x: f32, y: f32| {
                rendered.get_pixel(((x + g.inset) * 10.) as u32, ((y + g.inset) * 10.) as u32)
            };
            let left = g.center - g.status / 2.;
            let top = g.center - g.status;
            assert_eq!(at(left + g.status / 2., top + g.status * 0.6)[3], 0);
            assert_eq!(at(left + g.status / 2., top + g.status * 1.25)[3], 0);
            assert_eq!(
                at(left + g.status / 2., top + g.status * 0.125).0,
                [0, 0, 255, 255]
            );
            assert_eq!(at(left - 1., top + g.status / 2.)[3], 0);
        }
        let spec = Spec {
            size: Size::Member,
            pixels: 384,
            status: Status::Mobile,
            tint: [0, 0, 255, 255],
        };
        assert_eq!(status_cutout(20.4, 15.4, spec, false, 0.01), 1.);
        assert_eq!(status_cutout(20.4, 15.4, spec, true, 0.01), 0.);
    }

    #[test]
    fn avatar_and_decoration_leave_a_transparent_status_ring_and_glyph_holes() {
        let avatar = RgbaImage::from_pixel(32, 32, Rgba([255, 0, 0, 255]));
        let decoration = RgbaImage::from_pixel(32, 32, Rgba([0, 255, 0, 255]));
        let spec = Spec {
            size: Size::Member,
            pixels: 384,
            status: Status::Dnd,
            tint: [0, 0, 255, 255],
        };
        let rendered = compose(&avatar, Some(&decoration), spec);
        assert_eq!(rendered.get_pixel(302, 302)[3], 0); // DND bar.
        assert_eq!(rendered.get_pixel(302, 242)[3], 0); // Clear ring over the underlying row.
        assert_eq!(rendered.get_pixel(302, 262).0, [0, 0, 255, 255]);
        assert_eq!(rendered.get_pixel(1, 1).0, [0, 255, 0, 255]);
        let undecorated = compose(
            &avatar,
            None,
            Spec {
                status: Status::Online,
                ..spec
            },
        );
        assert_eq!(undecorated.get_pixel(1, 1)[3], 0);
        assert_eq!(undecorated.get_pixel(192, 192).0, [255, 0, 0, 255]);
        assert_eq!(status_mask(5., 5., Status::Offline, 0.1), 0.);
        assert_eq!(status_mask(2.5, 2.5, Status::Idle, 0.1), 0.);
    }

    #[test]
    fn changed_status_and_departed_members_return_their_gpu_textures() {
        let pixels = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let source = Image::from_bytes(gpui::ImageFormat::Png, bytes.into_inner());
        let mut cache = Cache::default();
        let spec = Spec {
            size: Size::Member,
            pixels: 77,
            status: Status::Online,
            tint: [1, 2, 3, 255],
        };
        let (first, retired) = cache.frame(1, &source, None, spec).unwrap();
        assert!(retired.is_none());
        assert!(Arc::ptr_eq(
            &first,
            &cache.frame(1, &source, None, spec).unwrap().0
        ));
        let (next, retired) = cache
            .frame(
                1,
                &source,
                None,
                Spec {
                    status: Status::Dnd,
                    ..spec
                },
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first, &retired.unwrap()));
        let (large, retired) = cache
            .frame(
                1,
                &source,
                None,
                Spec {
                    size: Size::Profile,
                    pixels: 192,
                    ..spec
                },
            )
            .unwrap();
        assert!(retired.is_none());
        assert!(!Arc::ptr_eq(&next, &large));
        let retired = cache.retain(&HashSet::from([(1, Size::Member)]));
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&large, &retired[0]));
        let retired = cache.retain(&HashSet::new());
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&next, &retired[0]));
        assert!(cache.frames.is_empty() && cache.decoded.is_empty());
    }

    #[test]
    fn profile_mask_uses_the_source_28px_hole_in_96px_artwork() {
        let source = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
        let image = compose(
            &source,
            Some(&source),
            Spec {
                size: Size::Profile,
                pixels: 960,
                status: Status::Online,
                tint: [0, 255, 0, 255],
            },
        );
        assert_eq!(image.get_pixel(760, 630)[3], 0); // 13px from center, inside the 14px cutout.
        assert_eq!(image.get_pixel(760, 760).0, [0, 255, 0, 255]);
        assert_eq!(image.get_pixel(760, 610).0, [255, 255, 255, 255]);
    }
}
