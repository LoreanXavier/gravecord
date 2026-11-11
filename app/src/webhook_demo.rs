//! The messages the webhook step of the harness posts to the fixture channel: components that need no application
//! (link buttons, and the Components V2 layout parts), so the official client can be captured on the same content.

use serde_json::{Value, json};

/// The only channel the step writes to: `#general` of the fixture server.
pub(crate) const FIXTURE_CHANNEL: u64 = 1_568_692_565_136_375_900;

/// The name of the attached text file the file component points at.
pub(crate) const FILE_NAME: &str = "components-report.txt";

const DOCS: &str = "https://discord.com/developers/docs/components/overview";

fn picture(number: u32) -> Value {
    json!({"media": {"url": format!("https://cdn.discordapp.com/embed/avatars/{number}.png")}})
}

fn link(label: Option<&str>, emoji: Option<&str>, url: &str) -> Value {
    let mut button = json!({"type": 2, "style": 5, "url": url});
    if let Some(label) = label {
        button["label"] = json!(label);
    }
    if let Some(emoji) = emoji {
        button["emoji"] = json!({"name": emoji});
    }
    button
}

fn separator(divider: bool, spacing: u64) -> Value {
    json!({"type": 14, "divider": divider, "spacing": spacing})
}

/// The Components V2 parts: text display, a section with a thumbnail, a section with a link button, the four
/// separators, galleries of one, two and three pictures, and a file.
fn layout_parts() -> Vec<Value> {
    vec![
        json!({"type": 10, "content": "## Text display\nBody text with **bold**, *italic*, `code` and a [link](https://discord.com)."}),
        json!({"type": 9,
            "components": [
                {"type": 10, "content": "**Section with a thumbnail**"},
                {"type": 10, "content": "The accessory sits at the end of the row."}
            ],
            "accessory": {"type": 11, "media": {"url": "https://cdn.discordapp.com/embed/avatars/1.png"}, "description": "avatar"}}),
        json!({"type": 9,
            "components": [{"type": 10, "content": "**Section with a link button**"}],
            "accessory": link(Some("Open docs"), None, DOCS)}),
        separator(true, 1),
        json!({"type": 10, "content": "Above: divider, small spacing"}),
        separator(false, 1),
        json!({"type": 10, "content": "Above: no divider, small spacing"}),
        separator(true, 2),
        json!({"type": 10, "content": "Above: divider, large spacing"}),
        separator(false, 2),
        json!({"type": 10, "content": "Above: no divider, large spacing"}),
        json!({"type": 12, "items": [picture(2)]}),
        json!({"type": 12, "items": [picture(3), picture(4)]}),
        json!({"type": 12, "items": [picture(0), picture(1), picture(2)]}),
        json!({"type": 13, "file": {"url": format!("attachment://{FILE_NAME}")}}),
    ]
}

/// The three messages, as JSON bodies; every one is sent with `with_components=true`.
pub(crate) fn payloads() -> Vec<Value> {
    let classic = json!({
        "username": "Components fixture",
        "content": "Link buttons: label only, emoji and label, a long label",
        "components": [{"type": 1, "components": [
            link(Some("Docs"), None, DOCS),
            link(Some("Docs"), Some("\u{1f4d6}"), DOCS),
            link(
                Some("A button label that is much longer than the others so it has to give way"),
                None,
                DOCS,
            ),
        ]}],
    });
    let in_container = json!({
        "username": "Components fixture",
        "flags": 1 << 15,
        "components": [{"type": 17, "accent_color": 0x5865f2, "components": layout_parts()}],
    });
    let loose = json!({
        "username": "Components fixture",
        "flags": 1 << 15,
        "components": layout_parts(),
    });
    vec![classic, in_container, loose]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_messages_the_second_and_third_are_components_v2() {
        let all = payloads();
        assert_eq!(all.len(), 3);
        assert!(all[0]["flags"].is_null());
        assert_eq!(all[1]["flags"], 32768);
        assert_eq!(all[2]["flags"], 32768);
        assert_eq!(all[1]["components"][0]["type"], 17);
        assert_eq!(all[1]["components"][0]["components"], all[2]["components"]);
    }

    #[test]
    fn a_v2_message_stays_under_the_forty_component_limit() {
        fn count(value: &Value) -> usize {
            match value {
                Value::Array(items) => items.iter().map(count).sum(),
                Value::Object(map) if map.contains_key("type") => {
                    1 + map.values().map(count).sum::<usize>()
                }
                Value::Object(map) => map.values().map(count).sum(),
                _ => 0,
            }
        }
        for message in &payloads()[1..] {
            assert!(count(&message["components"]) <= 40);
        }
    }

    #[test]
    fn only_link_buttons_are_posted() {
        let text = payloads()[0].to_string();
        assert!(!text.contains("custom_id"));
    }
}
