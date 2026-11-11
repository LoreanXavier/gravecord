//! Message components (`message.components`): the controls a bot or an app puts under a message, and the layout parts of
//! the newer component messages. The shape is the pinned build's normalizer `ZV` (module 257249): unknown types are
//! dropped, every component gets an id made of its place (`"0"`, `"0,1"`, the section accessory after its children),
//! and the enums are module 155718's (`I5` component types, `in` button styles, `C8` separator spacing, `qz` text input
//! styles).

use super::{Id, id, text};
use serde_json::Value;

/// `I5`: the component type numbers.
pub const ACTION_ROW: u64 = 1;
pub const BUTTON: u64 = 2;
pub const STRING_SELECT: u64 = 3;
pub const TEXT_INPUT: u64 = 4;
pub const USER_SELECT: u64 = 5;
pub const ROLE_SELECT: u64 = 6;
pub const MENTIONABLE_SELECT: u64 = 7;
pub const CHANNEL_SELECT: u64 = 8;
pub const SECTION: u64 = 9;
pub const TEXT_DISPLAY: u64 = 10;
pub const THUMBNAIL: u64 = 11;
pub const MEDIA_GALLERY: u64 = 12;
pub const FILE: u64 = 13;
pub const SEPARATOR: u64 = 14;
pub const CONTAINER: u64 = 17;
pub const LABEL: u64 = 18;

/// Message flags this module's callers read (`pr7`).
pub const FLAG_EPHEMERAL: u64 = 1 << 6;
pub const FLAG_LOADING: u64 = 1 << 7;
pub const FLAG_IS_COMPONENTS_V2: u64 = 1 << 15;

/// `in`: the button styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Destructive,
    Link,
    Premium,
}

impl ButtonStyle {
    fn from_number(number: u64) -> Self {
        match number {
            1 => Self::Primary,
            3 => Self::Success,
            4 => Self::Destructive,
            5 => Self::Link,
            6 => Self::Premium,
            _ => Self::Secondary,
        }
    }
}

/// An emoji on a button or an option (`p` in module 257249: id, name, animated).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentEmoji {
    pub id: Option<Id>,
    pub name: Option<String>,
    pub animated: bool,
}

impl ComponentEmoji {
    fn from_value(value: &Value) -> Option<Self> {
        if !value.is_object() {
            return None;
        }
        Some(Self {
            id: id(&value["id"]),
            name: text(&value["name"]),
            animated: value["animated"].as_bool().unwrap_or(false),
        })
    }
}

/// A media item of a thumbnail, a gallery or a file (`Uv` in module 448875).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub url: String,
    pub proxy_url: Option<String>,
    pub width: u32,
    pub height: u32,
    pub content_type: Option<String>,
}

impl Media {
    fn from_value(value: &Value) -> Self {
        Self {
            url: text(&value["url"]).unwrap_or_default(),
            proxy_url: text(&value["proxy_url"]),
            width: value["width"].as_u64().unwrap_or(0) as u32,
            height: value["height"].as_u64().unwrap_or(0) as u32,
            content_type: text(&value["content_type"]),
        }
    }

    /// The address a picture is fetched from: the proxy copy when there is one.
    pub fn source(&self) -> &str {
        self.proxy_url.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GalleryItem {
    pub media: Media,
    pub description: Option<String>,
    pub spoiler: bool,
}

/// One option of a string select.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    pub description: Option<String>,
    pub emoji: Option<ComponentEmoji>,
    pub default: bool,
}

/// What an entity select's `default_values` entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    User,
    Role,
    Channel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultValue {
    pub kind: EntityKind,
    pub id: Id,
}

/// The five select types.
#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub id: String,
    /// 3 string, 5 user, 6 role, 7 mentionable, 8 channel.
    pub kind: u64,
    pub custom_id: Option<String>,
    pub disabled: bool,
    pub required: bool,
    pub placeholder: Option<String>,
    pub min_values: u64,
    pub max_values: u64,
    pub options: Vec<SelectOption>,
    pub channel_types: Vec<u64>,
    pub default_values: Vec<DefaultValue>,
}

