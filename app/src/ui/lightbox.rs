//! The media viewer (`MediaViewerModal`, module 2323): a picture from an attachment or an embed over a 0.92 black
//! scrim, with the sender and time at the top left, the zoom / forward / save / open pill and a close button at the top
//! right, and, with several pictures in the message, previous and next. Measured in
//! `design/dom/modals/lightbox-darker-cozy.json` and `lightbox-zoomed-darker-cozy.json`; the viewer carries
//! `theme-midnight` whatever the app's theme is.

use super::embed::image_key;
use super::{Root, avatar_source, icon};
use crate::net::Request;
use dc_state::{Attachment, Message};
use dc_tokens::Theme;
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, KeyDownEvent, MouseButton, Pixels,
    Point, SharedString, Window, deferred, div, img, prelude::*, px, rgba,
};

/// `ZOOM`: `setZoomed(true)` scales to this.
const ZOOM: f32 = 2.5;
pub(super) const SAVED_KEY: &str = "lightbox-saved";

fn midnight(name: &str) -> gpui::Rgba {
    rgba(Theme::Midnight.color(name))
}

/// One picture of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Item {
    /// The key of the picture in `Root::images` (the one the message shows, until the full one arrives).
    pub key: String,
    /// The original file: what Open in Browser and Save use.
    pub url: String,
    /// The media proxy address the viewer fetches at its own size, with the query started.
    pub proxy: String,
    pub width: u32,
    pub height: u32,
    pub filename: String,
}

/// The state of the open viewer.
pub(super) struct Lightbox {
    pub message: Message,
    pub items: Vec<Item>,
    pub index: usize,
    pub zoomed: bool,
    /// Where the zoomed picture is dragged to, from the middle.
    pub offset: Point<Pixels>,
    /// The pointer position and offset at the press of a drag.
    pub drag: Option<(Point<Pixels>, Point<Pixels>)>,
    pub saving: bool,
    /// The More menu (`image-menu` in module 2323) is open.
    pub more: bool,
}

/// The message's pictures in the order the viewer pages through them: attachments, then embed pictures.
pub(super) fn items_of(message: &Message) -> Vec<Item> {
    let mut items: Vec<Item> = message
        .attachments
        .iter()
        .filter(|attachment| attachment.is_image())
        .map(attachment_item)
        .collect();
    for (index, embed) in message.embeds.iter().enumerate() {
        if let Some(image) = &embed.image {
            let (key, base) = (
                image_key(message.id, index, "image"),
                image.proxy_url.clone().unwrap_or_else(|| image.url.clone()),
            );
            items.push(Item {
                key,
                url: image.url.clone(),
                proxy: format!("{base}{}", if base.contains('?') { "&" } else { "?" }),
                width: image.width,
                height: image.height,
                filename: filename_of(&image.url),
            });
        }
    }
    items
}

fn attachment_item(attachment: &Attachment) -> Item {
    Item {
        key: format!("attachment:{}", attachment.id),
        url: attachment.url.clone(),
        proxy: attachment.proxy_url.clone(),
        width: attachment.width.unwrap_or(1),
        height: attachment.height.unwrap_or(1),
        filename: attachment.filename.clone(),
    }
}

fn filename_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        "image".to_owned()
    } else {
        name.to_owned()
    }
}

/// The index after stepping `delta` places in a list of `len`, wrapping at both ends as the carousel does.
pub(super) fn step(index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (index as i64 + i64::from(delta)).rem_euclid(len as i64) as usize
}

/// `Uj` in module 515718: scaled down to the width, then to the height, never up.
pub(super) fn fit(width: u32, height: u32, max_width: i64, max_height: i64) -> (u32, u32) {
    let (mut w, mut h) = (f64::from(width), f64::from(height));
    if width as i64 != max_width || height as i64 != max_height {
        let by_width = if w > max_width as f64 {
            max_width as f64 / w
        } else {
            1.
        };
        w = (w * by_width).round().max(0.);
        h = (h * by_width).round().max(0.);
        let by_height = if h > max_height as f64 {
            max_height as f64 / h
        } else {
            1.
        };
        w = (w * by_height).round().max(0.);
        h = (h * by_height).round().max(0.);
    }
    (w as u32, h as u32)
}

