//! LimitlessScreenshare (Equicord `src/equicordplugins/limitlessScreenshare`, GPL-3.0-or-later): Go Live's
//! resolution and frame rate past the account's limits. The original makes `canStreamWithSettings` answer yes, so
//! every choice is open, and puts a slider (3p up to Max Resolution, 1 up to Max FPS) and its own preset lists in
//! the stream menus. Here the same rules, kept as plain functions; the presets are the original's hidden
//! `resolutions` and `fpss` lists, stored apart from the settings. Not part of the pinned client.

use serde_json::{Value, json};

/// `MIN_RESOLUTION`, `MIN_FPS`: "0 FPS freezes (obviously) and anything less than 3p doesn't work".
pub(crate) const MIN_RESOLUTION: u32 = 3;
pub(crate) const MIN_FPS: u32 = 1;
/// The highest frame rate the slider can be set to go: the encoder takes any, the share is held to this.
pub(crate) const FPS_CEILING: u32 = dc_media::screen::MAX_FPS;
/// The tallest share the encoder sends.
pub(crate) const RESOLUTION_CEILING: u32 = dc_media::screen::MAX_HEIGHT;
/// `COOLDOWN_MS`: a slider moves the running stream at most once a second.
pub(crate) const COOLDOWN_MS: u64 = 1000;
/// Where the preset lists are kept.
const STORAGE_KEY: &str = "plugin-limitless-screenshare";

/// The default `resolutions` (0 is Source) and `fpss`.
const RESOLUTIONS: [u32; 5] = [480, 720, 1080, 1440, 0];
const FPSS: [u32; 3] = [15, 30, 60];

/// One preset: what the menu shows and the value it picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Preset {
    pub label: String,
    pub value: u32,
}

impl Preset {
    pub(crate) fn resolution(value: u32) -> Self {
        Self {
            label: if value == 0 {
                "Source".to_owned()
            } else {
                format!("{value}p")
            },
            value,
        }
    }

    pub(crate) fn fps(value: u32) -> Self {
        Self {
            label: format!("{value}fps"),
            value,
        }
    }
}

/// The two lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Presets {
    pub resolutions: Vec<Preset>,
    pub fpss: Vec<Preset>,
}

impl Default for Presets {
    fn default() -> Self {
        Self {
            resolutions: RESOLUTIONS.into_iter().map(Preset::resolution).collect(),
            fpss: FPSS.into_iter().map(Preset::fps).collect(),
        }
    }
}

fn list_of(value: &Value) -> Option<Vec<Preset>> {
    value.as_array().map(|list| {
        list.iter()
            .filter_map(|entry| {
                Some(Preset {
                    label: entry["label"].as_str()?.to_owned(),
                    value: u32::try_from(entry["value"].as_u64()?).ok()?,
                })
            })
            .collect()
    })
}

fn value_of(list: &[Preset]) -> Value {
    Value::Array(
        list.iter()
            .map(|preset| json!({ "label": preset.label, "value": preset.value }))
            .collect(),
    )
}

impl Presets {
    /// The original's settings shape: `{ "resolutions": [{label, value}], "fpss": [...] }`.
    pub(crate) fn from_value(value: Option<&Value>) -> Self {
        let mut presets = Self::default();
        if let Some(value) = value {
            if let Some(list) = list_of(&value["resolutions"]) {
                presets.resolutions = list;
            }
            if let Some(list) = list_of(&value["fpss"]) {
                presets.fpss = list;
            }
        }
        presets
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({ "resolutions": value_of(&self.resolutions), "fpss": value_of(&self.fpss) })
    }

    pub(crate) fn load() -> Self {
        Self::from_value(crate::storage::get(STORAGE_KEY).as_ref())
    }

    pub(crate) fn save(&self) {
        crate::plugins::note("storage:limitless-screenshare");
        crate::storage::set(STORAGE_KEY, self.to_value());
    }

