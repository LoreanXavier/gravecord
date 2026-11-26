//! Go Live capture settings, the quality ladder and the bitrate table, as pure functions.
//!
//! Shape and bounds follow Serein's `crates/client-core/src/screen.rs` (MIT OR Apache-2.0), but the values come from
//! the pinned official client wherever the bundle shows them (`design/snapshot/modules`):
//!
//! * resolutions 480 / 720 / 1080 / 1440 / source and frame rates 5 / 15 / 30 / 60, and which pairs exist and what
//!   they need, from the preset table in module 753070 (`ZV`). Serein's 854x480 and 90 / 120 fps are not official.
//! * the encoder's top bitrate from `setDesktopEncodingOptions` in module 31595 with the constants of module 731854:
//!   `lo` = 3.5 Mb/s, `oL` = 9 Mb/s. Source resolution at 10 fps or more, any height above 720 and any rate above 30
//!   take the high one. Serein's own table (2 / 4 / 8 / 16 Mb/s scaled by frame rate) is not used.
//! * the floor from `desktopBitrate.min` (500 kb/s) in module 731854.
//!
//! The official client also lowers 1080p30 to 3.5 or 5 Mb/s for guilds without boosts through the
//! `2026-05-frontier-tuning` experiment (module 474626) and AV1 through another; those are server-side experiments and
//! stay out of this table.

use crate::{Error, Result};

/// The largest picture a share can send.
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 2160;

/// Frame rates the picker offers.
pub const FRAME_RATES: [u32; 4] = [5, 15, 30, 60];
/// The highest frame rate a share runs at ([`Settings::unlocked`]); the official picker stops at 60.
pub const MAX_FPS: u32 = 240;

/// Top bitrate for 720p or smaller at 30 fps or less (`lo` in the official constants), in bits per second.
pub const BITRATE_STANDARD: u32 = 3_500_000;
/// Top bitrate for source resolution, above 720p, or above 30 fps (`oL`).
pub const BITRATE_HIGH: u32 = 9_000_000;
/// The lowest rate a share is asked to run at (`desktopBitrate.min`).
pub const BITRATE_MIN: u32 = 500_000;

/// A picker resolution: the capture's height, or the source's own size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    P480,
    P720,
    P1080,
    P1440,
    Source,
    /// Not in the pinned client: a height off the official table (gravecord's LimitlessScreenshare plugin). The
    /// official ladder has no pair for it, so [`Settings::new`] refuses it; [`Settings::unlocked`] takes it.
    Custom(u32),
}

impl Resolution {
    pub const ALL: [Self; 5] = [
        Self::P480,
        Self::P720,
        Self::P1080,
        Self::P1440,
        Self::Source,
    ];

    /// Pixel height; 0 for the source's own size, as in the official enum.
    pub fn height(self) -> u32 {
        match self {
            Self::P480 => 480,
            Self::P720 => 720,
            Self::P1080 => 1080,
            Self::P1440 => 1440,
            Self::Source => 0,
            Self::Custom(height) => height,
        }
    }

    pub fn from_height(height: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.height() == height)
    }
}

/// What a quality needs, in the official client's terms: `StreamQuality` of module 682901 (`MID_STREAMING_QUALITY`,
/// `HIGH_STREAMING_QUALITY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StreamQuality {
    Standard,
    Mid,
    High,
}

/// The pairs the official picker lists with what each needs (`ZV` in module 753070), in its order. Where a pair also
/// names a guild boost tier in the bundle the quality is the user-side alternative.
const LADDER: [(Resolution, u32, StreamQuality); 18] = [
    (Resolution::Source, 60, StreamQuality::High),
    (Resolution::Source, 30, StreamQuality::High),
    (Resolution::Source, 15, StreamQuality::High),
    (Resolution::Source, 5, StreamQuality::Standard),
    (Resolution::P1440, 60, StreamQuality::Mid),
    (Resolution::P1440, 30, StreamQuality::Mid),
    (Resolution::P1440, 15, StreamQuality::Mid),
    (Resolution::P1080, 60, StreamQuality::Mid),
    (Resolution::P1080, 30, StreamQuality::Mid),
    (Resolution::P1080, 15, StreamQuality::Mid),
    (Resolution::P720, 60, StreamQuality::Mid),
    (Resolution::P720, 30, StreamQuality::Standard),
    (Resolution::P720, 15, StreamQuality::Standard),
    (Resolution::P720, 5, StreamQuality::Standard),
    (Resolution::P480, 60, StreamQuality::Mid),
    (Resolution::P480, 30, StreamQuality::Standard),
    (Resolution::P480, 15, StreamQuality::Standard),
    (Resolution::P480, 5, StreamQuality::Standard),
];

