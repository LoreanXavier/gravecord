//! Application widgets on the Board: the app's widget configuration (`GET /applications/<id>/widget-configs`, module
//! 382483 `un`, kept by `ApplicationWidgetConfigStore` of module 385113, the first config of an app being its config)
//! laid out by module 886100 (`kH`) over the person's application identity (`GET /users/<id>/application-identities
//! ?with_profiles=true`, module 201718, the data of its profile read by `VG` of module 90947).
//!
//! The widget card (`widgetContainer_zEJODa`: no padding, the card's menu in a header 12px down and 16px in) carries
//! the `widget_top` surface (`widget_top_hero` or `widget_top_contained`: the app's icon and name, a title and up to
//! three subtitles, the hero picture on the right or the contained picture in a box) over the `widget_bottom` surface
//! (`widget_bottom_stats`: six stats in three columns; `widget_bottom_progress`: an objective with a bar;
//! `widget_bottom_collection`: four items in two columns) under a 1px `--border-subtle` line.

use super::Root;
use super::dm_profile::Palette;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Rgba, SharedString, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, relative,
};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const CONFIG_PREFIX: &str = "app-widget-config:";
pub(super) const IDENTITY_PREFIX: &str = "app-identities:";

/// A value a field resolves to (`STRING`, `NUMBER`, `MEDIA` of module 398251).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FieldValue {
    Text(String),
    Number(f64),
    Media {
        url: String,
        width: f64,
        height: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text,
    Number,
    Media,
}

impl FieldValue {
    fn kind(&self) -> Kind {
        match self {
            Self::Text(_) => Kind::Text,
            Self::Number(_) => Kind::Number,
            Self::Media { .. } => Kind::Media,
        }
    }
}

/// A resolved field and its `presentation_type` (`text`, `number`, `image`, `duration`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Resolved {
    pub value: FieldValue,
    pub presentation: String,
}

fn media(value: &Value) -> Option<FieldValue> {
    let (width, height) = (value["width"].as_f64()?, value["height"].as_f64()?);
    (width > 0. && height > 0.).then(|| FieldValue::Media {
        url: value["proxy_url"].as_str().unwrap_or_default().to_owned(),
        width,
        height,
    })
}

/// `VG` of module 90947: an identity profile's data by name: `username`, the `data.primary` values (strings, numbers
/// and pictures with a size) and the `data.dynamic` entries (type 1 string, 2 number, 3 picture).
pub(super) fn identity_data(profile: &Value) -> HashMap<String, FieldValue> {
    let mut data = HashMap::new();
    if let Some(name) = profile["username"].as_str() {
        data.insert("username".to_owned(), FieldValue::Text(name.to_owned()));
    }
    for (key, value) in profile["data"]["primary"].as_object().into_iter().flatten() {
        let entry = match value {
            Value::String(text) => Some(FieldValue::Text(text.clone())),
            Value::Number(number) => number.as_f64().map(FieldValue::Number),
            Value::Object(object)
                if ["url", "proxy_url", "loading_state"]
                    .iter()
                    .all(|key| object.contains_key(*key)) =>
            {
                media(value)
            }
            _ => None,
        };
        if let Some(entry) = entry {
            data.insert(key.clone(), entry);
        }
    }
    for entry in profile["data"]["dynamic"].as_array().into_iter().flatten() {
        let Some(name) = entry["name"].as_str() else {
            continue;
        };
        let value = match entry["type"].as_i64() {
            Some(1) => entry["value"]
                .as_str()
                .map(|text| FieldValue::Text(text.to_owned())),
            Some(2) => entry["value"].as_f64().map(FieldValue::Number),
            Some(3) => media(&entry["value"]),
            _ => None,
        };
        if let Some(value) = value {
            data.insert(name.to_owned(), value);
        }
    }
    data
}

/// The ladder of image sizes of module 776231 (`kr`: the first not smaller than the width, else the largest).
const SIZES: [u32; 31] = [
    16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320, 480,
    512, 600, 640, 1024, 1280, 1536, 2048, 3072, 4096,
];

