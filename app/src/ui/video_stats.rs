//! "Stats for Nerds" of a video (module 410694 `VideoStatsOverlay`, fed by module 338659's collector): the codec,
//! resolution, viewport and buffer of the playing file in a dark box at the player's top left.

use super::tooltip::{Side, Tip};
use super::{Root, color};
use crate::mp4_info::{CodecInfo, bitrate_text, channels_text, sample_rate_text};
use gpui::{AnyElement, Context, FontWeight, IntoElement, MouseButton, div, prelude::*, px, svg};

/// Module 338659 `getStats`, as far as this player knows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct VideoStats {
    pub resolution: String,
    pub video_width: u32,
    pub video_height: u32,
    pub viewport: (u32, u32),
    pub current_time: f64,
    pub duration: Option<f64>,
    pub buffered_seconds: f64,
    pub dropped_frames: u64,
    pub total_frames: u64,
    pub frame_rate: Option<u32>,
    pub file_size: Option<u64>,
    /// `null` until the file is read; every field `None` when it could not be.
    pub codec: Option<CodecInfo>,
    /// The `MediaError` code and message.
    pub error: Option<(u16, String)>,
}

impl VideoStats {
    /// The rows of the overlay, in module 410694's order: label, value, and whether it is the error row.
    pub(super) fn rows(&self) -> Vec<(&'static str, String, bool)> {
        let mut rows = Vec::new();
        if let Some(codec) = &self.codec {
            if let Some(container) = &codec.container_format {
                rows.push(("Container", container.clone(), false));
            }
            if codec.is_progressive.is_some() || codec.is_fragmented.is_some() {
                let mut kinds = Vec::new();
                if codec.is_progressive == Some(true) {
                    kinds.push("Progressive");
                }
                if codec.is_fragmented == Some(true) {
                    kinds.push("Fragmented");
                }
                let format = if kinds.is_empty() {
                    "Standard".to_owned()
                } else {
                    kinds.join(", ")
                };
                rows.push(("Format", format, false));
            }
            let rate = self
                .frame_rate
                .map_or_else(|| "N/A".to_owned(), |rate| format!("{rate} fps"));
            let dropped = if self.dropped_frames > 0 {
                format!(" ({} dropped)", self.dropped_frames)
            } else {
                String::new()
            };
            rows.push((
                "Resolution",
                format!("{} @ {rate}{dropped}", self.resolution),
                false,
            ));
            rows.push((
                "Viewport",
                format!("{}x{}", self.viewport.0, self.viewport.1),
                false,
            ));
            if let Some(raw) = &codec.video_codec {
                let name = codec
                    .video_codec_description
                    .clone()
                    .unwrap_or_else(|| raw.clone());
                let rate = codec
                    .video_bitrate
                    .map_or(String::new(), |bits| format!(" @ {}", bitrate_text(bits)));
                rows.push(("Video", format!("{name}{rate}"), false));
            }
            if let Some(raw) = &codec.audio_codec {
                let name = codec
                    .audio_codec_description
                    .clone()
                    .unwrap_or_else(|| raw.clone());
                let rate = codec
                    .audio_bitrate
                    .map_or(String::new(), |bits| format!(" @ {}", bitrate_text(bits)));
                rows.push(("Audio", format!("{name}{rate}"), false));
            }
            if let Some(channels) = codec.audio_channels {
                let rate = codec.audio_sample_rate.map_or(String::new(), |rate| {
                    format!(" @ {}", sample_rate_text(rate))
                });
                rows.push((
                    "Audio Channels",
                    format!("{}{rate}", channels_text(channels)),
                    false,
                ));
            }
        }
        let health = if self.buffered_seconds.is_finite() {
            format!("{:.1}s", self.buffered_seconds)
        } else {
            "Live".to_owned()
        };
        rows.push(("Buffer Health", health, false));
        if let Some((code, message)) = &self.error {
            let text = if message.is_empty() {
                code.to_string()
            } else {
                format!("{code}: {message}")
            };
            rows.push(("Error", text, true));
        }
        rows
    }

