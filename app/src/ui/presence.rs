//! Compact activity rows from modules 342312/566903 and the captured member DOM.

use super::{Root, color, icon};
use dc_state::{Id, User, presence::activity_text};
use gpui::{AnyElement, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

fn emoji_source(emoji: &Value) -> Option<String> {
    if let Some(id) = emoji["id"].as_str() {
        Some(format!(
            "https://cdn.discordapp.com/emojis/{id}.webp?size=32"
        ))
    } else {
        dc_tokens::emoji_file(emoji["name"].as_str()?)
            .map(|file| format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN))
    }
}

impl Root {
    pub(super) fn request_presence_images(&mut self) {
        let urls: Vec<String> = self
            .stores
            .presence
            .all()
            .filter_map(|p| p.custom())
            .filter_map(|a| emoji_source(&a["emoji"]))
            .collect();
        for url in urls {
            self.request_image(url.clone(), url);
        }
    }

    pub(super) fn presence_line(
        &self,
        user: &User,
        guild: Option<Id>,
        compact: bool,
    ) -> Option<AnyElement> {
        self.presence_line_with(
            user,
            guild,
            compact,
            if compact { 16. } else { 18. },
            "--channels-default",
        )
    }

    /// Whether `presence_line` has a line for this person, without building it (the member list needs the row's
    /// height, which depends on it). The conditions are those `presence_line_with` returns `None` on.
    pub(super) fn has_presence_line(&self, user: &User, guild: Option<Id>) -> bool {
        if self.is_blocked(user.id) || self.stores.is_ignored(user.id) {
            return false;
        }
        let Some(presence) = self.stores.presence.get(user.id, guild) else {
            return false;
        };
        if matches!(presence.status.as_str(), "offline" | "invisible") {
            return false;
        }
        presence.custom().is_some() || !presence.visible_activities().is_empty()
    }

    /// The same line at another line height and colour: the user panel draws it 13px high in its own text colour
    /// (`activityStatusText` under `panels_pog62O`).
    pub(super) fn presence_line_with(
        &self,
        user: &User,
        guild: Option<Id>,
        compact: bool,
        line: f32,
        tint: &'static str,
    ) -> Option<AnyElement> {
        if self.is_blocked(user.id) || self.stores.is_ignored(user.id) {
            return None;
        }
        let presence = self.stores.presence.get(user.id, guild)?;
        if matches!(presence.status.as_str(), "offline" | "invisible") {
            return None;
        }
        let custom = presence.custom();
        let activities = presence.visible_activities();
        let first = activities.first().copied();
        if custom.is_none() && first.is_none() {
            return None;
        }
        let custom_text = custom.and_then(|a| a["state"].as_str());
        let hide_activity_text = compact && custom_text.is_some();
        let mut row = div()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(line))
            .text_size(px(if compact { 12. } else { 14. }))
            .line_height(px(line))
            .font_weight(FontWeight(500.))
            .text_color(color(tint));
        if let Some(activity) = first {
            let glyph = match activity["type"].as_u64() {
                Some(0 | 5) => Some("activity_game"),
                Some(2) => Some("activity_music"),
                Some(1 | 3) => Some("activity_tv"),
                _ => None,
            };
            if !user.bot
                && let Some(glyph) = glyph
            {
                row = row.child(icon(
                    glyph,
                    if compact { 12. } else { 14. },
                    "--status-positive",
                ));
            }
            if !hide_activity_text && let Some(text) = activity_text(activity) {
                row = row.child(div().min_w(px(0.)).text_ellipsis().child(text));
            }
            if !user.bot && activities.len() > 1 {
                row = row.child(
                    div()
                        .flex_none()
                        .child(format!("+{}", activities.len() - 1)),
                );
            }
        }
        if let Some(custom) = custom {
            if first.is_some() {
                row = row.child(div().flex_none().child("•"));
            }
            if let Some(url) = emoji_source(&custom["emoji"]) {
                if let Some(image) = self.images.get(&url) {
                    row = row.child(
                        img(image.clone())
                            .size(px(if compact { 14. } else { 18. }))
                            .flex_none(),
                    );
                }
            } else if let Some(emoji) = custom["emoji"]["name"].as_str() {
                row = row.child(div().flex_none().child(emoji.to_owned()));
            }
            if let Some(text) = custom_text {
                row = row.child(div().min_w(px(0.)).text_ellipsis().child(text.to_owned()));
            }
        }
        Some(row.into_any_element())
    }
}
