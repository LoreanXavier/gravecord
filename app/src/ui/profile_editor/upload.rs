//! The avatar and banner image upload of the profile editor. The official client opens the select image modal
//! (module 468450 `XD` lazy-loads module 685663), takes the file into the cropper (module 142630, `ui/image_crop.rs`),
//! then stores the cropped image as the pending change with its data URI. The limits are the official ones: a file
//! over `maxFileSizeBytes` (module 478644, 10 MiB), before or after cropping, is refused with the "Your files are too
//! powerful" error, and the avatar's description is the file name without its extension.
//! GAP: `Rh` of module 101058 writes the description as "{name}, added {dateTime}" (`DYil93`); the bare name is kept.

use super::save::Picked;
use crate::ui::Root;
use gpui::Context;
use std::sync::Arc;

/// `maxFileSizeBytes` of the upload modal (module 478644).
pub(super) const MAX_BYTES: usize = 0xa0_0000;

/// What is being uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Avatar,
    Banner,
}

/// A picture ready to be the pending change: the data URI the API takes and the decoded picture for the preview.
pub(super) struct Prepared {
    pub picked: Picked,
    pub image: Arc<gpui::Image>,
}

/// `maxSize` in the error text (`Hb` of module 453771 prints whole megabytes without decimals).
fn size_text(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        let megabytes = bytes as f64 / (1024. * 1024.);
        if (megabytes - megabytes.round()).abs() < 0.005 {
            format!("{} MB", megabytes.round() as u64)
        } else {
            format!("{megabytes:.2} MB")
        }
    } else {
        format!("{:.2} KB", bytes as f64 / 1024.)
    }
}

/// The `q0rsrZ` error text with its sizes filled in, headed by the `/tGlcj` title.
pub(super) fn too_big(bytes: usize) -> String {
    format!(
        "Your files are too powerful. After cropping, this image is {}. The max file size is {}.",
        size_text(bytes),
        size_text(MAX_BYTES)
    )
}

/// The pending description: the file name without its extension; a banner without one is "Banner image" (`yiRnNO`).
fn description(stem: &str, target: Target) -> String {
    match target {
        Target::Banner if stem.is_empty() => dc_intl::t("yiRnNO").to_owned(),
        _ => stem.to_owned(),
    }
}

impl Root {
    /// `XD` of module 468450: the select image modal (module 685663) for the avatar or banner; its Upload Image tile
    /// takes a file and the cropper (module 142630) hands back the pending change (`profile_editor_cropped`).
    pub(super) fn profile_editor_pick_image(&mut self, target: Target, cx: &mut Context<Self>) {
        let purpose = match target {
            Target::Avatar => crate::ui::image_crop::Purpose::ProfileAvatar,
            Target::Banner => crate::ui::image_crop::Purpose::ProfileBanner,
        };
        self.open_image_select(purpose, cx);
    }

    /// The file input's `maxFileSizeBytes` (10 MiB): a bigger file is refused with the "too powerful" error.
    pub(in crate::ui) fn profile_editor_too_big(
        &mut self,
        size: usize,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if size <= MAX_BYTES {
            return None;
        }
        let message = too_big(size);
        if let Some(open) = self.profile_editor.as_mut() {
            open.error = Some(message.clone());
        }
        cx.notify();
        Some(message)
    }

    /// `onComplete` of module 468450: the cropped picture becomes the pending change unless it is over 10 MiB after
    /// cropping (`q0rsrZ`, measured on the data URI's bytes by `EW`).
    pub(in crate::ui) fn profile_editor_cropped(
        &mut self,
        banner: bool,
        image_uri: String,
        image: Arc<gpui::Image>,
        stem: &str,
        cx: &mut Context<Self>,
    ) {
        let target = if banner {
            Target::Banner
        } else {
            Target::Avatar
        };
        let bytes = image.bytes().len();
        let result = if bytes > MAX_BYTES {
            Err(too_big(bytes))
        } else {
            Ok(Prepared {
                picked: Picked {
                    image_uri,
                    description: Some(description(stem, target)),
                },
                image,
            })
        };
        self.profile_editor_set_image(target, result, cx);
    }

    /// Applies a prepared picture, or shows why it was refused.
    pub(super) fn profile_editor_set_image(
        &mut self,
        target: Target,
        result: Result<Prepared, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        match result {
            Ok(prepared) => {
                open.error = None;
                match target {
                    Target::Avatar => {
                        open.pending.avatar = Some(Some(prepared.picked));
                        open.avatar_image = Some(prepared.image);
                    }
                    Target::Banner => {
                        open.pending.banner = Some(Some(prepared.picked));
                        open.banner_image = Some(prepared.image);
                    }
                }
            }
            Err(message) => open.error = Some(message),
        }
        cx.notify();
    }

    /// The avatar the editor draws: the pending picture, else the account's own.
    pub(super) fn profile_editor_avatar_image(
        &self,
        user: &dc_state::User,
    ) -> Option<Arc<gpui::Image>> {
        self.profile_editor
            .as_ref()
            .and_then(|open| open.avatar_image.clone())
            .or_else(|| self.images.get(&crate::ui::avatar_source(user).0).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_is_the_file_name_and_a_nameless_banner_is_banner_image() {
        assert_eq!(description("my face", Target::Avatar), "my face");
        assert_eq!(description("", Target::Avatar), "");
        assert_eq!(description("", Target::Banner), "Banner image");
    }

    #[test]
    fn a_file_over_ten_megabytes_is_refused_with_the_official_text() {
        let message = too_big(MAX_BYTES + 1);
        assert!(message.starts_with("Your files are too powerful."));
        assert!(message.ends_with("The max file size is 10 MB."));
        assert_eq!(size_text(1536), "1.50 KB");
    }
}
