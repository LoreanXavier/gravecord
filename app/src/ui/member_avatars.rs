//! Member avatars and collectible artwork with the source's transparent SVG masks (97808).
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

const ART_SIZE: f32 = 38.4;
const ART_OFFSET: f32 = 3.2;

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Online,
    Idle,
    Dnd,
    Offline,
}

impl Status {
    fn parse(value: &str) -> Self {
        match value {
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            "offline" | "invisible" => Self::Offline,
            _ => Self::Online,
        }
    }
    fn token(self) -> &'static str {
        match self {
            Self::Online => "--icon-status-online",
            Self::Idle => "--icon-status-idle",
            Self::Dnd => "--icon-status-dnd",
            Self::Offline => "--icon-status-offline",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Spec {
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
    frames: HashMap<Id, CachedFrame>,
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

    fn retain(&mut self, users: &HashSet<Id>) -> Vec<Arc<RenderImage>> {
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
        if let Some(frame) = self.frames.get(&user)
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
                user,
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

fn status_mask(x: f32, y: f32, status: Status, feather: f32) -> f32 {
    // Coordinates within the 10px status glyph. SVG masks are normalized to its box.
    let disk = circle(x, y, 5., 5., 5., feather);
    let hole = match status {
        Status::Online => 0.,
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
    let feather = ART_SIZE / spec.pixels as f32;
    RgbaImage::from_fn(spec.pixels, spec.pixels, |x, y| {
        let x = (x as f32 + 0.5) * feather;
        let y = (y as f32 + 0.5) * feather;
        let ax = x - ART_OFFSET;
        let ay = y - ART_OFFSET;
        let cutout = 1. - circle(ax, ay, 27., 27., 8., feather);
        let mut output = [0.; 4];
        if let Some(pixel) = image::imageops::sample_bilinear(avatar, ax / 32., ay / 32.) {
            over(
                &mut output,
                pixel,
                circle(ax, ay, 16., 16., 16., feather) * cutout,
            );
        }
        if let Some(pixel) = decoration
            .and_then(|art| image::imageops::sample_bilinear(art, x / ART_SIZE, y / ART_SIZE))
        {
            over(&mut output, pixel, cutout);
        }
        over(
            &mut output,
            Rgba(spec.tint),
            status_mask(ax - 22., ay - 22., spec.status, feather),
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
        let members: Vec<_> = self
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
        let users = members.iter().map(|(user, _)| *user).collect();
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
        let status = Status::parse(status);
        let tint = color(status.token());
        let tint = [tint.r, tint.g, tint.b, tint.a].map(|value| (value * 255.).round() as u8);
        div().relative().size(px(32.)).flex_none().child(
            div()
                .absolute()
                .left(px(-ART_OFFSET))
                .top(px(-ART_OFFSET))
                .size(px(ART_SIZE))
                .when(probe && decoration.is_some(), |this| {
                    this.parity("member-avatar-decoration")
                })
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
                                pixels: (ART_SIZE * window.scale_factor() * 2.).ceil() as u32,
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
    fn avatar_and_decoration_leave_a_transparent_status_ring_and_glyph_holes() {
        let avatar = RgbaImage::from_pixel(32, 32, Rgba([255, 0, 0, 255]));
        let decoration = RgbaImage::from_pixel(32, 32, Rgba([0, 255, 0, 255]));
        let spec = Spec {
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
        let retired = cache.retain(&HashSet::new());
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&next, &retired[0]));
        assert!(cache.frames.is_empty() && cache.decoded.is_empty());
    }
}