/// `Q` of module 549699: an application asset's picture, sized from its width.
pub(super) fn asset_url(app: &str, asset: &Value) -> String {
    let id = asset["asset_id"].as_str().unwrap_or_default();
    let width = asset["metadata"]["width"].as_f64().unwrap_or(0.);
    let size = SIZES
        .iter()
        .copied()
        .find(|size| width <= f64::from(*size))
        .unwrap_or(4096);
    let mut url = format!("https://cdn.discordapp.com/app-assets/{app}/{id}.webp?size={size}");
    if asset["metadata"]["is_animated"] == true {
        url.push_str("&animated=true");
    }
    url
}

/// What fields resolve against: the identity's data and the config's assets.
pub(super) struct Context2<'a> {
    pub app: &'a str,
    pub data: &'a HashMap<String, FieldValue>,
    pub assets: &'a [Value],
}

/// `J` of module 398251: a field of a component as one of the `accepted` kinds, or none.
pub(super) fn resolve(
    field: &Value,
    accepted: &[Kind],
    context: &Context2<'_>,
) -> Option<Resolved> {
    if !field.is_object() {
        return None;
    }
    let presentation = field["presentation_type"].as_str().unwrap_or_default();
    match field["value_type"].as_str() {
        Some("data") => {
            let name = field["value"].as_str().unwrap_or_default();
            let fits = |value: &FieldValue| {
                let allowed: &[Kind] = match presentation {
                    "text" => &[Kind::Text],
                    "number" | "duration" => &[Kind::Number],
                    "image" => &[Kind::Media],
                    _ => &[],
                };
                allowed.contains(&value.kind()) && accepted.contains(&value.kind())
            };
            match context.data.get(name).filter(|value| fits(value)) {
                // `playtime_hours` shown as a duration is given in milliseconds.
                Some(FieldValue::Number(hours))
                    if name == "playtime_hours" && presentation == "duration" =>
                {
                    Some(Resolved {
                        value: FieldValue::Number((60. * hours * 60_000.).floor()),
                        presentation: presentation.to_owned(),
                    })
                }
                Some(value) => Some(Resolved {
                    value: value.clone(),
                    presentation: presentation.to_owned(),
                }),
                None => resolve(&field["fallback"], accepted, context),
            }
        }
        Some("custom_string") => {
            (presentation == "text" && accepted.contains(&Kind::Text)).then(|| Resolved {
                value: FieldValue::Text(field["value"].as_str().unwrap_or_default().to_owned()),
                presentation: "text".to_owned(),
            })
        }
        Some("application_asset") => {
            if !accepted.contains(&Kind::Media) {
                return None;
            }
            let key = field["value"].as_str()?;
            let asset = context
                .assets
                .iter()
                .find(|asset| asset["key"].as_str() == Some(key))?;
            Some(Resolved {
                value: FieldValue::Media {
                    url: asset_url(context.app, asset),
                    width: asset["metadata"]["width"].as_f64().unwrap_or(0.),
                    height: asset["metadata"]["height"].as_f64().unwrap_or(0.),
                },
                presentation: "image".to_owned(),
            })
        }
        _ => None,
    }
}