/// Module 967758: the room a picture gets in a window of `viewport`, with or without the previous/next columns.
pub(super) fn stage(many: bool, width: u32, height: u32, viewport: (f32, f32)) -> (u32, u32) {
    let (vw, vh) = (viewport.0 as i64, viewport.1 as i64);
    let side = if many { 76 } else { 24 };
    let wide = fit(width, height, vw - side * 2, vh - 176);
    let tall = fit(width, height, vw - 544, vh - if many { 88 } else { 36 } * 2);
    if wide.0 >= tall.0 { wide } else { tall }
}

/// The viewer's timestamp: the time alone for today's message, `M/D/YYYY h:mm A` for an older one (both measured
/// in the lightbox goldens).
fn stamp(message: &Message) -> String {
    let local = message.local_time();
    let clock = super::settings::prefs::clock(&local);
    if local.date_naive() == chrono::Local::now().date_naive() {
        clock
    } else {
        format!("{} {clock}", local.format("%-m/%-d/%Y"))
    }
}

impl Root {
    /// A click on a picture of `message`: opens the viewer on the one with `key`.
    pub(super) fn open_lightbox(&mut self, message: &Message, key: &str, cx: &mut Context<Self>) {
        let items = items_of(message);
        let Some(index) = items.iter().position(|item| item.key == key) else {
            return;
        };
        self.lightbox = Some(Lightbox {
            message: message.clone(),
            items,
            index,
            zoomed: false,
            offset: Point::default(),
            drag: None,
            saving: false,
            more: false,
        });
        self.lightbox_images();
        cx.notify();
    }

    /// The pictures next to the open one load at the viewer's size; the open one first.
    fn lightbox_images(&mut self) {
        let Some(open) = self.lightbox.as_ref() else {
            return;
        };
        let viewport = self.viewport;
        let many = open.items.len() > 1;
        let wanted: Vec<(String, String)> = [0i32, 1, -1]
            .iter()
            .filter(|delta| {
                **delta == 0 || open.items.len() > 2 || (open.items.len() == 2 && **delta == 1)
            })
            .map(|delta| {
                let item = &open.items[step(open.index, open.items.len(), *delta)];
                let (width, height) = stage(many, item.width, item.height, viewport);
                (
                    format!("lightbox:{}", item.key),
                    format!(
                        "{}format=webp&quality=lossless&width={width}&height={height}",
                        item.proxy
                    ),
                )
            })
            .collect();
        for (key, url) in wanted {
            self.request_image(key, url);
        }
    }

    pub(super) fn lightbox_step(&mut self, delta: i32, cx: &mut Context<Self>) {
        if let Some(open) = self.lightbox.as_mut() {
            open.index = step(open.index, open.items.len(), delta);
            open.zoomed = false;
            open.offset = Point::default();
            open.drag = None;
        }
        self.lightbox_images();
        cx.notify();
    }

    fn lightbox_zoom(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(open) = self.lightbox.as_mut() {
            open.zoomed = on;
            open.offset = Point::default();
            open.drag = None;
        }
        cx.notify();
    }

    fn lightbox_save(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.lightbox.as_mut() else {
            return;
        };
        let item = open.items[open.index].clone();
        open.saving = true;
        let _ = self.requests.send(Request::Download {
            url: item.url,
            filename: item.filename,
            done: Some(SAVED_KEY.to_owned()),
        });
        cx.notify();
    }

    /// The answer to Save Image: the "Image Saved" toast, or the critical one.
    pub(super) fn lightbox_saved(&mut self, body: &serde_json::Value, cx: &mut Context<Self>) {
        if let Some(open) = self.lightbox.as_mut() {
            open.saving = false;
        }
        if body["ok"].as_bool() == Some(true) {
            self.show_success_toast("Image Saved".to_owned(), cx);
        } else {
            self.put_app_toast("Unable to save image".to_owned(), false, cx);
        }
    }