    /// Module 410694's Copy to JSON: the media and playback facts, two-space indented.
    pub(super) fn json(&self) -> String {
        let codec = self.codec.clone().unwrap_or_default();
        // `JSON.stringify` writes whole numbers without a fraction and NaN as null.
        let number = |value: Option<f64>| match value.filter(|value| value.is_finite()) {
            Some(value) if value.fract() == 0.0 && value.abs() < 9e15 => {
                serde_json::json!(value as i64)
            }
            Some(value) => serde_json::json!(value),
            None => serde_json::Value::Null,
        };
        let percent = if self.total_frames > 0 {
            self.dropped_frames as f64 / self.total_frames as f64 * 100.0
        } else {
            0.0
        };
        let value = serde_json::json!({
            "media": {
                "video": {
                    "codec": codec.video_codec_description.clone().or(codec.video_codec.clone()),
                    "codecRaw": codec.video_codec,
                    "bitRate": number(codec.video_bitrate.map(f64::round)),
                    "frameRate": self.frame_rate,
                    "width": self.video_width,
                    "height": self.video_height,
                },
                "audio": {
                    "codec": codec.audio_codec_description.clone().or(codec.audio_codec.clone()),
                    "codecRaw": codec.audio_codec,
                    "bitRate": number(codec.audio_bitrate.map(f64::round)),
                    "channels": codec.audio_channels,
                    "sampleRate": codec.audio_sample_rate,
                },
                "fileSizeBytes": self.file_size,
                "durationSeconds": number(self.duration),
                "containerFormat": codec.container_format,
                "isProgressive": codec.is_progressive,
                "isFragmented": codec.is_fragmented,
            },
            "playback": {
                "viewportWidth": self.viewport.0,
                "viewportHeight": self.viewport.1,
                "currentTimeSeconds": number(Some(self.current_time)),
                "bufferedSeconds": number(Some(self.buffered_seconds)),
                "droppedFrames": self.dropped_frames,
                "totalDecodedFrames": self.total_frames,
                "droppedFramesPercent": number(Some((percent * 100.0).round() / 100.0)),
                "errorCode": self.error.as_ref().map(|(code, _)| *code),
                "errorMessage": self.error.as_ref().map(|(_, message)| message.clone()),
            },
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }
}

impl Root {
    /// `statsOverlay_WrOgGp`: 8px in from the player's top left, black at 72%, white 14/18 text.
    pub(super) fn video_stats_overlay(
        &self,
        src: &str,
        stats: &VideoStats,
        copied: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = |id: &'static str, glyph: &'static str| {
            // `copyButton_WrOgGp` / `closeButton_WrOgGp`: 24 square, 60% until hovered. The hover background is
            // `--custom-opacity-white-10`, which no stylesheet defines, so none is drawn.
            div()
                .id(id)
                .size(px(24.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .opacity(0.6)
                .hover(|style| style.opacity(1.))
                .child(
                    svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(px(24.))
                        .text_color(color("--white")),
                )
        };
        let (copy_src, close_src) = (src.to_owned(), src.to_owned());
        let json = stats.json();
        let copy = self.with_tip(
            format!("video-stats-copy-{src}"),
            Tip::text(if copied { "Copied!" } else { "Copy to JSON" }, Side::Top),
            button("video-stats-copy", "clips_copy").on_click(cx.listener(
                move |this, _, _, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(json.clone()));
                    this.video_stats_copied(&copy_src, cx);
                },
            )),
            cx,
        );
        let close = self.with_tip(
            format!("video-stats-close-{src}"),
            Tip::text("Close", Side::Top),
            button("video-stats-close", "x_small").on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.video_hub.borrow_mut().toggle_stats(&close_src);
                cx.notify();
            })),
            cx,
        );
        let row = |label: &'static str, value: String| {
            // `statRow_WrOgGp`: the label left, the bold value right, each span 8px short of the row's end.
            div()
                .flex()
                .justify_between()
                .mb(px(4.))
                .child(div().mr(px(8.)).child(label))
                .child(
                    div()
                        .mr(px(8.))
                        .font_weight(FontWeight::BOLD)
                        .text_right()
                        .child(value),
                )
        };
        let mut content = Vec::new();
        for (label, value, error) in stats.rows() {
            if error {
                // `errorSection_WrOgGp`: a 3px critical edge, critical text, 4px above.
                content.push(
                    div()
                        .mt(px(4.))
                        .py(px(6.))
                        .px(px(8.))
                        .rounded(px(3.))
                        .border_l(px(3.))
                        .border_color(color("--border-feedback-critical"))
                        .text_color(color("--text-feedback-critical"))
                        .child(row(label, value))
                        .into_any_element(),
                );
            } else {
                content.push(row(label, value).into_any_element());
            }
        }
        div()
            .id("video-stats")
            .absolute()
            .top(px(8.))
            .left(px(8.))
            .p(px(8.))
            .rounded(px(8.))
            .bg(color("--opacity-black-72"))
            .text_color(color("--white"))
            .text_size(px(14.))
            .line_height(px(18.))
            .flex()
            .flex_col()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(8.))
                    // `body-md/semibold`.
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Stats for Nerds"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(copy)
                            .child(close),
                    ),
            )
            .child(div().flex().flex_col().children(content))
            .into_any_element()
    }

    /// "Copied!" for two seconds after Copy to JSON.
    fn video_stats_copied(&mut self, src: &str, cx: &mut Context<Self>) {
        self.video_hub.borrow_mut().set_stats_copied(src);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(super::video_player::STATS_COPIED)
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(stats: &VideoStats) -> Vec<&'static str> {
        stats.rows().into_iter().map(|(label, ..)| label).collect()
    }

    #[test]
    fn rows_follow_what_is_known() {
        // Before the file is read only the buffer shows.
        let mut stats = VideoStats::default();
        assert_eq!(labels(&stats), ["Buffer Health"]);
        // A file that could not be read still shows the resolution and viewport.
        stats.codec = Some(CodecInfo::default());
        stats.resolution = "Unknown".to_owned();
        stats.viewport = (400, 225);
        assert_eq!(labels(&stats), ["Resolution", "Viewport", "Buffer Health"]);
        assert_eq!(stats.rows()[0].1, "Unknown @ N/A");
        stats.codec = Some(CodecInfo {
            video_codec: Some("avc1.64001f".to_owned()),
            video_codec_description: Some("H.264/AVC".to_owned()),
            video_bitrate: Some(2_500_000.0),
            audio_codec: Some("mp4a.40.2".to_owned()),
            audio_codec_description: Some("AAC-LC".to_owned()),
            audio_bitrate: Some(128_000.0),
            audio_channels: Some(2),
            audio_sample_rate: Some(48_000),
            is_progressive: Some(true),
            is_fragmented: Some(false),
            container_format: Some("MP4 (ISO Base Media)".to_owned()),
            ..CodecInfo::default()
        });
        stats.resolution = "1280x720".to_owned();
        stats.frame_rate = Some(30);
        stats.buffered_seconds = 12.34;
        let rows = stats.rows();
        let text: Vec<String> = rows
            .iter()
            .map(|(label, value, _)| format!("{label}={value}"))
            .collect();
        assert_eq!(
            text,
            [
                "Container=MP4 (ISO Base Media)",
                "Format=Progressive",
                "Resolution=1280x720 @ 30 fps",
                "Viewport=400x225",
                "Video=H.264/AVC @ 2.50 Mbps",
                "Audio=AAC-LC @ 128.0 Kbps",
                "Audio Channels=Stereo @ 48.0 kHz",
                "Buffer Health=12.3s",
            ]
        );
        stats.error = Some((4, "unsupported".to_owned()));
        assert_eq!(
            stats.rows().last().map(|row| (row.0, row.2)),
            Some(("Error", true))
        );
    }

    #[test]
    fn json_has_the_official_shape() {
        let stats = VideoStats {
            codec: Some(CodecInfo {
                video_codec: Some("avc1.64001f".to_owned()),
                video_bitrate: Some(1234.6),
                ..CodecInfo::default()
            }),
            duration: Some(f64::NAN),
            ..VideoStats::default()
        };
        let value: serde_json::Value = serde_json::from_str(&stats.json()).unwrap();
        assert_eq!(value["media"]["video"]["codec"], "avc1.64001f");
        assert_eq!(value["media"]["video"]["bitRate"], 1235);
        assert!(stats.json().contains("\"droppedFramesPercent\": 0,"));
        assert!(value["media"]["durationSeconds"].is_null());
        assert!(value["playback"]["errorCode"].is_null());
    }
}
