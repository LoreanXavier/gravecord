//! The overlay's widget layout: `LayoutStore` (module 83880, persisted under `LayoutStore` as `{layouts, widgets}`),
//! the v4 defaults of its registered widgets, the Game Overlay page's widget switches (module 667419 `$e`, `JA`, `j$`,
//! `eL`) and the anchor maths of module 5463.
//!
//! Only the overlay's layout (`overlay_v3_default`, module 644434) is read and written here; every other layout and
//! widget of a stored object is kept as it was.

use serde_json::{Map, Value, json};

pub const STORE: &str = "LayoutStore";
/// `GJ` (module 644434).
pub const LAYOUT: &str = "overlay_v3_default";
/// The default layout's version (`resetDefaultLayout(GJ, 4)`).
pub const VERSION: u64 = 4;

pub const VOICE: &str = "VOICE_V3";
pub const QUICK_ACTIONS: &str = "QUICK_ACTIONS";
pub const NOTIFICATIONS: &str = "NOTIFICATIONS";
pub const TEXT_CHAT: &str = "TEXT_CHAT_V3";
pub const ACTIVITY: &str = "ACTIVITY";
pub const VIDEO: &str = "VIDEO";
pub const GO_LIVE: &str = "GO_LIVE";
pub const FRIENDS: &str = "FRIENDS";

/// `meta.voiceStatesMaxShown` of the voice widget when unset (module 158935 `p$`, module 502812 `ew`).
pub const VOICE_MAX_SHOWN_DEFAULT: i64 = 8;

/// One side of an anchor: a fraction of the overlay's size, or unset.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Anchor {
    pub top: Option<f32>,
    pub left: Option<f32>,
    pub bottom: Option<f32>,
    pub right: Option<f32>,
}

/// A widget's size: `auto`, a fraction of the overlay, or fixed pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Fraction(f32),
    Pixels(f32),
}

/// A registered widget's `defaultSettings` at version 4 (module 83880 `m`).
pub struct WidgetDefault {
    pub kind: &'static str,
    pub anchor: Anchor,
    pub width: Length,
    pub height: Length,
    pub pinned: bool,
    pub min_size: (f32, f32),
}

const fn anchor(
    top: Option<f32>,
    left: Option<f32>,
    bottom: Option<f32>,
    right: Option<f32>,
) -> Anchor {
    Anchor {
        top,
        left,
        bottom,
        right,
    }
}

/// The widgets whose layout policy is REQUIRED or OPTIONAL_DEFAULT at version 4, in registration order.
pub const DEFAULTS: &[WidgetDefault] = &[
    WidgetDefault {
        kind: VOICE,
        anchor: anchor(Some(0.35), Some(0.01), None, None),
        width: Length::Auto,
        height: Length::Auto,
        pinned: true,
        min_size: (272., 24.),
    },
    WidgetDefault {
        kind: VIDEO,
        anchor: anchor(None, Some(0.01), Some(0.02), None),
        width: Length::Pixels(408.),
        height: Length::Pixels(128.),
        pinned: true,
        min_size: (128., 128.),
    },
    WidgetDefault {
        kind: GO_LIVE,
        anchor: anchor(None, None, Some(0.02), Some(0.01)),
        width: Length::Pixels(256.),
        height: Length::Pixels(144.),
        pinned: false,
        min_size: (276., 164.),
    },
    WidgetDefault {
        kind: QUICK_ACTIONS,
        anchor: anchor(Some(0.02), Some(0.34), None, None),
        width: Length::Auto,
        height: Length::Auto,
        pinned: false,
        min_size: (216., 56.),
    },
    WidgetDefault {
        kind: NOTIFICATIONS,
        anchor: anchor(Some(0.02), Some(0.01), None, None),
        width: Length::Auto,
        height: Length::Auto,
        pinned: true,
        min_size: (320., 100.),
    },
    WidgetDefault {
        kind: ACTIVITY,
        anchor: anchor(Some(0.35), None, None, Some(0.01)),
        width: Length::Auto,
        height: Length::Auto,
        pinned: false,
        min_size: (320., 100.),
    },
    WidgetDefault {
        kind: TEXT_CHAT,
        anchor: anchor(Some(0.14), None, None, Some(0.26)),
        width: Length::Pixels(500.),
        height: Length::Pixels(372.),
        pinned: false,
        min_size: (350., 100.),
    },
    WidgetDefault {
        kind: FRIENDS,
        anchor: anchor(Some(0.14), None, Some(0.3), Some(0.01)),
        width: Length::Pixels(350.),
        height: Length::Auto,
        pinned: false,
        min_size: (350., 350.),
    },
];