/// A text input of a modal.
#[derive(Clone, Debug, PartialEq)]
pub struct TextInput {
    pub id: String,
    pub custom_id: Option<String>,
    /// `qz`: 1 short, 2 paragraph.
    pub paragraph: bool,
    pub label: Option<String>,
    pub value: Option<String>,
    pub placeholder: Option<String>,
    pub disabled: bool,
    pub required: bool,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Component {
    ActionRow {
        id: String,
        components: Vec<Component>,
    },
    Button {
        id: String,
        custom_id: Option<String>,
        style: ButtonStyle,
        label: Option<String>,
        emoji: Option<ComponentEmoji>,
        url: Option<String>,
        sku_id: Option<Id>,
        disabled: bool,
    },
    Select(Select),
    TextInput(TextInput),
    Section {
        id: String,
        components: Vec<Component>,
        accessory: Box<Component>,
    },
    TextDisplay {
        id: String,
        content: String,
    },
    Thumbnail {
        id: String,
        media: Media,
        description: Option<String>,
        spoiler: bool,
    },
    MediaGallery {
        id: String,
        items: Vec<GalleryItem>,
    },
    File {
        id: String,
        file: Media,
        name: Option<String>,
        size: Option<u64>,
        spoiler: bool,
    },
    Separator {
        id: String,
        divider: bool,
        /// `C8.LARGE` (2): 8px of margin above and below.
        large: bool,
    },
    Container {
        id: String,
        accent_color: Option<u32>,
        spoiler: bool,
        components: Vec<Component>,
    },
    Label {
        id: String,
        label: Option<String>,
        description: Option<String>,
        component: Box<Component>,
    },
}

impl Component {
    /// The component's id: its place in the message.
    pub fn id(&self) -> &str {
        match self {
            Self::ActionRow { id, .. }
            | Self::Button { id, .. }
            | Self::Section { id, .. }
            | Self::TextDisplay { id, .. }
            | Self::Thumbnail { id, .. }
            | Self::MediaGallery { id, .. }
            | Self::File { id, .. }
            | Self::Separator { id, .. }
            | Self::Container { id, .. }
            | Self::Label { id, .. } => id,
            Self::Select(select) => &select.id,
            Self::TextInput(input) => &input.id,
        }
    }

    /// `I5` of this component.
    pub fn type_number(&self) -> u64 {
        match self {
            Self::ActionRow { .. } => ACTION_ROW,
            Self::Button { .. } => BUTTON,
            Self::Select(select) => select.kind,
            Self::TextInput(_) => TEXT_INPUT,
            Self::Section { .. } => SECTION,
            Self::TextDisplay { .. } => TEXT_DISPLAY,
            Self::Thumbnail { .. } => THUMBNAIL,
            Self::MediaGallery { .. } => MEDIA_GALLERY,
            Self::File { .. } => FILE,
            Self::Separator { .. } => SEPARATOR,
            Self::Container { .. } => CONTAINER,
            Self::Label { .. } => LABEL,
        }
    }

    /// `O` in module 911408: a button that is not a link and every select answers with an interaction.
    pub fn is_interactive(&self) -> bool {
        match self {
            Self::Button { style, .. } => *style != ButtonStyle::Link,
            Self::Select(_) => true,
            _ => false,
        }
    }

    /// The `custom_id` an interaction names.
    pub fn custom_id(&self) -> Option<&str> {
        match self {
            Self::Button { custom_id, .. } => custom_id.as_deref(),
            Self::Select(select) => select.custom_id.as_deref(),
            Self::TextInput(input) => input.custom_id.as_deref(),
            _ => None,
        }
    }

    /// Whether the whole tree holds a component of `id` (a message edit may drop the one that was pressed).
    pub fn contains(&self, wanted: &str) -> bool {
        self.find(wanted).is_some()
    }

    /// The component of `wanted` id inside this one, itself included.
    pub fn find(&self, wanted: &str) -> Option<&Component> {
        if self.id() == wanted {
            return Some(self);
        }
        match self {
            Self::ActionRow { components, .. } | Self::Container { components, .. } => {
                components.iter().find_map(|child| child.find(wanted))
            }
            Self::Section {
                components,
                accessory,
                ..
            } => components
                .iter()
                .find_map(|child| child.find(wanted))
                .or_else(|| accessory.find(wanted)),
            Self::Label { component, .. } => component.find(wanted),
            _ => None,
        }
    }
}

/// Finds a component by id in a message's components.
pub fn find<'a>(components: &'a [Component], wanted: &str) -> Option<&'a Component> {
    components
        .iter()
        .find_map(|component| component.find(wanted))
}

