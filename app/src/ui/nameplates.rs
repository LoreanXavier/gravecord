//! Member nameplate artwork, timed playback and masks: modules 214881/351952 and AFi54G styles.
use super::{
    Root,
    nameplate_animation::{self, Clip, Playback},
    theme,
};
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Corners, IntoElement, RenderImage, Window, canvas, div, prelude::*, px,
};
use image::{Rgba, RgbaImage};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

pub(super) const ANIMATION_PREFIX: &str = "nameplate-animation:";

#[derive(Clone, Copy, PartialEq, Eq)]
struct Spec {
    width: u32,
    height: u32,
    mask_start: u32,
    scale: u32,
    tint: Option<u32>,
    hovered: bool,
    selected: bool,
}

pub(super) struct Cache {
    epoch: Instant,
    decoded: HashMap<Id, Option<Arc<RgbaImage>>>,
    frames: HashMap<Id, CachedFrame>,
    encoded: HashMap<Id, Arc<Vec<u8>>>,
    animations: HashMap<Id, Option<Arc<Clip>>>,
    decoding: HashSet<Id>,
    clocks: HashMap<Id, (Id, Playback)>,
    frozen: HashMap<Id, (Id, usize, Arc<RgbaImage>)>,
    deadline: Option<Instant>,
    generation: u64,
    retired_frames: u64,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            decoded: HashMap::new(),
            frames: HashMap::new(),
            encoded: HashMap::new(),
            animations: HashMap::new(),
            decoding: HashSet::new(),
            clocks: HashMap::new(),
            frozen: HashMap::new(),
            deadline: None,
            generation: 0,
            retired_frames: 0,
        }
    }
}

struct CachedFrame {
    sku: Id,
    spec: Spec,
    index: Option<usize>,
    image: Arc<RenderImage>,
}
struct Frame {
    image: Arc<RenderImage>,
    retired: Option<Arc<RenderImage>>,
}
#[derive(Default)]
struct Preparation {
    decode: Vec<(Id, Arc<Vec<u8>>)>,
    fetch: Vec<Id>,
    retired: Vec<Arc<RenderImage>>,
    forgotten: Vec<Id>,
    deadline: Option<Instant>,
}

impl Cache {
    fn prepare(&mut self, active: &[(Id, Id, bool)], members: &[(Id, Id)]) -> Preparation {
        let active_users: HashSet<_> = active.iter().map(|(user, _, _)| *user).collect();
        let active_skus: HashSet<_> = active.iter().map(|(_, sku, _)| *sku).collect();
        let member_users: HashSet<_> = members.iter().map(|(user, _)| *user).collect();
        let member_skus: HashSet<_> = members.iter().map(|(_, sku)| *sku).collect();
        let now = self.epoch.elapsed();
        for (user, (sku, clock)) in &mut self.clocks {
            if !active_users.contains(user)
                && let Some(Some(clip)) = self.animations.get(sku)
            {
                clock.tick(now, false, false, clip.duration);
                if clock.started {
                    let index = clip.frame_at(clock.elapsed);
                    self.frozen
                        .insert(*user, (*sku, index, clip.frames[index].clone()));
                }
            }
        }
        self.animations.retain(|sku, _| active_skus.contains(sku));
        self.clocks.retain(|user, _| member_users.contains(user));
        self.frozen.retain(|user, _| member_users.contains(user));
        self.decoded.retain(|sku, _| member_skus.contains(sku));
        let mut preparation = Preparation::default();
        for (user, sku, hovered) in active {
            if let Some(Some(clip)) = self.animations.get(sku) {
                let (known, clock) = self
                    .clocks
                    .entry(*user)
                    .or_insert_with(|| (*sku, Playback::default()));
                if *known != *sku {
                    *known = *sku;
                    *clock = Playback::default();
                }
                clock.tick(now, true, *hovered, clip.duration);
                let index = clip.frame_at(clock.elapsed);
                self.frozen
                    .insert(*user, (*sku, index, clip.frames[index].clone()));
                if clock.playing {
                    let deadline = self.epoch + now + clip.next_delay(clock.elapsed);
                    preparation.deadline = Some(
                        preparation
                            .deadline
                            .map_or(deadline, |old| old.min(deadline)),
                    );
                }
            }
        }
        self.frames.retain(|user, frame| {
            if member_users.contains(user) {
                true
            } else {
                preparation.retired.push(frame.image.clone());
                false
            }
        });
        self.retired_frames += preparation.retired.len() as u64;
        self.encoded.retain(|sku, _| {
            if member_skus.contains(sku) || self.decoding.contains(sku) {
                true
            } else {
                preparation.forgotten.push(*sku);
                false
            }
        });
        for sku in active_skus {
            if self.animations.contains_key(&sku) || self.decoding.contains(&sku) {
                continue;
            }
            if let Some(bytes) = self.encoded.get(&sku) {
                self.decoding.insert(sku);
                preparation.decode.push((sku, bytes.clone()));
            } else {
                preparation.fetch.push(sku);
            }
        }
        preparation
    }

