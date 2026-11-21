//! The overlapping server icons of module 692617 (`avatars_o4eL8l`), as module 926418 lays them out: at most three
//! 24px icons (`SMALLER`), each 5px under the next (`margin-inline-end: -5px`), every one but the last cut by the
//! next one's notch (`svg-mask-notched-squircle-24`); more than three servers end in a "+N" tile (`>99` past 99) on
//! `--background-base-lowest`, 8px wider when its label is three characters.
//!
//! The squircle and its notch are the masks of the official SVG (`maskContentUnits: objectBoundingBox`): the
//! squircle path, and in the notched mask the same path moved 19/24 to the right, stroked 4/24 wide, cut away. gpui
//! cannot mask a picture, so each icon is drawn as it is and a picture of the mask's outside, in the colour the stack
//! lies on, is laid over it. That picture is drawn once per size, notch and colour on a background thread.

use super::{Root, color};
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, img, prelude::*, px};
use std::sync::Arc;

/// The official squircle (`svg-mask-squircle`), in a 1x1 box.
const SQUIRCLE: &str = "M0 0.464C0 0.301585 0 0.220377 0.0316081 0.158343C0.0594114 0.103776 0.103776 0.0594114 0.158343 0.0316081C0.220377 0 0.301585 0 0.464 0H0.536C0.698415 0 0.779623 0 0.841657 0.0316081C0.896224 0.0594114 0.940589 0.103776 0.968392 0.158343C1 0.220377 1 0.301585 1 0.464V0.536C1 0.698415 1 0.779623 0.968392 0.841657C0.940589 0.896224 0.896224 0.940589 0.841657 0.968392C0.779623 1 0.698415 1 0.536 1H0.464C0.301585 1 0.220377 1 0.158343 0.968392C0.103776 0.940589 0.0594114 0.896224 0.0316081 0.841657C0 0.779623 0 0.698415 0 0.536V0.464Z";
/// The icon size of `SMALLER` (`d.iu` of module 682163).
const SIZE: f32 = 24.;
/// `.avatars_o4eL8l>li:not(:last-child).size24_o4eL8l{margin-inline-end:-5px}`.
const OVERLAP: f32 = 5.;
/// `maxGuilds` of module 926418.
const MAX: usize = 3;
/// The pictures are drawn at twice the icon size so they stay sharp on a 2x screen.
const SCALE: u32 = 2;

/// What the stack shows: the servers drawn and the count of the "+N" tile (0: none), as `m` of module 692617 splits
/// them.
pub(super) fn split(count: usize) -> (usize, usize) {
    if count > MAX {
        (MAX - 1, count - MAX + 1)
    } else {
        (count, 0)
    }
}

/// The "+N" label (`m` of `j`).
pub(super) fn overflow_label(count: usize) -> String {
    if count > 99 {
        ">99".to_owned()
    } else {
        format!("+{count}")
    }
}

/// The outside of the squircle (and, notched, the next icon's notch) filled with `under` (`0xRRGGBBAA`), as an SVG.
pub(super) fn mask_svg(notched: bool, under: u32) -> String {
    let pixels = SIZE as u32 * SCALE;
    let fill = format!("#{:06x}", under >> 8);
    let opacity = (under & 0xff) as f32 / 255.;
    let notch = if notched {
        format!(
            r#"<path fill="{fill}" stroke="{fill}" stroke-width="{width}" stroke-linejoin="round" transform="translate({shift} 0)" d="{SQUIRCLE}"/>"#,
            width = 4. / SIZE,
            shift = 19. / SIZE,
        )
    } else {
        String::new()
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pixels}" height="{pixels}" viewBox="0 0 1 1"><g opacity="{opacity}"><path fill="{fill}" fill-rule="evenodd" d="M-1 -1H2V2H-1Z {SQUIRCLE}"/>{notch}</g></svg>"#
    )
}

/// `0xRRGGBBAA` of a colour.
fn pack(colour: gpui::Rgba) -> u32 {
    let byte = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
    (byte(colour.r) << 24) | (byte(colour.g) << 16) | (byte(colour.b) << 8) | byte(colour.a)
}

fn rasterize(svg: &str) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let pixels = SIZE as u32 * SCALE;
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