/// What a resolution and frame rate need, or `None` when the picker does not offer the pair.
pub fn required_quality(resolution: Resolution, fps: u32) -> Option<StreamQuality> {
    LADDER
        .iter()
        .find(|(r, f, _)| *r == resolution && *f == fps)
        .map(|(_, _, quality)| *quality)
}

/// Frame rates offered for a resolution, highest first.
pub fn offered_frame_rates(resolution: Resolution) -> Vec<u32> {
    LADDER
        .iter()
        .filter(|(r, _, _)| *r == resolution)
        .map(|(_, fps, _)| *fps)
        .collect()
}

/// Resolutions that fit inside a source of `source_height` pixels (the source's own size always does).
pub fn offered_resolutions(source_height: u32) -> Vec<Resolution> {
    Resolution::ALL
        .into_iter()
        .filter(|r| *r == Resolution::Source || r.height() <= source_height)
        .collect()
}

/// The largest picture a preset sends: `setGoLiveSource` in module 31595 gives the encoder a box of the preset's
/// height and `t <= 480 ? t / 3 * 4 : t / 9 * 16` wide (640x480, 1280x720, 1920x1080, 2560x1440) as the stream's
/// `maxResolution`; the source's own size has no box.
pub fn preset_box(resolution: Resolution) -> Option<(u32, u32)> {
    let height = resolution.height();
    match height {
        0 => None,
        1..=480 => Some((height / 3 * 4, height)),
        _ => Some((height / 9 * 16, height)),
    }
}

/// The capture size for a choice from a source of `source_width` x `source_height`: the source scaled down, keeping
/// its aspect ratio, until it fits the preset's [`preset_box`] (or 4K for the source's own size), both sides rounded
/// to even. A source wider than 16:9 is limited by the box's width, so a 3440x1440 window at 1080p sends 1920x804, as
/// the official encoder does, not 2580x1080. `None` when the source is unknown or shorter than the preset: nothing is
/// upscaled.
pub fn output_size(
    resolution: Resolution,
    source_width: u32,
    source_height: u32,
) -> Option<(u32, u32)> {
    if source_width < 2 || source_height < 2 {
        return None;
    }
    let (width, height) = match preset_box(resolution) {
        None => (source_width, source_height),
        Some(_) if resolution.height() > source_height => return None,
        Some((box_width, box_height)) => {
            let (sw, sh) = (u64::from(source_width), u64::from(source_height));
            let (bw, bh) = (u64::from(box_width), u64::from(box_height));
            // Scale by the tighter side: width over the box's width or height over its height.
            if sw * bh > bw * sh {
                (box_width, u32::try_from((sh * bw + sw / 2) / sw).ok()?)
            } else {
                (u32::try_from((sw * bh + sh / 2) / sh).ok()?, box_height)
            }
        }
    };
    fit_into_limit(width, height)
}

/// Scales down into 4K when needed and rounds both sides down to even numbers.
fn fit_into_limit(width: u32, height: u32) -> Option<(u32, u32)> {
    let (width, height) = if width <= MAX_WIDTH && height <= MAX_HEIGHT {
        (width, height)
    } else {
        let (width, height) = (u64::from(width), u64::from(height));
        let scale =
            ((u64::from(MAX_WIDTH) << 16) / width).min((u64::from(MAX_HEIGHT) << 16) / height);
        (
            u32::try_from((width * scale) >> 16).ok()?,
            u32::try_from((height * scale) >> 16).ok()?,
        )
    };
    let (width, height) = (width & !1, height & !1);
    (width >= 2 && height >= 2).then_some((width, height))
}

