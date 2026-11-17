//! Every built-in plugin and its settings. The ids and setting keys are the original plugins' own, so an Equicord
//! backup imports as it is (`backup.rs`); labels and descriptions are ours.

use serde_json::Value;

/// A constant setting value: what a select option or a default holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(&'static str),
    /// A number option (the original's enum values, as AnonymiseFileNames' `method`).
    Int(i64),
}

impl Lit {
    pub(crate) fn to_value(self) -> Value {
        match self {
            Lit::Str(text) => Value::from(text),
            Lit::Int(number) => Value::from(number),
        }
    }

    pub(crate) fn matches(self, value: &Value) -> bool {
        match self {
            Lit::Str(text) => value.as_str() == Some(text),
            Lit::Int(number) => value.as_f64() == Some(number as f64),
        }
    }
}

#[derive(Debug)]
pub(crate) enum Kind {
    Switch {
        default: bool,
    },
    /// A number between `min` and `max`, moved in `step`s.
    Number {
        default: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    /// One of `options`: the label shown and the value stored.
    Select {
        options: &'static [(&'static str, Lit)],
        default: usize,
    },
    /// Free text of at most `max_len` characters.
    Text {
        default: &'static str,
        max_len: usize,
    },
}

#[derive(Debug)]
pub(crate) struct Setting {
    /// The original plugin's settings key.
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: Kind,
}

impl Setting {
    pub(crate) fn default_value(&self) -> Value {
        match &self.kind {
            Kind::Switch { default } => Value::Bool(*default),
            Kind::Number { default, .. } => number_value(*default),
            Kind::Select { options, default } => options[*default].1.to_value(),
            Kind::Text { default, .. } => Value::from(*default),
        }
    }

    /// A valid value that is not the default (the other side of a switch, the far end of a range, another option):
    /// the plugins-off check turns every setting to it.
    #[cfg_attr(not(any(test, feature = "capture")), allow(dead_code))]
    pub(crate) fn scrambled(&self) -> Value {
        match &self.kind {
            Kind::Switch { default } => Value::Bool(!*default),
            Kind::Number {
                default, min, max, ..
            } => number_value(if *default == *max { *min } else { *max }),
            Kind::Select { options, default } => {
                let other = if *default + 1 < options.len() {
                    options.len() - 1
                } else {
                    0
                };
                options[other].1.to_value()
            }
            Kind::Text { default, .. } => Value::from(format!("{default}x")),
        }
    }

    /// The value as the setting keeps it: a number clamped into its range and put on its step, an option that
    /// exists. `None` when the value cannot be this setting's.
    pub(crate) fn normalize(&self, value: &Value) -> Option<Value> {
        match &self.kind {
            Kind::Switch { .. } => value.as_bool().map(Value::Bool),
            Kind::Number { min, max, step, .. } => {
                let number = value.as_f64().filter(|number| number.is_finite())?;
                let stepped = if *step > 0. {
                    min + ((number - min) / step).round() * step
                } else {
                    number
                };
                Some(number_value(stepped.clamp(*min, *max)))
            }
            Kind::Select { options, .. } => options
                .iter()
                .find(|(_, lit)| lit.matches(value))
                .map(|(_, lit)| lit.to_value()),
            Kind::Text { max_len, .. } => value
                .as_str()
                .map(|text| Value::from(text.chars().take(*max_len).collect::<String>())),
        }
    }
}

/// Whole numbers are kept as integers, as the original settings store them.
fn number_value(number: f64) -> Value {
    if number.fract() == 0. && number.abs() < 1e15 {
        Value::from(number as i64)
    } else {
        Value::from(number)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Vencord,
    Equicord,
}

impl Origin {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Origin::Vencord => "Vencord",
            Origin::Equicord => "Equicord",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Def {
    /// The original plugin's name, which is also its key in an Equicord backup.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub origin: Origin,
    /// Where the original lives in its repository.
    pub source: &'static str,
    /// On before the user ever touched its switch.
    pub default_on: bool,
    pub settings: &'static [Setting],
}

impl Def {
    pub(crate) fn setting(&self, key: &str) -> Option<&'static Setting> {
        self.settings.iter().find(|setting| setting.key == key)
    }
}

pub(crate) fn def(id: &str) -> Option<&'static Def> {
    DEFS.iter().find(|def| def.id == id)
}

