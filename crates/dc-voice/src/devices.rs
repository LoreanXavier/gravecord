//! New audio devices during a call, and the suggestion to switch to one: `ConnectedDeviceStore` (module 868162) and the
//! connected device manager (286980, opened as the "New Audio Device Detected" modal by 596339 `Nf`).
//!
//! The official client hears about devices from the OS (`MEDIA_ENGINE_DEVICES`); here a thread lists the devices every
//! two seconds through cpal and [`ConnectedDevices::update`] does what the store's handler does with each list. The
//! rules are ported as they read:
//! - the first list only records what exists;
//! - a device that disappeared clears every pending suggestion;
//! - a device that appeared while connected to voice is remembered (an input and an output with the same name become
//!   one input-and-output device);
//! - the suggestion is the first remembered device unless it is ignored, and it is not made when the device is the
//!   one already in use, when it is `DisplayPort`, or when the system default merely moved by itself (a device the
//!   user did not pick, whose default changed in this very list);
//! - switching, not switching, ignoring and "never show" each clear what was remembered.
//!
//! Discord Certified devices (a vendor and model database) are not part of gravecord, so every device is the
//! not-certified kind.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// The system's devices by the names cpal gives them, and the default ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceList {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
}

impl DeviceList {
    /// Reads the devices now.
    pub fn read() -> Self {
        Self {
            inputs: crate::input_devices(),
            outputs: crate::output_devices(),
            default_input: crate::default_input_device(),
            default_output: crate::default_output_device(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Input,
    Output,
    InputAndOutput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    pub display_name: String,
    pub kind: Kind,
}

/// `I(e)` in module 868162: on Windows, the text in the device name's trailing parentheses ("Speakers (USB Audio)"
/// is "USB Audio"); otherwise the name.
pub fn display_name(name: &str, windows: bool) -> String {
    if windows {
        // /\((.+)\)\s*$/: from the first "(" that has something and then a final ")".
        let trimmed = name.trim_end();
        if trimmed.ends_with(')') {
            for (open, _) in trimmed.match_indices('(') {
                let inner = &trimmed[open + 1..trimmed.len() - 1];
                if !inner.is_empty() {
                    return inner.to_owned();
                }
            }
        }
    }
    name.to_owned()
}

/// What the modal offers: the device and what to switch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub display_name: String,
    pub kind: Kind,
    /// The full device name to use as the microphone, when the device is one.
    pub input: Option<String>,
    pub output: Option<String>,
}

/// What is in use, for the rules that compare against it.
#[derive(Clone, Debug, Default)]
pub struct InUse {
    /// The microphone picked in settings; None follows the system default.
    pub chosen_input: Option<String>,
    pub chosen_output: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Default)]
pub struct ConnectedDevices {
    initialized: bool,
    /// Display name -> full name, in the order the system listed them.
    inputs: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
    remembered: Vec<Connected>,
    default_input: Option<String>,
    default_output: Option<String>,
    input_default_moved: bool,
    output_default_moved: bool,
    windows: bool,
    /// `neverShowModal`.
    never_show: bool,
    /// `ignoredDevices`.
    ignored: BTreeSet<String>,
}

fn by_display(names: &[String], windows: bool) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for name in names {
        let shown = display_name(name, windows);
        // An object key set twice keeps its first place and takes the last value.
        match found.iter_mut().find(|(known, _)| *known == shown) {
            Some(slot) => slot.1 = name.clone(),
            None => found.push((shown, name.clone())),
        }
    }
    found
}

fn keys(list: &[(String, String)]) -> Vec<&str> {
    list.iter().map(|(shown, _)| shown.as_str()).collect()
}

fn merge(remembered: &mut Vec<Connected>, name: &str, kind: Kind) {
    match remembered
        .iter_mut()
        .find(|known| known.display_name == name)
    {
        None => remembered.push(Connected {
            display_name: name.to_owned(),
            kind,
        }),
        Some(known) => {
            known.kind = match (known.kind, kind) {
                (Kind::Input, Kind::Output) | (Kind::Output, Kind::Input) => Kind::InputAndOutput,
                (existing, _) => existing,
            };
        }
    }
}

impl ConnectedDevices {
    pub fn new(windows: bool) -> Self {
        Self {
            windows,
            ..Self::default()
        }
    }

    pub fn never_show(&self) -> bool {
        self.never_show
    }

    pub fn ignored(&self) -> &BTreeSet<String> {
        &self.ignored
    }

