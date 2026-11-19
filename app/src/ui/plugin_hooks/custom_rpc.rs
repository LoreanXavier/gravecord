//! CustomRPC (Vencord `src/plugins/customRPC`, GPL-3.0-or-later): a rich presence of your own, sent as a local activity
//! (`LOCAL_ACTIVITY_UPDATE` from the socket "CustomRPC", after every game's) while the plugin and `enablePresence`
//! are on. Its card shows the original's notices (the presence or activity sharing being off, each with the button
//! that turns it on), the help lines, your activity as your profile draws it, then the form: presets (saved by name,
//! loaded, deleted), the activity type, application id and name, the two lines and their links, the stream link
//! (Streaming only), party sizes (Playing only), both pictures (keys or links) with their texts and links, two buttons,
//! and the timestamp mode with a custom start and end (a custom span repeats). Each field keeps the original's check
//! and message; a picture's key or link becomes the application's asset id (`fetchAssetIds`) before the activity
//! goes out. Not part of the pinned client.
//!
//! GAP: the toolbox's "Show Custom RPC" (the app has no toolbox), and the profile's own buttons (the original's patch
//! shows them on your own activity; the app's card draws buttons for everyone already).

use super::super::Root;
use crate::editor::{Editor, EditorEvent};
use crate::plugins::custom_rpc::{self as rpc, Config};
use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, div, px,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

const ID: &str = "CustomRPC";
pub(in crate::ui) const ASSETS_KEY: &str = "plugin-custom-rpc-assets:";
const PRESETS_KEY: &str = "CustomRPC_presets";

/// How a text field's value is read and checked.
#[derive(Clone, Copy)]
enum Check {
    Max(usize),
    Required(usize),
    AppId,
    Url,
    Image,
    Stream,
    Number,
}

/// The form's text fields: settings key, label, check.
const FIELDS: &[(&str, &str, Check)] = &[
    ("appID", "Application ID", Check::AppId),
    ("appName", "Application Name", Check::Required(128)),
    ("details", "Detail (line 1)", Check::Max(128)),
    ("detailsURL", "Detail URL", Check::Url),
    ("state", "State (line 2)", Check::Max(128)),
    ("stateURL", "State URL", Check::Url),
    (
        "streamLink",
        "Stream Link (Twitch or YouTube, only if activity type is Streaming)",
        Check::Stream,
    ),
    ("partySize", "Party Size", Check::Number),
    ("partyMaxSize", "Maximum Party Size", Check::Number),
    ("imageBig", "Large Image URL/Key", Check::Image),
    ("imageBigTooltip", "Large Image Text", Check::Max(128)),
    ("imageBigURL", "Large Image clickable URL", Check::Url),
    ("imageSmall", "Small Image URL/Key", Check::Image),
    ("imageSmallTooltip", "Small Image Text", Check::Max(128)),
    ("imageSmallURL", "Small Image clickable URL", Check::Url),
    ("buttonOneText", "Button1 Text", Check::Max(31)),
    ("buttonOneURL", "Button1 URL", Check::Url),
    ("buttonTwoText", "Button2 Text", Check::Max(31)),
    ("buttonTwoURL", "Button2 URL", Check::Url),
    (
        "startTime",
        "Start Timestamp (in milliseconds)",
        Check::Number,
    ),
    ("endTime", "End Timestamp (in milliseconds)", Check::Number),
];

/// The two selects: key, label, options.
const TYPES: &[(&str, u64)] = &[
    ("Playing", 0),
    ("Streaming", 1),
    ("Listening", 2),
    ("Watching", 3),
    ("Competing", 5),
];
const MODES: &[(&str, u64)] = &[
    ("None", 0),
    ("Since discord open", 1),
    ("Same as your current time (not reset after 24h)", 2),
    ("Custom", 3),
];

