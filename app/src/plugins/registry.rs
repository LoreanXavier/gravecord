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
        "AlwaysAnimate",
        "statusEmojis",
        "the app draws every custom status emoji as a still picture (the pinned client's member list plays an \
         animated one), so there is nothing playing to force",
    ),
    (
        "AlwaysAnimate",
        "serverBanners",
        "the channel list header does not draw the server's banner yet, so there is no banner to play",
    ),
    (
        "AlwaysTrust",
        "file",
        "the app does not show the pinned client's \"Potentially Dangerous Download\" dialog yet (module 432518, opened from module 889558), so there is no dialog to remove",
    ),
    (
        "GameActivityToggle",
        "location",
        "the original only reads it while EquicordToolbox is on, which is not ported; the button stays next to \
         Mute/Deafen as with the default",
    ),
    (
        "CallTimer",
        "watchLargeGuilds",
        "the original subscribes to every server's events for it (`GUILD_SUBSCRIPTIONS_FLUSH`); plugin code sends \
         nothing, so times come from the voice states the app already receives",
    ),
    (
        "ShowConnections",
        "showGithubOrgs",
        "the original asks api.github.com for the organisations of a GitHub account; plugin code sends nothing, so \
         only the accounts Discord lists show",
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
        id: "AccountPanelServerProfile",
        name: "AccountPanelServerProfile",
        description: "Right click your account panel in the bottom left to view your profile in the current server",
        origin: Origin::Vencord,
        source: "src/plugins/accountPanelServerProfile/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "prioritizeServerProfile",
            label: "Prioritize Server Profile",
            description: "Prioritize Server Profile when left clicking your account panel",
            kind: Kind::Switch { default: false },
        }],
    },
    Def {
        id: "AlwaysAnimate",
        name: "AlwaysAnimate",
        description: "Animates anything that can be animated",
        origin: Origin::Vencord,
        source: "src/plugins/alwaysAnimate/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "icons",
                label: "Icons",
                description: "Always animate server icons, avatars, decor and more",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "statusEmojis",
                label: "Status emojis",
                description: "Always animate status emojis",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "serverBanners",
                label: "Server banners",
                description: "Always animate server banners",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "nameplates",
                label: "Nameplates",
                description: "Always animate nameplates",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "roleGradients",
                label: "Role gradients",
                description: "Always animate role gradients",
                kind: Kind::Switch { default: true },
            },
        ],
    },
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
        id: "BetterRoleContext",
        name: "BetterRoleContext",
        description: "Adds options to copy role color / edit role / view role icon when right clicking roles in the user profile or in the member list",
        origin: Origin::Vencord,
        source: "src/plugins/betterRoleContext/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "roleIconFileFormat",
            label: "Role Icon File Format",
            description: "File format to use when viewing role icons",
            kind: Kind::Select {
                options: &[
                    ("png", Lit::Str("png")),
                    ("webp", Lit::Str("webp")),
                    ("jpg", Lit::Str("jpg")),
                ],
                default: 0,
            },
        }],
    },
    Def {
        id: "BetterRoleDot",
        name: "BetterRoleDot",
        description: "Copy role colour on RoleDot (accessibility setting) click. Also allows using both RoleDot and                       coloured names simultaneously",
        origin: Origin::Vencord,
        source: "src/plugins/betterRoleDot/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "bothStyles",
                label: "Both styles",
                description: "Show both role dot and coloured names",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "copyRoleColorInProfilePopout",
                label: "Copy in profile popout",
                description: "Allow click on role dot in profile popout to copy role color",
                kind: Kind::Switch { default: false },
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
        id: "BiggerStreamPreview",
        name: "BiggerStreamPreview",
        description: "This plugin allows you to enlarge stream previews",
        origin: Origin::Vencord,
        source: "src/plugins/biggerStreamPreview/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "CallTimer",
        name: "CallTimer",
        description: "Add call timers for all users in voice channels and in the connection status.",
        origin: Origin::Vencord,
        source: "src/plugins/callTimer/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "format",
                label: "Format",
                description: "Compact or human readable format.",
                kind: Kind::Select {
                    options: &[
                        ("30:23:00:42", Lit::Str("stopwatch")),
                        ("30d 23h 00m 42s", Lit::Str("human")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "allCallTimers",
                label: "Everyone's timer",
                description: "Add call timer to all users in a server voice channel.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "showWithoutHover",
                label: "Show without hovering",
                description: "Always show the timer without needing to hover.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "showRoleColor",
                label: "Role colour",
                description: "Show the user's role color.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "trackSelf",
                label: "Track yourself",
                description: "Also track yourself.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "showSeconds",
                label: "Show seconds",
                description: "Show seconds in the timer.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "watchLargeGuilds",
                label: "Watch large servers",
                description: "Track users in large servers.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "CancelFriendRequest",
        name: "CancelFriendRequest",
        description: "Adds a way to cancel outgoing friend requests from profiles.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/pendingFriendRequest/index.tsx",
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
        id: "DownloadAllAttachments",
        name: "DownloadAllAttachments",
        description: "Adds a popover button to download all attachments in a message at once.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/downloadAllAttachments/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "downloadAllFileTypes",
            label: "All file types",
            description: "Also download non-media attachments. Only enable this if you trust what people send you.",
            kind: Kind::Switch { default: false },
        }],
    },
    Def {
        id: "FavoriteEmojiFirst",
        name: "FavoriteEmojiFirst",
        description: "Puts your favorite emoji first in the emoji autocomplete and also has emoji alias.",
        origin: Origin::Vencord,
        source: "src/plugins/favEmojiFirst/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "FixImagesQuality",
        name: "FixImagesQuality",
        description: "Improves quality of images by loading them at their original resolution.",
        origin: Origin::Vencord,
        source: "src/plugins/fixImagesQuality/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "originalImagesInChat",
            label: "Original images in chat",
            description: "Also load the original image in chat. Animated images then always play, and it may be slow.",
            kind: Kind::Switch { default: false },
        }],
    },
    Def {
        id: "FullVCPFP",
        name: "FullVCPFP",
        description: "Makes avatars take up the entire vc tile",
        origin: Origin::Equicord,
        source: "src/equicordplugins/fullVcPfp/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "GameActivityToggle",
        name: "GameActivityToggle",
        description: "Adds a button next to the mic and deafen button to toggle game activity. Right click it to \
                      toggle Spotify activity.",
        origin: Origin::Vencord,
        source: "src/plugins/gameActivityToggle/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "oldIcon",
                label: "Old icon",
                description: "Use the old icon style before Discord icon redesign",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "location",
                label: "Location",
                description: "Where to show the game activity toggle button",
                kind: Kind::Select {
                    options: &[
                        ("Next to Mute/Deafen", Lit::Str("PANEL")),
                        ("Equicord Toolbox", Lit::Str("TOOLBOX")),
                    ],
                    default: 0,
                },
            },
        ],
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
        id: "IgnoreActivities",
        name: "IgnoreActivities",
        description: "Ignore activities from showing up on your status ONLY. You can configure which ones are specifically ignored from the Registered Games and Activities tabs, or use the general settings below",
        origin: Origin::Vencord,
        source: "src/plugins/ignoreActivities/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "listMode",
                label: "List Mode",
                description: "Change the mode of the filter list",
                kind: Kind::Select {
                    options: &[("Whitelist", Lit::Int(0)), ("Blacklist", Lit::Int(1))],
                    default: 0,
                },
            },
            Setting {
                key: "idsList",
                label: "Filter List",
                description: "Comma separated list of activity IDs to filter (Useful for filtering specific RPC activities and CustomRPC",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "ignorePlaying",
                label: "Ignore Playing",
                description: "Ignore all playing activities (These are usually game and RPC activities)",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreStreaming",
                label: "Ignore Streaming",
                description: "Ignore all streaming activities",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreListening",
                label: "Ignore Listening",
                description: "Ignore all listening activities (These are usually Spotify activities)",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreWatching",
                label: "Ignore Watching",
                description: "Ignore all watching activities",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreCompeting",
                label: "Ignore Competing",
                description: "Ignore all competing activities (These are normally special game activities)",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "MemberCount",
        name: "MemberCount",
        description: "Shows the number of online members, total members, and users in voice channels on the server - in the member list and tooltip.",
        origin: Origin::Vencord,
        source: "src/plugins/memberCount/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "toolTip",
                label: "In the server tooltip",
                description: "Show member count on the server tooltip.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "memberList",
                label: "In the member list",
                description: "Show member count in the member list.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "voiceActivity",
                label: "Voice activity",
                description: "Show voice activity with member count in the member list.",
                kind: Kind::Switch { default: true },
            },
        ],
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
        id: "MessagePeek",
        name: "MessagePeek",
        description: "Shows the last message preview and timestamp in the Direct Messages list.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/messagePeek/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "hideMuted",
            label: "Hide muted",
            description: "Hide message previews and timestamps for muted DMs and group chats.",
            kind: Kind::Switch { default: false },
        }],
    },
    Def {
        id: "MuteChannelMentions",
        name: "MuteChannelMentions",
        description: "Hides mention badges from the channels you pick, both on the channel and on the server icon.                       Nothing is sent to Discord.",
        origin: Origin::Equicord,
        source: "src/userplugins/muteChannelMentions/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "channelIds",
                label: "Channel IDs",
                description: "Comma separated channel IDs whose mention badges are hidden.",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "onlyEveryone",
                label: "Only @everyone",
                description: "Only hide @everyone, @here and role pings. Direct mentions of you still show.",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "NoNitroUpsell",
        name: "NoNitroUpsell",
        description: "Removes all of Discord's nitro upsells by tricking the client into thinking you have nitro.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/noNitroUpsell/index.ts",
        default_on: false,
        settings: &[],
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
        id: "OnePingPerDM",
        name: "OnePingPerDM",
        description: "If unread messages are sent by a user in DMs multiple times, you'll only receive one audio ping. Read the messages to reset the limit.",
        origin: Origin::Vencord,
        source: "src/plugins/onePingPerDM/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "channelToAffect",
                label: "DMs affected",
                description: "Select the type of DM for the plugin to affect.",
                kind: Kind::Select {
                    options: &[
                        ("Both", Lit::Str("both_dms")),
                        ("User DMs", Lit::Str("user_dm")),
                        ("Group DMs", Lit::Str("group_dm")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "allowMentions",
                label: "Ping for mentions",
                description: "Receive audio pings for @mentions.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "allowEveryone",
                label: "Ping for everyone",
                description: "Receive audio pings for @everyone and @here in group DMs.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreUsers",
                label: "Never throttled",
                description: "User IDs (comma + space) whose pings should never be throttled.",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "alwaysPlaySound",
                label: "Always play the sound",
                description: "Play the message notification sound even when it is disabled.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "PlatformIndicators",
        name: "PlatformIndicators",
        description: "Adds platform indicators (Desktop, Mobile, Web...) to users.",
        origin: Origin::Equicord,
        source: "src/plugins/platformIndicators/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "list",
                label: "Member list",
                description: "Show indicators in the member list.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "profiles",
                label: "Profiles",
                description: "Show indicators in user profiles.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "messages",
                label: "Messages",
                description: "Show indicators inside messages.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "colorMobileIndicator",
                label: "Coloured mobile indicator",
                description: "Whether to make the mobile indicator match the color of the user status.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showBots",
                label: "Bots",
                description: "Whether to show platform indicators on bots.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ConsoleIcon",
                label: "Console icon",
                description: "What console icon to use.",
                kind: Kind::Select {
                    options: &[
                        ("Equicord", Lit::Str("equicord")),
                        ("Suncord", Lit::Str("suncord")),
                        ("Vencord", Lit::Str("vencord")),
                    ],
                    default: 0,
                },
            },
        ],
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
        id: "ReadAllNotificationsButton",
        name: "ReadAllNotificationsButton",
        description: "Read all server notifications with a single button click!",
        origin: Origin::Vencord,
        source: "src/plugins/readAllNotificationsButton/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "compact",
            label: "Compact",
            description: "Show a small text button instead of a server list icon.",
            kind: Kind::Switch { default: false },
        }],
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
        id: "RoleColorEverywhere",
        name: "RoleColorEverywhere",
        description: "Adds the top role color anywhere possible",
        origin: Origin::Vencord,
        source: "src/plugins/roleColorEverywhere/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "chatMentions",
                label: "Chat Mentions",
                description: "Show role colors in chat mentions (including in the message box)",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "memberList",
                label: "Member List",
                description: "Show role colors in member list role headers",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "voiceUsers",
                label: "Voice Users",
                description: "Show role colors in the voice chat user list",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "reactorsList",
                label: "Reactors List",
                description: "Show role colors in the reactors list",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "pollResults",
                label: "Poll Results",
                description: "Show role colors in the poll results",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "colorChatMessages",
                label: "Color Chat Messages",
                description: "Color chat messages based on the author's role color",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "messageSaturation",
                label: "Message Saturation",
                description: "Intensity of message coloring.",
                kind: Kind::Number {
                    default: 30.,
                    min: 0.,
                    max: 100.,
                    step: 10.,
                },
            },
        ],
    },
    Def {
        id: "ServerInfo",
        name: "ServerInfo",
        description: "Allows you to view info about a server",
        origin: Origin::Vencord,
        source: "src/plugins/serverInfo/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "sorting",
            label: "Sorting",
            description: "Username or if applicable Display Name",
            kind: Kind::Select {
                options: &[
                    ("Username", Lit::Str("username")),
                    ("Display Name", Lit::Str("displayname")),
                    ("Dont Sort", Lit::Str("none")),
                ],
                default: 1,
            },
        }],
    },
    Def {
        id: "ServerListIndicators",
        name: "ServerListIndicators",
        description: "Add online friend count or server count in the server list.",
        origin: Origin::Vencord,
        source: "src/plugins/serverListIndicators/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "mode",
            label: "Mode",
            description: "What the item counts.",
            kind: Kind::Select {
                options: &[
                    ("Only online friend count", Lit::Int(2)),
                    ("Only server count", Lit::Int(1)),
                    ("Both server and online friend counts", Lit::Int(3)),
                ],
                default: 0,
            },
        }],
    },
    Def {
        id: "ShowConnections",
        name: "ShowConnections",
        description: "Show connected accounts in user popouts and the DM sidebar.",
        origin: Origin::Vencord,
        source: "src/plugins/showConnections/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "iconSize",
                label: "Icon size (px)",
                description: "Icon size in pixels.",
                kind: Kind::Number {
                    default: 32.,
                    min: 1.,
                    max: 128.,
                    step: 1.,
                },
            },
            Setting {
                key: "iconSpacing",
                label: "Icon margin",
                description: "Space between the icons.",
                kind: Kind::Select {
                    options: &[
                        ("Compact", Lit::Int(0)),
                        ("Cozy", Lit::Int(1)),
                        ("Roomy", Lit::Int(2)),
                    ],
                    default: 1,
                },
            },
            Setting {
                key: "showGithubOrgs",
                label: "GitHub organizations",
                description: "Show GitHub organizations the user is a member of.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showOnModal",
                label: "In the full profile",
                description: "Show connected accounts in the full-size profile modal.",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "TypingIndicator",
        name: "TypingIndicator",
        description: "Adds an indicator if someone is typing on a channel.",
        origin: Origin::Vencord,
        source: "src/plugins/typingIndicator/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "includeCurrentChannel",
                label: "Current channel",
                description: "Whether to show the typing indicator for the currently selected channel.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "includeMutedChannels",
                label: "Muted channels",
                description: "Whether to show the typing indicator for muted channels.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "includeIgnoredUsers",
                label: "Ignored users",
                description: "Whether to show the typing indicator for ignored users.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "includeBlockedUsers",
                label: "Blocked users",
                description: "Whether to show the typing indicator for blocked users.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "indicatorMode",
                label: "Indicator",
                description: "How should the indicator be displayed?",
                kind: Kind::Select {
                    options: &[
                        ("Avatars and animated dots", Lit::Int(3)),
                        ("Animated dots", Lit::Int(1)),
                        ("Avatars", Lit::Int(2)),
                    ],
                    default: 0,
                },
            },
        ],
    },
    Def {
        id: "TypingTweaks",
        name: "TypingTweaks",
        description: "Show avatars and role colours in the typing indicator",
        origin: Origin::Vencord,
        source: "src/plugins/typingTweaks/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "showAvatars",
                label: "Avatars",
                description: "Show avatars in the typing indicator",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showRoleColors",
                label: "Role colours",
                description: "Show role colors in the typing indicator",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "alternativeFormatting",
                label: "Alternative line",
                description: "Show a more useful message when several users are typing",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "amITyping",
                label: "Am I typing",
                description: "Shows you if other people can see you typing",
                kind: Kind::Switch { default: false },
            },
        ],
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
        id: "UserMessagesPronouns",
        name: "UserMessagesPronouns",
        description: "Adds pronouns to chat user messages",
        origin: Origin::Vencord,
        source: "src/plugins/userMessagesPronouns/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "pronounsFormat",
                label: "Pronouns format",
                description: "The format for pronouns to appear in chat",
                kind: Kind::Select {
                    options: &[
                        ("Lowercase", Lit::Str("LOWERCASE")),
                        ("Capitalized", Lit::Str("CAPITALIZED")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "showSelf",
                label: "Show yourself",
                description: "Enable or disable showing pronouns for yourself",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "ValidUser",
        name: "ValidUser",
        description: "Fix mentions for unknown users showing up as '@unknown-user' (hover over a mention to fix it)",
        origin: Origin::Vencord,
        source: "src/plugins/validUser/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "ViewIcons",
        name: "ViewIcons",
        description: "Makes avatars and banners in user profiles clickable, adds View Icon/Banner/Avatar Decoration entries in the user, server and group channel context menu.",
        origin: Origin::Vencord,
        source: "src/plugins/viewIcons/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "format",
                label: "Format",
                description: "Choose the image format to use for non-animated images. Animated images will always use .webp",
                kind: Kind::Select {
                    options: &[
                        ("webp", Lit::Str("webp")),
                        ("png", Lit::Str("png")),
                        ("jpg", Lit::Str("jpg")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "imgSize",
                label: "Image Size",
                description: "The image size to use",
                kind: Kind::Select {
                    options: &[
                        ("128", Lit::Str("128")),
                        ("256", Lit::Str("256")),
                        ("512", Lit::Str("512")),
                        ("1024", Lit::Str("1024")),
                        ("2048", Lit::Str("2048")),
                        ("4096", Lit::Str("4096")),
                    ],
                    default: 3,
                },
            },
        ],
    },
    Def {
        id: "VoiceDownload",
        name: "VoiceDownload",
        description: "Adds a download to voice messages. (Opens a new browser tab)",
        origin: Origin::Vencord,
        source: "src/plugins/voiceDownload/index.tsx",
        default_on: false,
        settings: &[],
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
    Def {
        id: "WhoReacted",
        name: "WhoReacted",
        description: "Renders the avatars of users who reacted to a message",
        origin: Origin::Vencord,
        source: "src/plugins/whoReacted/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "clickableAvatars",
            label: "Clickable avatars",
            description: "While this is enabled, clicking a reacting user's avatar will open their profile instead of adding the reaction",
            kind: Kind::Switch { default: true },
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