pub fn default_of(kind: &str) -> Option<&'static WidgetDefault> {
    DEFAULTS.iter().find(|widget| widget.kind == kind)
}

/// A widget of the overlay's layout as it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: String,
    pub kind: String,
    pub anchor: Anchor,
    pub pinned: bool,
    pub hidden: bool,
    pub opacity: f32,
    raw: Map<String, Value>,
}

fn side(value: &Value, name: &str) -> Option<f32> {
    value.get(name).and_then(Value::as_f64).map(|v| v as f32)
}

impl Widget {
    fn from_json(value: &Value) -> Option<Self> {
        let raw = value.as_object()?.clone();
        let anchor = value
            .get("anchor")
            .map_or(Anchor::default(), |anchor| Anchor {
                top: side(anchor, "top"),
                left: side(anchor, "left"),
                bottom: side(anchor, "bottom"),
                right: side(anchor, "right"),
            });
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            kind: value["type"].as_str()?.to_owned(),
            anchor,
            pinned: value["pinned"].as_bool().unwrap_or(false),
            hidden: value["hidden"].as_bool().unwrap_or(false),
            opacity: value["opacity"].as_f64().unwrap_or(1.) as f32,
            raw,
        })
    }

    fn to_json(&self) -> Value {
        let mut map = self.raw.clone();
        map.insert("pinned".into(), json!(self.pinned));
        map.insert("hidden".into(), json!(self.hidden));
        Value::Object(map)
    }

    fn from_default(default: &WidgetDefault, id: String) -> Self {
        let length = |length: Length| match length {
            Length::Auto => json!("auto"),
            Length::Fraction(value) => json!(value),
            Length::Pixels(value) => json!(value),
        };
        let fixed = matches!(default.width, Length::Pixels(_))
            || matches!(default.height, Length::Pixels(_));
        let mut size = Map::new();
        size.insert("width".into(), length(default.width));
        size.insert("height".into(), length(default.height));
        if fixed {
            size.insert("fixed".into(), json!(true));
        }
        let raw = json!({
            "id": id,
            "layoutId": LAYOUT,
            "type": default.kind,
            "anchor": {
                "top": default.anchor.top,
                "left": default.anchor.left,
                "bottom": default.anchor.bottom,
                "right": default.anchor.right,
            },
            "size": Value::Object(size),
            "minSize": {"width": default.min_size.0, "height": default.min_size.1},
            "pinned": default.pinned,
            "hidden": false,
            "zIndex": 0,
            "opacity": 1,
            "meta": {},
        });
        Self::from_json(&raw).expect("a default widget is complete")
    }

    pub fn meta(&self, key: &str) -> Option<&Value> {
        self.raw.get("meta").and_then(|meta| meta.get(key))
    }
}

/// The stored `LayoutStore` object, with the overlay's layout read out.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    stored: Map<String, Value>,
}