/// `actions` of the original: single and triple clicks.
const CLICK_ACTIONS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Delete", Lit::Str("DELETE")),
    ("Copy Link", Lit::Str("COPY_LINK")),
    ("Copy ID", Lit::Str("COPY_ID")),
    ("Copy Content", Lit::Str("COPY_CONTENT")),
    ("Copy User ID", Lit::Str("COPY_USER_ID")),
    ("Edit", Lit::Str("EDIT")),
    ("Reply", Lit::Str("REPLY")),
    ("React", Lit::Str("REACT")),
    ("Open Thread", Lit::Str("OPEN_THREAD")),
    ("Open Tab", Lit::Str("OPEN_TAB")),
];

/// `doubleClickOwnActions`.
const DOUBLE_CLICK_OWN_ACTIONS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Delete", Lit::Str("DELETE")),
    ("Reply", Lit::Str("REPLY")),
    ("Edit", Lit::Str("EDIT")),
    ("Quote", Lit::Str("QUOTE")),
    ("Copy Content", Lit::Str("COPY_CONTENT")),
    ("Copy Link", Lit::Str("COPY_LINK")),
    ("Copy ID", Lit::Str("COPY_ID")),
    ("Copy User ID", Lit::Str("COPY_USER_ID")),
    ("React", Lit::Str("REACT")),
    ("Pin", Lit::Str("PIN")),
];

/// `doubleClickOthersActions`.
const DOUBLE_CLICK_OTHERS_ACTIONS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Delete", Lit::Str("DELETE")),
    ("Reply", Lit::Str("REPLY")),
    ("Quote", Lit::Str("QUOTE")),
    ("Copy Content", Lit::Str("COPY_CONTENT")),
    ("Copy Link", Lit::Str("COPY_LINK")),
    ("Copy ID", Lit::Str("COPY_ID")),
    ("Copy User ID", Lit::Str("COPY_USER_ID")),
    ("React", Lit::Str("REACT")),
    ("Pin", Lit::Str("PIN")),
];

/// `modifiers`.
const MODIFIERS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Shift", Lit::Str("SHIFT")),
    ("Ctrl", Lit::Str("CTRL")),
    ("Alt", Lit::Str("ALT")),
];

/// `singleClickModifiers`: Backspace and Delete first.
const SINGLE_CLICK_MODIFIERS: &[(&str, Lit)] = &[
    ("Backspace", Lit::Str("BACKSPACE")),
    ("Delete", Lit::Str("DELETE")),
    ("None", Lit::Str("NONE")),
    ("Shift", Lit::Str("SHIFT")),
    ("Ctrl", Lit::Str("CTRL")),
    ("Alt", Lit::Str("ALT")),
];

/// A time in milliseconds. The original types it into a box (`OptionType.NUMBER`); here it moves in 50 ms steps.
const fn millis(default: f64) -> Kind {
    Kind::Number {
        default,
        min: 0.,
        max: 5000.,
        step: 50.,
    }
}

/// Settings of the originals that have nothing to act on in the app yet, with why. Every other setting must be read
/// by the code (a test checks both ways), so a setting can never sit in the page doing nothing unnoticed.
pub(crate) const IDLE_SETTINGS: &[(&str, &str, &str)] = &[
    (
        "AlwaysTrust",
        "file",
        "the app does not show the pinned client's \"Potentially Dangerous Download\" dialog yet (module 432518, opened from module 889558), so there is no dialog to remove",
    ),
    (
        "NoPendingCount",
        "hidePremiumOffersCount",
        "the app's Home badge does not count unseen Nitro offers yet (`getUnacknowledgedOffers`), so there is no count to hide",
    ),
];

/// Settings shown only while a switch of the same plugin is on (`hidden` in the original).
pub(crate) const SHOWN_WHEN: &[(&str, &str, &str)] = &[(
    "MessageClickActions",
    "additionalReactEmojis",
    "addAdditionalReacts",
)];