/// `e` of module 108089 in en-US: the compact number (`1.2K`, `88`, `3.4M`), its last digit cut rather than rounded.
pub(super) fn compact_number(value: f64) -> String {
    if value == 0. || !value.is_finite() {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    let (unit, suffix) = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")]
        .into_iter()
        .find(|(unit, _)| magnitude >= *unit)
        .unwrap_or((1., ""));
    let scaled = value / unit;
    let digits = scaled.abs().log10().floor() as i32;
    let step = 10f64.powi((digits - 1).clamp(-15, 0));
    let cut = (scaled / step).floor() * step;
    let text = if cut.abs() >= 10. || cut.fract() == 0. {
        format!("{}", cut.round() as i64)
    } else {
        format!("{cut:.1}")
    };
    format!("{text}{suffix}")
}

/// `Intl.DurationFormat` with `style: "narrow"` over hours, minutes, seconds and milliseconds (module 886100 `z`):
/// the units that are not zero, `1h 22m 12s`.
pub(super) fn narrow_duration(ms: f64) -> String {
    let total = if ms.is_finite() {
        ms.max(0.).floor() as u64
    } else {
        0
    };
    let parts = [
        (total / 3_600_000, "h"),
        (total / 60_000 % 60, "m"),
        (total / 1_000 % 60, "s"),
        (total % 1_000, "ms"),
    ];
    let shown: Vec<String> = parts
        .iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect();
    if shown.is_empty() {
        "0s".to_owned()
    } else {
        shown.join(" ")
    }
}

/// A text line of `T` (`Hx` of module 779300): "label: value", the value a dash when it is empty, or none when the
/// component or both fields are missing.
pub(super) fn text_line(
    component: &Value,
    context: &Context2<'_>,
) -> Option<(String, Option<FieldValue>)> {
    if !component.is_object() {
        return None;
    }
    let fields = &component["fields"];
    let text = resolve(&fields["text"], &[Kind::Text, Kind::Number], context);
    let label = resolve(&fields["label"], &[Kind::Text, Kind::Number], context);
    if text.is_none() && label.is_none() {
        return None;
    }
    let show = |value: &FieldValue| match value {
        FieldValue::Text(text) => text.clone(),
        FieldValue::Number(number) => compact_number(*number),
        FieldValue::Media { .. } => String::new(),
    };
    let prefix = label
        .map(|label| show(&label.value))
        .filter(|label| !label.is_empty())
        .map(|label| format!("{label}: "))
        .unwrap_or_default();
    let value = text
        .map(|text| show(&text.value))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "\u{2013}".to_owned());
    let icon = resolve(&fields["icon"], &[Kind::Media], context).map(|icon| icon.value);
    Some((format!("{prefix}{value}"), icon))
}

/// A stat of `z` (`CZ` of module 779300): its value (a string, a duration or a compact number) and its label.
pub(super) fn stat(component: &Value, context: &Context2<'_>) -> Option<(String, Option<String>)> {
    if !component.is_object() {
        return None;
    }
    let fields = &component["fields"];
    let value = resolve(&fields["value"], &[Kind::Text, Kind::Number], context)?;
    let text = match &value.value {
        FieldValue::Text(text) => text.clone(),
        FieldValue::Number(number) if value.presentation == "duration" => narrow_duration(*number),
        FieldValue::Number(number) => compact_number(*number),
        FieldValue::Media { .. } => return None,
    };
    let label =
        resolve(&fields["label"], &[Kind::Text], context).and_then(|label| match label.value {
            FieldValue::Text(text) => Some(text),
            _ => None,
        });
    Some((text, label))
}

/// `eq` of module 779300: a progress bar's share in percent, 0 to 100.
pub(super) fn progress_percent(current: Option<f64>, max: Option<f64>) -> f64 {
    let share = |value: f64| {
        if value.is_nan() {
            0.
        } else {
            (100. * value).round().clamp(0., 100.)
        }
    };
    match (current, max) {
        (None, _) => 0.,
        (Some(current), None) => share(current),
        (Some(_), Some(0.)) => 0.,
        (Some(current), Some(max)) => share(current / max),
    }
}

/// The config of an application (`getConfig`: the first one of the answer).
fn first_config(body: &Value) -> Option<&Value> {
    body.as_array()
        .and_then(|configs| configs.first())
        .or_else(|| {
            body["configs"]
                .as_array()
                .and_then(|configs| configs.first())
        })
}

/// The identity of `user` for `app` (`getUserIdentityByApplication`).
fn identity<'a>(body: &'a Value, app: &str) -> Option<&'a Value> {
    body["identities"]
        .as_array()?
        .iter()
        .find(|identity| identity["application_id"].as_str() == Some(app))
}

fn opaque_over(base: Rgba, over: Rgba) -> Rgba {
    super::profile_theme::blend_over(base, over)
}

impl Root {
    /// Asks for what an application widget needs: the app's widget config and the person's identities.
    pub(super) fn request_app_widget(&mut self, user: Id, app: &str) {
        for (key, route) in [
            (
                format!("{CONFIG_PREFIX}{app}"),
                format!("/applications/{app}/widget-configs"),
            ),
            (
                format!("{IDENTITY_PREFIX}{user}"),
                format!("/users/{user}/application-identities?with_profiles=true"),
            ),
        ] {
            if !self.fetched.contains_key(&key) && self.requested.insert(key.clone()) {
                let _ = self
                    .requests
                    .send(crate::net::Request::Fetch { key, route });
            }
        }
    }