    /// Escape closes, the arrows page. True when the key was the viewer's.
    pub(super) fn lightbox_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.lightbox.is_none() {
            return false;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.lightbox = None;
                cx.notify();
            }
            "left" => self.lightbox_step(-1, cx),
            "right" => self.lightbox_step(1, cx),
            _ => return false,
        }
        true
    }

    fn lightbox_button(
        &self,
        id: &'static str,
        name: &'static str,
        on: impl Fn(&mut Root, &mut Window, &mut Context<Root>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .size(px(32.))
            .flex_none()
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(midnight("--interactive-background-hover")))
            .child(icon(name, 16., "--icon-subtle").into_any_element())
            .on_click(cx.listener(move |this, _, window, cx| {
                on(this, window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn lightbox_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.lightbox.as_ref()?;
        let item = &open.items[open.index];
        let many = open.items.len() > 1;
        let (width, height) = stage(many, item.width, item.height, self.viewport);
        let scale = if open.zoomed { ZOOM } else { 1. };
        let picture = self
            .images
            .get(&format!("lightbox:{}", item.key))
            .or_else(|| self.images.get(&item.key))
            .cloned();
        let avatar = self
            .images
            .get(&avatar_source(&open.message.author).0)
            .cloned();
        let panel = midnight("--background-surface-highest");
        let zoomed = open.zoomed;
        let (offset_x, offset_y) = (open.offset.x, open.offset.y);

        let bar = div()
            .absolute()
            .top(px(36.))
            .left(px(24.))
            .h(px(40.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .size(px(40.))
                    .flex_none()
                    .rounded_full()
                    .overflow_hidden()
                    .children(avatar.map(|picture| img(picture).size(px(40.)).rounded_full())),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(22.))
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(FontWeight(500.))
                            .text_color(midnight("--text-strong"))
                            .child(open.message.author.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .h(px(16.))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(500.))
                            .text_color(midnight("--text-muted"))
                            .child(stamp(&open.message)),
                    ),
            );

        let forward_message = open.message.clone();
        let open_url = item.url.clone();
        let pill = div()
            .absolute()
            .top(px(36.))
            .right(px(76.))
            .h(px(40.))
            .p(px(3.))
            .gap(px(4.))
            .flex()
            .items_center()
            .rounded(px(12.))
            .border_1()
            .border_color(midnight("--border-subtle"))
            .bg(panel)
            .child(self.lightbox_button(
                "lightbox-zoom",
                if zoomed { "lb_zoom_out" } else { "lb_zoom_in" },
                move |this, _, cx| this.lightbox_zoom(!zoomed, cx),
                cx,
            ))
            .child(self.lightbox_button(
                "lightbox-forward",
                "lb_forward",
                move |this, window, cx| {
                    this.lightbox = None;
                    this.open_forward(forward_message.clone(), window, cx);
                },
                cx,
            ))
            .child(self.lightbox_button(
                "lightbox-save",
                "lb_download",
                |this, _, cx| this.lightbox_save(cx),
                cx,
            ))
            .child(self.lightbox_button(
                "lightbox-open",
                "lb_open",
                move |_, _, cx| cx.open_url(&open_url),
                cx,
            ))
            .child(self.lightbox_button(
                "lightbox-more",
                "lb_more",
                |this, _, cx| {
                    if let Some(open) = this.lightbox.as_mut() {
                        open.more = !open.more;
                    }
                    cx.notify();
                },
                cx,
            ));

        let close = div()
            .id("lightbox-close")
            .absolute()
            .top(px(36.))
            .right(px(24.))
            .size(px(40.))
            .rounded(px(12.))
            .bg(panel)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon("lb_close", 20., "--icon-default"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.lightbox = None;
                cx.notify();
                cx.stop_propagation();
            }));

        let stage = div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .child(
                div()
                    .id("lightbox-picture")
                    .w(px(width as f32 * scale))
                    .h(px(height as f32 * scale))
                    .flex_none()
                    .relative()
                    .left(offset_x)
                    .top(offset_y)
                    .cursor_pointer()
                    .children(picture.map(|picture| {
                        img(picture)
                            .w(px(width as f32 * scale))
                            .h(px(height as f32 * scale))
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            if let Some(open) = this.lightbox.as_mut() {
                                open.drag = Some((event.position, open.offset));
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        if let Some(open) = this.lightbox.as_mut()
                            && open.zoomed
                            && let Some((from, start)) = open.drag
                        {
                            open.offset = start + (event.position - from);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                            // A press that moved is a drag; one that did not toggles the zoom.
                            let moved = this.lightbox.as_ref().is_some_and(|open| {
                                open.drag.is_some_and(|(from, _)| {
                                    let delta = event.position - from;
                                    f32::from(delta.x).abs() > 3. || f32::from(delta.y).abs() > 3.
                                })
                            });
                            if let Some(open) = this.lightbox.as_mut() {
                                open.drag = None;
                            }
                            if !moved {
                                let on = this.lightbox.as_ref().is_some_and(|open| !open.zoomed);
                                this.lightbox_zoom(on, cx);
                            }
                            cx.stop_propagation();
                        }),
                    ),
            );

        let arrows = (many && !zoomed).then(|| {
            div()
                .absolute()
                .inset_0()
                .child(self.lightbox_arrow("lightbox-previous", -1, cx))
                .child(self.lightbox_arrow("lightbox-next", 1, cx))
        });

        let layer = div()
            .id("lightbox")
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000eb))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.lightbox = None;
                cx.notify();
            }))
            .child(stage)
            .children(arrows)
            .children((many && !zoomed).then(|| self.lightbox_gallery(cx)))
            // The sender and the pill fade while zoomed; the close button stays.
            .children((!zoomed).then_some(bar))
            .children((!zoomed).then_some(pill))
            .children((!zoomed && open.more).then(|| self.lightbox_more_menu(cx)))
            .child(close);
        Some(deferred(layer).with_priority(2).into_any_element())
    }

    /// The menu under More: Copy Image and Copy Media Link, as module 2323's `image-menu` lists them.
    fn lightbox_more_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let row = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|this| this.bg(midnight("--background-mod-subtle")))
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(midnight("--text-default"))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.lightbox_copy(id == "lightbox-copy-image", cx)
                }))
        };
        div()
            .id("lightbox-more-menu")
            .occlude()
            .absolute()
            .top(px(84.))
            .right(px(76.))
            .w(px(200.))
            .p(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(midnight("--border-subtle"))
            .bg(midnight("--background-surface-highest"))
            .flex()
            .flex_col()
            .child(row("lightbox-copy-image", "Copy Image", cx))
            .child(row("lightbox-copy-link", "Copy Media Link", cx))
            .into_any_element()
    }

    /// Copy Image puts the picture on the clipboard; Copy Media Link, its address.
    fn lightbox_copy(&mut self, image: bool, cx: &mut Context<Self>) {
        let Some(open) = self.lightbox.as_mut() else {
            return;
        };
        open.more = false;
        let item = open.items[open.index].clone();
        if image {
            let picture = self
                .images
                .get(&format!("lightbox:{}", item.key))
                .or_else(|| self.images.get(&item.key))
                .cloned();
            match picture {
                Some(picture) => {
                    cx.write_to_clipboard(ClipboardItem::new_image(&picture));
                    self.show_success_toast("Image Copied!".to_owned(), cx);
                }
                None => self.put_app_toast("Unable to copy image".to_owned(), false, cx),
            }
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(item.url));
            self.show_success_toast("Link Copied!".to_owned(), cx);
        }
        cx.notify();
    }

    /// `navPrev_ELGJ1P` / `navNext_ELGJ1P`: 40px buttons 24px from the window's sides, centered on its height.
    fn lightbox_arrow(&self, id: &'static str, delta: i32, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .id(id)
            .absolute()
            .top(px(self.viewport.1 / 2. - 20.))
            .size(px(40.))
            .rounded(px(12.))
            .bg(midnight("--background-surface-highest"))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(icon(
                if delta < 0 {
                    "lb_arrow_left"
                } else {
                    "lb_arrow_right"
                },
                20.,
                "--icon-default",
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.lightbox_step(delta, cx);
                cx.stop_propagation();
            }));
        if delta < 0 {
            base.left(px(24.))
        } else {
            base.right(px(24.))
        }
        .into_any_element()
    }

    /// `galleryContainer_wS56g1`: 40px thumbnails 2px apart, 24px above the window's bottom, the current one lit and
    /// the rest at half brightness; the ends are rounded 8px and the others 2px.
    fn lightbox_gallery(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.lightbox.as_ref().expect("the viewer is open");
        let count = open.items.len();
        let thumbs = open.items.iter().enumerate().map(|(index, item)| {
            let picture = self
                .images
                .get(&item.key)
                .or_else(|| self.images.get(&format!("lightbox:{}", item.key)))
                .cloned();
            let (left, right) = (
                if index == 0 { 8. } else { 2. },
                if index + 1 == count { 8. } else { 2. },
            );
            div()
                .id(SharedString::from(format!("lightbox-thumb-{index}")))
                .size(px(40.))
                .flex_none()
                .overflow_hidden()
                .bg(gpui::rgb(0x000000))
                .rounded_tl(px(left))
                .rounded_bl(px(left))
                .rounded_tr(px(right))
                .rounded_br(px(right))
                .cursor_pointer()
                .when(index != open.index, |this| this.opacity(0.5))
                .children(
                    picture
                        .map(|picture| img(picture).size_full().object_fit(gpui::ObjectFit::Cover)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let current = this.lightbox.as_ref().map_or(0, |open| open.index);
                    this.lightbox_step(index as i32 - current as i32, cx);
                    cx.stop_propagation();
                }))
        });
        div()
            .absolute()
            .bottom(px(24.))
            .left_0()
            .right_0()
            .h(px(56.))
            .flex()
            .justify_center()
            .child(
                div()
                    .h(px(56.))
                    .pl(px(8.))
                    .pr(px(10.))
                    .pt(px(4.))
                    .flex()
                    .gap(px(2.))
                    .children(thumbs),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_at_both_ends() {
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(1, 3, 1), 2);
        assert_eq!(step(0, 1, 1), 0);
        assert_eq!(step(0, 0, 1), 0);
    }

    #[test]
    fn a_picture_is_scaled_down_never_up() {
        // The measured case: 400x240 in a 1280x720 window stays 400x240.
        assert_eq!(stage(false, 400, 240, (1280., 720.)), (400, 240));
        assert_eq!(fit(4000, 1000, 1000, 600), (1000, 250));
        assert_eq!(fit(1000, 4000, 1000, 600), (150, 600));
        assert_eq!(fit(100, 100, 100, 100), (100, 100));
    }

    #[test]
    fn the_wider_of_the_two_rooms_wins() {
        // 2000x1000 in 1280x720: the first room is 1232x496 (616 wide), the second 736 by 648.
        let wide = fit(2000, 1000, 1280 - 48, 720 - 176);
        let tall = fit(2000, 1000, 1280 - 544, 720 - 72);
        assert_eq!(wide, (1088, 544));
        assert_eq!(tall, (736, 368));
        assert_eq!(stage(false, 2000, 1000, (1280., 720.)), (1088, 544));
    }

    #[test]
    fn filenames_come_from_the_address() {
        assert_eq!(
            filename_of("https://cdn.example/a/b/cat.png?x=1"),
            "cat.png"
        );
        assert_eq!(filename_of("https://cdn.example/"), "image");
    }
}