#[derive(Default)]
struct Form {
    fields: HashMap<&'static str, (Entity<Editor>, Subscription)>,
    errors: HashMap<&'static str, String>,
    preset_name: Option<(Entity<Editor>, Subscription)>,
    selected_preset: String,
    /// What was sent last, to send again only on a change.
    sent: Option<Option<Value>>,
    /// The custom span's start (`loopAnchor`).
    anchor: u64,
    /// When the app started (`Since discord open` is the activity's own making time in the original).
    made: u64,
    /// The asset ids by application and key; `None` while asked for.
    assets: HashMap<(String, String), Option<String>>,
    /// The settings revision and asset answers the last build saw, and when the custom loop turns next: the activity
    /// is built again only when one of them moved (not on every frame).
    built: Option<(u64, u64)>,
    answers: u64,
    next_turn: u64,
}

thread_local! {
    static FORM: RefCell<Form> = RefCell::new(Form::default());
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugin_on!("CustomRPC")
}

fn text_of(state: &crate::plugins::State, key: &str) -> String {
    match state.value(ID, key) {
        Value::String(text) => text,
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn number_of(state: &crate::plugins::State, key: &str) -> u64 {
    state.value(ID, key).as_u64().unwrap_or(0)
}

/// The plugin's values (`settings.store`).
fn config() -> Config {
    crate::plugins::with(|state| Config {
        app_id: text_of(state, "appID"),
        app_name: text_of(state, "appName"),
        details: text_of(state, "details"),
        details_url: text_of(state, "detailsURL"),
        state: text_of(state, "state"),
        state_url: text_of(state, "stateURL"),
        kind: number_of(state, "type"),
        stream_link: text_of(state, "streamLink"),
        timestamp_mode: number_of(state, "timestampMode"),
        start_time: number_of(state, "startTime"),
        end_time: number_of(state, "endTime"),
        image_big: text_of(state, "imageBig"),
        image_big_url: text_of(state, "imageBigURL"),
        image_big_tooltip: text_of(state, "imageBigTooltip"),
        image_small: text_of(state, "imageSmall"),
        image_small_url: text_of(state, "imageSmallURL"),
        image_small_tooltip: text_of(state, "imageSmallTooltip"),
        button_one_text: text_of(state, "buttonOneText"),
        button_one_url: text_of(state, "buttonOneURL"),
        button_two_text: text_of(state, "buttonTwoText"),
        button_two_url: text_of(state, "buttonTwoURL"),
        party_size: number_of(state, "partySize"),
        party_max_size: number_of(state, "partyMaxSize"),
    })
}

fn enabled() -> bool {
    crate::plugins::with(|state| state.value(ID, "enablePresence") == Value::Bool(true))
}

/// A field's message, or the value to store.
fn checked(check: Check, text: &str, kind: u64) -> Result<Value, String> {
    let error = match check {
        Check::Max(max) => rpc::length_error(text, max, false),
        Check::Required(max) => rpc::length_error(text, max, true),
        Check::AppId => rpc::app_id_error(text),
        Check::Url => rpc::url_error(text),
        Check::Image => rpc::image_key_error(text),
        Check::Stream => rpc::stream_link_error(text, kind == rpc::STREAMING),
        Check::Number => return rpc::number(text).map(Value::from),
    };
    match error {
        Some(error) => Err(error),
        None => Ok(Value::from(text.to_owned())),
    }
}

/// Whether a field is greyed by the activity type or the timestamp mode.
fn disabled(key: &str, config: &Config) -> bool {
    match key {
        "streamLink" => config.kind != rpc::STREAMING,
        "partySize" | "partyMaxSize" => config.kind != rpc::PLAYING,
        "startTime" | "endTime" => config.timestamp_mode != rpc::CUSTOM,
        _ => false,
    }
}

/// The local midnight of now (`Same as your current time`).
fn local_midnight(now: u64) -> u64 {
    use chrono::Timelike;
    let local = chrono::Local::now();
    now.saturating_sub(u64::from(local.num_seconds_from_midnight()) * 1000)
}

impl Root {
    /// Each frame: the activity follows the plugin's values, the custom span starts again once it ends, and the
    /// pictures' asset ids are asked for once.
    pub(in crate::ui) fn custom_rpc_prepare(&mut self) {
        let active = on() && enabled();
        let sent_something =
            FORM.with(|form| form.borrow().sent.as_ref().is_some_and(Option::is_some));
        if !active {
            if sent_something {
                FORM.with(|form| {
                    let mut form = form.borrow_mut();
                    form.sent = Some(None);
                    form.built = None;
                });
                self.set_plugin_activity(None);
            }
            return;
        }
        let now = now_ms();
        let seen = (
            crate::plugins::revision(),
            FORM.with(|form| form.borrow().answers),
        );
        let fresh = FORM.with(|form| {
            let form = form.borrow();
            form.built == Some(seen) && (form.next_turn == 0 || now < form.next_turn)
        });
        if fresh {
            return;
        }
        let config = config();
        let span = config.end_time.saturating_sub(config.start_time);
        let looping = config.timestamp_mode == rpc::CUSTOM
            && config.start_time != 0
            && config.end_time > config.start_time;
        let (anchor, made) = FORM.with(|form| {
            let mut form = form.borrow_mut();
            if form.made == 0 {
                form.made = now;
            }
            if looping && (form.anchor == 0 || now >= form.anchor + span) {
                form.anchor = now;
            }
            form.built = Some(seen);
            form.next_turn = if looping { form.anchor + span } else { 0 };
            (form.anchor, form.made)
        });
        // `getApplicationAsset`: each picture's id, asked for once.
        let mut wanted = Vec::new();
        let mut asset = |key: &str| -> Option<Option<String>> {
            if key.is_empty() {
                return Some(None);
            }
            let slot = (config.app_id.clone(), key.to_owned());
            match FORM.with(|form| form.borrow().assets.get(&slot).cloned()) {
                Some(found) => found.map(Some),
                None => {
                    wanted.push(key.to_owned());
                    None
                }
            }
        };
        let big = asset(&config.image_big);
        let small = asset(&config.image_small);
        if !wanted.is_empty() {
            FORM.with(|form| {
                let mut form = form.borrow_mut();
                for key in &wanted {
                    form.assets
                        .insert((config.app_id.clone(), key.clone()), None);
                }
            });
            let _ = self.requests.send(crate::net::Request::PluginRpcAssets {
                key: format!("{ASSETS_KEY}{}", config.app_id),
                app_id: config.app_id.clone(),
                keys: wanted,
            });
            return;
        }
        let (Some(big), Some(small)) = (big, small) else {
            return;
        };
        let activity = rpc::activity(
            &config,
            made,
            local_midnight(now),
            anchor,
            big.as_deref(),
            small.as_deref(),
        );
        // Every start above is fixed (the form's open time, today's midnight, the loop's anchor), so the activity is
        // sent again only when the form or a loop turn changes it.
        let changed = FORM.with(|form| form.borrow().sent.as_ref() != Some(&activity));
        if changed {
            FORM.with(|form| form.borrow_mut().sent = Some(activity.clone()));
            self.set_plugin_activity(activity);
        }
    }

    /// The asset ids came. True when the key was the plugin's.
    pub(in crate::ui) fn custom_rpc_fetched(&mut self, key: &str, body: &Value) -> bool {
        let Some(app) = key.strip_prefix(ASSETS_KEY) else {
            return false;
        };
        FORM.with(|form| {
            let mut form = form.borrow_mut();
            form.answers += 1;
            for (name, id) in body.as_object().into_iter().flatten() {
                form.assets.insert(
                    (app.to_owned(), name.clone()),
                    Some(id.as_str().map(str::to_owned).unwrap_or_default()),
                );
            }
        });
        true
    }

    /// Makes the form's boxes once, filled with the values.
    fn custom_rpc_fields(&self, cx: &mut Context<Self>) {
        let made = FORM.with(|form| !form.borrow().fields.is_empty());
        if made {
            return;
        }
        let values = crate::plugins::with(|state| {
            FIELDS
                .iter()
                .map(|(key, _, check)| {
                    let text = text_of(state, key);
                    // A number left at 0 shows empty, as `?? ""` of an unset value.
                    let shown = if matches!(check, Check::Number) && text == "0" {
                        String::new()
                    } else {
                        text
                    };
                    (*key, shown)
                })
                .collect::<Vec<_>>()
        });
        let mut fields = HashMap::new();
        for ((key, text), (_, _, check)) in values.into_iter().zip(FIELDS) {
            let check = *check;
            let editor = cx.new(|cx| {
                let mut editor = Editor::new(super::super::editor_style(), cx);
                editor.set_single_line(true);
                editor.set_placeholder("Enter a value".to_owned());
                editor.set_text(&text);
                editor
            });
            let sub = cx.subscribe(&editor, move |_, editor, event: &EditorEvent, cx| {
                if !matches!(event, EditorEvent::Edited) {
                    return;
                }
                let typed = editor.read(cx).text().to_owned();
                let kind = config().kind;
                match checked(check, &typed, kind) {
                    Ok(value) => {
                        FORM.with(|form| form.borrow_mut().errors.remove(key));
                        crate::plugins::update(|state| {
                            state.set_value(ID, key, &value);
                        });
                    }
                    Err(error) => {
                        FORM.with(|form| form.borrow_mut().errors.insert(key, error));
                    }
                }
                cx.notify();
            });
            fields.insert(key, (editor, sub));
        }
        let name = cx.new(|cx| {
            let mut editor = Editor::new(super::super::editor_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("Preset name".to_owned());
            editor
        });
        let name_sub = cx.subscribe(&name, |_, _, _: &EditorEvent, cx| cx.notify());
        FORM.with(|form| {
            let mut form = form.borrow_mut();
            form.fields = fields;
            form.preset_name = Some((name, name_sub));
        });
    }

    /// The presets kept (`DataStore` `CustomRPC_presets`), by name.
    fn custom_rpc_presets(&self) -> Vec<(String, Value)> {
        crate::storage::get(PRESETS_KEY)
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|preset| {
                Some((
                    preset["name"].as_str()?.to_owned(),
                    preset["config"].clone(),
                ))
            })
            .collect()
    }

    fn custom_rpc_save_preset(&mut self, cx: &mut Context<Self>) {
        let Some(name) = FORM.with(|form| {
            form.borrow()
                .preset_name
                .as_ref()
                .map(|(editor, _)| editor.read(cx).text().trim().to_owned())
        }) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        // `getCurrentConfig`: every value but `config` (and the switch, which the original keeps out too).
        let current: serde_json::Map<String, Value> = crate::plugins::with(|state| {
            FIELDS
                .iter()
                .map(|(key, _, _)| *key)
                .chain(["type", "timestampMode", "enablePresence"])
                .map(|key| (key.to_owned(), state.value(ID, key)))
                .collect()
        });
        let mut presets: Vec<(String, Value)> = self
            .custom_rpc_presets()
            .into_iter()
            .filter(|(known, _)| *known != name)
            .collect();
        presets.push((name.clone(), Value::Object(current)));
        presets.sort_by(|a, b| a.0.cmp(&b.0));
        crate::storage::set(
            PRESETS_KEY,
            Value::Array(
                presets
                    .into_iter()
                    .map(|(name, config)| serde_json::json!({"name": name, "config": config}))
                    .collect(),
            ),
        );
        FORM.with(|form| form.borrow_mut().selected_preset = name.clone());
        self.show_success_toast(format!("Saved preset {name}."), cx);
    }

    fn custom_rpc_load_preset(&mut self, cx: &mut Context<Self>) {
        let selected = FORM.with(|form| form.borrow().selected_preset.clone());
        let Some((name, config)) = self
            .custom_rpc_presets()
            .into_iter()
            .find(|(known, _)| *known == selected)
        else {
            return;
        };
        crate::plugins::update(|state| {
            for (key, value) in config.as_object().into_iter().flatten() {
                if let Some(known) = FIELDS
                    .iter()
                    .map(|(key, _, _)| *key)
                    .chain(["type", "timestampMode", "enablePresence"])
                    .find(|known| known == key)
                {
                    state.set_value(ID, known, value);
                }
            }
        });
        // The form is drawn again from the values (`key={formVersion}`).
        FORM.with(|form| {
            let mut form = form.borrow_mut();
            form.fields.clear();
            form.errors.clear();
        });
        self.show_success_toast(format!("Loaded preset {name}."), cx);
    }

    fn custom_rpc_delete_preset(&mut self, cx: &mut Context<Self>) {
        let selected = FORM.with(|form| form.borrow().selected_preset.clone());
        let presets = self.custom_rpc_presets();
        let kept: Vec<(String, Value)> = presets
            .iter()
            .filter(|(known, _)| *known != selected)
            .cloned()
            .collect();
        if kept.len() == presets.len() {
            return;
        }
        crate::storage::set(
            PRESETS_KEY,
            Value::Array(
                kept.into_iter()
                    .map(|(name, config)| serde_json::json!({"name": name, "config": config}))
                    .collect(),
            ),
        );
        FORM.with(|form| form.borrow_mut().selected_preset.clear());
        self.show_success_toast(format!("Deleted preset {selected}."), cx);
    }

    /// The plugin card's own section: the notices, the help, your activity, then the form.
    pub(in crate::ui) fn custom_rpc_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::super::color;
        self.custom_rpc_fields(cx);
        let config = config();
        let heading = |text: &str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text.to_owned())
        };
        let paragraph = |text: &str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text.to_owned())
        };
        let divider = || div().h(px(1.)).w_full().bg(color("--border-subtle"));
        let button = |key: &'static str, label: &str, danger: bool, off: bool| {
            div()
                .id(key)
                .h(px(32.))
                .px(px(16.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .bg(color(if danger {
                    "--control-critical-primary-background-default"
                } else {
                    "--control-primary-background-default"
                }))
                .when(off, |this| this.opacity(0.5))
                .when(!off, |this| this.cursor_pointer())
                .child(label.to_owned())
        };
        let mut about: Vec<AnyElement> = Vec::new();
        let card = |title: &str, text: &str, critical: bool| {
            div()
                .p(px(16.))
                .rounded(px(8.))
                .bg(color(if critical {
                    "--background-feedback-mod-critical"
                } else {
                    "--background-feedback-mod-warning"
                }))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(title.to_owned()),
                )
                .child(paragraph(text))
        };
        if !enabled() {
            about.push(
                card(
                    "Custom RPC disabled",
                    "The \"Enable Presence\" setting is disabled, so your presence won't show. If this was unintentional, enable it below.",
                    false,
                )
                .child(button("crpc-enable", "Enable Presence", false, false).on_click(cx.listener(
                    |_, _, _, cx| {
                        crate::plugins::update(|state| {
                            state.set_value(ID, "enablePresence", &Value::Bool(true));
                        });
                        cx.notify();
                    },
                )))
                .into_any_element(),
            );
        }
        if !self.pref_on("activity_privacy_setting", true) {
            about.push(
                card(
                    "Activity Sharing disabled",
                    "Activity Sharing isn't enabled, so people won't be able to see your custom rich presence!",
                    true,
                )
                .child(
                    button("crpc-sharing", "Enable Activity Sharing", false, false).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.toggle_pref("activity_privacy_setting", true);
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
            );
        }
        let link = |key: &'static str, label: &'static str, url: &'static str| {
            div()
                .id(key)
                .text_color(color("--text-link"))
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.follow_link(url, label, cx)))
        };
        about.push(
            div()
                .flex()
                .flex_wrap()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child("Go to the\u{a0}")
                .child(link(
                    "crpc-portal",
                    "Discord Developer Portal",
                    "https://discord.com/developers/applications",
                ))
                .child("\u{a0}to create an application and get the application ID.")
                .into_any_element(),
        );
        about.push(
            paragraph("Upload images in the Rich Presence tab to get the image keys.")
                .into_any_element(),
        );
        about.push(
            div()
                .flex()
                .flex_wrap()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child("If you want to use an image link, download your image and reupload the image to\u{a0}")
                .child(link("crpc-imgur", "Imgur", "https://imgur.com"))
                .child("\u{a0}and get the image link by right-clicking the image and selecting \"Copy image address\".")
                .into_any_element(),
        );
        about.push(paragraph("You can't see your own buttons on your profile, but everyone else can see it fine.").into_any_element());
        about.push(
            paragraph("Some weird unicode text (\"fonts\" 𝖑𝖎𝖐𝖊 𝖙𝖍𝖎𝖘) may cause the rich presence to not show up, try using normal letters instead.")
                .into_any_element(),
        );
        about.push(divider().into_any_element());
        // The activity as the profile draws it: the plugin's entry of your own presence.
        if let Some(me) = self.stores.current_user.as_ref().map(|user| user.id) {
            let index = self.stores.presence.get(me, None).and_then(|presence| {
                presence.activities.iter().position(|activity| {
                    activity["name"].as_str() == Some(config.app_name.as_str())
                        && activity["type"].as_u64() != Some(4)
                })
            });
            if let Some(index) = index {
                let cards = self.live_activity_cards_picked(
                    me,
                    super::super::live_activity::Surface::Popout,
                    color("--text-strong").into(),
                    color("--text-muted").into(),
                    Some(&[(index, true)]),
                    cx,
                );
                about.push(
                    div()
                        .w(px(284.))
                        .rounded(px(8.))
                        .bg(color("--background-mod-muted"))
                        .children(cards)
                        .into_any_element(),
                );
            }
        }
        // `PresetSettings`.
        let presets = self.custom_rpc_presets();
        let (name_editor, selected, typed_name) = FORM.with(|form| {
            let form = form.borrow();
            let editor = form.preset_name.as_ref().map(|(editor, _)| editor.clone());
            (editor, form.selected_preset.clone(), String::new())
        });
        let typed_name = name_editor.as_ref().map_or(typed_name, |editor| {
            editor.read(cx).text().trim().to_owned()
        });
        let field_box = |child: AnyElement, error: bool| {
            div()
                .min_h(px(40.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(color(if error {
                    "--text-feedback-critical"
                } else {
                    "--input-border-default"
                }))
                .bg(color("--input-background-default"))
                .child(child)
        };
        let mut form: Vec<AnyElement> = Vec::new();
        form.push(heading("Presets").into_any_element());
        form.push(
            div()
                .flex()
                .gap(px(8.))
                .children(name_editor.map(|editor| {
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(field_box(editor.into_any_element(), false))
                }))
                .child(
                    button("crpc-save", "Save", false, typed_name.is_empty()).when(
                        !typed_name.is_empty(),
                        |this| {
                            this.on_click(
                                cx.listener(|this, _, _, cx| this.custom_rpc_save_preset(cx)),
                            )
                        },
                    ),
                )
                .into_any_element(),
        );
        if presets.is_empty() {
            form.push(paragraph("No saved presets yet.").into_any_element());
        } else {
            let rows = presets.iter().enumerate().map(|(index, (name, _))| {
                let chosen = *name == selected;
                let pick = name.clone();
                div()
                    .id(SharedString::from(format!("crpc-preset-{index}")))
                    .p(px(8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(chosen, |this| this.bg(color("--background-mod-normal")))
                    .hover(|style| style.bg(color("--background-mod-muted")))
                    .text_size(px(14.))
                    .text_color(color("--text-default"))
                    .child(name.clone())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        FORM.with(|form| form.borrow_mut().selected_preset = pick.clone());
                        cx.notify();
                    }))
            });
            let none = selected.is_empty();
            form.push(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1().flex().flex_col().gap(px(2.)).children(rows))
                    .child(
                        button("crpc-load", "Load", false, none).when(!none, |this| {
                            this.on_click(
                                cx.listener(|this, _, _, cx| this.custom_rpc_load_preset(cx)),
                            )
                        }),
                    )
                    .child(
                        button("crpc-delete", "Delete", true, none).when(!none, |this| {
                            this.on_click(
                                cx.listener(|this, _, _, cx| this.custom_rpc_delete_preset(cx)),
                            )
                        }),
                    )
                    .into_any_element(),
            );
        }
        form.push(divider().into_any_element());
        let select = |key: &'static str,
                      label: &str,
                      options: &'static [(&'static str, u64)],
                      value: u64,
                      cx: &Context<Self>| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(heading(label))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .children(options.iter().map(move |(name, option)| {
                            let option = *option;
                            div()
                                .id(SharedString::from(format!("crpc-{key}-{option}")))
                                .p(px(8.))
                                .rounded(px(8.))
                                .cursor_pointer()
                                .when(value == option, |this| {
                                    this.bg(color("--background-mod-normal"))
                                })
                                .hover(|style| style.bg(color("--background-mod-muted")))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(super::super::create_channel::radio(value == option))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(*name),
                                )
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    crate::plugins::update(|state| {
                                        state.set_value(ID, key, &Value::from(option));
                                    });
                                    cx.notify();
                                }))
                        })),
                )
                .into_any_element()
        };
        form.push(select("type", "Activity Type", TYPES, config.kind, cx));
        let (fields, errors) = FORM.with(|form| {
            let form = form.borrow();
            (
                form.fields
                    .iter()
                    .map(|(key, (editor, _))| (*key, editor.clone()))
                    .collect::<HashMap<_, _>>(),
                form.errors.clone(),
            )
        });
        let single =
            |key: &'static str| -> AnyElement {
                let label = FIELDS
                    .iter()
                    .find(|(known, _, _)| *known == key)
                    .map_or("", |(_, label, _)| *label);
                let off = disabled(key, &config);
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .when(off, |this| this.opacity(0.5))
                    .child(heading(label))
                    .children(fields.get(key).cloned().map(|editor| {
                        field_box(editor.into_any_element(), errors.contains_key(key))
                    }))
                    .children(errors.get(key).map(|error| {
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-feedback-critical"))
                            .child(error.clone())
                    }))
                    .into_any_element()
            };
        let pair = |left: &'static str, right: &'static str| -> AnyElement {
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(single(left))
                .child(single(right))
                .into_any_element()
        };
        form.push(pair("appID", "appName"));
        form.push(pair("details", "detailsURL"));
        form.push(pair("state", "stateURL"));
        form.push(single("streamLink"));
        form.push(pair("partySize", "partyMaxSize"));
        form.push(divider().into_any_element());
        form.push(pair("imageBig", "imageBigTooltip"));
        form.push(single("imageBigURL"));
        form.push(pair("imageSmall", "imageSmallTooltip"));
        form.push(single("imageSmallURL"));
        form.push(divider().into_any_element());
        form.push(pair("buttonOneText", "buttonOneURL"));
        form.push(pair("buttonTwoText", "buttonTwoURL"));
        form.push(divider().into_any_element());
        form.push(select(
            "timestampMode",
            "Timestamp Mode",
            MODES,
            config.timestamp_mode,
            cx,
        ));
        form.push(pair("startTime", "endTime"));
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(about)
            .child(div().flex().flex_col().gap(px(8.)).children(form))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_check_as_the_form_checks() {
        assert_eq!(checked(Check::Number, "12", 0), Ok(Value::from(12u64)));
        assert!(checked(Check::Required(128), "", 0).is_err());
        assert!(checked(Check::Stream, "x", rpc::STREAMING).is_err());
        assert!(checked(Check::Stream, "x", rpc::PLAYING).is_ok());
        let config = Config::default();
        assert!(disabled("streamLink", &config));
        assert!(!disabled("partySize", &config));
        assert!(disabled("startTime", &config));
    }
}
