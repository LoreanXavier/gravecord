//! Uploaded image formats and native animation state (619517/652215).
use super::Root;
use dc_state::Attachment;
use gpui::{AnyElement, Context, Img, IntoElement, SharedString, prelude::*};

pub(super) fn attachment_url(attachment: &Attachment, width: u32, height: u32) -> String {
    image_url(
        &attachment.proxy_url,
        attachment.is_animated(),
        width,
        height,
    )
}

pub(super) fn image_url(base: &str, animated: bool, width: u32, height: u32) -> String {
    let Ok(mut url) = reqwest::Url::parse(base) else {
        return base.to_owned();
    };
    {
        let mut query = url.query_pairs_mut();
        if !animated {
            query
                .append_pair("format", "webp")
                .append_pair("quality", "lossless");
        }
        query
            .append_pair("width", &width.to_string())
            .append_pair("height", &height.to_string());
    }
    url.into()
}

impl Root {
    pub(super) fn media_image(&self, picture: Img, key: &str) -> AnyElement {
        let play = (self.pref_on("animate_gifs", true) && !crate::ui::motion::reduced())
            || self.hovered_media.as_deref() == Some(key);
        // GPUI retains the animation frame only for identified images. An anonymous image is its poster.
        if play {
            picture
                .id(SharedString::from(format!("media-{key}")))
                .into_any_element()
        } else {
            picture.into_any_element()
        }
    }

    pub(super) fn media_hover(&mut self, key: &str, inside: bool, cx: &mut Context<Self>) {
        if inside {
            self.hovered_media = Some(key.to_owned());
        } else if self.hovered_media.as_deref() == Some(key) {
            self.hovered_media = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_uploads_keep_their_format_and_signed_queries() {
        let mut attachment = Attachment {
            id: 1,
            filename: "motion.GIF".into(),
            url: String::new(),
            proxy_url: "https://example.com/motion.gif?signature=preserve&".into(),
            content_type: Some("image/gif".into()),
            width: Some(160),
            height: Some(96),
            size: 10,
            flags: 32,
            description: None,
            duration_secs: None,
            waveform: None,
        };
        let url = reqwest::Url::parse(&attachment_url(&attachment, 160, 96)).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs.get("signature").unwrap(), "preserve");
        assert_eq!(pairs.get("width").unwrap(), "160");
        assert!(!pairs.contains_key("format"));
        attachment.filename = "motion.webp".into();
        assert!(attachment.is_animated());
        attachment.flags = 0;
        assert!(!attachment.is_animated());
        attachment.proxy_url = "https://example.com/static.webp".into();
        let url = attachment_url(&attachment, 160, 96);
        assert!(url.contains("?format=webp&quality=lossless&width=160&height=96"));
    }
}