    /// The persisted choices (`ConnectedDeviceStore`'s state) loaded at start.
    pub fn restore(&mut self, never_show: bool, ignored: BTreeSet<String>) {
        self.never_show = never_show;
        self.ignored = ignored;
    }

    /// `MEDIA_ENGINE_DEVICES`. True when the set of devices changed.
    pub fn update(&mut self, list: &DeviceList, connected: bool) -> bool {
        let inputs = by_display(&list.inputs, self.windows);
        let outputs = by_display(&list.outputs, self.windows);
        // `justChanged`: this list's default is not the last list's.
        self.input_default_moved = list.default_input != self.default_input;
        self.output_default_moved = list.default_output != self.default_output;
        self.default_input = list.default_input.clone();
        self.default_output = list.default_output.clone();
        if !self.initialized {
            self.inputs = inputs;
            self.outputs = outputs;
            self.initialized = true;
            return false;
        }
        let gone = keys(&self.inputs)
            .iter()
            .any(|name| !keys(&inputs).contains(name))
            || keys(&self.outputs)
                .iter()
                .any(|name| !keys(&outputs).contains(name));
        if gone {
            self.remembered.clear();
        }
        if connected {
            let new_inputs: Vec<String> = keys(&inputs)
                .into_iter()
                .filter(|name| !keys(&self.inputs).contains(name))
                .map(str::to_owned)
                .collect();
            let new_outputs: Vec<String> = keys(&outputs)
                .into_iter()
                .filter(|name| !keys(&self.outputs).contains(name))
                .map(str::to_owned)
                .collect();
            for name in new_inputs {
                merge(&mut self.remembered, &name, Kind::Input);
            }
            for name in new_outputs {
                merge(&mut self.remembered, &name, Kind::Output);
            }
        }
        let changed = keys(&self.inputs) != keys(&inputs) || keys(&self.outputs) != keys(&outputs);
        if changed {
            self.inputs = inputs;
            self.outputs = outputs;
        }
        changed
    }

    /// The call ended: nothing is suggested after it (`RTC_CONNECTION_STATE` when disconnected).
    pub fn disconnected(&mut self) {
        self.remembered.clear();
    }

    /// Whether a suggestion should show (`I()`), and which (`Hs()`).
    pub fn suggestion(&self, in_use: &InUse) -> Option<Suggestion> {
        if !in_use.connected || self.never_show || self.remembered.is_empty() {
            return None;
        }
        let input = in_use
            .chosen_input
            .clone()
            .or_else(|| self.default_input.clone())
            .map(|name| display_name(&name, self.windows));
        let output = in_use
            .chosen_output
            .clone()
            .or_else(|| self.default_output.clone())
            .map(|name| display_name(&name, self.windows));
        if self.remembered.iter().any(|device| {
            device.display_name == "DisplayPort"
                || Some(&device.display_name) == input.as_ref()
                || Some(&device.display_name) == output.as_ref()
        }) {
            return None;
        }
        // The system default moving by itself (a headset plugged in) is not worth a question, unless the user picked
        // a device by hand.
        let input_ok = in_use.chosen_input.is_some() || !self.input_default_moved;
        let output_ok = in_use.chosen_output.is_some() || !self.output_default_moved;
        if !(input_ok && output_ok) {
            return None;
        }
        let first = self.remembered.first()?;
        if self.ignored.contains(&first.display_name) {
            return None;
        }
        let find = |list: &[(String, String)]| {
            list.iter()
                .find(|(shown, _)| *shown == first.display_name)
                .map(|(_, full)| full.clone())
        };
        let (input, output) = (find(&self.inputs), find(&self.outputs));
        // The device has to exist as the kind it is said to be.
        if input.is_none() && output.is_none() {
            return None;
        }
        Some(Suggestion {
            display_name: first.display_name.clone(),
            kind: first.kind,
            input,
            output,
        })
    }

    /// `CONNECTED_DEVICE_SWITCH`: the devices to use for `choice`; nothing is remembered after.
    pub fn switch(&mut self, display: &str, choice: Kind) -> (Option<String>, Option<String>) {
        let find = |list: &[(String, String)]| {
            list.iter()
                .find(|(shown, _)| shown == display)
                .map(|(_, full)| full.clone())
        };
        let use_input = matches!(choice, Kind::Input | Kind::InputAndOutput);
        let use_output = matches!(choice, Kind::Output | Kind::InputAndOutput);
        let result = (
            use_input.then(|| find(&self.inputs)).flatten(),
            use_output.then(|| find(&self.outputs)).flatten(),
        );
        self.remembered.clear();
        result
    }