fn path_id(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn optional_text(value: &Value) -> Option<String> {
    text(value).filter(|text| !text.is_empty())
}

fn children(value: &Value, path: &[usize]) -> Vec<Component> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, child)| {
            let mut next = path.to_vec();
            next.push(index);
            parse_one(child, &next)
        })
        .collect()
}

fn select(value: &Value, path: &[usize], kind: u64) -> Select {
    Select {
        id: path_id(path),
        kind,
        custom_id: text(&value["custom_id"]),
        disabled: value["disabled"].as_bool().unwrap_or(false),
        required: value["required"].as_bool().unwrap_or(false),
        placeholder: optional_text(&value["placeholder"]),
        min_values: value["min_values"].as_u64().unwrap_or(1),
        max_values: value["max_values"].as_u64().unwrap_or(1),
        options: value["options"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|option| SelectOption {
                label: text(&option["label"]).unwrap_or_default(),
                value: text(&option["value"]).unwrap_or_default(),
                description: optional_text(&option["description"]),
                emoji: ComponentEmoji::from_value(&option["emoji"]),
                default: option["default"].as_bool().unwrap_or(false),
            })
            .collect(),
        channel_types: value["channel_types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect(),
        default_values: value["default_values"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let kind = match entry["type"].as_str()? {
                    "user" => EntityKind::User,
                    "role" => EntityKind::Role,
                    "channel" => EntityKind::Channel,
                    _ => return None,
                };
                Some(DefaultValue {
                    kind,
                    id: id(&entry["id"])?,
                })
            })
            .collect(),
    }
}

fn parse_one(value: &Value, path: &[usize]) -> Option<Component> {
    let kind = value["type"].as_u64()?;
    let own = path_id(path);
    Some(match kind {
        ACTION_ROW => Component::ActionRow {
            id: own,
            components: children(&value["components"], path),
        },
        BUTTON => Component::Button {
            id: own,
            custom_id: text(&value["custom_id"]),
            style: ButtonStyle::from_number(value["style"].as_u64().unwrap_or(2)),
            label: optional_text(&value["label"]),
            emoji: ComponentEmoji::from_value(&value["emoji"]),
            url: text(&value["url"]),
            sku_id: id(&value["sku_id"]),
            disabled: value["disabled"].as_bool().unwrap_or(false),
        },
        STRING_SELECT | USER_SELECT | ROLE_SELECT | MENTIONABLE_SELECT | CHANNEL_SELECT => {
            Component::Select(select(value, path, kind))
        }
        TEXT_INPUT => Component::TextInput(TextInput {
            id: own,
            custom_id: text(&value["custom_id"]),
            paragraph: value["style"].as_u64() == Some(2),
            label: optional_text(&value["label"]),
            value: text(&value["value"]),
            placeholder: optional_text(&value["placeholder"]),
            disabled: value["disabled"].as_bool().unwrap_or(false),
            required: value["required"].as_bool().unwrap_or(false),
            min_length: value["min_length"].as_u64(),
            max_length: value["max_length"].as_u64(),
        }),
        SECTION => {
            let components = children(&value["components"], path);
            let mut accessory_path = path.to_vec();
            accessory_path.push(components.len());
            let accessory = parse_one(&value["accessory"], &accessory_path)?;
            // A section with no text, or no accessory, is dropped (the normalizer returns null).
            if components.is_empty() {
                return None;
            }
            Component::Section {
                id: own,
                components,
                accessory: Box::new(accessory),
            }
        }
        TEXT_DISPLAY => Component::TextDisplay {
            id: own,
            content: text(&value["content"]).unwrap_or_default(),
        },
        THUMBNAIL => Component::Thumbnail {
            id: own,
            media: Media::from_value(&value["media"]),
            description: optional_text(&value["description"]),
            spoiler: value["spoiler"].as_bool().unwrap_or(false),
        },
        MEDIA_GALLERY => Component::MediaGallery {
            id: own,
            items: value["items"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| GalleryItem {
                    media: Media::from_value(&item["media"]),
                    description: optional_text(&item["description"]),
                    spoiler: item["spoiler"].as_bool().unwrap_or(false),
                })
                .collect(),
        },
        FILE => Component::File {
            id: own,
            file: Media::from_value(&value["file"]),
            name: text(&value["name"]),
            size: value["size"].as_u64(),
            spoiler: value["spoiler"].as_bool().unwrap_or(false),
        },
        SEPARATOR => Component::Separator {
            id: own,
            divider: value["divider"].as_bool().unwrap_or(true),
            large: value["spacing"].as_u64() == Some(2),
        },
        CONTAINER => Component::Container {
            id: own,
            accent_color: value["accent_color"].as_u64().map(|color| color as u32),
            spoiler: value["spoiler"].as_bool().unwrap_or(false),
            components: children(&value["components"], path),
        },
        LABEL => {
            let mut inner = path.to_vec();
            inner.push(0);
            Component::Label {
                id: own,
                label: optional_text(&value["label"]),
                description: optional_text(&value["description"]),
                component: Box::new(parse_one(&value["component"], &inner)?),
            }
        }
        _ => return None,
    })
}