/// The encoder's top bitrate for a choice, in bits per second.
pub fn max_bitrate(resolution: Resolution, fps: u32) -> u32 {
    let high = match resolution {
        Resolution::Source => fps >= 10,
        preset => preset.height() > 720,
    } || fps > 30;
    if high { BITRATE_HIGH } else { BITRATE_STANDARD }
}

/// Whether a software encoder should restart for a new transport target: congestion cuts of at least 15% always, and
/// growth in 25% steps, so a gradual recovery cannot cost a keyframe every second. Adapted from Serein's
/// `software_rate_change` (`crates/discord-voice/src/screen.rs`).
pub fn software_rate_change(current: u32, target: u32) -> bool {
    u64::from(target) * 100 <= u64::from(current) * 85
        || u64::from(target) * 100 >= u64::from(current) * 125
}

/// Which capture API item a share reads: an HMONITOR or an HWND on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceId {
    Display(u64),
    Window(u64),
}

/// One share. Build it with [`Settings::new`] and check it with [`Settings::validate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub source: SourceId,
    /// The encoded picture. Even, at most 3840x2160. A source of another shape is letterboxed.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The encoder's starting and top rate, in bits per second.
    pub bitrate: u32,
    pub cursor: bool,
    /// Share system audio with the screen; never the microphone.
    pub audio: bool,
}

impl Settings {
    /// Settings for a picker choice. `None` when the pair is not offered or does not fit the source.
    pub fn new(
        source: &super::Source,
        resolution: Resolution,
        fps: u32,
        cursor: bool,
        audio: bool,
    ) -> Option<Self> {
        required_quality(resolution, fps)?;
        let (width, height) = output_size(resolution, source.width, source.height)?;
        Some(Self {
            source: source.id,
            width,
            height,
            fps,
            bitrate: max_bitrate(resolution, fps),
            cursor,
            audio,
        })
    }