    /// `CONNECTED_DEVICE_DONT_SWITCH`.
    pub fn dont_switch(&mut self) {
        self.remembered.clear();
    }

    /// `CONNECTED_DEVICE_IGNORE`: never suggest this device again.
    pub fn ignore(&mut self, display: &str) {
        self.ignored.insert(display.to_owned());
        self.remembered.clear();
    }

    /// `CONNECTED_DEVICE_NEVER_SHOW_MODAL`: turning it on forgets what was remembered.
    pub fn set_never_show(&mut self, never_show: bool) {
        if never_show {
            self.remembered.clear();
        }
        self.never_show = never_show;
    }
}

/// A thread that lists the devices every two seconds. Cheap to read.
pub struct DeviceWatch {
    latest: Arc<Mutex<DeviceList>>,
}

impl DeviceWatch {
    pub fn latest(&self) -> DeviceList {
        self.latest
            .lock()
            .map(|list| list.clone())
            .unwrap_or_default()
    }
}

/// How often the devices are listed.
pub const POLL: Duration = Duration::from_secs(2);

/// The process-wide watcher; the first call lists the devices and starts the thread.
pub fn watch() -> &'static DeviceWatch {
    static WATCH: OnceLock<DeviceWatch> = OnceLock::new();
    WATCH.get_or_init(|| {
        let latest = Arc::new(Mutex::new(DeviceList::read()));
        let shared = latest.clone();
        let _ = std::thread::Builder::new()
            .name("dc-voice-devices".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(POLL);
                    let list = DeviceList::read();
                    if let Ok(mut slot) = shared.lock() {
                        *slot = list;
                    }
                }
            });
        DeviceWatch { latest }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(inputs: &[&str], outputs: &[&str], default_in: &str, default_out: &str) -> DeviceList {
        DeviceList {
            inputs: inputs.iter().map(|name| (*name).to_owned()).collect(),
            outputs: outputs.iter().map(|name| (*name).to_owned()).collect(),
            default_input: Some(default_in.to_owned()),
            default_output: Some(default_out.to_owned()),
        }
    }

    fn calling() -> InUse {
        InUse {
            connected: true,
            ..InUse::default()
        }
    }

    /// A store that has seen a desk microphone and speakers.
    fn started() -> ConnectedDevices {
        let mut store = ConnectedDevices::new(true);
        store.update(
            &list(
                &["Microphone (Desk)"],
                &["Speakers (Desk)"],
                "Microphone (Desk)",
                "Speakers (Desk)",
            ),
            true,
        );
        store
    }

    #[test]
    fn windows_names_show_what_is_in_the_last_parentheses() {
        assert_eq!(display_name("Speakers (USB Audio)", true), "USB Audio");
        assert_eq!(
            display_name("Headset (Realtek(R) Audio)", true),
            "Realtek(R) Audio"
        );
        assert_eq!(display_name("Speakers (USB Audio)  ", true), "USB Audio");
        assert_eq!(display_name("Plain", true), "Plain");
        assert_eq!(display_name("Empty ()", true), "Empty ()");
        assert_eq!(
            display_name("Speakers (USB Audio)", false),
            "Speakers (USB Audio)"
        );
    }

    #[test]
    fn the_first_list_only_records() {
        let mut store = ConnectedDevices::new(true);
        let first = list(&["Microphone (A)"], &[], "Microphone (A)", "x");
        assert!(!store.update(&first, true));
        assert_eq!(store.suggestion(&calling()), None);
    }

    #[test]
    fn a_new_microphone_during_a_call_is_suggested() {
        let mut store = started();
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        assert!(store.update(&plugged, true));
        let suggestion = store.suggestion(&calling()).unwrap();
        assert_eq!(suggestion.display_name, "Headset");
        assert_eq!(suggestion.kind, Kind::Input);
        assert_eq!(suggestion.input.as_deref(), Some("Microphone (Headset)"));
        assert_eq!(suggestion.output, None);
    }

    #[test]
    fn a_headset_with_both_is_one_input_and_output_device() {
        let mut store = started();
        let plugged = list(
            &["Microphone (Desk)", "Headset Microphone (Headset)"],
            &["Speakers (Desk)", "Headphones (Headset)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        store.update(&plugged, true);
        let suggestion = store.suggestion(&calling()).unwrap();
        assert_eq!(suggestion.kind, Kind::InputAndOutput);
        let (input, output) = store.switch("Headset", Kind::InputAndOutput);
        assert_eq!(input.as_deref(), Some("Headset Microphone (Headset)"));
        assert_eq!(output.as_deref(), Some("Headphones (Headset)"));
        assert_eq!(store.suggestion(&calling()), None, "switching clears it");
    }

    #[test]
    fn nothing_is_remembered_outside_a_call() {
        let mut store = started();
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        store.update(&plugged, false);
        assert_eq!(store.suggestion(&calling()), None);
        assert_eq!(store.suggestion(&InUse::default()), None);
    }

    #[test]
    fn a_device_going_away_clears_the_suggestion() {
        let mut store = started();
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        store.update(&plugged, true);
        assert!(store.suggestion(&calling()).is_some());
        store.update(
            &list(
                &["Microphone (Desk)"],
                &["Speakers (Desk)"],
                "Microphone (Desk)",
                "Speakers (Desk)",
            ),
            true,
        );
        assert_eq!(store.suggestion(&calling()), None);
    }

    #[test]
    fn the_device_already_in_use_is_not_suggested() {
        let mut store = started();
        // The headset becomes the default, and is also what is used.
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Headset)",
            "Speakers (Desk)",
        );
        store.update(&plugged, true);
        assert_eq!(store.suggestion(&calling()), None);
        // Picked by hand and in use.
        let mut store = started();
        store.update(&plugged, true);
        let picked = InUse {
            chosen_input: Some("Microphone (Headset)".into()),
            connected: true,
            ..InUse::default()
        };
        assert_eq!(store.suggestion(&picked), None);
    }

    #[test]
    fn a_default_that_moved_by_itself_is_not_worth_asking() {
        let mut store = started();
        // The new microphone is not used, but the system made the new one the default in the same list.
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Headset)",
            "Speakers (Desk)",
        );
        store.update(&plugged, true);
        let picked_desk = InUse {
            chosen_input: Some("Microphone (Desk)".into()),
            connected: true,
            ..InUse::default()
        };
        // The user picked the desk microphone by hand: still asked.
        assert!(store.suggestion(&picked_desk).is_some());
        // Following the default, the default having just moved: not asked (and it is the one in use anyway).
        assert_eq!(store.suggestion(&calling()), None);
        // The next list, the default steady: asked about nothing new, and the memory is still there.
        store.update(&plugged, true);
        assert_eq!(
            store.suggestion(&calling()),
            None,
            "headset is the default in use"
        );
    }

    #[test]
    fn displayport_is_never_suggested() {
        let mut store = started();
        let plugged = list(
            &["Microphone (Desk)"],
            &["Speakers (Desk)", "Monitor (DisplayPort)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        store.update(&plugged, true);
        assert_eq!(store.suggestion(&calling()), None);
    }

    #[test]
    fn ignoring_and_never_showing() {
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        let mut store = started();
        store.update(&plugged, true);
        store.ignore("Headset");
        assert_eq!(store.suggestion(&calling()), None);
        assert!(store.ignored().contains("Headset"));
        // It comes back later (unplugged and plugged again) but stays ignored.
        store.update(
            &list(
                &["Microphone (Desk)"],
                &["Speakers (Desk)"],
                "Microphone (Desk)",
                "Speakers (Desk)",
            ),
            true,
        );
        store.update(&plugged, true);
        assert_eq!(store.suggestion(&calling()), None);

        let mut store = started();
        store.set_never_show(true);
        store.update(&plugged, true);
        assert_eq!(store.suggestion(&calling()), None);
        store.set_never_show(false);
        store.update(
            &list(
                &["Microphone (Desk)"],
                &["Speakers (Desk)"],
                "Microphone (Desk)",
                "Speakers (Desk)",
            ),
            true,
        );
        store.update(&plugged, true);
        assert!(store.suggestion(&calling()).is_some());
    }

    #[test]
    fn not_switching_and_the_end_of_the_call_forget_it() {
        let plugged = list(
            &["Microphone (Desk)", "Microphone (Headset)"],
            &["Speakers (Desk)"],
            "Microphone (Desk)",
            "Speakers (Desk)",
        );
        let mut store = started();
        store.update(&plugged, true);
        store.dont_switch();
        assert_eq!(store.suggestion(&calling()), None);
        let mut store = started();
        store.update(&plugged, true);
        store.disconnected();
        assert_eq!(store.suggestion(&calling()), None);
    }
}
