//! The clip metadata document, schema version 6 (module 489496: the validator `V`, the migrations `F`, the writer `w`;
//! the first fill is `createClipMetadata` in module 160330), and the file name rule (module 439818).
//!
//! The JSON written into a clip carries every field of the official document that this engine has a value for, under
//! the official names and in the order `createClipMetadata` builds them. `filepath` and `sizeBytes` are never written
//! (`w` strips them); the library adds them from the path and the file's size when it loads a directory.
//! Fields the editor owns (`editMetadata`, `tracks`, `audioEvents`) and the free-form parts (`activity`, `decision`,
//! the timeline) are kept as JSON values, so a document read back is written out unchanged.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The schema version of the pinned build (`F` has six steps: 0 to 6).
pub const VERSION: u32 = 6;

/// `R.nQ`: what a clip file holds.
pub const TYPE_CLIP: &str = "clip";
pub const TYPE_SCREENSHOT: &str = "screenshot";
pub const TYPE_VOICE_CLIP: &str = "voice_clip";

/// What `ClipsStore` and the gallery know of one clip. Field order is `createClipMetadata`'s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipMetadata {
    pub id: String,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
    /// The same instant on the NTP-synced clock, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_timestamp: Option<u64>,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub application_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<Value>,
    pub users: Vec<String>,
    /// `auto` or `manual`.
    pub clip_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub timeline: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_clip_id: Option<String>,
    /// Milliseconds, set from the save result.
    pub length: u64,
    /// A data URL, or empty.
    pub thumbnail: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_candidate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_favorite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_temporary: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_start_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_end_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_events: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<Value>>,
}

impl ClipMetadata {
    /// A manual clip's first fill, as `createClipMetadata("manual", clip, timeline, decision)` makes it: the length and
    /// the thumbnail are filled in after the save.
    pub fn new_manual(
        id: String,
        created_at: u64,
        application_name: String,
        users: Vec<String>,
    ) -> Self {
        Self {
            id,
            created_at,
            sync_timestamp: None,
            version: VERSION,
            name: None,
            application_name,
            application_id: None,
            activity: None,
            users,
            clip_method: "manual".to_owned(),
            guild_id: None,
            channel_id: None,
            timeline: Vec::new(),
            decision: None,
            game_session_id: None,
            remote_clip_id: None,
            length: 0,
            thumbnail: String::new(),
            kind: TYPE_CLIP.to_owned(),
            is_candidate: Some(false),
            is_favorite: None,
            is_temporary: None,
            edit_metadata: None,
            original_start_ms: None,
            original_end_ms: None,
            audio_events: None,
            tracks: None,
        }
    }

    /// The JSON text that goes into the file (`JSON.stringify` of the document without `filepath` and `sizeBytes`).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parses and checks a document as the validator `V` does for version 6: the required fields are present, the
    /// kinds and methods are the listed ones, and an old version is refused (the migrations are not ported: this
    /// engine only ever wrote version 6). `None` is what the official code does with an invalid document: the clip is
    /// dropped without a word.
    pub fn parse(text: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(text).ok()?;
        let version = value.get("version").and_then(Value::as_u64)?;
        if version != u64::from(VERSION) {
            return None;
        }
        let meta: Self = serde_json::from_value(value).ok()?;
        let kinds = [TYPE_CLIP, TYPE_SCREENSHOT, TYPE_VOICE_CLIP];
        let valid = !meta.id.is_empty()
            && matches!(meta.clip_method.as_str(), "auto" | "manual")
            && kinds.contains(&meta.kind.as_str());
        valid.then_some(meta)
    }
}

/// `<application name, first 20 characters>_<clip id>.mp4` (module 489496 `ed`, with module 439818 for the name: trim,
/// spaces to underscores, then every character outside `a-zA-Z0-9-_.` is dropped). The cut is 20 UTF-16 units and comes
/// before the trim, as `substring(0, 20)` does; a character that straddles the cut is not ASCII and would be dropped
/// anyway.
pub fn file_name(application_name: &str, id: &str) -> String {
    let mut taken = String::new();
    let mut units = 0;
    for c in application_name.chars() {
        units += c.len_utf16();
        if units > 20 {
            break;
        }
        taken.push(c);
    }
    let name: String = taken
        .trim()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    format!("{name}_{id}.mp4")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0b2c3d4e-0000-4000-8000-123456789abc";

    #[test]
    fn names_follow_the_official_rule() {
        assert_eq!(
            file_name("Counter-Strike 2", ID),
            format!("Counter-Strike_2_{ID}.mp4")
        );
        // Cut at 20 characters first (two spaces count), then trimmed.
        assert_eq!(
            file_name("  A Very Long Game Title That Goes On  ", ID),
            format!("A_Very_Long_Game_T_{ID}.mp4")
        );
        // Characters outside the set are dropped, dots and dashes stay.
        assert_eq!(
            file_name("\u{dc}n\u{ef}code: game (v1.2)!", ID),
            format!("ncode_game_v1.2_{ID}.mp4")
        );
        assert_eq!(file_name("", ID), format!("_{ID}.mp4"));
    }

    #[test]
    fn the_cut_counts_utf16_units() {
        // An emoji is two units: 18 letters plus one emoji make 20; the second emoji is past the cut.
        let name = format!("{}\u{1f600}\u{1f600}", "a".repeat(18));
        assert_eq!(file_name(&name, "x"), format!("{}_x.mp4", "a".repeat(18)));
    }

    #[test]
    fn a_document_round_trips_in_the_official_key_order() {
        let mut meta = ClipMetadata::new_manual(
            "id-1".into(),
            1_700_000_000_000,
            "Synthetic".into(),
            vec!["42".into()],
        );
        meta.length = 12_345;
        meta.thumbnail = "data:image/jpeg;base64,AAAA".into();
        meta.channel_id = Some("7".into());
        meta.timeline = vec![serde_json::json!({"signal": {"type": "manual"}, "timestamp": 99})];
        meta.decision = Some(serde_json::json!({"signal": {"type": "manual"}, "timestamp": 99}));
        let text = meta.to_json();
        assert!(text.starts_with(
            r#"{"id":"id-1","createdAt":1700000000000,"version":6,"applicationName":"Synthetic""#
        ));
        assert!(!text.contains("filepath") && !text.contains("sizeBytes"));
        assert!(text.contains(r#""type":"clip""#) && text.contains(r#""isCandidate":false"#));
        assert_eq!(ClipMetadata::parse(text.as_bytes()), Some(meta));
    }

    #[test]
    fn invalid_documents_are_dropped() {
        let mut meta = ClipMetadata::new_manual("a".into(), 1, "G".into(), vec![]);
        meta.clip_method = "sometimes".into();
        assert_eq!(ClipMetadata::parse(meta.to_json().as_bytes()), None);
        let mut meta = ClipMetadata::new_manual("a".into(), 1, "G".into(), vec![]);
        meta.version = 5;
        assert_eq!(ClipMetadata::parse(meta.to_json().as_bytes()), None);
        assert_eq!(ClipMetadata::parse(b"not json"), None);
    }
}
