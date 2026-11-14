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

/// How a registered widget may be handled while unlocked (module 83880 `m`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetConfig {
    pub kind: &'static str,
    pub resize_x: bool,
    pub resize_y: bool,
    /// A press anywhere on the widget starts a move (else only its own drag handle does).
    pub drag_anywhere: bool,
}

const fn config(
    kind: &'static str,
    resize_x: bool,
    resize_y: bool,
    drag_anywhere: bool,
) -> WidgetConfig {
    WidgetConfig {
        kind,
        resize_x,
        resize_y,
        drag_anywhere,
    }
}

pub const CONFIGS: &[WidgetConfig] = &[
    config(VOICE, false, false, true),
    config(VIDEO, true, true, true),
    config(GO_LIVE, true, true, true),
    config(QUICK_ACTIONS, false, false, true),
    config(NOTIFICATIONS, false, false, true),
    config(ACTIVITY, false, false, true),
    config(TEXT_CHAT, true, true, false),
    config(FRIENDS, false, true, true),
];

pub fn config_of(kind: &str) -> Option<&'static WidgetConfig> {
    CONFIGS.iter().find(|config| config.kind == kind)
}

/// One side of a stored size: `"auto"` or a number (a fraction of the window, or pixels when the size is fixed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    Auto,
    Value(f32),
}

/// A widget's stored size (`size` of class `E`, module 83880).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: Extent,
    pub height: Extent,
    pub fixed: bool,
}

impl Size {
    fn from_json(value: &Value) -> Self {
        let extent = |value: &Value| match value {
            Value::String(text) if text == "auto" => Extent::Auto,
            other => Extent::Value(other.as_f64().unwrap_or(0.) as f32),
        };
        Self {
            width: extent(&value["width"]),
            height: extent(&value["height"]),
            fixed: value["fixed"].as_bool().unwrap_or(false),
        }
    }

    fn to_json(self) -> Value {
        let extent = |extent: Extent| match extent {
            Extent::Auto => json!("auto"),
            Extent::Value(value) => json!(value),
        };
        let mut map = Map::new();
        map.insert("width".into(), extent(self.width));
        map.insert("height".into(), extent(self.height));
        if self.fixed {
            map.insert("fixed".into(), json!(true));
        }
        Value::Object(map)
    }

    /// `fd` (module 5463): the size in pixels of a window, `None` for auto.
    pub fn pixels(self, width: f32, height: f32) -> (Option<f32>, Option<f32>) {
        let at = |extent: Extent, size: f32| match extent {
            Extent::Auto => None,
            Extent::Value(value) if self.fixed => Some(value),
            Extent::Value(value) => Some((value.clamp(0., 1.) * size).floor()),
        };
        (at(self.width, width), at(self.height, height))
    }

    /// `R9` (module 5463): a size in pixels as it is stored (fractions unless fixed); auto sides stay auto.
    pub fn stored(self, pixels: (f32, f32), width: f32, height: f32) -> Self {
        let at = |extent: Extent, value: f32, size: f32| match extent {
            Extent::Auto => Extent::Auto,
            Extent::Value(_) if self.fixed => Extent::Value(value),
            Extent::Value(_) => Extent::Value(value / size),
        };
        Self {
            width: at(self.width, pixels.0, width),
            height: at(self.height, pixels.1, height),
            fixed: self.fixed,
        }
    }
}

/// A widget of the overlay's layout as it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: String,
    pub kind: String,
    pub anchor: Anchor,
    pub size: Size,
    /// `minSize`; a missing side is 0 to the container (`R ?? {width: 0, height: 0}`).
    pub min_size: (f32, f32),
    pub pinned: bool,
    pub hidden: bool,
    pub z_index: i64,
    pub opacity: f32,
    raw: Map<String, Value>,
}

fn side(value: &Value, name: &str) -> Option<f32> {
    value.get(name).and_then(Value::as_f64).map(|v| v as f32)
}

