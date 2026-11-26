//! Every built-in plugin and its settings. The ids and setting keys are the original plugins' own, so an Equicord
//! backup imports as it is (`backup.rs`); labels and descriptions are ours.

use serde_json::Value;

/// A constant setting value: what a select option or a default holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(&'static str),
    /// A number option (the original's enum values, as AnonymiseFileNames' `method`).
    Int(i64),
    /// A true/false option (USRBG's `nitroFirst`).
    Bool(bool),
}

impl Lit {
    pub(crate) fn to_value(self) -> Value {
        match self {
            Lit::Str(text) => Value::from(text),
            Lit::Int(number) => Value::from(number),
            Lit::Bool(on) => Value::from(on),
        }
    }

    pub(crate) fn matches(self, value: &Value) -> bool {
        match self {
            Lit::Str(text) => value.as_str() == Some(text),
            Lit::Int(number) => value.as_f64() == Some(number as f64),
            Lit::Bool(on) => value.as_bool() == Some(on),
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
        "SpotifyCrack",
        "keepSpotifyActivityOnIdle",
        "this client does not put your Spotify listening activity into your presence yet (shouldShowActivity's          place), so there is no idle check to lift",
    ),
    (
        "StreamingCodecDisabler",
        "disableAv1Codec",
        "this client offers H264 only in a voice connection's codec list, so there is no AV1 entry to take out",
    ),
    (
        "StreamingCodecDisabler",
        "disableH265Codec",
        "this client offers H264 only in a voice connection's codec list, so there is no H265 entry to take out",
    ),
    (
        "StreamingCodecDisabler",
        "disableVP8Codec",
        "this client offers H264 only in a voice connection's codec list, so there is no VP8 entry to take out",
    ),
    (
        "StreamingCodecDisabler",
        "disableVP9Codec",
        "this client offers H264 only in a voice connection's codec list, so there is no VP9 entry to take out",
    ),
    (
        "MoreCommands",
        "uwuEverything",
        "it uwuifies every text node of the interface (a patch of React's element creation); the app's text is drawn          by each view itself and has no single place to pass it through yet",
    ),
    (
        "ImageZoom",
        "nearestNeighbour",
        "the renderer samples pictures smoothly and has no pixelated mode yet, so the lens cannot switch to nearest \
         neighbour",
    ),
    (
        "BetterCommands",
        "autoFillArguments",
        "the app's command flow already asks every option in turn, optional ones too, so there is nothing more to fill",
    ),
    (
        "PinDMs",
        "friendsSidebarGroupDms",
        "it lists pinned group DMs in the friends sidebar's pinned sections, and the app has no friends sidebar yet",
    ),
    (
        "SuperReactionTweaks",
        "unlimitedSuperReactionPlaying",
        "it lifts the cap on super reaction effects playing at once, and the app plays no super reaction effects yet",
    ),
    (
        "SuperReactionTweaks",
        "superReactionPlayingLimit",
        "it caps the super reaction effects playing at once, and the app plays no super reaction effects yet",
    ),
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

/// Settings shown only while a switch of the same plugin is on (`hidden` in the original); an empty switch is never
/// on, so its setting is never listed (`hidden: true`).
pub(crate) const SHOWN_WHEN: &[(&str, &str, &str)] = &[
    // CustomRPC's private settings (`withPrivateSettings`): its own form edits them.
    ("CustomRPC", "appID", ""),
    ("CustomRPC", "appName", ""),
    ("CustomRPC", "details", ""),
    ("CustomRPC", "detailsURL", ""),
    ("CustomRPC", "state", ""),
    ("CustomRPC", "stateURL", ""),
    ("CustomRPC", "type", ""),
    ("CustomRPC", "streamLink", ""),
    ("CustomRPC", "timestampMode", ""),
    ("CustomRPC", "startTime", ""),
    ("CustomRPC", "endTime", ""),
    ("CustomRPC", "imageBig", ""),
    ("CustomRPC", "imageBigURL", ""),
    ("CustomRPC", "imageBigTooltip", ""),
    ("CustomRPC", "imageSmall", ""),
    ("CustomRPC", "imageSmallURL", ""),
    ("CustomRPC", "imageSmallTooltip", ""),
    ("CustomRPC", "buttonOneText", ""),
    ("CustomRPC", "buttonOneURL", ""),
    ("CustomRPC", "buttonTwoText", ""),
    ("CustomRPC", "buttonTwoURL", ""),
    ("CustomRPC", "partySize", ""),
    ("CustomRPC", "partyMaxSize", ""),
    // `hidden: () => !settings.store.resendOnFail`.
    ("BetterForwards", "forwardPreface", "resendOnFail"),
    (
        "MessageClickActions",
        "additionalReactEmojis",
        "addAdditionalReacts",
    ),
    // Folded by a click on the "Direct Messages" title.
    ("PinDMs", "dmSectionCollapsed", ""),
    // `hidden: !IS_DEV`: a development setting.
    ("UserPFP", "databaseSource", ""),
    // `hidden: true`: Quoter's dialog remembers its switches in these.
    ("Quoter", "grayscale", ""),
    ("Quoter", "showWatermark", ""),
    ("Quoter", "saveAsGif", ""),
];

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
        id: "AltKrispSwitch",
        name: "AltKrispSwitch",
        description: "Makes the Noise Suppression Popout switch between None and Krisp instead of Krisp and Standard",
        origin: Origin::Equicord,
        source: "src/equicordplugins/altKrispSwitch/index.ts",
        default_on: false,
        settings: &[],
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
        id: "BetterActivities",
        name: "BetterActivities",
        description: "Shows activity icons in the member list and allows showing all activities",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterActivities/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "memberList",
                label: "Member List",
                description: "Show activity icons in the member list",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "iconSize",
                label: "Icon Size",
                description: "Size of the activity icons",
                kind: Kind::Number {
                    default: 15.,
                    min: 10.,
                    max: 20.,
                    step: 1.,
                },
            },
            Setting {
                key: "specialFirst",
                label: "Special First",
                description: "Show special activities first (Currently Spotify and Twitch)",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "renderGifs",
                label: "Render Gifs",
                description: "Allow rendering GIFs",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "removeGameActivityStatus",
                label: "Remove Game Activity Status",
                description: "Remove game activity icon and status",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "userPopout",
                label: "User Popout",
                description: "Show all activities in the profile popout/sidebar",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "hideTooltip",
                label: "Hide Tooltip",
                description: "Hides activities in various places",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "allActivitiesStyle",
                label: "All Activities Style",
                description: "Style for showing all activities",
                kind: Kind::Select {
                    options: &[
                        ("Carousel", Lit::Str("carousel")),
                        ("List", Lit::Str("list")),
                    ],
                    default: 0,
                },
            },
        ],
    },
    Def {
        id: "BetterAudioPlayer",
        name: "BetterAudioPlayer",
        description: "Adds a spectrograph and oscilloscope visualizer to audio attachment players.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterAudioPlayer.desktop/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "oscilloscope",
                label: "Oscilloscope",
                description: "Enable oscilloscope visualizer.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "spectrograph",
                label: "Spectrograph",
                description: "Enable spectrograph visualizer.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "oscilloscopeSolidColor",
                label: "Oscilloscope Solid Color",
                description: "Use a solid color for the oscilloscope.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "oscilloscopeColor",
                label: "Oscilloscope Color",
                description: "Color for the oscilloscope (R, G, B or #hex).",
                kind: Kind::Text {
                    default: "255, 255, 255",
                    max_len: 32,
                },
            },
            Setting {
                key: "spectrographSolidColor",
                label: "Spectrograph Solid Color",
                description: "Use a solid color for the spectrograph.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "spectrographColor",
                label: "Spectrograph Color",
                description: "Color for the spectrograph (R, G, B or #hex).",
                kind: Kind::Text {
                    default: "33, 150, 243",
                    max_len: 32,
                },
            },
        ],
    },
    Def {
        id: "BetterBlockedUsers",
        name: "BetterBlockedUsers",
        description: "Allows you to search in blocked users list, make names selectable in settings, skips blocked profile prompt, and groups blocked / ignored messages.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterBlockedUsers/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "BetterCommands",
        name: "BetterCommands",
        description: "Enhances the command system with miscellaneous improvements.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterCommands/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "autoFillArguments",
            label: "Auto Fill Arguments",
            description: "Automatically fill command with all arguments instead of just required ones.",
            kind: Kind::Switch { default: true },
        }],
    },
    Def {
        id: "BetterForwards",
        name: "BetterForwards",
        description: "Message forward utilities including NSFW bypass and UI improvements.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterForwards/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "resendOnFail",
                label: "Resend On Fail",
                description: "This will attempt to resend a forwarded message if the forward fails. Could cause unintentional pings or text spam. Bypasses NSFW restrictions.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "forwardPreface",
                label: "Forward Preface",
                description: "What should the forwarded message be prefaced with",
                kind: Kind::Select {
                    options: &[(">", Lit::Str(">")), ("-#", Lit::Str("-#"))],
                    default: 0,
                },
            },
            Setting {
                key: "dontFollowForwards",
                label: "Don't Follow Forwards",
                description: "After forwarding a single message, don't jump to it. Hold shift to ignore this behavior.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "selfForward",
                label: "Self Forward",
                description: "Show the current channel in the forward list popup.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "BetterGifAltText",
        name: "BetterGifAltText",
        description: "Change GIF alt text from simply being 'GIF' to containing the gif tags / filename",
        origin: Origin::Vencord,
        source: "src/plugins/betterGifAltText/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "BetterPlusReacts",
        name: "BetterPlusReacts",
        description: "The amount of plus before :emoji: is the message to add it to",
        origin: Origin::Equicord,
        source: "src/equicordplugins/betterPlusReacts/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "BetterQuickReact",
        name: "MoreQuickReactions",
        description: "Improves the quick react buttons in the message context menu.",
        origin: Origin::Vencord,
        source: "src/plugins/moreQuickReactions/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "reactionCount",
                label: "Reaction Count",
                description: "Number of reactions (0-42)",
                kind: Kind::Number {
                    default: 5.,
                    min: 0.,
                    max: 42.,
                    step: 1.,
                },
            },
            Setting {
                key: "frequentEmojis",
                label: "Frequent Emojis",
                description: "Use frequently used emojis instead of favorite emojis",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "rows",
                label: "Rows",
                description: "Rows of quick reactions to display",
                kind: Kind::Number {
                    default: 2.,
                    min: 1.,
                    max: 16.,
                    step: 1.,
                },
            },
            Setting {
                key: "columns",
                label: "Columns",
                description: "Columns of quick reactions to display",
                kind: Kind::Number {
                    default: 4.,
                    min: 1.,
                    max: 12.,
                    step: 1.,
                },
            },
            Setting {
                key: "compactMode",
                label: "Compact Mode",
                description: "Scales the buttons to 75% of their original scale, whilst increasing the inner emoji to 125% scale. Emojis will be 93.75% of the original size. Recommended to have a minimum of 5 columns",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "scroll",
                label: "Scroll",
                description: "Enable scrolling the list of emojis",
                kind: Kind::Switch { default: true },
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
        id: "BypassStatus",
        name: "BypassStatus",
        description: "Still get notifications from specific sources when in do not disturb mode. Right-click on users/channels/guilds to set them to bypass do not disturb mode.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/bypassStatus/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "guilds",
                label: "Guilds",
                description: "Guilds to let bypass (notified when pinged anywhere in guild)",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "channels",
                label: "Channels",
                description: "Channels to let bypass (notified when pinged in that channel)",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "users",
                label: "Users",
                description: "Users to let bypass (notified for all messages sent in DMs)",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "allowOutsideOfDms",
                label: "Allow Outside Of Dms",
                description: "Allow selected users to bypass status outside of DMs too (acts like a channel/guild bypass, but it's for all messages sent by the selected users)",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "notificationSound",
                label: "Notification Sound",
                description: "Whether the notification sound should be played",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "respectSilentPings",
                label: "Respect Silent Pings",
                description: "Respect silent pings (@silent / suppress notifications)",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "statusToUse",
                label: "Status To Use",
                description: "Status to use for whitelist",
                kind: Kind::Select {
                    options: &[
                        ("Online", Lit::Str("online")),
                        ("Idle", Lit::Str("idle")),
                        ("Do Not Disturb", Lit::Str("dnd")),
                        ("Invisible", Lit::Str("invisible")),
                    ],
                    default: 2,
                },
            },
        ],
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
        id: "ClipsEnhancements",
        name: "ClipsEnhancements",
        description: "Add more Clip FPS and duration options, custom clip length, RPC tagging and more",
        origin: Origin::Equicord,
        source: "src/equicordplugins/clipsEnhancements.discordDesktop/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "richPresenceTagging",
            label: "Rich Presence Tagging",
            description: "When should clips be tagged with the current Rich Presence?",
            kind: Kind::Select {
                options: &[
                    ("Always", Lit::Str("always")),
                    (
                        "Only when beginning or end of activity name matches",
                        Lit::Str("whenMatched"),
                    ),
                    ("Never", Lit::Str("never")),
                ],
                default: 1,
            },
        }],
    },
    Def {
        id: "ClipUpload",
        name: "ClipUpload",
        description: "Adds a button to upload a local video file as a Discord clip.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/clipUpload.desktop/index.tsx",
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
        id: "CustomRPC",
        name: "CustomRPC",
        description: "Add a fully customisable Rich Presence (Game status) to your Discord profile",
        origin: Origin::Vencord,
        source: "src/plugins/customRPC/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "enablePresence",
                label: "Enable Presence",
                description: "Whether to show the presence",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "appID",
                label: "Application ID",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "appName",
                label: "Application Name",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "details",
                label: "Detail",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "detailsURL",
                label: "Detail URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "state",
                label: "State",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "stateURL",
                label: "State URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "type",
                label: "Activity Type",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
            Setting {
                key: "streamLink",
                label: "Stream Link",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "timestampMode",
                label: "Timestamp Mode",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
            Setting {
                key: "startTime",
                label: "Start Timestamp",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
            Setting {
                key: "endTime",
                label: "End Timestamp",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
            Setting {
                key: "imageBig",
                label: "Large Image",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "imageBigURL",
                label: "Large Image URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "imageBigTooltip",
                label: "Large Image Text",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "imageSmall",
                label: "Small Image",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "imageSmallURL",
                label: "Small Image URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "imageSmallTooltip",
                label: "Small Image Text",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "buttonOneText",
                label: "Button1 Text",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "buttonOneURL",
                label: "Button1 URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "buttonTwoText",
                label: "Button2 Text",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "buttonTwoURL",
                label: "Button2 URL",
                description: "",
                kind: Kind::Text {
                    default: "",
                    max_len: 600,
                },
            },
            Setting {
                key: "partySize",
                label: "Party Size",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
            Setting {
                key: "partyMaxSize",
                label: "Maximum Party Size",
                description: "",
                kind: Kind::Number {
                    default: 0.,
                    min: 0.,
                    max: 9_007_199_254_740_991.,
                    step: 1.,
                },
            },
        ],
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
        id: "ExpressionCloner",
        name: "ExpressionManager",
        description: "Clone Emotes & Stickers. Adds context menu options to quickly clone, edit, or delete them. Also adds Copy & Save options for stickers",
        origin: Origin::Vencord,
        source: "src/plugins/expressionManager/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "editAndDelete",
                label: "Edit And Delete",
                description: "Add \"Edit\" and \"Delete\" options to the expression picker context menu",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "stickerCopyAndOpen",
                label: "Sticker Copy And Open",
                description: "Add \"Copy Sticker Link\" and \"Open Sticker Link\" options to the sticker context menu",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "recents",
                label: "Recents",
                description: "Add \"Remove from Recents\" option to the context menu of frequently used expressions",
                kind: Kind::Switch { default: true },
            },
        ],
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
        id: "FixYoutubeEmbeds",
        name: "FixYoutubeEmbeds",
        description: "Bypasses youtube videos being blocked from display on Discord (for example by UMG)",
        origin: Origin::Vencord,
        source: "src/plugins/fixYoutubeEmbeds.desktop/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "youtubeDescription",
            label: "Youtube Description",
            description: "Adds descriptions to youtube video embeds",
            kind: Kind::Switch { default: false },
        }],
    },
    Def {
        id: "FriendInvites",
        name: "FriendInvites",
        description: "Create and manage friend invite links via slash commands (/create friend invite, /view friend invites, /revoke friend invites).",
        origin: Origin::Vencord,
        source: "src/plugins/friendInvites/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "FullSearchContext",
        name: "FullSearchContext",
        description: "Makes the message context menu in message search results have all options you'd expect",
        origin: Origin::Vencord,
        source: "src/plugins/fullSearchContext/index.tsx",
        default_on: false,
        settings: &[],
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
        id: "GlobalBadges",
        name: "GlobalBadges",
        description: "Adds global badges from other client mods",
        origin: Origin::Equicord,
        source: "src/equicordplugins/globalBadges/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "apiUrl",
                label: "Api Url",
                description: "API to use",
                kind: Kind::Text {
                    default: "https://badges.equicord.org/",
                    max_len: 2000,
                },
            },
            Setting {
                key: "showCustom",
                label: "Show Custom",
                description: "Show Custom Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showNekocord",
                label: "Show Nekocord",
                description: "Show Nekocord Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showReviewDB",
                label: "Show Review D B",
                description: "Show ReviewDB Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showAero",
                label: "Show Aero",
                description: "Show Aero Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showAliucord",
                label: "Show Aliucord",
                description: "Show Aliucord Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showRaincord",
                label: "Show Raincord",
                description: "Show Raincord Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showVelocity",
                label: "Show Velocity",
                description: "Show Velocity Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showEnmity",
                label: "Show Enmity",
                description: "Show Enmity Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showPaicord",
                label: "Show Paicord",
                description: "Show Paicord Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showBunny",
                label: "Show Bunny",
                description: "Show Bunny Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showGooseMod",
                label: "Show Goose Mod",
                description: "Show GooseMod Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showReplugged",
                label: "Show Replugged",
                description: "Show Replugged Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showBetterDiscord",
                label: "Show Better Discord",
                description: "Show BetterDiscord Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showVendroidEnhanced",
                label: "Show Vendroid Enhanced",
                description: "Show Vendroid Enhanced Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showRevenge",
                label: "Show Revenge",
                description: "Show Revenge Badges",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showReCord",
                label: "Show Re Cord",
                description: "Show ReCord Badges",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "GreetStickerPicker",
        name: "GreetStickerPicker",
        description: "Allows you to use any greet sticker instead of only the random one by right-clicking the 'Wave to say hi!' button",
        origin: Origin::Vencord,
        source: "src/plugins/greetStickerPicker/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "greetMode",
            label: "Greet Mode",
            description: "Choose the greet mode",
            kind: Kind::Select {
                options: &[
                    ("Greet (you can only greet 3 times)", Lit::Str("Greet")),
                    ("Normal Message (you can greet spam)", Lit::Str("Message")),
                ],
                default: 0,
            },
        }],
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
        id: "ImageZoom",
        name: "ImageZoom",
        description: "Lets you zoom in to images and gifs. Use scroll wheel to zoom in and shift + scroll wheel to increase lens radius / size",
        origin: Origin::Vencord,
        source: "src/plugins/imageZoom/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "saveZoomValues",
                label: "Save Zoom Values",
                description: "Whether to save zoom and lens size values",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "invertScroll",
                label: "Invert Scroll",
                description: "Invert scroll",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "nearestNeighbour",
                label: "Nearest Neighbour",
                description: "Use Nearest Neighbour Interpolation when scaling images",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "square",
                label: "Square",
                description: "Make the lens square",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "zoom",
                label: "Zoom",
                description: "Zoom of the lens",
                kind: Kind::Number {
                    default: 2.,
                    min: 1.,
                    max: 50.,
                    step: 0.1,
                },
            },
            Setting {
                key: "size",
                label: "Size",
                description: "Radius / Size of the lens",
                kind: Kind::Number {
                    default: 100.,
                    min: 50.,
                    max: 1000.,
                    step: 1.,
                },
            },
            Setting {
                key: "zoomSpeed",
                label: "Zoom Speed",
                description: "How fast the zoom / lens size changes",
                kind: Kind::Number {
                    default: 0.5,
                    min: 0.1,
                    max: 5.,
                    step: 0.1,
                },
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
        id: "MessageLogger",
        name: "MessageLogger",
        description: "Temporarily logs deleted and edited messages.",
        origin: Origin::Vencord,
        source: "src/plugins/messageLogger/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "deleteStyle",
                label: "Delete Style",
                description: "The style of deleted messages",
                kind: Kind::Select {
                    options: &[
                        ("Red text", Lit::Str("text")),
                        ("Red overlay", Lit::Str("overlay")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "logDeletes",
                label: "Log Deletes",
                description: "Whether to log deleted messages",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "collapseDeleted",
                label: "Collapse Deleted",
                description: "Whether to collapse deleted messages, similar to blocked messages",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "logEdits",
                label: "Log Edits",
                description: "Whether to log edited messages",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "logDeletedAttachments",
                label: "Log Deleted Attachments",
                description: "Whether to log deleted attachments",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "inlineEdits",
                label: "Inline Edits",
                description: "Whether to display edit history as part of message content",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "ignoreBots",
                label: "Ignore Bots",
                description: "Whether to ignore messages by bots",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "ignoreSelf",
                label: "Ignore Self",
                description: "Whether to ignore messages by yourself",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreSelfEdits",
                label: "Ignore Self Edits",
                description: "Whether to ignore edits by yourself",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "ignoreUsers",
                label: "Ignore Users",
                description: "Comma-separated list of user IDs to ignore",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "ignoreChannels",
                label: "Ignore Channels",
                description: "Comma-separated list of channel IDs to ignore",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "ignoreGuilds",
                label: "Ignore Guilds",
                description: "Comma-separated list of guild IDs to ignore",
                kind: Kind::Text {
                    default: "",
                    max_len: 4000,
                },
            },
            Setting {
                key: "showEditDiffs",
                label: "Show Edit Diffs",
                description: "Show visual differences between edited message versions",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "separatedDiffs",
                label: "Separated Diffs",
                description: "Separate addition and removals in diffs for a more readable differential",
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
        id: "MessageTags",
        name: "MessageTags",
        description: "Allows you to create custom slash commands / tags",
        origin: Origin::Vencord,
        source: "src/plugins/customCommands/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "MoreCommands",
        name: "MoreCommands",
        description: "Adds various fun and useful commands",
        origin: Origin::Equicord,
        source: "src/equicordplugins/moreCommands/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "addFreakyEnding",
                label: "Add Freaky Ending",
                description: "Add 👅 or ❤️ at the end",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "uwuEveryMessage",
                label: "Uwu Every Message",
                description: "Make every single message uwuified",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "uwuEverything",
                label: "Uwu Everything",
                description: "Makes *all* text uwuified - really bad idea",
                kind: Kind::Switch { default: false },
            },
        ],
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
        id: "NewGuildSettings",
        name: "NewGuildSettings",
        description: "Automatically mute new servers and change various other settings upon joining",
        origin: Origin::Vencord,
        source: "src/plugins/newGuildSettings/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "guild",
                label: "Guild",
                description: "Mute Guild automatically",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "messages",
                label: "Messages",
                description: "Server Notification Settings",
                kind: Kind::Select {
                    options: &[
                        ("All messages", Lit::Int(0)),
                        ("Only @mentions", Lit::Int(1)),
                        ("Nothing", Lit::Int(2)),
                        ("Server default", Lit::Int(3)),
                    ],
                    default: 3,
                },
            },
            Setting {
                key: "everyone",
                label: "Everyone",
                description: "Suppress @everyone and @here",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "role",
                label: "Role",
                description: "Suppress All Role @mentions",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "highlights",
                label: "Highlights",
                description: "Suppress Highlights automatically",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "events",
                label: "Events",
                description: "Mute New Events automatically",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showAllChannels",
                label: "Show All Channels",
                description: "Show all channels automatically",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "mobilePush",
                label: "Mobile Push",
                description: "Mute Mobile Push Notifications automatically",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "voiceChannels",
                label: "Voice Channels",
                description: "Hide names in Voice channels automatically",
                kind: Kind::Switch { default: false },
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
        id: "OpenInApp",
        name: "OpenInApp",
        description: "Open links in their respective apps instead of your browser",
        origin: Origin::Vencord,
        source: "src/plugins/openInApp/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "spotify",
                label: "Spotify",
                description: "Open Spotify links in the Spotify app",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "steam",
                label: "Steam",
                description: "Open Steam links in the Steam app",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "epic",
                label: "Epic",
                description: "Open Epic Games links in the Epic Games Launcher",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "tidal",
                label: "Tidal",
                description: "Open Tidal links in the Tidal app",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "itunes",
                label: "iTunes",
                description: "Open Apple Music links in the iTunes app",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "vrcx",
                label: "Vrcx",
                description: "Open VRChat links in the VRCX app",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "telegram",
                label: "Telegram",
                description: "Open Telegram links in the Telegram app",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "PermissionsViewer",
        name: "PermissionsViewer",
        description: "View the permissions a user or channel has, and the roles of a server",
        origin: Origin::Vencord,
        source: "src/plugins/permissionsViewer/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "permissionsSortOrder",
            label: "Permissions Sort Order",
            description: "The sort method used for defining which role grants an user a certain permission",
            kind: Kind::Select {
                options: &[("Highest Role", Lit::Int(0)), ("Lowest Role", Lit::Int(1))],
                default: 0,
            },
        }],
    },
    Def {
        id: "PinDMs",
        name: "PinDMs",
        description: "Allows you to pin private channels to the top of your DM list. To pin/unpin or re-order pins, right click DMs",
        origin: Origin::Vencord,
        source: "src/plugins/pinDms/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "pinOrder",
                label: "Pin Order",
                description: "Which order should pinned DMs be displayed in?",
                kind: Kind::Select {
                    options: &[
                        ("Most recent message", Lit::Int(0)),
                        ("Custom (right click channels to reorder)", Lit::Int(1)),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "canCollapseDmSection",
                label: "Can Collapse DM Section",
                description: "Allow uncategorised DMs section to be collapsable",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "dmSectionCollapsed",
                label: "DM Section Collapsed",
                description: "Collapse DM section",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "friendsSidebarGroupDms",
                label: "Friends Sidebar Group Dms",
                description: "Show pinned group DMs in the friends sidebar.",
                kind: Kind::Switch { default: true },
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
        id: "PreviewMessage",
        name: "PreviewMessage",
        description: "Lets you preview your message before sending it.",
        origin: Origin::Vencord,
        source: "src/plugins/previewMessage/index.tsx",
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
        id: "Quoter",
        name: "Quoter",
        description: "Adds the ability to create an inspirational quote image from a message.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/quoter/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "watermark",
                label: "Watermark",
                description: "Custom watermark text (max 32 characters)",
                kind: Kind::Text {
                    default: "Made with Equicord",
                    max_len: 32,
                },
            },
            Setting {
                key: "grayscale",
                label: "Grayscale",
                description: "Enable grayscale by default",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showWatermark",
                label: "Show Watermark",
                description: "Show watermark by default",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "saveAsGif",
                label: "Save as GIF",
                description: "Save as GIF by default",
                kind: Kind::Switch { default: false },
            },
        ],
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
        id: "RelationshipNotifier",
        name: "RelationshipNotifier",
        description: "Notifies you when a friend, group chat, or server removes you.",
        origin: Origin::Vencord,
        source: "src/plugins/relationshipNotifier/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "notices",
                label: "Notices",
                description: "Also show a notice at the top of your screen when removed (use this if you don't want to miss any notifications).",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "offlineRemovals",
                label: "Offline Removals",
                description: "Notify you when starting discord if you were removed while offline.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "friends",
                label: "Friends",
                description: "Notify when a friend removes you",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "friendRequestCancels",
                label: "Friend Request Cancels",
                description: "Notify when a friend request is cancelled",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "servers",
                label: "Servers",
                description: "Notify when removed from a server",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "groups",
                label: "Groups",
                description: "Notify when removed from a group chat",
                kind: Kind::Switch { default: true },
            },
        ],
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
        id: "SearchFix",
        name: "SearchFix",
        description: "Fixes the annoying \"We dropped the magnifying glass!\" error.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/searchFix/index.tsx",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "SedEnhanced",
        name: "SedEnhanced",
        description: "Expands on Discord's rudimentary `sed` support.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/sedEnhanced/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "regexByDefault",
            label: "Regex By Default",
            description: "Inverts the `r` flag, so using the `r` flag enables non-regex mode, and omitting it uses regex mode.",
            kind: Kind::Switch { default: false },
        }],
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
        id: "ShowHiddenThings",
        name: "ShowHiddenThings",
        description: "Displays various hidden & moderator-only things regardless of permissions.",
        origin: Origin::Vencord,
        source: "src/plugins/showHiddenThings/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "showTimeouts",
                label: "Show Timeouts",
                description: "Show member timeout icons in chat.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showInvitesPaused",
                label: "Show Invites Paused",
                description: "Show the invites paused tooltip in the server list.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "showModView",
                label: "Show Mod View",
                description: "Show the member mod view context menu item in all servers.",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "SilentMessageToggle",
        name: "SilentMessageToggle",
        description: "Adds a button to the chat bar to toggle sending a silent message.",
        origin: Origin::Vencord,
        source: "src/plugins/silentMessageToggle/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "persistState",
                label: "Persist State",
                description: "How to persist the silent message toggle state",
                kind: Kind::Select {
                    options: &[
                        ("Don't persist (reset on channel change)", Lit::Str("none")),
                        ("Persist between channels", Lit::Str("channels")),
                        (
                            "Persist between channels and restarts",
                            Lit::Str("restarts"),
                        ),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "autoDisable",
                label: "Auto Disable",
                description: "Automatically disable the silent message toggle again after sending one",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "SpotifyCrack",
        name: "SpotifyCrack",
        description: "Free listen along, no auto-pausing in voice chat, and allows activity to continue playing when idling",
        origin: Origin::Vencord,
        source: "src/plugins/spotifyCrack/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "noSpotifyAutoPause",
                label: "No Spotify Auto Pause",
                description: "Disable Spotify auto-pause",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "keepSpotifyActivityOnIdle",
                label: "Keep Spotify Activity On Idle",
                description: "Keep Spotify activity playing when idling",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "SpotifyShareCommands",
        name: "SpotifyShareCommands",
        description: "Share your current Spotify track, album or artist via slash command (/track, /album, /artist)",
        origin: Origin::Vencord,
        source: "src/plugins/spotifyShareCommands/index.ts",
        default_on: false,
        settings: &[],
    },
    Def {
        id: "SteamStatusSync",
        name: "SteamStatusSync",
        description: "Sync your status to Steam! (Online, Away, Invisible, or Offline.)",
        origin: Origin::Equicord,
        source: "src/equicordplugins/steamStatusSync/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "onlineStatus",
                label: "Online Status",
                description: "Steam status when on Online",
                kind: Kind::Select {
                    options: &[
                        ("Online", Lit::Str("online")),
                        ("Away", Lit::Str("away")),
                        ("Invisible", Lit::Str("invisible")),
                        ("Offline (Disconnect Steam Chat)", Lit::Str("offline")),
                        ("Disabled", Lit::Str("none")),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "idleStatus",
                label: "Idle Status",
                description: "Steam status when on Idle",
                kind: Kind::Select {
                    options: &[
                        ("Online", Lit::Str("online")),
                        ("Away", Lit::Str("away")),
                        ("Invisible", Lit::Str("invisible")),
                        ("Offline (Disconnect Steam Chat)", Lit::Str("offline")),
                        ("Disabled", Lit::Str("none")),
                    ],
                    default: 1,
                },
            },
            Setting {
                key: "dndStatus",
                label: "Dnd Status",
                description: "Steam status when on Do Not Disturb",
                kind: Kind::Select {
                    options: &[
                        ("Online", Lit::Str("online")),
                        ("Away", Lit::Str("away")),
                        ("Invisible", Lit::Str("invisible")),
                        ("Offline (Disconnect Steam Chat)", Lit::Str("offline")),
                        ("Disabled", Lit::Str("none")),
                    ],
                    default: 4,
                },
            },
            Setting {
                key: "invisibleStatus",
                label: "Invisible Status",
                description: "Steam status when on Invisible",
                kind: Kind::Select {
                    options: &[
                        ("Online", Lit::Str("online")),
                        ("Away", Lit::Str("away")),
                        ("Invisible", Lit::Str("invisible")),
                        ("Offline (Disconnect Steam Chat)", Lit::Str("offline")),
                        ("Disabled", Lit::Str("none")),
                    ],
                    default: 2,
                },
            },
            Setting {
                key: "goInvisibleIfActivityIsHidden",
                label: "Go Invisible If Activity Is Hidden",
                description: "Always go invisible if hiding game activity on Discord",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "StreamingCodecDisabler",
        name: "StreamingCodecDisabler",
        description: "Disable video codecs of your choice for streaming and cameras.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/streamingCodecDisabler/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "disableAv1Codec",
                label: "Disable Av1 Codec",
                description: "Make Discord not consider using AV1 for streaming and cameras.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "disableH265Codec",
                label: "Disable H265 Codec",
                description: "Make Discord not consider using H265 for streaming and cameras.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "disableH264Codec",
                label: "Disable H264 Codec",
                description: "Make Discord not consider using H264 for streaming and cameras.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "disableVP8Codec",
                label: "Disable VP8 Codec",
                description: "Make Discord not consider using VP8 for streaming and cameras.",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "disableVP9Codec",
                label: "Disable VP9 Codec",
                description: "Make Discord not consider using VP9 for streaming and cameras.",
                kind: Kind::Switch { default: false },
            },
        ],
    },
    Def {
        id: "SuperReactionTweaks",
        name: "SuperReactionTweaks",
        description: "Customize the limit of Super Reactions playing at once, and super react by default",
        origin: Origin::Vencord,
        source: "src/plugins/superReactionTweaks/index.ts",
        default_on: false,
        settings: &[
            Setting {
                key: "superReactByDefault",
                label: "Super React By Default",
                description: "Reaction picker will default to Super Reactions",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "unlimitedSuperReactionPlaying",
                label: "Unlimited Super Reaction Playing",
                description: "Remove the limit on Super Reactions playing at once",
                kind: Kind::Switch { default: false },
            },
            Setting {
                key: "superReactionPlayingLimit",
                label: "Super Reaction Playing Limit",
                description: "Max Super Reactions to play at once. 0 to disable playing Super Reactions",
                kind: Kind::Number {
                    default: 20.,
                    min: 0.,
                    max: 100.,
                    step: 1.,
                },
            },
        ],
    },
    Def {
        id: "TenorGifSearch",
        name: "GifProviderSwitcher",
        description: "Allows you to use Tenor or Giphy GIF search instead of Klipy",
        origin: Origin::Vencord,
        source: "src/plugins/gifProviderSwitcher/index.tsx",
        default_on: false,
        settings: &[Setting {
            key: "provider",
            label: "Provider",
            description: "The GIF search the picker uses",
            kind: Kind::Select {
                options: &[
                    ("Tenor", Lit::Str("tenor")),
                    ("Giphy", Lit::Str("giphy")),
                    ("Klipy", Lit::Str("klipy")),
                ],
                default: 0,
            },
        }],
    },
    Def {
        id: "Translate+",
        name: "Translate+",
        description: "Vencord's translate plugin but with support for artistic languages!",
        origin: Origin::Equicord,
        source: "src/equicordplugins/translatePlus/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "target",
                label: "Target",
                description: "Target language",
                kind: Kind::Text {
                    default: "en",
                    max_len: 200,
                },
            },
            Setting {
                key: "toki",
                label: "Toki",
                description: "Enable Toki Pona",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "sitelen",
                label: "Sitelen",
                description: "Enable Sitelen Pona",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "shavian",
                label: "Shavian",
                description: "Enable Shavian",
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
        id: "UnlockedAvatarZoom",
        name: "UnlockedAvatarZoom",
        description: "Allows you to zoom in further in the image crop tool when changing your avatar",
        origin: Origin::Vencord,
        source: "src/plugins/unlockedAvatarZoom/index.ts",
        default_on: false,
        settings: &[Setting {
            key: "zoomMultiplier",
            label: "Zoom Multiplier",
            description: "Zoom multiplier",
            kind: Kind::Number {
                default: 4.,
                min: 2.,
                max: 16.,
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
        id: "UserPFP",
        name: "UserPFP",
        description: "Allows you to set local static or animated avatars for yourself and other users",
        origin: Origin::Equicord,
        source: "src/equicordplugins/userpfp/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "overrideServerAvatars",
                label: "Override Server Avatars",
                description: "Override server avatars with custom avatars or the default user avatar if no custom avatar is set.",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "preferNitro",
                label: "Prefer Nitro",
                description: "Which avatar to use if both default animated (Nitro) pfp and UserPFP avatars are present",
                kind: Kind::Select {
                    options: &[("UserPFP", Lit::Bool(false)), ("Nitro", Lit::Bool(true))],
                    default: 1,
                },
            },
            Setting {
                key: "databaseSource",
                label: "Database Source",
                description: "URL to load database from",
                kind: Kind::Text {
                    default: "https://userpfp.github.io/UserPFP/source/data.json",
                    max_len: 2000,
                },
            },
        ],
    },
    Def {
        id: "USRBG",
        name: "USRBG",
        description: "Displays user banners from USRBG, allowing anyone to get a banner without Nitro",
        origin: Origin::Vencord,
        source: "src/plugins/usrbg/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "nitroFirst",
                label: "Nitro First",
                description: "Banner to use if both Nitro and USRBG banners are present",
                kind: Kind::Select {
                    options: &[
                        ("Nitro banner", Lit::Bool(true)),
                        ("USRBG banner", Lit::Bool(false)),
                    ],
                    default: 0,
                },
            },
            Setting {
                key: "voiceBackground",
                label: "Voice Background",
                description: "Use USRBG banners as voice chat backgrounds",
                kind: Kind::Switch { default: true },
            },
        ],
    },
    Def {
        id: "ValidReply",
        name: "ValidReply",
        description: "Fixes \"Message could not be loaded\" upon hovering over the reply",
        origin: Origin::Vencord,
        source: "src/plugins/validReply/index.ts",
        default_on: false,
        settings: &[],
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
        id: "VoiceMessages",
        name: "VoiceMessages",
        description: "Allows you to send voice messages like on mobile. To do so, right click the upload button and click Send Voice Message",
        origin: Origin::Vencord,
        source: "src/plugins/voiceMessages/index.tsx",
        default_on: false,
        settings: &[
            Setting {
                key: "noiseSuppression",
                label: "Noise Suppression",
                description: "Noise Suppression",
                kind: Kind::Switch { default: true },
            },
            Setting {
                key: "echoCancellation",
                label: "Echo Cancellation",
                description: "Echo Cancellation",
                kind: Kind::Switch { default: true },
            },
        ],
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
    Def {
        id: "ZipPreview",
        name: "ZipPreview",
        description: "Previews ZIP contents inside file attachments.",
        origin: Origin::Equicord,
        source: "src/equicordplugins/zipPreview/index.tsx",
        default_on: false,
        settings: &[],
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
