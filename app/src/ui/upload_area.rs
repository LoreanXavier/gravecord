//! The drop area shown while files are dragged over the window (module 738876 around the UploadArea of module 65593):
//! the window darkened to `--opacity-black-80`, a 310 by 170 brand card with a dashed inner frame, the document, image
//! and code art fanned out above "Upload to **#channel**" (or "Insta Upload Mode!" while Shift is held) and "You can add
//! comments before uploading. / Hold shift to upload directly.". A drop with Shift held sends the files at once, as
//! `requireConfirm: false` does (module 518960); otherwise they wait in the composer.
//!
//! Geometry is `design/dom/audio-attachment/card-hover-darker-cozy.json`, which holds the area in its final state: the
//! art is drawn there (the end of the 0.8s fan-out, as reduced motion shows it). GAP: the fan-out and bounce
//! animations, the text fade-in, the sparkles of `u` and the middle art's drop shadow are not drawn.

use super::{Root, color};
use gpui::{
    AnyElement, Context, FontWeight, Image, ImageFormat, IntoElement, ParentElement, Styled,
    Window, div, img, prelude::*, px,
};
use std::sync::Arc;

/// The art (`$8` of module 698638, `ir = [1, 0, 2]`) in paint order (`wrapOne`, `wrapThree`, `wrapTwo`): the asset,
/// its turn in degrees and the centre of its 100 by 130 box from the top left of the 100 by 60 `.icons` box (the
/// wrap's lift of 70 or 80px plus the art's final `translate3d`).
const ART: [(&str, f32, (f32, f32)); 3] = [
    ("9ebe3ef63aafee84", -30., (6., -11.)),
    ("f4bad77442422ded", 30., (94., -11.)),
    ("5dea307a4037ec4e", 0., (50., -21.)),
];

/// A canvas around each turned 72 by 96 drawing large enough for its turned bounds (151.6 by 162.6 in the capture).
const CANVAS: (f32, f32) = (160., 170.);

fn art_key(asset: &str) -> String {
    format!("text:upload-area-{asset}")
}

/// The title: `dYP2Fc` with the channel as module 47167 `m1(channel, ..., true)` names it, or `h76ulG` with Shift.
pub(super) fn title(destination: &str, shift: bool) -> (String, Option<String>) {
    if shift {
        (dc_intl::t("h76ulG").to_owned(), None)
    } else {
        (
            dc_intl::t("9uKafS").to_owned() + " ",
            Some(destination.to_owned()),
        )
    }
}

impl Root {
    /// The channel as the title names it: `#name` for a server channel, `"name"` for a thread, a voice channel's
    /// bare name, `@name` for a DM and a group's name.
    fn upload_destination(&self) -> String {
        let title = self.channel_title();
        if self.current_thread().is_some() {
            return format!("\"{title}\"");
        }
        if let Some(channel) = self.current_channel() {
            return match channel.kind {
                dc_state::ChannelKind::Text | dc_state::ChannelKind::Other(5 | 15 | 16 | 21) => {
                    format!("#{title}")
                }
                _ => title,
            };
        }
        match self.current_dm() {
            Some(dm) if dm.kind == 3 => title,
            Some(_) => format!("@{title}"),
            None => title,
        }
    }

    /// `R` of module 738876: no layer (settings) is open, and the channel is a DM or a server channel the account may
    /// attach files in.
    pub(super) fn upload_area_allowed(&self) -> bool {
        if self.settings.is_some()
            || self.server_settings.is_some()
            || self.channel_settings.is_some()
            || self.profile_editor.is_some()
        {
            return false;
        }
        let Some(channel) = self.channel else {
            return false;
        };
        let Some(guild) = self.guild else {
            return self
                .stores
                .private_channels
                .iter()
                .any(|known| known.id == channel);
        };
        let place = self.current_channel().cloned().or_else(|| {
            let parent = self.current_thread()?.parent_id;
            self.stores
                .guild(guild)?
                .channels
                .iter()
                .find(|known| known.id == parent)
                .cloned()
        });
        place.is_some_and(|place| {
            self.my_permission(guild, Some(&place), dc_state::permissions::ATTACH_FILES)
        })
    }

    /// Asks for the three drawings the first time a drag is seen.
    pub(super) fn upload_area_frame(&mut self, cx: &Context<Self>) {
        if !cx.has_active_drag() || self.texts.contains_key(&art_key(ART[0].0)) {
            return;
        }
        for (asset, _, _) in ART {
            self.request_image(
                art_key(asset),
                format!("https://discordapp.com/assets/{asset}.svg"),
            );
        }
    }