    /// Settings for any resolution and frame rate the encoder takes, off the official table too (at most
    /// [`MAX_FPS`], never upscaled, at most 4K). `None` when the pair does not fit the source.
    pub fn unlocked(
        source: &super::Source,
        resolution: Resolution,
        fps: u32,
        cursor: bool,
        audio: bool,
    ) -> Option<Self> {
        if !(1..=MAX_FPS).contains(&fps) {
            return None;
        }
        let (width, height) = output_size(resolution, source.width, source.height)?;
        Some(Self {
            source: source.id,
            width,
            height,
            fps,
            bitrate: max_bitrate(resolution, fps),
            cursor,
            audio,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |what: &str| Err(Error::invalid(format!("screen share {what}")));
        if matches!(self.source, SourceId::Display(0) | SourceId::Window(0)) {
            return bad("source is missing");
        }
        if self.width < 2
            || self.height < 2
            || self.width > MAX_WIDTH
            || self.height > MAX_HEIGHT
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            return bad("size must be even and between 2x2 and 3840x2160");
        }
        // The picker's rates, or any up to MAX_FPS for an unlocked share.
        if !(1..=MAX_FPS).contains(&self.fps) {
            return bad("frame rate must be between 1 and 240");
        }
        if !(BITRATE_MIN..=BITRATE_HIGH).contains(&self.bitrate) {
            return bad("bitrate must be between 500 kb/s and 9 Mb/s");
        }
        Ok(())
    }

    /// A requested transport rate held to this share's bounds.
    pub fn clamp_bitrate(&self, requested: u32) -> u32 {
        requested.clamp(BITRATE_MIN, self.bitrate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::Source;

    fn display(width: u32, height: u32) -> Source {
        Source {
            id: SourceId::Display(1),
            name: "Display".into(),
            width,
            height,
            refresh_hz: 60,
            icon: None,
        }
    }

    #[test]
    fn ladder_matches_the_official_preset_table() {
        // ZV in module 753070: 18 entries; free pairs are 720p and 480p up to 30 fps and every 5 fps pair listed.
        assert_eq!(LADDER.len(), 18);
        assert_eq!(
            required_quality(Resolution::P720, 30),
            Some(StreamQuality::Standard)
        );
        assert_eq!(
            required_quality(Resolution::P720, 60),
            Some(StreamQuality::Mid)
        );
        assert_eq!(
            required_quality(Resolution::P480, 60),
            Some(StreamQuality::Mid)
        );
        assert_eq!(
            required_quality(Resolution::P1080, 15),
            Some(StreamQuality::Mid)
        );
        assert_eq!(
            required_quality(Resolution::P1440, 60),
            Some(StreamQuality::Mid)
        );
        assert_eq!(
            required_quality(Resolution::Source, 60),
            Some(StreamQuality::High)
        );
        assert_eq!(
            required_quality(Resolution::Source, 5),
            Some(StreamQuality::Standard)
        );
        // Not offered: 5 fps above 720p, and rates outside the ladder.
        assert_eq!(required_quality(Resolution::P1080, 5), None);
        assert_eq!(required_quality(Resolution::P1440, 5), None);
        assert_eq!(required_quality(Resolution::P720, 90), None);
        assert_eq!(offered_frame_rates(Resolution::P480), [60, 30, 15, 5]);
        assert_eq!(offered_frame_rates(Resolution::P1080), [60, 30, 15]);
        assert!(StreamQuality::High > StreamQuality::Mid);
    }

    #[test]
    fn bitrate_table_follows_the_official_two_levels() {
        assert_eq!(max_bitrate(Resolution::P480, 30), 3_500_000);
        assert_eq!(max_bitrate(Resolution::P720, 30), 3_500_000);
        assert_eq!(max_bitrate(Resolution::P720, 15), 3_500_000);
        // Above 30 fps takes the high level whatever the height.
        assert_eq!(max_bitrate(Resolution::P720, 60), 9_000_000);
        assert_eq!(max_bitrate(Resolution::P480, 60), 9_000_000);
        // Above 720p takes it at any rate.
        assert_eq!(max_bitrate(Resolution::P1080, 15), 9_000_000);
        assert_eq!(max_bitrate(Resolution::P1440, 30), 9_000_000);
        // Source resolution: high from 10 fps, standard below.
        assert_eq!(max_bitrate(Resolution::Source, 15), 9_000_000);
        assert_eq!(max_bitrate(Resolution::Source, 5), 3_500_000);
    }

    #[test]
    fn presets_box_the_picture_as_the_official_encoder_options_do() {
        assert_eq!(preset_box(Resolution::P480), Some((640, 480)));
        assert_eq!(preset_box(Resolution::P720), Some((1280, 720)));
        assert_eq!(preset_box(Resolution::P1080), Some((1920, 1080)));
        assert_eq!(preset_box(Resolution::P1440), Some((2560, 1440)));
        assert_eq!(preset_box(Resolution::Source), None);
    }

    #[test]
    fn output_sizes_keep_the_source_aspect_and_never_upscale() {
        assert_eq!(
            output_size(Resolution::P1080, 2560, 1440),
            Some((1920, 1080))
        );
        assert_eq!(output_size(Resolution::P720, 1920, 1080), Some((1280, 720)));
        // Ultrawide: the box's width limits it, the height follows, rounded to even.
        assert_eq!(
            output_size(Resolution::P1080, 3440, 1440),
            Some((1920, 804))
        );
        // A 4:3 window fills the 4:3 box of 480p; a 16:9 display fits inside it.
        assert_eq!(output_size(Resolution::P480, 800, 600), Some((640, 480)));
        assert_eq!(output_size(Resolution::P480, 1920, 1080), Some((640, 360)));
        // Taller than 16:9: the preset's height limits it.
        assert_eq!(output_size(Resolution::P720, 1920, 1200), Some((1152, 720)));
        assert_eq!(
            output_size(Resolution::P1440, 7000, 1500),
            Some((2560, 548))
        );
        assert_eq!(output_size(Resolution::P1080, 1280, 720), None);
        assert_eq!(
            output_size(Resolution::Source, 1281, 721),
            Some((1280, 720))
        );
        // Above 4K the source size is fitted down, keeping the aspect.
        assert_eq!(
            output_size(Resolution::Source, 7680, 4320),
            Some((3840, 2160))
        );
        assert_eq!(output_size(Resolution::Source, 0, 720), None);
        assert_eq!(output_size(Resolution::P720, 1, 1), None);
        // A very wide preset still fits the 4K width limit.
        let (width, height) = output_size(Resolution::P1440, 7000, 1500).unwrap();
        assert!(width <= MAX_WIDTH && height <= MAX_HEIGHT && width % 2 == 0 && height % 2 == 0);
        assert_eq!(
            offered_resolutions(900),
            [Resolution::P480, Resolution::P720, Resolution::Source]
        );
    }

    #[test]
    fn settings_for_a_choice_validate_and_clamp() {
        let settings =
            Settings::new(&display(2560, 1440), Resolution::P1080, 60, true, false).unwrap();
        assert_eq!(
            (
                settings.width,
                settings.height,
                settings.fps,
                settings.bitrate
            ),
            (1920, 1080, 60, 9_000_000)
        );
        settings.validate().unwrap();
        assert_eq!(settings.clamp_bitrate(100_000), BITRATE_MIN);
        assert_eq!(settings.clamp_bitrate(20_000_000), 9_000_000);
        assert_eq!(settings.clamp_bitrate(2_000_000), 2_000_000);
        // 5 fps at 1080p is not offered; neither is a preset larger than the source.
        assert!(Settings::new(&display(2560, 1440), Resolution::P1080, 5, true, false).is_none());
        assert!(Settings::new(&display(1280, 720), Resolution::P1080, 30, true, false).is_none());
    }

    #[test]
    fn validation_rejects_every_out_of_bounds_field() {
        let good = Settings {
            source: SourceId::Window(7),
            width: 1280,
            height: 720,
            fps: 30,
            bitrate: 3_500_000,
            cursor: true,
            audio: true,
        };
        good.validate().unwrap();
        let bad = [
            Settings {
                source: SourceId::Display(0),
                ..good
            },
            Settings {
                source: SourceId::Window(0),
                ..good
            },
            Settings { width: 1, ..good },
            Settings {
                width: 1281,
                ..good
            },
            Settings {
                height: 721,
                ..good
            },
            Settings {
                width: 3842,
                ..good
            },
            Settings {
                height: 2162,
                ..good
            },
            Settings { fps: 0, ..good },
            Settings { fps: 241, ..good },
            Settings {
                bitrate: 499_999,
                ..good
            },
            Settings {
                bitrate: 9_000_001,
                ..good
            },
        ];
        for settings in bad {
            assert!(settings.validate().is_err(), "{settings:?}");
        }
        Settings {
            width: 3840,
            height: 2160,
            fps: 60,
            bitrate: 9_000_000,
            ..good
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn unlocked_settings_take_any_height_and_rate_the_encoder_does() {
        let source = display(1920, 1080);
        // Off the official table: refused by `new`, taken by `unlocked`.
        assert!(Settings::new(&source, Resolution::Custom(900), 90, true, false).is_none());
        let custom = Settings::unlocked(&source, Resolution::Custom(900), 90, true, false).unwrap();
        assert_eq!((custom.width, custom.height, custom.fps), (1600, 900, 90));
        assert_eq!(custom.bitrate, BITRATE_HIGH);
        custom.validate().unwrap();
        // An official height at a rate the picker does not list.
        let rate = Settings::unlocked(&source, Resolution::P720, 24, true, false).unwrap();
        assert_eq!(
            (rate.width, rate.height, rate.fps, rate.bitrate),
            (1280, 720, 24, BITRATE_STANDARD)
        );
        // Never upscaled, never past MAX_FPS.
        assert!(Settings::unlocked(&source, Resolution::Custom(1440), 30, true, false).is_none());
        assert!(Settings::unlocked(&source, Resolution::P720, MAX_FPS + 1, true, false).is_none());
        assert_eq!(Resolution::from_height(900), None);
    }

    #[test]
    fn software_encoder_restarts_only_on_large_rate_moves() {
        assert!(software_rate_change(1_000_000, 850_000));
        assert!(!software_rate_change(1_000_000, 900_000));
        assert!(!software_rate_change(1_000_000, 1_100_000));
        assert!(software_rate_change(1_000_000, 1_250_000));
        assert_eq!(Resolution::from_height(1080), Some(Resolution::P1080));
        assert_eq!(Resolution::from_height(0), Some(Resolution::Source));
        assert_eq!(Resolution::from_height(900), None);
    }
}