    fn frame(&mut self, user: Id, sku: Id, encoded: &[u8], spec: Spec) -> Option<Frame> {
        let frozen = self.frozen.get(&user).filter(|(known, _, _)| *known == sku);
        let index = frozen.map(|(_, index, _)| *index);
        if let Some(known) = self.frames.get(&user)
            && known.sku == sku
            && known.spec == spec
            && known.index == index
        {
            return Some(Frame {
                image: known.image.clone(),
                retired: None,
            });
        }
        let source = if let Some((_, _, source)) = frozen {
            source.clone()
        } else {
            self.decoded
                .entry(sku)
                .or_insert_with(|| {
                    image::load_from_memory(encoded)
                        .ok()
                        .map(|image| Arc::new(image.to_rgba8()))
                })
                .as_ref()?
                .clone()
        };
        let mut buffer = compose(&source, spec);
        for pixel in buffer.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        let frame = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        // Replace the previous geometry/state for this user instead of accumulating every hover frame.
        let retired = self
            .frames
            .insert(
                user,
                CachedFrame {
                    sku,
                    spec,
                    index,
                    image: frame.clone(),
                },
            )
            .map(|old| old.image);
        self.retired_frames += u64::from(retired.is_some());
        Some(Frame {
            image: frame,
            retired,
        })
    }
}

fn gradient_alpha(position: f32, hovered: bool, selected: bool) -> f32 {
    let middle = 20. / 255.;
    let end = if selected {
        128. / 255.
    } else if hovered {
        77. / 255.
    } else {
        51. / 255.
    };
    if position < 0.2 {
        middle * position / 0.2
    } else if position < 0.5 {
        middle
    } else {
        middle + (end - middle) * (position - 0.5) / 0.5
    }
}

fn compose(source: &RgbaImage, spec: Spec) -> RgbaImage {
    let picture_width = ((source.width() as f32 * spec.height as f32
        / source.height().max(1) as f32)
        .round() as u32)
        .max(1);
    let picture = image::imageops::resize(
        source,
        picture_width,
        spec.height,
        image::imageops::FilterType::Triangle,
    );
    let left = spec.width as i64 - picture_width as i64;
    let start = f32::from_bits(spec.mask_start);
    let transition = 50. * f32::from_bits(spec.scale);
    let tint = spec.tint.unwrap_or(0);
    let tint_rgb = [(tint >> 16) & 255, (tint >> 8) & 255, tint & 255];
    RgbaImage::from_fn(spec.width, spec.height, |x, y| {
        let source_x = x as i64 - left;
        let pixel = if source_x >= 0 && source_x < picture_width as i64 {
            *picture.get_pixel(source_x as u32, y)
        } else {
            Rgba([0; 4])
        };
        let mask = if spec.tint.is_some() {
            0.3 + 0.7 * ((x as f32 + 0.5 - start) / transition).clamp(0., 1.)
        } else {
            1.
        };
        let image_alpha = pixel[3] as f32 / 255. * mask * if spec.selected { 0.9 } else { 0.6 };
        let backdrop = if spec.tint.is_some() {
            gradient_alpha(
                (x as f32 + 0.5) / spec.width as f32,
                spec.hovered,
                spec.selected,
            )
        } else {
            0.
        };
        let alpha = image_alpha + backdrop * (1. - image_alpha);
        if alpha == 0. {
            return Rgba([0; 4]);
        }
        let mut output = [0; 4];
        for channel in 0..3 {
            output[channel] = ((pixel[channel] as f32 * image_alpha
                + tint_rgb[channel] as f32 * backdrop * (1. - image_alpha))
                / alpha)
                .round() as u8;
        }
        output[3] = (alpha * 255.).round() as u8;
        Rgba(output)
    })
}

impl Root {
    pub(super) fn release_nameplates(&mut self, cx: &mut gpui::App) {
        let images = self.nameplates.borrow_mut().prepare(&[], &[]).retired;
        // The active window is back in App's window map when deferred effects run.
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }

    pub(super) fn nameplate_hover(&mut self, user: Id, inside: bool, cx: &mut Context<Self>) {
        let old = self.hovered_nameplate;
        if inside {
            self.hovered_nameplate = Some(user);
        } else if old == Some(user) {
            self.hovered_nameplate = None;
        }
        if old != self.hovered_nameplate {
            cx.notify();
        }
    }