    /// The turned drawing, made once: the same image every frame, so it is rasterised once.
    fn upload_art(&self, asset: &'static str, degrees: f32) -> Option<Arc<Image>> {
        thread_local! {
            static ART_CACHE: std::cell::RefCell<std::collections::HashMap<&'static str, Arc<Image>>> =
                Default::default();
        }
        if let Some(image) = ART_CACHE.with(|cache| cache.borrow().get(asset).cloned()) {
            return Some(image);
        }
        let source = self.texts.get(&art_key(asset))?;
        let svg = super::quickswitcher::rotated_svg(source, degrees, false, CANVAS.0, CANVAS.1)?;
        let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()));
        ART_CACHE.with(|cache| cache.borrow_mut().insert(asset, image.clone()));
        Some(image)
    }

    /// The area over the whole window while an external drag is in progress; it shows once the drag is over it
    /// (`uploadModalIn`: opacity 1).
    pub(super) fn file_drop_area(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        if !cx.has_active_drag() || !self.upload_area_allowed() {
            return None;
        }
        let (text, destination) = title(&self.upload_destination(), window.modifiers().shift);
        let icons = div()
            .relative()
            .w(px(100.))
            .h(px(60.))
            .flex_none()
            .children(ART.iter().filter_map(|(asset, degrees, (x, y))| {
                let image = self.upload_art(asset, *degrees)?;
                Some(
                    img(image)
                        .absolute()
                        .left(px(x - CANVAS.0 / 2.))
                        .top(px(y - CANVAS.1 / 2.))
                        .w(px(CANVAS.0))
                        .h(px(CANVAS.1)),
                )
            }));
        let card = div()
            .relative()
            .w(px(310.))
            .h(px(170.))
            .flex()
            // `.bgScale`: the brand card, 10px past the frame on each side in the capture, radius 10.
            .child(
                div()
                    .absolute()
                    .left(px(-10.))
                    .top(px(-10.))
                    .w(px(330.))
                    .h(px(190.))
                    .rounded(px(10.))
                    .bg(color("--brand-500")),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .border_2()
                    .border_dashed()
                    .border_color(color("--opacity-white-40"))
                    .rounded(px(6.))
                    .text_color(color("--white"))
                    .child(icons)
                    .child(
                        div()
                            .w_full()
                            .px(px(8.))
                            .flex()
                            .justify_center()
                            .text_size(px(22.))
                            .line_height(px(22.))
                            .font_weight(FontWeight(700.))
                            .whitespace_nowrap()
                            .child(text)
                            .children(destination),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .ml(px(4.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .children(
                                dc_intl::t("usQh4J")
                                    .split('\n')
                                    .map(|line| div().child(line.to_owned())),
                            ),
                    ),
            );
        Some(
            div()
                .id("upload-area")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(color("--opacity-black-80"))
                .opacity(0.)
                .drag_over::<gpui::ExternalPaths>(|style, _, _, _| style.opacity(1.))
                .child(card)
                .into_any_element(),
        )
    }

    /// A drop over the area: with Shift the files go out at once with no text (`sendMessage` with
    /// `attachmentsToUpload`, location INSTANT_UPLOAD); otherwise they join the composer.
    pub(super) fn drop_files(
        &mut self,
        paths: &gpui::ExternalPaths,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        // `preventUnwantedDrop` of module 65593: a drop holding anything but files (a folder) is refused with
        // "Invalid file type".
        if !all_files(paths.paths()) {
            self.upload_error = Some(super::upload_error::UploadError::Plain {
                title: dc_intl::t("azO1Pe"),
                help: dc_intl::t("Koklr/").to_owned(),
            });
            cx.notify();
            return;
        }
        let instant = shift && self.upload_area_allowed();
        let Some(channel) = self.channel.filter(|_| instant) else {
            self.on_drop_files(paths, cx);
            return;
        };
        let paths = paths.paths().to_vec();
        let sizes: Vec<u64> = paths
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .collect();
        if self.refuse_upload(&sizes) {
            cx.notify();
            return;
        }
        let files: Vec<_> = paths
            .into_iter()
            .filter_map(super::uploads::Pending::new)
            .map(|pending| pending.upload())
            .collect();
        if !files.is_empty() {
            self.start_local_send(channel, String::new(), files, None, Vec::new());
        }
        cx.notify();
    }
}

/// `isAllDropFiles` of module 65593: every dropped entry is a file (a folder, or a path that is gone, is not).
fn all_files(paths: &[std::path::PathBuf]) -> bool {
    paths
        .iter()
        .all(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_in_the_drop_is_an_invalid_file_type() {
        let folder = std::env::temp_dir();
        let file = folder.join("gravecord-upload-area-test.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(all_files(std::slice::from_ref(&file)));
        assert!(!all_files(&[file.clone(), folder]));
        let _ = std::fs::remove_file(file);
        assert_eq!(dc_intl::t("azO1Pe"), "Invalid file type");
    }

    #[test]
    fn shift_turns_the_title_into_insta_upload_mode() {
        assert_eq!(
            title("#general", false),
            ("Upload to ".to_owned(), Some("#general".to_owned()))
        );
        assert_eq!(
            title("#general", true),
            ("Insta Upload Mode!".to_owned(), None)
        );
    }
}
