//! Timed PNG frames for the source's nameplate play/pause/loop behavior (214881/373054).
use image::{AnimationDecoder, RgbaImage};
use std::{io::Cursor, sync::Arc, time::Duration};

pub(super) struct Clip {
    pub frames: Vec<Arc<RgbaImage>>,
    ends: Vec<Duration>,
    pub duration: Duration,
}

impl Clip {
    pub fn from_frames(
        frames: impl IntoIterator<Item = (Duration, RgbaImage)>,
    ) -> Option<Arc<Self>> {
        let mut clip = Self {
            frames: Vec::new(),
            ends: Vec::new(),
            duration: Duration::ZERO,
        };
        for (delay, frame) in frames {
            if delay.is_zero() {
                return None;
            }
            clip.duration = clip.duration.checked_add(delay)?;
            clip.ends.push(clip.duration);
            clip.frames.push(Arc::new(frame));
        }
        (!clip.frames.is_empty()).then(|| Arc::new(clip))
    }
    pub fn frame_at(&self, elapsed: Duration) -> usize {
        self.ends
            .partition_point(|end| *end <= elapsed)
            .min(self.frames.len() - 1)
    }
    pub fn next_delay(&self, elapsed: Duration) -> Duration {
        self.ends[self.frame_at(elapsed)].saturating_sub(elapsed)
    }
}

fn frame_delay(frame: &image::Frame) -> Duration {
    let (numerator, denominator) = frame.delay().numer_denom_ms();
    if numerator == 0 || denominator == 0 {
        return Duration::from_millis(100);
    }
    let asked = Duration::from_secs_f64(numerator as f64 / denominator as f64 / 1000.);
    if asked <= Duration::from_millis(10) {
        Duration::from_millis(100)
    } else {
        asked
    }
}

pub(super) fn decode(bytes: &[u8]) -> Option<Arc<Clip>> {
    let frames: Vec<image::Frame> = if bytes.starts_with(b"RIFF") {
        let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
        if !decoder.has_animation() {
            return None;
        }
        let _ = decoder.set_background_color(image::Rgba([0, 0, 0, 0]));
        decoder.into_frames().collect_frames().ok()?
    } else if bytes.starts_with(b"GIF8") {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
        decoder.into_frames().collect_frames().ok()?
    } else {
        let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).ok()?;
        if !decoder.is_apng().ok()? {
            return None;
        }
        decoder.apng().ok()?.into_frames().collect_frames().ok()?
    };
    if frames.is_empty() {
        return None;
    }
    let mut clip = Clip {
        frames: Vec::new(),
        ends: Vec::new(),
        duration: Duration::ZERO,
    };
    for frame in frames {
        clip.duration += frame_delay(&frame);
        clip.ends.push(clip.duration);
        clip.frames.push(Arc::new(frame.into_buffer()));
    }
    Some(Arc::new(clip))
}

#[derive(Default)]
pub(super) struct Playback {
    pub elapsed: Duration,
    pub started: bool,
    pub playing: bool,
    last: Option<Duration>,
    active: bool,
    looping: bool,
    ended: bool,
}

impl Playback {
    pub fn tick(&mut self, now: Duration, active: bool, looping: bool, duration: Duration) {
        if self.playing
            && let Some(last) = self.last
        {
            self.elapsed += now.saturating_sub(last);
        }
        if active && (!self.active || (looping && !self.looping && self.ended)) {
            if self.ended {
                self.elapsed = Duration::ZERO;
                self.ended = false;
            }
            self.started = true;
            self.playing = true;
        }
        if !active {
            self.playing = false;
        }
        if self.elapsed >= duration {
            if active && looping {
                let remainder = self.elapsed.as_nanos() % duration.as_nanos();
                self.elapsed = Duration::new(
                    (remainder / 1_000_000_000) as u64,
                    (remainder % 1_000_000_000) as u32,
                );
            } else {
                self.elapsed = duration;
                self.playing = false;
                self.ended = true;
            }
        }
        self.last = Some(now);
        self.active = active;
        self.looping = looping;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn hover_loops_selection_finishes_once_and_focus_pause_resumes() {
        let mut clock = Playback::default();
        clock.tick(ms(0), true, true, ms(3000));
        clock.tick(ms(3500), true, true, ms(3000));
        assert_eq!(clock.elapsed, ms(500));
        clock.tick(ms(3600), false, false, ms(3000));
        assert_eq!(clock.elapsed, ms(600));
        clock.tick(ms(10000), false, false, ms(3000));
        assert_eq!(clock.elapsed, ms(600));
        clock.tick(ms(11000), true, false, ms(3000));
        clock.tick(ms(14000), true, false, ms(3000));
        assert_eq!(clock.elapsed, ms(3000));
        assert!(!clock.playing);
        clock.tick(ms(15000), true, false, ms(3000));
        assert!(!clock.playing);
        clock.tick(ms(16000), true, true, ms(3000));
        assert!(clock.playing);
        assert_eq!(clock.elapsed, Duration::ZERO);
    }

    #[test]
    fn frame_selection_uses_each_encoded_delay_and_holds_the_last_frame() {
        let frame = Arc::new(RgbaImage::new(1, 1));
        let clip = Clip {
            frames: vec![frame.clone(), frame.clone(), frame],
            ends: vec![ms(50), ms(200), ms(300)],
            duration: ms(300),
        };
        assert_eq!(clip.frame_at(ms(0)), 0);
        assert_eq!(clip.frame_at(ms(50)), 1);
        assert_eq!(clip.frame_at(ms(199)), 1);
        assert_eq!(clip.frame_at(ms(200)), 2);
        assert_eq!(clip.frame_at(ms(500)), 2);
    }
}
