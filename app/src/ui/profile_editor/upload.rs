//! The avatar and banner image upload of the profile editor. The official client opens a file dialog, then a
//! cropper (module 468450 `XD` and the upload modal it lazy-loads), then stores the cropped image as the pending
//! change with its data URI. The cropper is not drawn here: the chosen picture is used whole. The limits are the
//! official ones: a file over `maxFileSizeBytes` (module 478644, 10 MiB) is refused with the "Your files are too
//! powerful" error, and the avatar's description is the file name without its extension (`Rh` of module 101058).

use super::save::Picked;
use crate::ui::Root;
use base64::Engine;
use gpui::{Context, PathPromptOptions};
use std::path::Path;
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

/// Reads a picture file for the editor. `None` for a file type the API does not take.
pub(super) fn prepare(
    path: &Path,
    target: Target,
    reader: impl Fn(&Path) -> Option<Vec<u8>>,
) -> Option<Result<Prepared, String>> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let (mime, format) = match extension.as_str() {
        "png" => ("image/png", gpui::ImageFormat::Png),
        "jpg" | "jpeg" => ("image/jpeg", gpui::ImageFormat::Jpeg),
        "gif" => ("image/gif", gpui::ImageFormat::Gif),
        "webp" => ("image/webp", gpui::ImageFormat::Webp),
        _ => return None,
    };
    let bytes = reader(path)?;
    if bytes.len() > MAX_BYTES {
        return Some(Err(too_big(bytes.len())));
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let description = match target {
        Target::Avatar => Some(stem.to_owned()),
        // The banner's pending description is its file name, or "Banner image" when it has none.
        Target::Banner => Some(if stem.is_empty() {
            "Banner image".to_owned()
        } else {
            stem.to_owned()
        }),
    };
    let image_uri = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    Some(Ok(Prepared {
        picked: Picked {
            image_uri,
            description,
        },
        image: Arc::new(gpui::Image::from_bytes(format, bytes)),
    }))
}

impl Root {
    /// Opens the file dialog for the avatar or banner and keeps the chosen picture as the pending change.
    pub(super) fn profile_editor_pick_image(&mut self, target: Target, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first() else {
                return;
            };
            let Some(result) = prepare(path, target, |path| std::fs::read(path).ok()) else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.profile_editor_set_image(target, result, cx);
            });
        })
        .detach();
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

    fn read(bytes: Vec<u8>) -> impl Fn(&Path) -> Option<Vec<u8>> {
        move |_| Some(bytes.clone())
    }

    #[test]
    fn a_png_becomes_a_data_uri_described_by_its_name() {
        let result = prepare(
            Path::new("C:/pictures/my face.png"),
            Target::Avatar,
            read(vec![1, 2, 3]),
        )
        .expect("a png is taken")
        .expect("it is small enough");
        assert_eq!(result.picked.image_uri, "data:image/png;base64,AQID");
        assert_eq!(result.picked.description.as_deref(), Some("my face"));
    }

    #[test]
    fn other_file_types_are_ignored() {
        assert!(prepare(Path::new("notes.txt"), Target::Avatar, read(vec![1])).is_none());
    }

    #[test]
    fn a_file_over_ten_megabytes_is_refused_with_the_official_text() {
        let big = vec![0u8; MAX_BYTES + 1];
        let Some(Err(message)) = prepare(Path::new("big.gif"), Target::Banner, read(big)) else {
            panic!("expected a refusal");
        };
        assert!(message.starts_with("Your files are too powerful."));
        assert!(message.ends_with("The max file size is 10 MB."));
        let exact = vec![0u8; MAX_BYTES];
        assert!(matches!(
            prepare(Path::new("ok.gif"), Target::Banner, read(exact)),
            Some(Ok(_))
        ));
    }
}