impl Layout {
    pub fn from_json(value: &Value) -> Self {
        Self {
            stored: value.as_object().cloned().unwrap_or_default(),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut map = self.stored.clone();
        map.entry("layouts").or_insert_with(|| json!({}));
        map.entry("widgets").or_insert_with(|| json!({}));
        Value::Object(map)
    }

    fn layout(&self) -> Option<&Value> {
        self.stored.get("layouts")?.get(LAYOUT)
    }

    /// The overlay layout's widgets, in its order.
    pub fn widgets(&self) -> Vec<Widget> {
        let Some(layout) = self.layout() else {
            return Vec::new();
        };
        let all = self.stored.get("widgets");
        layout["widgets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|id| all?.get(id.as_str()?))
            .filter_map(Widget::from_json)
            .filter(|widget| widget.raw.get("layoutId").and_then(Value::as_str) == Some(LAYOUT))
            .collect()
    }

    /// The first widget of a kind in the overlay's layout, or the default one when there is no layout yet.
    pub fn widget(&self, kind: &str) -> Option<Widget> {
        if self.layout().is_none() {
            return default_of(kind).map(|default| Widget::from_default(default, String::new()));
        }
        self.widgets()
            .into_iter()
            .find(|widget| widget.kind == kind)
    }

    /// `$e` (module 667419): the switch of a widget kind on the Game Overlay page.
    pub fn is_enabled(&self, kind: &str) -> bool {
        if self.layout().is_none() {
            return default_of(kind).is_some();
        }
        self.widgets()
            .iter()
            .any(|widget| widget.kind == kind && !widget.hidden)
    }

    fn put(&mut self, widgets: Vec<Widget>, replace: bool) {
        let mut all = self
            .stored
            .get("widgets")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if replace {
            all.retain(|_, widget| widget["layoutId"] != LAYOUT);
        }
        let ids: Vec<Value> = widgets.iter().map(|widget| json!(widget.id)).collect();
        for widget in widgets {
            all.insert(widget.id.clone(), widget.to_json());
        }
        let mut layouts = self
            .stored
            .get("layouts")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        layouts.insert(
            LAYOUT.to_owned(),
            json!({"id": LAYOUT, "widgets": ids, "version": VERSION}),
        );
        self.stored.insert("layouts".into(), Value::Object(layouts));
        self.stored.insert("widgets".into(), Value::Object(all));
    }

    /// `getDefaultLayout(GJ, 4)` with fresh ids from `new_id`.
    fn defaults(new_id: &mut dyn FnMut() -> String) -> Vec<Widget> {
        DEFAULTS
            .iter()
            .map(|default| Widget::from_default(default, new_id()))
            .collect()
    }

    /// `j$`: the layout is created with the defaults unless it exists at this version.
    pub fn ensure(&mut self, new_id: &mut dyn FnMut() -> String) {
        if self.layout().and_then(|layout| layout["version"].as_u64()) == Some(VERSION) {
            return;
        }
        let widgets = Self::defaults(new_id);
        self.put(widgets, true);
    }

    /// The page's `setValue`: `j$()` then `JA(kind, !on)`.
    pub fn set_enabled(&mut self, kind: &str, on: bool, new_id: &mut dyn FnMut() -> String) {
        self.ensure(new_id);
        let mut widgets = self.widgets();
        let mut found = false;
        for widget in widgets.iter_mut().filter(|widget| widget.kind == kind) {
            widget.hidden = !on;
            found = true;
        }
        if !found
            && on
            && let Some(default) = default_of(kind)
        {
            widgets.push(Widget::from_default(default, new_id()));
        }
        self.put(widgets, true);
    }

    /// `eL`: `resetDefaultLayout(GJ, 4)`.
    pub fn reset(&mut self, new_id: &mut dyn FnMut() -> String) {
        let widgets = Self::defaults(new_id);
        self.put(widgets, true);
    }

    /// `voiceStatesMaxShown` of the voice widget: -1 (no limit, "Off") or the count.
    pub fn voice_max_shown(&self) -> i64 {
        self.widget(VOICE)
            .and_then(|widget| widget.meta("voiceStatesMaxShown").and_then(Value::as_i64))
            .unwrap_or(VOICE_MAX_SHOWN_DEFAULT)
    }

    /// `p$.setValue`: under 1 is stored as -1, otherwise the floor (`LAYOUT_SET_WIDGET_META`).
    pub fn set_voice_max_shown(&mut self, value: f32, new_id: &mut dyn FnMut() -> String) {
        self.ensure(new_id);
        let stored = if value < 1. { -1 } else { value.floor() as i64 };
        let mut widgets = self.widgets();
        if let Some(widget) = widgets.iter_mut().find(|widget| widget.kind == VOICE) {
            let mut meta = widget
                .raw
                .get("meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            meta.insert("voiceStatesMaxShown".into(), json!(stored));
            widget.raw.insert("meta".into(), Value::Object(meta));
        }
        self.put(widgets, true);
    }
}

/// `r` of module 5463: the anchor in pixels of a container, each side `floor(clamp(fraction) * size)`.
pub fn anchor_pixels(anchor: Anchor, width: f32, height: f32) -> Anchor {
    let at = |fraction: Option<f32>, size: f32| {
        fraction.map(|value| (value.clamp(0., 1.) * size).floor())
    };
    Anchor {
        top: at(anchor.top, height),
        left: at(anchor.left, width),
        bottom: at(anchor.bottom, height),
        right: at(anchor.right, width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> String {
        let mut next = 0;
        move || {
            next += 1;
            format!("w{next}")
        }
    }

    #[test]
    fn widgets_are_on_until_the_layout_says_otherwise() {
        let mut layout = Layout::from_json(&Value::Null);
        for kind in [
            VOICE,
            QUICK_ACTIONS,
            NOTIFICATIONS,
            TEXT_CHAT,
            ACTIVITY,
            VIDEO,
            GO_LIVE,
        ] {
            assert!(layout.is_enabled(kind), "{kind}");
        }
        let mut ids = counter();
        layout.set_enabled(VOICE, false, &mut ids);
        assert!(!layout.is_enabled(VOICE));
        assert!(layout.is_enabled(VIDEO));
        let stored = layout.to_json();
        assert_eq!(stored["layouts"][LAYOUT]["version"], 4);
        assert_eq!(
            stored["layouts"][LAYOUT]["widgets"]
                .as_array()
                .map(Vec::len),
            Some(8)
        );
        // Read back from storage, then reset: every widget on again, new ids.
        let mut read = Layout::from_json(&stored);
        assert!(!read.is_enabled(VOICE));
        read.reset(&mut ids);
        assert!(read.is_enabled(VOICE));
        assert_eq!(read.widgets().len(), 8);
        assert!(
            read.widgets()
                .iter()
                .all(|widget| widget.id.starts_with('w'))
        );
    }

    #[test]
    fn other_layouts_survive_a_change() {
        let stored = json!({
            "layouts": {"other": {"id": "other", "widgets": ["x"], "version": 0}},
            "widgets": {"x": {"id": "x", "layoutId": "other", "type": "TEXT"}},
        });
        let mut layout = Layout::from_json(&stored);
        layout.set_enabled(TEXT_CHAT, false, &mut counter());
        let out = layout.to_json();
        assert_eq!(out["layouts"]["other"]["widgets"][0], "x");
        assert_eq!(out["widgets"]["x"]["type"], "TEXT");
    }

    #[test]
    fn the_voice_widget_keeps_its_max_users() {
        let mut layout = Layout::default();
        assert_eq!(layout.voice_max_shown(), 8);
        let mut ids = counter();
        layout.set_voice_max_shown(12.7, &mut ids);
        assert_eq!(layout.voice_max_shown(), 12);
        layout.set_voice_max_shown(0.4, &mut ids);
        assert_eq!(layout.voice_max_shown(), -1);
        let read = Layout::from_json(&layout.to_json());
        assert_eq!(read.voice_max_shown(), -1);
    }

    #[test]
    fn anchors_resolve_against_the_overlay() {
        let voice = default_of(VOICE).unwrap();
        let at = anchor_pixels(voice.anchor, 1920., 1080.);
        assert_eq!(at.left, Some(19.));
        assert_eq!(at.top, Some(378.));
        assert_eq!(at.right, None);
        let wide = anchor_pixels(
            Anchor {
                top: Some(1.5),
                ..Anchor::default()
            },
            100.,
            100.,
        );
        assert_eq!(wide.top, Some(100.));
        assert!(Layout::default().widget(VOICE).unwrap().pinned);
        assert!(!Layout::default().widget(TEXT_CHAT).unwrap().pinned);
    }
}