/// Class `E`'s constructor: a pair with both sides set keeps both only for an auto size; otherwise the bottom (or
/// right) wins. A pair with neither side starts at 0 from the top (or left).
fn normalized(anchor: Anchor, size: Size) -> Anchor {
    let Anchor {
        mut top,
        mut left,
        mut bottom,
        mut right,
    } = anchor;
    if !(top.is_some() && bottom.is_some() && size.height == Extent::Auto) {
        if bottom.is_some() {
            top = None;
        } else {
            bottom = None;
        }
    }
    if !(left.is_some() && right.is_some() && size.width == Extent::Auto) {
        if right.is_some() {
            left = None;
        } else {
            right = None;
        }
    }
    if top.is_none() && bottom.is_none() {
        top = Some(0.);
    }
    if left.is_none() && right.is_none() {
        left = Some(0.);
    }
    Anchor {
        top,
        left,
        bottom,
        right,
    }
}

fn anchor_json(anchor: Anchor) -> Value {
    // `undefined` sides are left out of the stored object, as `JSON.stringify` does.
    let mut map = Map::new();
    for (name, value) in [
        ("top", anchor.top),
        ("left", anchor.left),
        ("bottom", anchor.bottom),
        ("right", anchor.right),
    ] {
        if let Some(value) = value {
            map.insert(name.into(), json!(value));
        }
    }
    Value::Object(map)
}

impl Widget {
    fn from_json(value: &Value) -> Option<Self> {
        let raw = value.as_object()?.clone();
        let size = Size::from_json(&value["size"]);
        let anchor = value
            .get("anchor")
            .map_or(Anchor::default(), |anchor| Anchor {
                top: side(anchor, "top"),
                left: side(anchor, "left"),
                bottom: side(anchor, "bottom"),
                right: side(anchor, "right"),
            });
        let min = &value["minSize"];
        Some(Self {
            id: value["id"].as_str()?.to_owned(),
            kind: value["type"].as_str()?.to_owned(),
            anchor: normalized(anchor, size),
            size,
            min_size: (
                side(min, "width").unwrap_or(0.),
                side(min, "height").unwrap_or(0.),
            ),
            pinned: value["pinned"].as_bool().unwrap_or(false),
            hidden: value["hidden"].as_bool().unwrap_or(false),
            z_index: value["zIndex"].as_i64().unwrap_or(0),
            opacity: value["opacity"].as_f64().unwrap_or(1.) as f32,
            raw,
        })
    }

    fn to_json(&self) -> Value {
        let mut map = self.raw.clone();
        map.insert("anchor".into(), anchor_json(self.anchor));
        map.insert("size".into(), self.size.to_json());
        map.insert("pinned".into(), json!(self.pinned));
        map.insert("hidden".into(), json!(self.hidden));
        map.insert("zIndex".into(), json!(self.z_index));
        Value::Object(map)
    }

