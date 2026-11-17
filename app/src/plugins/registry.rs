//! Every built-in plugin and its settings. The ids and setting keys are the original plugins' own, so an Equicord
//! backup imports as it is (`backup.rs`); labels and descriptions are ours.

use serde_json::Value;

/// A constant setting value: what a select option or a default holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(&'static str),
}

impl Lit {
    pub(crate) fn to_value(self) -> Value {
        match self {
            Lit::Str(text) => Value::from(text),
        }
    }

    pub(crate) fn matches(self, value: &Value) -> bool {
        match self {
            Lit::Str(text) => value.as_str() == Some(text),
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

const MESSAGE_ACTIONS_OWN: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Edit", Lit::Str("EDIT")),
    ("Reply", Lit::Str("REPLY")),
    ("Copy Text", Lit::Str("COPY_CONTENT")),
    ("Copy Message Link", Lit::Str("COPY_LINK")),
    ("Copy Message ID", Lit::Str("COPY_ID")),
    ("Copy User ID", Lit::Str("COPY_USER_ID")),
];

const MESSAGE_ACTIONS_OTHERS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Reply", Lit::Str("REPLY")),
    ("Copy Text", Lit::Str("COPY_CONTENT")),
    ("Copy Message Link", Lit::Str("COPY_LINK")),
    ("Copy Message ID", Lit::Str("COPY_ID")),
    ("Copy User ID", Lit::Str("COPY_USER_ID")),
];

const MODIFIERS: &[(&str, Lit)] = &[
    ("None", Lit::Str("NONE")),
    ("Shift", Lit::Str("SHIFT")),
    ("Ctrl", Lit::Str("CTRL")),
    ("Alt", Lit::Str("ALT")),
];

/// Every plugin, in the order the Plugins page lists them (by name).
pub(crate) const DEFS: &[Def] = &[
    Def {
        id: "AlwaysTrust",
        name: "AlwaysTrust",
        description: "Opens links without the \"Leaving Discord\" question.",
        origin: Origin::Vencord,
        source: "src/plugins/alwaysTrust/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "domain",
            label: "Trust every link",
            description: "Skip the question for any web address.",
            kind: Kind::Switch { default: true },
        }],
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
        description: "Double-click a message to edit it, reply to it, or copy it.",
        origin: Origin::Vencord,
        source: "src/plugins/messageClickActions/index.ts",
        // Double-click to edit was always on before plugins existed; it stays on until turned off.
        default_on: true,
        settings: &[
            Setting {
                key: "doubleClickAction",
                label: "Double-click on your messages",
                description: "What a double-click on one of your own messages does.",
                kind: Kind::Select {
                    options: MESSAGE_ACTIONS_OWN,
                    default: 1,
                },
            },
            Setting {
                key: "doubleClickOthersAction",
                label: "Double-click on other people's messages",
                description: "What a double-click on someone else's message does.",
                kind: Kind::Select {
                    options: MESSAGE_ACTIONS_OTHERS,
                    default: 1,
                },
            },
            Setting {
                key: "doubleClickModifier",
                label: "Key to hold",
                description: "The double-click only acts while this key is held.",
                kind: Kind::Select {
                    options: MODIFIERS,
                    default: 0,
                },
            },
            Setting {
                key: "disableInDms",
                label: "Off in direct messages",
                description: "Double-clicks do nothing in DMs and group DMs.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "NoPendingCount",
        name: "NoPendingCount",
        description: "Hides the count badges of friend requests and message requests.",
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
        id: "RevealAllSpoilers",
        name: "RevealAllSpoilers",
        description: "Ctrl-click a spoiler to open every spoiler in its message, Ctrl+Shift-click for the whole channel.",
        origin: Origin::Vencord,
        source: "src/plugins/revealAllSpoilers/index.ts",
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
        id: "VolumeBooster",
        name: "VolumeBooster",
        description: "Lets a person's volume go past 200%.",
        origin: Origin::Vencord,
        source: "src/plugins/volumeBooster/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "multiplier",
            label: "Volume multiplier",
            description: "The User Volume slider goes up to 200% times this.",
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
        assert_eq!(select.normalize(&Value::from("DELETE")), None);
    }
}
