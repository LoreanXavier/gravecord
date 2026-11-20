//! "An error occurred loading {locale}" (module 638421): when the interface catalog of the chosen locale could not be
//! loaded, the app gives way to the error page of module 721668 (`wrapper_jScFSg`) with the error's message as its
//! note, until a change of locale loads one.
//!
//! GAP: while a catalog loads, module 638421 draws an empty `loading_2kO-Fc` box in place of the app; here the frames
//! in between keep showing en-US.

use super::{Root, color};
use gpui::{AnyElement, FontWeight, ObjectFit, div, img, prelude::*, px};

/// `wrapper_jScFSg`'s background picture.
const BACKGROUND_ART: &str = "https://discord.com/assets/069714e321aa543f.svg";
/// `images-dark .image_jScFSg` (the light theme's is `f9b5479cf5959262.svg`).
const IMAGE_ART: &str = "https://discord.com/assets/4ced53d842c0ca67.svg";

/// The page's title: `7basa2` with the locale.
pub(super) fn title(locale: &str) -> String {
    dc_intl::format("7basa2", &[("locale", locale.into())])
}

impl Root {
    /// Module 638421 in place of the app while the catalog's load has failed.
    pub(super) fn locale_error_layer(&mut self) -> Option<AnyElement> {
        let error = crate::intl::load_error()?;
        for url in [BACKGROUND_ART, IMAGE_ART] {
            self.request_image(url.to_owned(), url.to_owned());
        }
        let background = self.images.get(BACKGROUND_ART).cloned();
        let picture = self.images.get(IMAGE_ART).cloned();
        Some(
            div()
                .id("locale-error")
                .occlude()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .bg(color("--background-base-lower"))
                .text_color(color("--text-default"))
                // `background-position: 50% 0; background-size: cover`.
                .children(background.map(|art| {
                    img(art)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                }))
                .child(
                    // `flexWrapper_jScFSg`: the full height, a centred column.
                    div()
                        .relative()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .w(px(254.))
                                .h(px(154.))
                                .mb(px(40.))
                                .flex_none()
                                .children(picture.map(|art| img(art).size_full())),
                        )
                        .child(
                            div()
                                .w(px(440.))
                                .mb(px(20.))
                                .flex()
                                .flex_col()
                                .child(
                                    // `title_jScFSg`: 24px semibold, an 8px line, 24px above the note.
                                    div()
                                        .text_center()
                                        .text_size(px(24.))
                                        .line_height(px(8.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .mb(px(24.))
                                        .child(title(dc_intl::locale())),
                                )
                                .child(
                                    div()
                                        .text_center()
                                        .text_size(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(error),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_title_names_the_locale() {
        assert_eq!(super::title("fr"), "An error occurred loading fr");
    }
}
