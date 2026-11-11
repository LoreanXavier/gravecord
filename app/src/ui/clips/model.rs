//! The clip editor's rules, apart from any drawing: the crop range, the time of a pointer position, the clock text, the
//! ruler's notches and what an edit writes into the clip's document. Module 923904 (the editor's provider: initial
//! state, `setCropStart`, `setCropEnd`, `getEditedClip`), module 423699 (the timeline `e5`, the ruler `eY`, the clock
//! `e0`) and module 489496 (`Yy` writes the document, `VO` exports).
use dc_clips::ClipMetadata;
use serde_json::{Value, json};

/// The crop range may not shrink below this many seconds: `setCropStart` clamps to `end - 1` and `setCropEnd` to
/// `start + 1` (module 923904). The handles' `aria-valuemax` text says 0.1, but the stored state never gets closer
/// than this.
pub(super) const MIN_GAP_S: f64 = 1.0;

/// `lodash.clamp`.
fn clamp(value: f64, lower: f64, upper: f64) -> f64 {
    value.max(lower).min(upper)
}

/// The editor's crop and playhead state, all in seconds (module 923904: `cropStart`, `cropEnd`, `duration`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) struct Crop {
    pub start: f64,
    pub end: f64,
    pub duration: f64,
}

impl Crop {
    /// The initial state from a clip's `editMetadata`: `start` 0 and `end` 0 mean the whole clip; the playhead starts at
    /// the crop start of a cropped clip and in the middle of an uncropped one. Returns the crop and that playhead.
    pub(super) fn initial(meta: &ClipMetadata, duration: f64) -> (Self, f64) {
        let edit = meta.edit_metadata.as_ref();
        let start = edit
            .and_then(|edit| edit.get("start"))
            .and_then(Value::as_f64)
            .unwrap_or(0.);
        let end = edit
            .and_then(|edit| edit.get("end"))
            .and_then(Value::as_f64)
            .unwrap_or(0.);
        let cropped = start != 0. || (end != 0. && end != duration);
        let crop = Self {
            start,
            end: if end == 0. { duration } else { end },
            duration,
        };
        (crop, if cropped { start } else { duration / 2. })
    }

    /// `setCropStart`: clamped to `[0, end - 1]`. Returns the start that was set (the playhead goes there).
    pub(super) fn set_start(&mut self, seconds: f64) -> f64 {
        let upper = (self.end - MIN_GAP_S).max(0.);
        self.start = clamp(seconds, 0., upper);
        self.start
    }

    /// `setCropEnd`: clamped to `[start + 1, duration]`.
    pub(super) fn set_end(&mut self, seconds: f64) -> f64 {
        let lower = (self.start + MIN_GAP_S).min(self.duration);
        self.end = clamp(seconds, lower, self.duration);
        self.end
    }

    pub(super) fn length(&self) -> f64 {
        self.end - self.start
    }

    /// Whether the range is narrower than the clip (`isCropped`).
    #[cfg(test)]
    pub(super) fn is_cropped(&self) -> bool {
        self.start != 0. || self.end != self.duration
    }

    /// The playhead kept inside the range (`F`: `seek(clamp(n, cropStart, cropEnd))`).
    pub(super) fn keep_inside(&self, seconds: f64) -> f64 {
        clamp(seconds, self.start, self.end)
    }
}

/// The time under a pointer at `x` over a timeline from `left` of width `width`: `clamp(x, left, right) - left` as a
/// fraction of the duration (module 423699 `F`).
pub(super) fn time_at(x: f32, left: f32, width: f32, duration: f64) -> f64 {
    if width <= 0. {
        return 0.;
    }
    let along = (x.clamp(left, left + width) - left) / width;
    clamp(f64::from(along) * duration, 0., duration)
}

/// The share of the timeline's width a time is at, 0 to 1 (`left: ${start / duration * 100}%`).
pub(super) fn fraction(seconds: f64, duration: f64) -> f32 {
    if duration <= 0. {
        0.
    } else {
        (seconds / duration).clamp(0., 1.) as f32
    }
}

