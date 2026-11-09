//! Member nameplate posters and masks: modules 214881/351952 and AFi54G styles.
use super::{Root, theme};
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{AnyElement, Corners, IntoElement, RenderImage, canvas, div, prelude::*, px};
use image::{Rgba, RgbaImage};
use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc};

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

#[derive(Default)]
pub(super) struct Cache {
    decoded: HashMap<Id, Option<RgbaImage>>,
    frames: HashMap<Id, (Id, Spec, Arc<RenderImage>)>,
}

impl Cache {
    fn frame(&mut self, user: Id, sku: Id, encoded: &[u8], spec: Spec) -> Option<Arc<RenderImage>> {
        if let Some((known_sku, known_spec, frame)) = self.frames.get(&user)
            && *known_sku == sku
            && *known_spec == spec
        {
            return Some(frame.clone());
        }
        let source = self
            .decoded
            .entry(sku)
            .or_insert_with(|| {
                image::load_from_memory(encoded)
                    .ok()
                    .map(|image| image.to_rgba8())
            })
            .as_ref()?;
        let mut buffer = compose(source, spec);
        for pixel in buffer.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        let frame = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        // Replace the previous geometry/state for this user instead of accumulating every hover frame.
        self.frames.insert(user, (sku, spec, frame.clone()));
        Some(frame)
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
                        move |bounds, _, window, _| {
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
                                hovered: bounds.contains(&window.mouse_position()),
                                selected,
                            };
                            if let Some(frame) =
                                cache.borrow_mut().frame(user_id, sku, image.bytes(), spec)
                            {
                                let _ = window.paint_image(
                                    bounds,
                                    bounds,
                                    Corners::all(px(8.)),
                                    frame,
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