/// `components` of a message or a modal: the array, each top-level entry numbered from 0.
pub fn parse(value: &Value) -> Vec<Component> {
    children(value, &[])
}

/// The text of every text display in the tree, joined by line breaks (`kC` in module 257249: what a notification or a
/// reply preview shows of a layout message).
pub fn display_text(components: &[Component]) -> String {
    fn walk(component: &Component, out: &mut Vec<String>) {
        match component {
            Component::TextDisplay { content, .. } => out.push(content.clone()),
            Component::ActionRow { components, .. } | Component::Container { components, .. } => {
                components.iter().for_each(|child| walk(child, out));
            }
            Component::Section { components, .. } => {
                components.iter().for_each(|child| walk(child, out));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    components
        .iter()
        .for_each(|component| walk(component, &mut out));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!([
            {"type": 1, "components": [
                {"type": 2, "style": 1, "label": "Accept", "custom_id": "accept"},
                {"type": 2, "style": 4, "label": "Decline", "custom_id": "decline", "disabled": true},
                {"type": 2, "style": 5, "label": "Docs", "url": "https://example.com/docs"},
                {"type": 2, "style": 2, "emoji": {"id": null, "name": "\u{1f44d}"}, "custom_id": "up"},
                {"type": 2, "style": 6, "sku_id": "99"},
            ]},
            {"type": 1, "components": [
                {"type": 3, "custom_id": "pick", "placeholder": "Pick one", "min_values": 1, "max_values": 2,
                 "options": [
                    {"label": "A", "value": "a", "description": "first", "default": true},
                    {"label": "B", "value": "b", "emoji": {"id": "5", "name": "x", "animated": true}},
                 ]},
            ]},
            {"type": 1, "components": [
                {"type": 5, "custom_id": "who", "max_values": 3, "default_values": [{"id": "7", "type": "user"}]},
            ]},
            {"type": 1, "components": [
                {"type": 8, "custom_id": "where", "channel_types": [0, 5]},
            ]},
        ])
    }

    #[test]
    fn an_action_row_of_buttons_keeps_style_label_state_and_place() {
        let parsed = parse(&sample());
        assert_eq!(parsed.len(), 4);
        let Component::ActionRow { id, components } = &parsed[0] else {
            panic!("not a row");
        };
        assert_eq!(id, "0");
        assert_eq!(components.len(), 5);
        assert_eq!(components[1].id(), "0,1");
        assert!(matches!(
            &components[0],
            Component::Button { style: ButtonStyle::Primary, label: Some(label), custom_id: Some(custom), disabled: false, .. }
                if label == "Accept" && custom == "accept"
        ));
        assert!(matches!(
            &components[1],
            Component::Button {
                style: ButtonStyle::Destructive,
                disabled: true,
                ..
            }
        ));
        assert!(
            matches!(&components[2], Component::Button { style: ButtonStyle::Link, url: Some(url), .. } if url == "https://example.com/docs")
        );
        assert!(
            matches!(&components[3], Component::Button { emoji: Some(emoji), label: None, .. } if emoji.name.as_deref() == Some("\u{1f44d}"))
        );
        assert!(matches!(
            &components[4],
            Component::Button {
                style: ButtonStyle::Premium,
                sku_id: Some(99),
                ..
            }
        ));
        // A link button answers with nothing: only the other styles are interactive.
        assert!(!components[2].is_interactive());
        assert!(components[0].is_interactive());
    }

    #[test]
    fn selects_keep_their_options_limits_and_defaults() {
        let parsed = parse(&sample());
        let Component::ActionRow { components, .. } = &parsed[1] else {
            panic!()
        };
        let Component::Select(string) = &components[0] else {
            panic!()
        };
        assert_eq!((string.min_values, string.max_values), (1, 2));
        assert_eq!(string.options.len(), 2);
        assert!(string.options[0].default);
        assert_eq!(string.options[0].description.as_deref(), Some("first"));
        assert!(
            string.options[1]
                .emoji
                .as_ref()
                .is_some_and(|emoji| emoji.animated && emoji.id == Some(5))
        );
        assert_eq!(string.placeholder.as_deref(), Some("Pick one"));
        let Component::ActionRow { components, .. } = &parsed[2] else {
            panic!()
        };
        let Component::Select(user) = &components[0] else {
            panic!()
        };
        assert_eq!(user.kind, USER_SELECT);
        assert_eq!(user.max_values, 3);
        assert_eq!(
            user.default_values,
            vec![DefaultValue {
                kind: EntityKind::User,
                id: 7
            }]
        );
        let Component::ActionRow { components, .. } = &parsed[3] else {
            panic!()
        };
        let Component::Select(channel) = &components[0] else {
            panic!()
        };
        assert_eq!(channel.channel_types, vec![0, 5]);
        // A select with no limits asks for exactly one value.
        assert_eq!((channel.min_values, channel.max_values), (1, 1));
    }

    #[test]
    fn layout_components_nest_and_unknown_types_are_dropped() {
        let parsed = parse(&json!([
            {"type": 17, "accent_color": 5793266, "components": [
                {"type": 10, "content": "## Title"},
                {"type": 14, "divider": true, "spacing": 2},
                {"type": 9, "components": [{"type": 10, "content": "body"}],
                 "accessory": {"type": 2, "style": 1, "label": "Go", "custom_id": "go"}},
                {"type": 12, "items": [{"media": {"url": "https://cdn/x.png", "width": 4, "height": 3}, "description": "alt"}]},
                {"type": 13, "file": {"url": "attachment://a.zip"}, "name": "a.zip", "size": 10},
                {"type": 99},
            ]},
            {"type": 77},
        ]));
        assert_eq!(parsed.len(), 1);
        let Component::Container {
            accent_color,
            components,
            ..
        } = &parsed[0]
        else {
            panic!()
        };
        assert_eq!(*accent_color, Some(5793266));
        assert_eq!(components.len(), 5);
        assert!(matches!(
            &components[1],
            Component::Separator {
                divider: true,
                large: true,
                ..
            }
        ));
        let Component::Section { accessory, id, .. } = &components[2] else {
            panic!()
        };
        // The accessory is numbered after the section's children.
        assert_eq!(id, "0,2");
        assert_eq!(accessory.id(), "0,2,1");
        assert!(
            matches!(&components[3], Component::MediaGallery { items, .. } if items.len() == 1 && items[0].description.as_deref() == Some("alt"))
        );
        assert_eq!(display_text(&parsed), "## Title\nbody");
        assert!(find(&parsed, "0,2,1").is_some_and(Component::is_interactive));
    }

    #[test]
    fn a_modal_label_wraps_a_text_input() {
        let parsed = parse(&json!([
            {"type": 1, "components": [{"type": 4, "custom_id": "name", "style": 1, "label": "Name", "required": true, "min_length": 2, "max_length": 20}]},
            {"type": 18, "label": "Story", "description": "Tell us", "component": {"type": 4, "custom_id": "story", "style": 2}},
        ]));
        let Component::ActionRow { components, .. } = &parsed[0] else {
            panic!()
        };
        let Component::TextInput(input) = &components[0] else {
            panic!()
        };
        assert!(!input.paragraph);
        assert_eq!((input.min_length, input.max_length), (Some(2), Some(20)));
        let Component::Label {
            component, label, ..
        } = &parsed[1]
        else {
            panic!()
        };
        assert_eq!(label.as_deref(), Some("Story"));
        assert!(matches!(&**component, Component::TextInput(input) if input.paragraph));
    }
}