/// One arrow press or one nudge of a crop handle (`O` in module 423699): a thirtieth of the clip, a tenth of that with
/// Shift.
pub(super) fn key_step(duration: f64, shift: bool) -> f64 {
    let step = duration / 30.;
    if shift { step / 10. } else { step }
}

/// The clock's text, `mm:ss.cc` (module 423699 `e0`: rounded to hundredths, negatives and tiny values are zero).
pub(super) fn clock(seconds: f64) -> String {
    // Whole hundredths first: dividing a rounded float back down drops a hundredth (5.68 reads as 5.67).
    let total = (100. * seconds.max(0.)).round() as u64;
    let (minutes, secs, hundredths) = (total / 6000, total / 100 % 60, total % 100);
    format!("{minutes:02}:{secs:02}.{hundredths:02}")
}

/// The ruler under the clock (`eY`): the long notches with their labels and the short ones between them, each as a time
/// in seconds and a position in percent.
#[derive(Debug, PartialEq)]
pub(super) struct Notches {
    pub long: Vec<(f64, f64)>,
    pub short: Vec<f64>,
}

pub(super) fn notches(length: f64) -> Notches {
    if length <= 0. {
        return Notches {
            long: Vec::new(),
            short: Vec::new(),
        };
    }
    let rough = length / 6.;
    let step: f64 = if rough <= 1. {
        1.
    } else if rough <= 2. {
        2.
    } else if rough <= 5. {
        5.
    } else if rough <= 10. {
        10.
    } else if rough <= 15. {
        15.
    } else if rough <= 20. {
        20.
    } else if rough <= 30. {
        30.
    } else {
        10. * (rough / 10.).round()
    };
    let mut long = Vec::new();
    let mut time = 0.;
    while time <= length {
        long.push((time, time / length * 100.));
        time += step;
    }
    if long.last().is_none_or(|(time, _)| *time < length - 2.) {
        long.push((length, 100.));
    }
    let mut short = Vec::new();
    let sub = step / 5.;
    let mut time = sub;
    while time < length {
        if !long.iter().any(|(at, _)| (at - time).abs() < 0.01) {
            short.push(time / length * 100.);
        }
        time += sub;
    }
    Notches { long, short }
}

/// The document an edit stores: the title, the range, the three audio switches and the crop preset, in the clip's
/// `editMetadata` (module 923904 `getEditedClip` and the unmount effect).
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Edit {
    pub name: Option<String>,
    pub crop: Crop,
    pub application_audio: bool,
    pub voice_audio: bool,
    pub soundboard_audio: bool,
}

impl Edit {
    /// The initial edit of a clip: its stored title, range and switches (all on when there is no `editMetadata`).
    pub(super) fn of(meta: &ClipMetadata, duration: f64) -> (Self, f64) {
        let (crop, playhead) = Crop::initial(meta, duration);
        let flag = |name: &str| {
            meta.edit_metadata
                .as_ref()
                .and_then(|edit| edit.get(name))
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        (
            Self {
                name: meta.name.clone(),
                crop,
                application_audio: flag("applicationAudio"),
                voice_audio: flag("voiceAudio"),
                soundboard_audio: flag("soundboardAudio"),
            },
            playhead,
        )
    }

    /// `editMetadata` as the official writes it (`start`, `end`, three audio flags, `crop: {preset}`).
    pub(super) fn edit_metadata(&self, preset: &str) -> Value {
        json!({
            "start": self.crop.start,
            "end": self.crop.end,
            "applicationAudio": self.application_audio,
            "voiceAudio": self.voice_audio,
            "soundboardAudio": self.soundboard_audio,
            "crop": {"preset": preset},
        })
    }

    /// The clip's document with this edit in it; everything else is kept as it was. An empty title is no title.
    pub(super) fn apply(
        &self,
        meta: &ClipMetadata,
        preset: &str,
        thumbnail: Option<String>,
    ) -> ClipMetadata {
        let mut out = meta.clone();
        out.name = self.name.clone().filter(|name| !name.is_empty());
        out.edit_metadata = Some(self.edit_metadata(preset));
        if let Some(thumbnail) = thumbnail {
            out.thumbnail = thumbnail;
        }
        out
    }

    /// The audio tracks an export keeps. Our files hold `application` and `voice`; there is no separate soundboard
    /// track, so that switch changes nothing in the file.
    pub(super) fn kept_tracks(&self) -> Vec<&'static str> {
        let mut kept = Vec::new();
        if self.application_audio {
            kept.push("application");
        }
        if self.voice_audio {
            kept.push("voice");
        }
        kept
    }