/// Every plugin, in the order the Plugins page lists them (by name).
pub(crate) const DEFS: &[Def] = &[
    Def {
        id: "AlwaysTrust",
        name: "AlwaysTrust",
        description: "Removes the annoying untrusted domain and suspicious file popup.",
        origin: Origin::Vencord,
        source: "src/plugins/alwaysTrust/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "domain",
                label: "Trust every link",
                description: "Remove the untrusted domain popup when opening links.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "file",
                label: "Trust every download",
                description: "Remove the \"Potentially Dangerous Download\" popup when opening links.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "noDeleteSafety",
                label: "No server name to delete",
                description: "Removes the enter server name requirement when deleting a server.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "confirmModal",
                label: "Ask before deleting",
                description: "Show an \"are you sure\" dialog before deleting a server (with the setting above).",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "AnonymiseFileNames",
        name: "AnonymiseFileNames",
        description: "Anonymise uploaded file names.",
        origin: Origin::Vencord,
        source: "src/plugins/anonymiseFileNames/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "anonymiseByDefault",
                label: "Anonymise by default",
                description: "Whether to anonymise file names by default.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "spoilerMessages",
                label: "Spoiler files",
                description: "Mark anonymised files as spoilers.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "method",
                label: "Anonymising method",
                description: "How the new name is made.",
                kind: Kind::Select {
                    options: &[
                        ("Random Characters", Lit::Int(0)),
                        ("Consistent", Lit::Int(1)),
                        ("Timestamp", Lit::Int(2)),
                        ("Date", Lit::Int(3)),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "randomisedLength",
                label: "Random characters length",
                description: "How many random characters the name has (Random Characters).",
                kind: Kind::Number {
                    default: 7.,
                    min: 1.,
                    max: 100.,
                    step: 1.,
                },
            },
            Setting {
                key: "consistent",
                label: "Consistent filename",
                description: "The name every file gets (Consistent).",
                kind: Kind::Text {
                    default: "image",
                    max_len: 200,
                },
            },
            Setting {
                key: "dateFormat",
                label: "Date format",
                description: "YYYY, MM, DD, HH, mm, ss, SSS are supported (Date).",
                kind: Kind::Text {
                    default: "YYYY-MM-DD_HH-mm-ss-SSS",
                    max_len: 200,
                },
            },
        ],
    },
    Def {
        id: "BetterUploadButton",
        name: "BetterUploadButton",
        description: "Upload with a single click, open menu with right click.",
        origin: Origin::Vencord,
        source: "src/plugins/betterUploadButton/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "ClearURLs",
        name: "ClearURLs",
        description: "Removes tracking parameters from links in the messages you send and edit.",
        origin: Origin::Vencord,
        source: "src/plugins/clearURLs/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "ColorSighted",
        name: "ColorSighted",
        description: "Removes the colorblind-friendly icons from statuses, just like 2015-2017 Discord.",
        origin: Origin::Vencord,
        source: "src/plugins/colorSighted/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "CopyUserURLs",
        name: "CopyUserURLs",
        description: "Adds a 'Copy User URL' option to the user context menu.",
        origin: Origin::Vencord,
        source: "src/plugins/copyUserURLs/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "DisableCallIdle",
        name: "DisableCallIdle",
        description: "Stays in a DM call when you are alone in it, instead of leaving after three minutes.",
        origin: Origin::Vencord,
        source: "src/plugins/disableCallIdle/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "GifPaste",
        name: "GifPaste",
        description: "Picking a GIF puts its link in the message box instead of sending it.",
        origin: Origin::Vencord,
        source: "src/plugins/gifPaste/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "MessageClickActions",
        name: "MessageClickActions",
        description: "Customize click actions on messages: hold Backspace and click to delete, double-click to edit or reply, and more.",
        origin: Origin::Vencord,
        source: "src/plugins/messageClickActions/index.ts",
        // Double-click to edit was always on before plugins existed; it stays on until turned off.
        default_on: true,
        settings: &[
            Setting {
                key: "singleClickAction",
                label: "Single click (your messages)",
                description: "Action on single click on your own messages.",
                kind: Kind::Select {
                    options: CLICK_ACTIONS,
                    default: 1,
                },
            },
            Setting {
                key: "singleClickModifier",
                label: "Key for single click (your messages)",
                description: "Key that must be held for the single click action on your messages.",
                kind: Kind::Select {
                    options: SINGLE_CLICK_MODIFIERS,
                    default: 0,
                },
            },
            Setting {
                key: "singleClickOthersAction",
                label: "Single click (others' messages)",
                description: "Action on single click on other people's messages.",
                kind: Kind::Select {
                    options: CLICK_ACTIONS,
                    default: 1,
                },
            },
            Setting {
                key: "singleClickOthersModifier",
                label: "Key for single click (others' messages)",
                description: "Key that must be held for the single click action on other people's messages.",
                kind: Kind::Select {
                    options: SINGLE_CLICK_MODIFIERS,
                    default: 0,
                },
            },
            Setting {
                key: "doubleClickAction",
                label: "Double-click (your messages)",
                description: "Action on double-click on your own messages.",
                kind: Kind::Select {
                    options: DOUBLE_CLICK_OWN_ACTIONS,
                    default: 3,
                },
            },
            Setting {
                key: "doubleClickOthersAction",
                label: "Double-click (others' messages)",
                description: "Action on double-click on other people's messages.",
                kind: Kind::Select {
                    options: DOUBLE_CLICK_OTHERS_ACTIONS,
                    default: 2,
                },
            },
            Setting {
                key: "doubleClickModifier",
                label: "Key for double-click",
                description: "Key that must be held for the double-click action.",
                kind: Kind::Select {
                    options: MODIFIERS,
                    default: 0,
                },
            },
            Setting {
                key: "tripleClickAction",
                label: "Triple-click",
                description: "Action on triple-click (needs \"Delay double-click\" below).",
                kind: Kind::Select {
                    options: CLICK_ACTIONS,
                    default: 8,
                },
            },
            Setting {
                key: "tripleClickModifier",
                label: "Key for triple-click",
                description: "Key that must be held for the triple-click action.",
                kind: Kind::Select {
                    options: MODIFIERS,
                    default: 0,
                },
            },
            Setting {
                key: "reactEmoji",
                label: "Reaction emoji",
                description: "Emoji the React action adds: an emoji, or a custom one as name:id.",
                kind: Kind::Text {
                    default: "\u{1f480}",
                    max_len: 200,
                },
            },
            Setting {
                key: "addAdditionalReacts",
                label: "More reactions",
                description: "Also add the extra reaction emojis below.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "additionalReactEmojis",
                label: "Extra reaction emojis",
                description: "Up to 8 more emojis, separated by commas or new lines.",
                kind: Kind::Text {
                    default: "",
                    max_len: 2000,
                },
            },
            Setting {
                key: "disableInDms",
                label: "Off in direct messages",
                description: "Disable all click actions in direct messages.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "disableInSystemDms",
                label: "Off in system DMs",
                description: "Disable all click actions in direct messages from Discord itself.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "clickTimeout",
                label: "Click timeout (ms)",
                description: "Time that tells double and triple clicks apart from single ones.",
                kind: millis(300.),
            },
            Setting {
                key: "doubleClickHoldThreshold",
                label: "Double-click hold limit (ms)",
                description: "Longest hold of a double-click; holding longer selects text instead.",
                kind: millis(150.),
            },
            Setting {
                key: "deferDoubleClickForTriple",
                label: "Delay double-click",
                description: "Wait to allow triple-click actions (triple-click is off without this).",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "selectionHoldTimeout",
                label: "Selection hold (ms)",
                description: "A press held longer than this selects text and runs no action.",
                kind: millis(300.),
            },
            Setting {
                key: "quoteWithReply",
                label: "Quote replies",
                description: "When quoting, also reply to the message.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "useSelectionForQuote",
                label: "Quote the selection",
                description: "When quoting, use the selected text if there is one.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "NoPendingCount",
        name: "NoPendingCount",
        description: "Removes the ping count of incoming friend requests, message requests, and Nitro offers.",
        origin: Origin::Vencord,
        source: "src/plugins/noPendingCount/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "hideFriendRequestsCount",
                label: "Hide friend request count",
                description: "No number for incoming friend requests on Home and Friends.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "hideMessageRequestsCount",
                label: "Hide message request count",
                description: "No number on Message Requests; the row stays.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "hidePremiumOffersCount",
                label: "Hide Nitro offers count",
                description: "No number for unseen Nitro trials and discounts.",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "NoTypingAnimation",
        name: "NoTypingAnimation",
        description: "Shows the typing dots still instead of pulsing.",
        origin: Origin::Vencord,
        source: "src/plugins/noTypingAnimation/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "QuickMention",
        name: "QuickMention",
        description: "Adds a quick mention button to the message actions bar.",
        origin: Origin::Vencord,
        source: "src/plugins/quickMention/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "RevealAllSpoilers",
        name: "RevealAllSpoilers",
        description: "Ctrl-click a spoiler to open every spoiler in its message, Ctrl+Shift-click for the whole channel.",
        origin: Origin::Vencord,
        source: "src/plugins/revealAllSpoilers/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "ReverseImageSearch",
        name: "ReverseImageSearch",
        description: "Adds ImageSearch to image context menus.",
        origin: Origin::Vencord,
        source: "src/plugins/reverseImageSearch/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "UnlimitedAccounts",
        name: "UnlimitedAccounts",
        description: "Lets the account switcher hold more than five accounts.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/unlimitedAccounts/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "maxAccounts",
            label: "Most accounts",
            description: "How many accounts the switcher keeps; 0 is no limit.",
            kind: Kind::Number {
                default: 0.,
                min: 0.,
                max: 100.,
                step: 1.,
            },
        }],
    },
    Def {
        id: "VoiceStats",
        name: "VoiceStats",
        description: "Shows how long you've spent in voice with each user in their profile.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/voiceStats/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "VolumeBooster",
        name: "VolumeBooster",
        description: "Allows you to set the user and stream volume above the default maximum.",
        origin: Origin::Vencord,
        source: "src/plugins/volumeBooster/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "multiplier",
            label: "Volume multiplier",
            description: "The User Volume and Stream Volume sliders reach their usual top times this.",
            kind: Kind::Number {
                default: 2.,
                min: 1.,
                max: 5.,
                step: 0.5,
            },
        }],
    },
];