impl Root {
    /// The mask picture for `notched` over `under`, drawn off the UI thread the first time it is asked for.
    fn guild_stack_mask(
        &self,
        notched: bool,
        under: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> Option<Arc<gpui::Image>> {
        let packed = pack(under);
        let key = format!("guild-stack-mask:{notched}:{packed:08x}");
        if let Some(found) = self.generated_images.borrow().get(&key) {
            return Some(found.clone());
        }
        if !self.guild_stack_pending.borrow_mut().insert(key.clone()) {
            return None;
        }
        let svg = mask_svg(notched, packed);
        cx.spawn(async move |this, cx| {
            let png = cx
                .background_executor()
                .spawn(async move { rasterize(&svg) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(png) = png {
                    this.generated_images.borrow_mut().insert(
                        key,
                        Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
        None
    }

    /// One 24px squircle tile with `content`, its mask laid over it.
    fn guild_stack_tile(
        &self,
        content: AnyElement,
        width: f32,
        notched: bool,
        under: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let mask = self.guild_stack_mask(notched, under, cx);
        div()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(SIZE))
            .overflow_hidden()
            .child(content)
            .children(mask.map(|mask| {
                // A tile wider than the icon (">99") keeps the squircle's corners at both ends.
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .child(img(mask.clone()).size(px(SIZE)))
                    .when(width > SIZE, |this| {
                        this.child(img(mask).size(px(SIZE)).ml(px(width - 2. * SIZE)))
                    })
            }))
    }

    /// Module 692617 for `guilds` (already in the order to show), lying on `under`.
    pub(super) fn guild_stack(
        &self,
        guilds: &[Id],
        under: gpui::Rgba,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if guilds.is_empty() {
            return None;
        }
        let (shown, more) = split(guilds.len());
        let mut tiles: Vec<gpui::Div> = Vec::new();
        for (index, guild) in guilds.iter().take(shown).enumerate() {
            let known = self.stores.guild(*guild);
            let picture = self.images.get(&format!("guild:{guild}")).cloned();
            let content = match picture {
                Some(picture) => img(picture).size(px(SIZE)).into_any_element(),
                None => {
                    let acronym = known.map(dc_state::Guild::acronym).unwrap_or_default();
                    // `R[SMALLER]` of module 682163: the acronym's size by its length.
                    let sizes = [13., 13., 11., 11., 9., 7., 5.];
                    let size = sizes[acronym.chars().count().min(sizes.len() - 1)];
                    div()
                        .size(px(SIZE))
                        .bg(color("--background-base-lower"))
                        .text_color(color("--text-default"))
                        .text_size(px(size))
                        .font_weight(FontWeight(500.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(acronym)
                        .into_any_element()
                }
            };
            let notched = more > 0 || index + 1 != shown;
            tiles.push(self.guild_stack_tile(content, SIZE, notched, under, cx));
        }
        if more > 0 {
            let label = overflow_label(more);
            let width = if label.chars().count() <= 2 {
                SIZE
            } else {
                SIZE + 8.
            };
            let content = div()
                .size_full()
                .bg(color("--background-base-lowest"))
                .text_color(color("--interactive-text-default"))
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .flex()
                .items_center()
                .justify_center()
                .child(label)
                .into_any_element();
            tiles.push(self.guild_stack_tile(content, width, false, under, cx));
        }
        let last = tiles.len().saturating_sub(1);
        Some(
            div()
                .flex()
                .items_center()
                .children(
                    tiles.into_iter().enumerate().map(|(index, tile)| {
                        tile.when(index != last, |this| this.mr(px(-OVERLAP)))
                    }),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_servers_show_and_more_end_in_a_count() {
        assert_eq!(split(1), (1, 0));
        assert_eq!(split(3), (3, 0));
        assert_eq!(split(4), (2, 2));
        assert_eq!(split(150), (2, 148));
        assert_eq!(overflow_label(2), "+2");
        assert_eq!(overflow_label(148), ">99");
    }

    #[test]
    fn the_mask_cuts_the_next_icons_notch_only_when_notched() {
        let plain = mask_svg(false, 0x11223344);
        assert!(plain.contains("#112233") && !plain.contains("translate"));
        let notched = mask_svg(true, 0x112233ff);
        assert!(notched.contains("translate(0.7916667 0)") || notched.contains("translate(0.7916"));
        assert!(rasterize(&notched).is_some());
    }
}