    /// Whether the edit differs from what the document already holds (so closing an untouched editor writes nothing).
    pub(super) fn differs_from(&self, meta: &ClipMetadata, preset: &str) -> bool {
        let stored = meta.name.clone().filter(|name| !name.is_empty());
        let wanted = self.name.clone().filter(|name| !name.is_empty());
        stored != wanted || meta.edit_metadata.as_ref() != Some(&self.edit_metadata(preset))
    }
}

/// The title of a clip (module 132319 `l`): its own name; else the description of the game event that made it; else
/// the activity's details; else `Clip from {time}` with the time as `Mon D, H:MM AM` (`YMk5qU`).
pub(super) fn title(meta: &ClipMetadata) -> String {
    use chrono::TimeZone;
    if let Some(name) = meta.name.as_deref().filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    let signal = meta
        .decision
        .as_ref()
        .and_then(|decision| decision.get("signal"));
    if signal
        .and_then(|signal| signal.get("type"))
        .and_then(Value::as_str)
        == Some("game_event")
        && let Some(text) = signal
            .and_then(|signal| signal.get("description"))
            .and_then(Value::as_str)
    {
        return text.to_owned();
    }
    if let Some(details) = meta
        .activity
        .as_ref()
        .and_then(|activity| activity.get("details"))
        .and_then(Value::as_str)
        .filter(|details| !details.is_empty())
    {
        return details.to_owned();
    }
    let when = chrono::Local
        .timestamp_millis_opt(meta.created_at as i64)
        .single()
        .map(|when| when.format("%b %-d, %-I:%M %p").to_string())
        .unwrap_or_default();
    format!("Clip from {when}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ClipMetadata {
        ClipMetadata::new_manual("id".into(), 1_700_000_000_000, "Game".into(), vec![])
    }

    #[test]
    fn handles_keep_a_second_between_them_and_stay_inside_the_clip() {
        let mut crop = Crop {
            start: 0.,
            end: 20.,
            duration: 20.,
        };
        assert_eq!(crop.set_start(25.), 19.);
        assert_eq!(crop.set_start(-3.), 0.);
        assert_eq!(crop.set_end(-1.), 1.);
        assert_eq!(crop.set_end(99.), 20.);
        crop.set_start(5.);
        assert_eq!(crop.set_end(5.4), 6.);
        assert_eq!(crop.length(), 1.);
        assert!(crop.is_cropped());
        assert_eq!(crop.keep_inside(0.), 5.);
        assert_eq!(crop.keep_inside(9.), 6.);
    }

    #[test]
    fn a_pointer_maps_to_a_time_and_stops_at_the_ends() {
        assert_eq!(time_at(150., 100., 200., 60.), 15.);
        assert_eq!(time_at(0., 100., 200., 60.), 0.);
        assert_eq!(time_at(900., 100., 200., 60.), 60.);
        assert_eq!(time_at(5., 5., 0., 60.), 0.);
        assert!((fraction(15., 60.) - 0.25).abs() < 1e-6);
        assert_eq!(fraction(5., 0.), 0.);
    }

    #[test]
    fn arrow_steps_are_a_thirtieth_and_a_three_hundredth() {
        assert!((key_step(30., false) - 1.).abs() < 1e-9);
        assert!((key_step(30., true) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn the_clock_reads_minutes_seconds_and_hundredths() {
        assert_eq!(clock(0.), "00:00.00");
        assert_eq!(clock(-4.), "00:00.00");
        assert_eq!(clock(5.678), "00:05.68");
        assert_eq!(clock(75.5), "01:15.50");
        assert_eq!(clock(0.004), "00:00.00");
    }

    #[test]
    fn the_ruler_picks_a_step_for_the_length() {
        let thirty = notches(30.);
        assert_eq!(thirty.long.first(), Some(&(0., 0.)));
        assert_eq!(thirty.long.get(1).map(|n| n.0), Some(5.));
        assert_eq!(thirty.long.last(), Some(&(30., 100.)));
        // Four short notches between two long ones.
        assert_eq!(
            thirty
                .short
                .iter()
                .filter(|p| **p > 0. && **p < 100. / 6.)
                .count(),
            4
        );
        let long = notches(120.);
        assert_eq!(long.long.get(1).map(|n| n.0), Some(20.));
        assert!(notches(0.).long.is_empty());
        // A length just past a step gets its own closing notch.
        let last = notches(33.).long.last().copied();
        assert_eq!(last, Some((33., 100.)));
    }

    #[test]
    fn a_new_clip_opens_in_the_middle_and_a_cropped_one_at_its_start() {
        let (crop, playhead) = Crop::initial(&meta(), 40.);
        assert_eq!((crop.start, crop.end, playhead), (0., 40., 20.));
        let mut cropped = meta();
        cropped.edit_metadata = Some(json!({"start": 4.5, "end": 12.0}));
        let (crop, playhead) = Crop::initial(&cropped, 40.);
        assert_eq!((crop.start, crop.end, playhead), (4.5, 12., 4.5));
        // An end equal to the length with a zero start is not a crop.
        let mut whole = meta();
        whole.edit_metadata = Some(json!({"start": 0.0, "end": 40.0}));
        assert_eq!(Crop::initial(&whole, 40.).1, 20.);
    }

    #[test]
    fn an_edit_is_written_as_the_official_document_and_nothing_else_changes() {
        let mut original = meta();
        original.thumbnail = "data:image/jpeg;base64,AAAA".into();
        original.length = 40_000;
        let (mut edit, _) = Edit::of(&original, 40.);
        assert!(!edit.differs_from(&original, "original") || original.edit_metadata.is_none());
        edit.name = Some("Ace".into());
        edit.crop.set_start(3.);
        edit.crop.set_end(11.5);
        edit.voice_audio = false;
        let written = edit.apply(&original, "original", None);
        assert_eq!(written.name.as_deref(), Some("Ace"));
        assert_eq!(written.thumbnail, original.thumbnail);
        assert_eq!(written.id, original.id);
        assert_eq!(
            written.edit_metadata,
            Some(json!({
                "start": 3.0, "end": 11.5, "applicationAudio": true, "voiceAudio": false,
                "soundboardAudio": true, "crop": {"preset": "original"}
            }))
        );
        // Written and read back, the edit is the same and an untouched reopen writes nothing.
        let text = written.to_json();
        let back = ClipMetadata::parse(text.as_bytes()).unwrap();
        let (again, playhead) = Edit::of(&back, 40.);
        assert_eq!(again, edit);
        assert_eq!(playhead, 3.);
        assert!(!again.differs_from(&back, "original"));
        assert_eq!(edit.kept_tracks(), vec!["application"]);
    }

    #[test]
    fn an_empty_title_clears_the_name() {
        let mut named = meta();
        named.name = Some("Old".into());
        let (mut edit, _) = Edit::of(&named, 10.);
        edit.name = Some(String::new());
        assert_eq!(edit.apply(&named, "original", None).name, None);
    }

    #[test]
    fn a_clip_without_a_name_is_titled_by_its_date() {
        assert!(title(&meta()).starts_with("Clip from "));
        let mut named = meta();
        named.name = Some("Ace".into());
        assert_eq!(title(&named), "Ace");
        let mut detailed = meta();
        detailed.activity = Some(json!({"details": "Round 3"}));
        assert_eq!(title(&detailed), "Round 3");
    }
}