    /// A config or identity answer arrived: keep it and ask for the pictures it names.
    pub(super) fn app_widget_fetched(&mut self, key: String, body: Value, cx: &mut Context<Self>) {
        let mut urls = Vec::new();
        if let Some(app) = key.strip_prefix(CONFIG_PREFIX)
            && let Some(config) = first_config(&body)
        {
            for asset in config["resolved_assets"].as_array().into_iter().flatten() {
                urls.push(asset_url(app, asset));
            }
        }
        for identity in body["identities"].as_array().into_iter().flatten() {
            let data = identity_data(&identity["profile"]);
            for value in data.values() {
                if let FieldValue::Media { url, .. } = value
                    && !url.is_empty()
                {
                    urls.push(url.clone());
                }
            }
        }
        self.fetched.insert(key, body);
        for url in urls {
            self.request_image(url.clone(), url);
        }
        cx.notify();
    }

    /// The application widget of `user` for `app`, laid out from its config: none until the config and the identity
    /// arrived, or when the config has neither a top nor a bottom surface.
    pub(super) fn app_widget_body(
        &self,
        user: Id,
        app: &str,
        name: &str,
        icon: Option<std::sync::Arc<gpui::Image>>,
        card_width: f32,
        palette: &Palette,
    ) -> Option<AnyElement> {
        let config = first_config(self.fetched.get(&format!("{CONFIG_PREFIX}{app}"))?)?;
        let identity = identity(self.fetched.get(&format!("{IDENTITY_PREFIX}{user}"))?, app)?;
        let data = identity_data(&identity["profile"]);
        let assets: Vec<Value> = config["resolved_assets"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let context = Context2 {
            app,
            data: &data,
            assets: &assets,
        };
        let top = &config["surfaces"]["widget_top"];
        let bottom = &config["surfaces"]["widget_bottom"];
        if !top.is_object() && !bottom.is_object() {
            return None;
        }
        let strong: Hsla = palette.token("--text-default").into();
        let subtle: Hsla = palette.token("--text-subtle").into();
        let picture = |value: &FieldValue| match value {
            FieldValue::Media { url, .. } => self.images.get(url).cloned(),
            _ => None,
        };
        // `headerTitle_zEJODa`: the app's 16px icon (2px corners) and its name in body-sm/medium, 24px tall.
        let header = div()
            .h(px(24.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .size(px(16.))
                    .flex_none()
                    .rounded(px(2.))
                    .overflow_hidden()
                    .bg(palette.token("--background-mod-subtle"))
                    .children(icon.map(|icon| img(icon).size(px(16.)))),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(strong)
                    .child(name.to_owned()),
            );
        // `Z` of module 886100: the title (body-lg/medium) and three subtitles (body-sm/normal, text-subtle), 2px
        // apart, the title 4px above them, inside 32px above and below (`content_rafRmG`).
        let line = |component: &Value, size: f32, height: f32, weight: f32, tint: Hsla| {
            text_line(component, &context).map(|(text, icon)| {
                div()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        div()
                            .min_w(px(0.))
                            .line_clamp(2)
                            .text_size(px(size))
                            .line_height(px(height))
                            .font_weight(FontWeight(weight))
                            .text_color(tint)
                            .child(text),
                    )
                    .children(
                        icon.as_ref()
                            .and_then(picture)
                            .map(|icon| img(icon).size(px(height)).flex_none()),
                    )
            })
        };
        let components = &top["components"];
        let texts = div()
            .min_w(px(0.))
            .overflow_hidden()
            .py(px(32.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(2.))
            .children(
                line(&components["title"], 20., 24., 500., strong).map(|title| title.mb(px(4.))),
            )
            .children(line(&components["subtitle_1"], 14., 18., 400., subtle))
            .children(line(&components["subtitle_2"], 14., 18., 400., subtle))
            .children(line(&components["subtitle_3"], 14., 18., 400., subtle));
        let column = (card_width - 32. - 4.) / 2.;
        let top_element = match top["layout"].as_str() {
            // `el`: the hero picture fills the right column up to the card's top and right edges, its left side
            // fading into the card (`imageFadeToLeftMask_sIDBpr`).
            Some("widget_top_hero") => {
                let hero = resolve(
                    &components["hero_image"]["fields"]["image"],
                    &[Kind::Media],
                    &context,
                );
                let fade = opaque_over(
                    palette.token("--background-base-lower"),
                    palette.token("--user-profile-overlay-background"),
                );
                let mut clear = fade;
                clear.a = 0.;
                let art = hero.as_ref().and_then(|hero| {
                    let FieldValue::Media { width, height, .. } = hero.value else {
                        return None;
                    };
                    let image = picture(&hero.value)?;
                    let art_width = column + 16.;
                    Some(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(art_width))
                            .rounded_tr(px(16.))
                            .overflow_hidden()
                            .child(
                                img(image)
                                    .w(px(art_width))
                                    .h(px(art_width * (height / width.max(1.)) as f32)),
                            )
                            .child(div().absolute().inset_0().bg(linear_gradient(
                                90.,
                                linear_color_stop(fade, 0.),
                                linear_color_stop(clear, 1.),
                            ))),
                    )
                });
                div()
                    .relative()
                    .pt(px(12.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .children(art)
                    .child(header)
                    .child(div().w(px(column)).child(texts))
                    .into_any_element()
            }
            // `ee`: the contained picture in a bordered box on the right (`image_jlRl3z`, at most 150px, square).
            _ => {
                let contained = resolve(
                    &components["contained_image"]["fields"]["image"],
                    &[Kind::Media],
                    &context,
                );
                let size = column.min(150.);
                div()
                    .pt(px(12.))
                    .px(px(16.))
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(div().w(px(column)).child(texts))
                            .child(
                                div().w(px(column)).flex().justify_end().child(
                                    div()
                                        .mt(px(12.))
                                        .mb(px(16.))
                                        .size(px(size))
                                        .rounded(px(12.))
                                        .border_1()
                                        .border_color(palette.token("--border-subtle"))
                                        .bg(palette.token("--background-mod-subtle"))
                                        .overflow_hidden()
                                        .children(
                                            contained
                                                .as_ref()
                                                .and_then(|value| picture(&value.value))
                                                .map(|image| {
                                                    img(image)
                                                        .size(px(size))
                                                        .object_fit(gpui::ObjectFit::Contain)
                                                }),
                                        ),
                                ),
                            ),
                    )
                    .into_any_element()
            }
        };
        let bottom_components = &bottom["components"];
        let bottom_element: Option<AnyElement> = match bottom["layout"].as_str() {
            // `J`: six stats in a 3x2 grid, 24px apart, 24px above and 16px around (`statsGrid_JTy5Cu`).
            Some("widget_bottom_stats") => {
                let stats: Vec<AnyElement> = (1..=6)
                    .map(|index| {
                        let stat = stat(&bottom_components[format!("stat_{index}")], &context);
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .children(stat.map(|(value, label)| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .line_clamp(2)
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(strong)
                                            .child(value),
                                    )
                                    .children(label.map(|label| {
                                        div()
                                            .line_clamp(2)
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(subtle)
                                            .child(label)
                                    }))
                            }))
                            .into_any_element()
                    })
                    .collect();
                Some(
                    div()
                        .pt(px(24.))
                        .px(px(16.))
                        .pb(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(24.))
                        .children({
                            let mut cells = stats.into_iter();
                            (0..2)
                                .map(move |_| {
                                    div()
                                        .flex()
                                        .gap(px(24.))
                                        .children(cells.by_ref().take(3).collect::<Vec<_>>())
                                })
                                .collect::<Vec<_>>()
                        })
                        .into_any_element(),
                )
            }
            // `q`: the objective's 48px picture beside its name, description and the bar (6px, `--icon-strong` on
            // `--opacity-32`), the count on the right.
            Some("widget_bottom_progress") => {
                let objective = &bottom_components["objective"]["fields"];
                let progress = &bottom_components["progress"]["fields"];
                let text_of = |field: &Value| {
                    resolve(field, &[Kind::Text], &context).and_then(|value| match value.value {
                        FieldValue::Text(text) => Some(text),
                        _ => None,
                    })
                };
                let number_of = |field: &Value| {
                    resolve(field, &[Kind::Number], &context).and_then(|value| match value.value {
                        FieldValue::Number(number) => Some(number),
                        _ => None,
                    })
                };
                let (current, max) = (number_of(&progress["current"]), number_of(&progress["max"]));
                let percent = progress_percent(current, max);
                let count = current.map(|current| match max {
                    Some(max) => format!("{current}/{max}"),
                    None => format!("{percent}%"),
                });
                let image = resolve(&objective["image"], &[Kind::Media], &context)
                    .and_then(|value| picture(&value.value));
                Some(
                    div()
                        .pt(px(24.))
                        .px(px(16.))
                        .pb(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .size(px(48.))
                                .flex_none()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(palette.token("--border-subtle"))
                                .bg(palette.token("--background-mod-subtle"))
                                .overflow_hidden()
                                .children(image.map(|image| {
                                    img(image)
                                        .size(px(46.))
                                        .object_fit(gpui::ObjectFit::Contain)
                                })),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(6.))
                                        .rounded(px(12.))
                                        .bg(palette.token("--opacity-32"))
                                        .child(
                                            div()
                                                .h(px(6.))
                                                .w(relative((percent / 100.) as f32))
                                                .rounded(px(12.))
                                                .bg(palette.token("--icon-strong")),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .flex()
                                                .flex_col()
                                                .children(text_of(&objective["name"]).map(|name| {
                                                    div()
                                                        .line_clamp(2)
                                                        .text_size(px(14.))
                                                        .line_height(px(18.))
                                                        .font_weight(FontWeight(500.))
                                                        .text_color(strong)
                                                        .child(name)
                                                }))
                                                .children(text_of(&objective["description"]).map(
                                                    |text| {
                                                        div()
                                                            .line_clamp(2)
                                                            .text_size(px(12.))
                                                            .line_height(px(16.))
                                                            .font_weight(FontWeight(500.))
                                                            .text_color(subtle)
                                                            .child(text)
                                                    },
                                                )),
                                        )
                                        .children(count.map(|count| {
                                            div()
                                                .text_size(px(14.))
                                                .line_height(px(18.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(strong)
                                                .child(count)
                                        })),
                                ),
                        )
                        .into_any_element(),
                )
            }
            // `$`: four items in two columns, each a 48px picture beside its name and description.
            Some("widget_bottom_collection") => {
                let cell = (card_width - 32. - 16.) / 2.;
                let items: Vec<AnyElement> = (1..=4)
                    .map(|index| {
                        let fields = &bottom_components[format!("item_{index}")]["fields"];
                        let text = |field: &Value| {
                            resolve(field, &[Kind::Text], &context).and_then(|value| {
                                match value.value {
                                    FieldValue::Text(text) => Some(text),
                                    _ => None,
                                }
                            })
                        };
                        let image = resolve(&fields["image"], &[Kind::Media], &context)
                            .and_then(|value| picture(&value.value));
                        div()
                            .w(px(cell))
                            .flex()
                            .gap(px(12.))
                            .child(
                                div()
                                    .size(px(48.))
                                    .flex_none()
                                    .rounded(px(8.))
                                    .bg(palette.token("--background-mod-subtle"))
                                    .overflow_hidden()
                                    .children(image.map(|image| {
                                        img(image)
                                            .size(px(48.))
                                            .object_fit(gpui::ObjectFit::Contain)
                                    })),
                            )
                            .child(
                                div()
                                    .my_auto()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .children(text(&fields["name"]).map(|name| {
                                        div()
                                            .line_clamp(2)
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(strong)
                                            .child(name)
                                    }))
                                    .children(text(&fields["description"]).map(|text| {
                                        div()
                                            .line_clamp(2)
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(subtle)
                                            .child(text)
                                    })),
                            )
                            .into_any_element()
                    })
                    .collect();
                Some(
                    div()
                        .pt(px(24.))
                        .px(px(16.))
                        .pb(px(16.))
                        .flex()
                        .flex_wrap()
                        .gap(px(16.))
                        .children(items)
                        .into_any_element(),
                )
            }
            _ => None,
        };
        Some(
            div()
                .id(SharedString::from(format!("app-widget-{user}-{app}")))
                .flex()
                .flex_col()
                .child(top_element)
                .when_some(bottom_element, |this, bottom| {
                    // `divider_ZaYWLb`: a thin `--border-subtle` line between the surfaces.
                    this.child(
                        div()
                            .w_full()
                            .h(px(1.))
                            .bg(palette.token("--border-subtle")),
                    )
                    .child(bottom)
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile() -> Value {
        json!({
            "username": "Player",
            "data": {
                "primary": {
                    "season": "Season 1",
                    "rank_name": "Gold",
                    "playtime_hours": 1.5,
                    "total_kills": 12345,
                    "portrait": {"url": "u", "proxy_url": "https://media.example/p.png", "loading_state": 2,
                                 "width": 10, "height": 20},
                    "empty_picture": {"url": "u", "proxy_url": "p", "loading_state": 2, "width": 0, "height": 0}
                },
                "dynamic": [{"type": 2, "name": "level", "value": 7}]
            }
        })
    }

    #[test]
    fn identity_data_reads_names_numbers_and_sized_pictures() {
        let data = identity_data(&profile());
        assert_eq!(data["username"], FieldValue::Text("Player".to_owned()));
        assert_eq!(data["total_kills"], FieldValue::Number(12345.));
        assert_eq!(data["level"], FieldValue::Number(7.));
        assert!(
            matches!(&data["portrait"], FieldValue::Media { url, .. } if url == "https://media.example/p.png")
        );
        assert!(!data.contains_key("empty_picture"));
    }

    #[test]
    fn fields_resolve_from_data_strings_and_assets_with_fallbacks() {
        let data = identity_data(&profile());
        let assets = vec![
            json!({"key": "Hero", "asset_id": "9", "metadata": {"width": 300, "height": 600}}),
        ];
        let context = Context2 {
            app: "5",
            data: &data,
            assets: &assets,
        };
        let text = resolve(
            &json!({"value_type": "data", "presentation_type": "text", "value": "rank_name"}),
            &[Kind::Text],
            &context,
        )
        .unwrap();
        assert_eq!(text.value, FieldValue::Text("Gold".to_owned()));
        // Hours shown as a duration are milliseconds.
        let played = resolve(
            &json!({"value_type": "data", "presentation_type": "duration", "value": "playtime_hours"}),
            &[Kind::Number],
            &context,
        )
        .unwrap();
        assert_eq!(played.value, FieldValue::Number(5_400_000.));
        assert_eq!(narrow_duration(5_400_000.), "1h 30m");
        let fallback = resolve(
            &json!({"value_type": "data", "presentation_type": "image", "value": "missing",
                    "fallback": {"value_type": "application_asset", "presentation_type": "image", "value": "Hero"}}),
            &[Kind::Media],
            &context,
        )
        .unwrap();
        assert_eq!(
            fallback.value,
            FieldValue::Media {
                url: "https://cdn.discordapp.com/app-assets/5/9.webp?size=300".to_owned(),
                width: 300.,
                height: 600.
            }
        );
        // A number is not text.
        assert!(
            resolve(
                &json!({"value_type": "data", "presentation_type": "text", "value": "total_kills"}),
                &[Kind::Text],
                &context
            )
            .is_none()
        );
        let line = text_line(
            &json!({"fields": {
                "text": {"value_type": "data", "presentation_type": "text", "value": "rank_name"},
                "label": {"value_type": "data", "presentation_type": "text", "value": "season"}
            }}),
            &context,
        )
        .unwrap();
        assert_eq!(line.0, "Season 1: Gold");
        let (value, label) = stat(
            &json!({"fields": {
                "value": {"value_type": "data", "presentation_type": "number", "value": "total_kills"},
                "label": {"value_type": "custom_string", "presentation_type": "text", "value": "KOs"}
            }}),
            &context,
        )
        .unwrap();
        assert_eq!((value.as_str(), label.as_deref()), ("12K", Some("KOs")));
    }

    #[test]
    fn numbers_compact_and_progress_clamps() {
        assert_eq!(compact_number(8.), "8");
        assert_eq!(compact_number(88.), "88");
        assert_eq!(compact_number(1234.), "1.2K");
        assert_eq!(compact_number(1299.), "1.2K");
        assert_eq!(compact_number(3_456_789.), "3.4M");
        assert_eq!(narrow_duration(4_932_000.), "1h 22m 12s");
        assert_eq!(progress_percent(Some(3.), Some(4.)), 75.);
        assert_eq!(progress_percent(Some(3.), Some(0.)), 0.);
        assert_eq!(progress_percent(Some(2.), None), 100.);
        assert_eq!(progress_percent(None, Some(4.)), 0.);
    }
}