    /// The widget's handling while unlocked; an unknown kind can be neither moved nor resized.
    pub fn config(&self) -> WidgetConfig {
        config_of(&self.kind)
            .copied()
            .unwrap_or(config("", false, false, false))
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
            "anchor": anchor_json(default.anchor),
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

    /// `getDefaultLayout(GJ, 4)` with fresh ids from `new_id`, each at its place's z-index (`LAYOUT_CREATE`).
    fn defaults(new_id: &mut dyn FnMut() -> String) -> Vec<Widget> {
        DEFAULTS
            .iter()
            .enumerate()
            .map(|(index, default)| {
                let mut widget = Widget::from_default(default, new_id());
                widget.z_index = index as i64;
                widget
            })
            .collect()
    }

    /// Applies `change` to the overlay layout's widget `id` (`T` of module 83880: only a widget of a layout that
    /// exists). Returns whether it was found.
    fn change(&mut self, id: &str, change: impl FnOnce(&mut Widget)) -> bool {
        let mut widgets = self.widgets();
        let Some(widget) = widgets.iter_mut().find(|widget| widget.id == id) else {
            return false;
        };
        change(widget);
        self.put(widgets, false);
        true
    }

    /// `LAYOUT_SET_PINNED`: the given state, or the other one.
    pub fn set_pinned(&mut self, id: &str, pinned: Option<bool>) -> bool {
        self.change(id, |widget| {
            widget.pinned = pinned.unwrap_or(!widget.pinned)
        })
    }

    /// `LAYOUT_SET_WIDGETS_HIDDEN`.
    pub fn set_hidden(&mut self, ids: &[String], hidden: bool) -> bool {
        let mut widgets = self.widgets();
        let mut changed = false;
        for widget in widgets.iter_mut().filter(|widget| ids.contains(&widget.id)) {
            if widget.hidden != hidden {
                widget.hidden = hidden;
                changed = true;
            }
        }
        if changed {
            self.put(widgets, false);
        }
        changed
    }

    /// `ff` (module 667419), the toolbar's Hide All: every widget of the layout hidden.
    pub fn hide_all(&mut self) -> bool {
        let ids: Vec<String> = self.widgets().into_iter().map(|widget| widget.id).collect();
        self.set_hidden(&ids, true)
    }

    /// `LAYOUT_UPDATE_WIDGET` with what a finished move or resize gives: the anchor and size as stored (fractions
    /// unless fixed).
    pub fn update_widget(&mut self, id: &str, anchor: Option<Anchor>, size: Option<Size>) -> bool {
        self.change(id, |widget| {
            if let Some(anchor) = anchor {
                widget.anchor = anchor;
            }
            if let Some(size) = size {
                widget.size = size;
            }
        })
    }

    /// `LAYOUT_SET_TOP_WIDGET`: the widget goes over the others, every z-index renumbered from 0 in drawing order.
    pub fn set_top(&mut self, id: &str) -> bool {
        let mut widgets = self.widgets();
        let mut order: Vec<usize> = (0..widgets.len()).collect();
        order.sort_by_key(|&index| widgets[index].z_index);
        let Some(at) = order.iter().position(|&index| widgets[index].id == id) else {
            return false;
        };
        if at == order.len() - 1 {
            return false;
        }
        let moved = order.remove(at);
        order.push(moved);
        for (z, index) in order.into_iter().enumerate() {
            widgets[index].z_index = z as i64;
        }
        self.put(widgets, false);
        true
    }

    /// The widgets drawn, lowest z-index first (the order of the layout breaks ties, as the page's does).
    pub fn drawn(&self) -> Vec<Widget> {
        let mut widgets: Vec<Widget> = self
            .widgets()
            .into_iter()
            .filter(|widget| !widget.hidden)
            .collect();
        widgets.sort_by_key(|widget| widget.z_index);
        widgets
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
            // `LAYOUT_CREATE_WIDGETS`: the new one goes on top.
            let mut widget = Widget::from_default(default, new_id());
            widget.z_index = widgets.len() as i64;
            widgets.push(widget);
        }
        let first = widgets
            .iter()
            .find(|widget| widget.kind == kind)
            .map(|widget| widget.id.clone());
        self.put(widgets, true);
        // `JA`: a widget shown again comes to the top.
        if found
            && on
            && let Some(first) = first
        {
            self.set_top(&first);
        }
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
    fn a_stored_anchor_keeps_one_side_of_each_pair() {
        let stored = json!({
            "layouts": {LAYOUT: {"id": LAYOUT, "widgets": ["a", "b"], "version": 4}},
            "widgets": {
                "a": {"id": "a", "layoutId": LAYOUT, "type": VOICE,
                      "anchor": {"top": 0.1, "bottom": 0.2, "left": 0.3}, "size": {"width": "auto", "height": "auto"}},
                "b": {"id": "b", "layoutId": LAYOUT, "type": VIDEO,
                      "anchor": {"top": 0.1, "bottom": 0.2}, "size": {"width": 408, "height": 128, "fixed": true}},
            },
        });
        let layout = Layout::from_json(&stored);
        let widgets = layout.widgets();
        // An auto size keeps both; a set size keeps the bottom; a missing left/right starts at the left.
        assert_eq!(widgets[0].anchor.top, Some(0.1));
        assert_eq!(widgets[0].anchor.bottom, Some(0.2));
        assert_eq!(widgets[1].anchor.top, None);
        assert_eq!(widgets[1].anchor.left, Some(0.));
        assert_eq!(
            widgets[1].size.pixels(1280., 720.),
            (Some(408.), Some(128.))
        );
        assert!(widgets[0].config().drag_anywhere);
        assert!(!widgets[0].config().resize_x);
        assert!(widgets[1].config().resize_x);
    }

    #[test]
    fn a_finished_drag_is_stored_and_read_back() {
        let mut layout = Layout::default();
        let mut ids = counter();
        layout.ensure(&mut ids);
        let voice = layout.widget(VOICE).unwrap();
        let moved = Anchor {
            bottom: Some(0.5),
            right: Some(0.25),
            ..Anchor::default()
        };
        assert!(layout.update_widget(&voice.id, Some(moved), None));
        assert!(layout.set_top(&voice.id));
        let stored = layout.to_json();
        // `undefined` sides are not written.
        let anchor = &stored["widgets"][voice.id.as_str()]["anchor"];
        assert_eq!(anchor, &json!({"bottom": 0.5, "right": 0.25}));
        let read = Layout::from_json(&stored);
        let back = read.widget(VOICE).unwrap();
        assert_eq!(back.anchor, moved);
        assert_eq!(back.z_index, 7);
        assert_eq!(read.drawn().last().unwrap().kind, VOICE);
        // Already on top: nothing to do.
        let mut again = read.clone();
        assert!(!again.set_top(&voice.id));
        // A widget of no layout cannot be changed.
        assert!(!Layout::default().set_pinned("nobody", None));
    }

    #[test]
    fn pin_hide_and_hide_all_follow_the_store() {
        let mut layout = Layout::default();
        let mut ids = counter();
        layout.ensure(&mut ids);
        let voice = layout.widget(VOICE).unwrap();
        assert!(voice.pinned);
        layout.set_pinned(&voice.id, None);
        assert!(!layout.widget(VOICE).unwrap().pinned);
        layout.set_pinned(&voice.id, Some(true));
        assert!(layout.widget(VOICE).unwrap().pinned);
        // The widget's Hide button.
        assert!(layout.set_hidden(std::slice::from_ref(&voice.id), true));
        assert!(!layout.is_enabled(VOICE));
        assert!(!layout.set_hidden(std::slice::from_ref(&voice.id), true));
        // Shown again from the page, it comes to the top.
        layout.set_enabled(VOICE, true, &mut ids);
        assert!(layout.is_enabled(VOICE));
        assert_eq!(layout.drawn().last().unwrap().kind, VOICE);
        assert!(layout.hide_all());
        assert!(layout.drawn().is_empty());
        let size = Size {
            width: Extent::Value(0.5),
            height: Extent::Auto,
            fixed: false,
        };
        assert_eq!(size.pixels(1280., 720.), (Some(640.), None));
        let stored = size.stored((320., 99.), 1280., 720.);
        assert_eq!(stored.width, Extent::Value(0.25));
        assert_eq!(stored.height, Extent::Auto);
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

    #[test]
    fn quick_actions_and_text_chat_do_not_overlap_by_default() {
        const QUICK_ACTIONS_WRAPPER_HEIGHT: f32 = 84.;
        const EXTRAS_CLEARANCE: f32 = 10. + 24.;
        let mut layout = Layout::default();
        let mut ids = counter();
        layout.ensure(&mut ids);
        let quick = layout.widget(QUICK_ACTIONS).unwrap();
        let chat = layout.widget(TEXT_CHAT).unwrap();
        assert_eq!(quick.anchor.top, Some(0.02));
        assert_eq!(chat.anchor.top, Some(0.14));

        // 1280x720: Quick Actions wrapper Y=14..98 clears Text Chat Y=100..472.
        let q720 = anchor_pixels(quick.anchor, 1280., 720.);
        let c720 = anchor_pixels(chat.anchor, 1280., 720.);
        let (_, chat_h) = crate::drag::requested_size(chat.size.pixels(1280., 720.), chat.min_size);
        let q720_edges =
            crate::drag::resolve(q720, 1280., 720., 356., QUICK_ACTIONS_WRAPPER_HEIGHT);
        let c720_edges = crate::drag::resolve(c720, 1280., 720., 500., chat_h.unwrap());
        assert_eq!(q720_edges.top, 14.);
        assert_eq!(q720_edges.top + QUICK_ACTIONS_WRAPPER_HEIGHT, 98.);
        assert_eq!(c720_edges.top, 100.);
        assert_eq!(c720_edges.top + chat_h.unwrap(), 472.);
        assert!(q720_edges.top + QUICK_ACTIONS_WRAPPER_HEIGHT < c720_edges.top);

        // 1920x1080: Quick Actions + floating `.extras_yjvjn7` (105..139) fully clears Text Chat (151..523).
        let q1080 = anchor_pixels(quick.anchor, 1920., 1080.);
        let c1080 = anchor_pixels(chat.anchor, 1920., 1080.);
        assert_eq!(q1080.top, Some(21.));
        assert_eq!(c1080.top, Some(151.));
        assert!(
            q1080.top.unwrap() + QUICK_ACTIONS_WRAPPER_HEIGHT + EXTRAS_CLEARANCE
                < c1080.top.unwrap()
        );
    }
}