#[cfg(test)]
mod tests {
    /// A fresh install has the plugins' master switch on and exactly one plugin on: MessageClickActions, which
    /// the owner asked for (double-click to edit). Everything else starts as the pinned client.
    #[test]
    fn only_message_click_actions_is_on_by_default() {
        let on: Vec<&str> = DEFS
            .iter()
            .filter(|def| def.default_on)
            .map(|def| def.id)
            .collect();
        assert_eq!(on, ["MessageClickActions"]);
    }

    use super::*;

    #[test]
    fn ids_are_unique_and_listed_by_name() {
        let ids: Vec<&str> = DEFS.iter().map(|def| def.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_by_key(|id| id.to_ascii_lowercase());
        sorted.dedup();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn every_default_is_a_valid_value_of_its_setting() {
        for def in DEFS {
            for setting in def.settings {
                let default = setting.default_value();
                assert_eq!(
                    setting.normalize(&default).as_ref(),
                    Some(&default),
                    "{}.{}",
                    def.id,
                    setting.key
                );
            }
        }
    }

    #[test]
    fn numbers_are_clamped_and_stepped_and_options_must_exist() {
        let number = Setting {
            key: "n",
            label: "",
            description: "",
            kind: Kind::Number {
                default: 2.,
                min: 1.,
                max: 5.,
                step: 0.5,
            },
        };
        assert_eq!(number.normalize(&Value::from(9)), Some(Value::from(5)));
        assert_eq!(number.normalize(&Value::from(2.3)), Some(Value::from(2.5)));
        assert_eq!(number.normalize(&Value::from("2")), None);
        let select = def("MessageClickActions")
            .unwrap()
            .setting("doubleClickAction")
            .unwrap();
        assert_eq!(
            select.normalize(&Value::from("REPLY")),
            Some(Value::from("REPLY"))
        );
        assert_eq!(select.normalize(&Value::from("SPIN")), None);
    }
}