    pub(super) fn nameplate_animation_fetched(
        &mut self,
        key: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        if let Some(sku) = key
            .strip_prefix(ANIMATION_PREFIX)
            .and_then(|id| id.parse().ok())
        {
            self.nameplates
                .borrow_mut()
                .encoded
                .insert(sku, Arc::new(bytes));
            cx.notify();
        }
    }

    pub(super) fn prepare_nameplate_animations(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let members: Vec<_> = self
            .guild
            .and_then(|guild| self.stores.member_lists.get(&guild))
            .into_iter()
            .flat_map(|list| list.rows.iter())
            .filter_map(|row| match row {
                dc_state::member_list::Row::Member { user, .. } => {
                    user.nameplate.as_ref().map(|plate| (user.id, plate.sku_id))
                }
                _ => None,
            })
            .collect();
        let selected = self.profile.as_ref().map(|profile| profile.user);
        let can_animate = window.is_window_active() && !self.pref_on("reduced_motion", false);
        let active: Vec<_> = members
            .iter()
            .filter_map(|(user, sku)| {
                let hovered = self.hovered_nameplate == Some(*user);
                (can_animate && (hovered || selected == Some(*user)))
                    .then_some((*user, *sku, hovered))
            })
            .collect();
        let preparation = self.nameplates.borrow_mut().prepare(&active, &members);
        for image in preparation.retired {
            cx.drop_image(image, Some(window));
        }
        for sku in preparation.forgotten {
            self.requested.remove(&format!("{ANIMATION_PREFIX}{sku}"));
        }
        for sku in preparation.fetch {
            self.request_image(
                format!("{ANIMATION_PREFIX}{sku}"),
                format!("https://cdn.discordapp.com/media/v1/collectibles-shop/{sku}/animated"),
            );
        }
        for (sku, bytes) in preparation.decode {
            cx.spawn(async move |this, cx| {
                let clip = cx
                    .background_executor()
                    .spawn(async move { nameplate_animation::decode(&bytes) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let mut cache = this.nameplates.borrow_mut();
                    cache.decoding.remove(&sku);
                    cache.animations.insert(sku, clip);
                    cx.notify();
                });
            })
            .detach();
        }
        let scheduled = {
            let mut cache = self.nameplates.borrow_mut();
            match preparation.deadline {
                Some(deadline) if cache.deadline.is_none_or(|old| deadline < old) => {
                    cache.generation += 1;
                    cache.deadline = Some(deadline);
                    Some((cache.generation, deadline))
                }
                None if cache.deadline.is_some() => {
                    cache.generation += 1;
                    cache.deadline = None;
                    None
                }
                _ => None,
            }
        };
        if let Some((generation, deadline)) = scheduled {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(deadline.saturating_duration_since(Instant::now()))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let mut cache = this.nameplates.borrow_mut();
                    if cache.generation == generation {
                        cache.deadline = None;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    #[cfg(feature = "capture")]
    pub(super) fn nameplate_status(&self) -> String {
        let cache = self.nameplates.borrow();
        let indices: Vec<_> = cache.frozen.values().map(|(_, index, _)| *index).collect();
        format!(
            "decoded={} playing={} frames={indices:?} retired_textures={}",
            cache
                .animations
                .values()
                .filter(|clip| clip.is_some())
                .count(),
            cache
                .clocks
                .values()
                .filter(|(_, clock)| clock.playing)
                .count(),
            cache.retired_frames
        )
    }

    pub(super) fn member_nameplate(
        &self,
        user: &User,
        text_width: Rc<Cell<f32>>,
        probe: bool,
    ) -> Option<AnyElement> {
        let plate = user.nameplate.as_ref()?;
        let image = self
            .images
            .get(&format!("nameplate:{}", plate.sku_id))?
            .clone();
        let sku = plate.sku_id;
        let user_id = user.id;
        let selected = self
            .profile
            .as_ref()
            .is_some_and(|profile| profile.user == user_id);
        let tint = plate.background(theme() == dc_tokens::Theme::Light);
        let hovered = self.hovered_nameplate == Some(user_id);
        let cache = self.nameplates.clone();
        Some(
            div()
                .when(probe, |this| this.parity("member-nameplate"))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, cx| {
                            let visible = bounds.intersect(&window.content_mask().bounds);
                            if visible.size.width <= px(0.) || visible.size.height <= px(0.) {
                                return;
                            }
                            let scale = window.scale_factor();
                            let width = (f32::from(bounds.size.width) * scale).round() as u32;
                            let height = (f32::from(bounds.size.height) * scale).round() as u32;
                            if width == 0 || height == 0 {
                                return;
                            }
                            let spec = Spec {
                                width,
                                height,
                                // Member content includes a 32px avatar, 12px gap, then the measured text; source adds 10px.
                                mask_start: ((text_width.get() + 32. + 12. + 10.) * scale)
                                    .to_bits(),
                                scale: scale.to_bits(),
                                tint,
                                hovered,
                                selected,
                            };
                            if let Some(frame) =
                                cache.borrow_mut().frame(user_id, sku, image.bytes(), spec)
                            {
                                if let Some(retired) = frame.retired {
                                    cx.drop_image(retired, Some(window));
                                }
                                let _ = window.paint_image(
                                    bounds,
                                    bounds,
                                    Corners::all(px(8.)),
                                    frame.image,
                                    0,
                                    false,
                                );
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_clips_are_discarded_and_departed_members_release_cached_assets() {
        let mut cache = Cache::default();
        for sku in [10, 20, 30] {
            cache.animations.insert(sku, None);
            cache.encoded.insert(sku, Arc::new(vec![0]));
        }
        let pixel = Arc::new(RgbaImage::new(1, 1));
        cache.frozen.insert(2, (20, 3, pixel.clone()));
        cache.frozen.insert(3, (30, 4, pixel));
        let cleanup = cache.prepare(&[(1, 10, true)], &[(1, 10), (2, 20)]);
        assert_eq!(cache.animations.len(), 1);
        assert!(cache.animations.contains_key(&10));
        assert!(cache.frozen.contains_key(&2));
        assert!(!cache.frozen.contains_key(&3));
        assert_eq!(cleanup.forgotten, [30]);
        assert_eq!(cache.encoded.len(), 2);
    }

    #[test]
    fn replaced_and_removed_posters_return_their_gpu_images_for_release() {
        let source = RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        source
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let spec = Spec {
            width: 40,
            height: 20,
            mask_start: 0f32.to_bits(),
            scale: 1f32.to_bits(),
            tint: None,
            hovered: false,
            selected: false,
        };
        let mut cache = Cache::default();
        let first = cache.frame(1, 2, encoded.get_ref(), spec).unwrap();
        assert!(first.retired.is_none());
        let same = cache.frame(1, 2, encoded.get_ref(), spec).unwrap();
        assert!(same.retired.is_none());
        assert!(Arc::ptr_eq(&first.image, &same.image));
        let changed = cache
            .frame(
                1,
                2,
                encoded.get_ref(),
                Spec {
                    selected: true,
                    ..spec
                },
            )
            .unwrap();
        assert!(Arc::ptr_eq(&first.image, changed.retired.as_ref().unwrap()));
        let cleanup = cache.prepare(&[], &[]);
        assert_eq!(cleanup.retired.len(), 1);
        assert!(Arc::ptr_eq(&changed.image, &cleanup.retired[0]));
        assert_eq!(cache.retired_frames, 2);
        assert!(cache.frames.is_empty());
        assert!(cache.decoded.is_empty());
    }

    #[test]
    fn member_mask_and_state_opacities_follow_source() {
        assert_eq!(gradient_alpha(0.2, false, false), 20. / 255.);
        assert_eq!(gradient_alpha(1., false, false), 51. / 255.);
        assert_eq!(gradient_alpha(1., true, false), 77. / 255.);
        assert_eq!(gradient_alpha(1., true, true), 128. / 255.);
        let source = RgbaImage::from_pixel(200, 20, Rgba([255, 255, 255, 255]));
        let spec = Spec {
            width: 200,
            height: 20,
            mask_start: 50f32.to_bits(),
            scale: 1f32.to_bits(),
            tint: Some(0),
            hovered: false,
            selected: false,
        };
        let image = compose(&source, spec);
        assert!(image.get_pixel(20, 10)[3] < image.get_pixel(120, 10)[3]);
        let selected = compose(
            &source,
            Spec {
                selected: true,
                ..spec
            },
        );
        assert!(selected.get_pixel(120, 10)[3] > image.get_pixel(120, 10)[3]);
    }

    #[test]
    fn posters_fit_the_height_and_align_to_the_right() {
        let source = RgbaImage::from_pixel(20, 20, Rgba([255, 0, 0, 255]));
        let spec = Spec {
            width: 40,
            height: 20,
            mask_start: 0f32.to_bits(),
            scale: 1f32.to_bits(),
            tint: None,
            hovered: false,
            selected: false,
        };
        let image = compose(&source, spec);
        assert_eq!(image.get_pixel(19, 10).0, [0; 4]);
        assert_eq!(image.get_pixel(20, 10).0, [255, 0, 0, 153]);
        assert_eq!(image.get_pixel(39, 10).0, [255, 0, 0, 153]);
    }
}