    /// `handleAddPreset`: the value joins its list, sorted with Source (0) last.
    pub(crate) fn add(&mut self, resolution: bool, value: u32) {
        let list = if resolution {
            &mut self.resolutions
        } else {
            &mut self.fpss
        };
        list.push(if resolution {
            Preset::resolution(value)
        } else {
            Preset::fps(value)
        });
        list.sort_by_key(|preset| (preset.value == 0, preset.value));
    }

    /// The delete button of a preset (Source cannot be deleted).
    pub(crate) fn remove(&mut self, resolution: bool, index: usize) {
        let list = if resolution {
            &mut self.resolutions
        } else {
            &mut self.fpss
        };
        if list
            .get(index)
            .is_some_and(|preset| preset.label != "Source")
        {
            list.remove(index);
        }
    }
}

/// `errorString` of the add box: `Must be >3` / `Must be >1`.
pub(crate) fn add_error(resolution: bool, value: u32) -> Option<String> {
    let minimum = if resolution { MIN_RESOLUTION } else { MIN_FPS };
    (value < minimum).then(|| format!("Must be >{minimum}"))
}

/// A slider's range in its own units and the step a unit is worth: Round Resolution moves the resolution in 10p
/// steps.
pub(crate) fn slider_units(resolution: bool, top: u32, round: bool) -> (u32, u32, u32) {
    let step = if resolution && round { 10 } else { 1 };
    let minimum = if resolution { MIN_RESOLUTION } else { MIN_FPS };
    let top = top.min(if resolution {
        RESOLUTION_CEILING
    } else {
        FPS_CEILING
    });
    let low = minimum.div_ceil(step);
    let high = (top / step).max(low);
    (low, high, step)
}

/// A throttle of one call a [`COOLDOWN_MS`], leading and trailing (`lodash.throttle`): what to do with a new value
/// at `now` given when the last one ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Throttle {
    Now,
    /// Run the latest value after this many milliseconds.
    After(u64),
}

pub(crate) fn throttle(last_ms: Option<u64>, now_ms: u64) -> Throttle {
    match last_ms {
        Some(last) if now_ms < last + COOLDOWN_MS => Throttle::After(last + COOLDOWN_MS - now_ms),
        _ => Throttle::Now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_follow_the_original_defaults_and_rules() {
        let mut presets = Presets::default();
        let labels: Vec<_> = presets
            .resolutions
            .iter()
            .map(|p| p.label.as_str())
            .collect();
        assert_eq!(labels, ["480p", "720p", "1080p", "1440p", "Source"]);
        presets.add(true, 900);
        presets.add(false, 120);
        let values: Vec<_> = presets.resolutions.iter().map(|p| p.value).collect();
        assert_eq!(values, [480, 720, 900, 1080, 1440, 0]);
        assert_eq!(presets.fpss.last().unwrap().label, "120fps");
        // Source stays.
        presets.remove(true, 5);
        assert_eq!(presets.resolutions.len(), 6);
        presets.remove(true, 0);
        assert_eq!(presets.resolutions[0].value, 720);
        let back = Presets::from_value(Some(&presets.to_value()));
        assert_eq!(back, presets);
        assert_eq!(add_error(true, 2).as_deref(), Some("Must be >3"));
        assert_eq!(add_error(false, 0).as_deref(), Some("Must be >1"));
        assert_eq!(add_error(false, 1), None);
    }

    #[test]
    fn sliders_and_the_throttle() {
        assert_eq!(slider_units(true, 1080, false), (3, 1080, 1));
        assert_eq!(slider_units(true, 1080, true), (1, 108, 10));
        assert_eq!(slider_units(false, 120, false), (1, 120, 1));
        // The encoder's ceilings hold whatever Max says.
        assert_eq!(slider_units(false, 10_000, false).1, FPS_CEILING);
        assert_eq!(throttle(None, 5), Throttle::Now);
        assert_eq!(throttle(Some(1000), 1500), Throttle::After(500));
        assert_eq!(throttle(Some(1000), 2000), Throttle::Now);
    }
}
