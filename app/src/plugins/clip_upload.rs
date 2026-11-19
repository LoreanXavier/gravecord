//! ClipUpload (Equicord `src/equicordplugins/clipUpload.desktop`, GPL-3.0-or-later): the pure part. A local MP4 is
//! sent as a Discord clip: the file's clip metadata (the official `uuid` box last in the file, written here by
//! `dc_clips::mp4::replace_metadata` as the client's `updateClipMetadata(path, "{}")` writes it), then the upload
//! slot with the clip's fields (`reserveClipUpload`), the bytes, and the message naming the upload (`sendClipUpload`).
//! Not part of the pinned client.

use serde_json::{Map, Value, json};

/// `defaultFileName`.
pub(crate) const DEFAULT_FILE_NAME: &str = "clip.mp4";
/// `MAX_CLIP_SIZE` of native.ts: 500 MB.
pub(crate) const MAX_CLIP_SIZE: u64 = 500 * 1024 * 1024;
/// `CLIP_FOOTER`: the box type and user type of the clip metadata box.
const CLIP_FOOTER: [u8; 20] = [
    0x75, 0x75, 0x69, 0x64, 0xA1, 0xC8, 0x52, 0x99, 0x33, 0x46, 0x4D, 0xB8, 0x88, 0xF0, 0x83, 0xF5,
    0x7A, 0x75, 0xA5, 0xEF,
];

/// `ALLOWED_EXTENSIONS` of native.ts (the file dialog's "MP4 Video" filter).
pub(crate) fn allowed(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".mp4") || lower.ends_with(".m4v")
}

/// `getClipTitleFromName`: the name without its extension (a leading dot is kept).
pub(crate) fn title_from_name(name: &str) -> String {
    match name.rfind('.') {
        Some(at) if at > 0 => name[..at].to_owned(),
        _ => name.to_owned(),
    }
}

/// `ParsedClipMetadata` (`parseClipMetadata` of native.ts, first entry).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Parsed {
    pub application_id: Option<String>,
    pub application_name: Option<String>,
    pub users: Vec<String>,
}

/// `parseClipMetadata`: the JSON after the first clip footer, an object or the first of an array.
pub(crate) fn parse_metadata(bytes: &[u8]) -> Option<Parsed> {
    if bytes.len() <= CLIP_FOOTER.len() {
        return None;
    }
    let at = bytes
        .windows(CLIP_FOOTER.len())
        .position(|window| window == CLIP_FOOTER)?;
    let value: Value = serde_json::from_slice(&bytes[at + CLIP_FOOTER.len()..]).ok()?;
    let entry = match value {
        Value::Array(entries) => entries.into_iter().next()?,
        entry @ Value::Object(_) => entry,
        _ => return None,
    };
    let text = |key: &str| {
        entry[key]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    Some(Parsed {
        application_id: text("applicationId"),
        application_name: text("applicationName"),
        users: entry["users"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|user| user.as_str().map(str::to_owned))
            .collect(),
    })
}

/// `getDateTimeLocalValue`: the `datetime-local` text of a moment, in the local zone (`offset_minutes` east of UTC).
pub(crate) fn datetime_local(ms: i64, offset_minutes: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms + offset_minutes * 60_000)
        .map(|local| local.format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_default()
}

/// `new Date(value)` of the field: a `datetime-local` text (local time, seconds optional) or an ISO time with its
/// zone, in milliseconds. `isValidDate` is this being some.
pub(crate) fn parse_datetime(text: &str, offset_minutes: i64) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(zoned) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(zoned.timestamp_millis());
    }
    [
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ]
    .iter()
    .find_map(|format| chrono::NaiveDateTime::parse_from_str(text, format).ok())
    .or_else(|| {
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    })
    .map(|local| local.and_utc().timestamp_millis() - offset_minutes * 60_000)
}

/// `Date.prototype.toISOString`.
pub(crate) fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// `ClipUploadOptions`, the fields both requests carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Options {
    pub file_name: String,
    pub participants: Vec<String>,
    pub title: String,
    pub spoiler: bool,
    pub remix: bool,
    pub thumbnail: bool,
    /// ISO time.
    pub created_at: String,
    pub message: String,
    pub application_id: Option<String>,
}

impl Options {
    /// The clip fields of the file entry, as `reserveClipUpload` and `sendClipUpload` both write them (an unset
    /// application id is left out, as `undefined` is by `JSON.stringify`).
    pub(crate) fn clip_fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        fields.insert("is_clip".into(), json!(true));
        fields.insert("is_spoiler".into(), json!(self.spoiler));
        fields.insert("is_remix".into(), json!(self.remix));
        fields.insert("is_thumbnail".into(), json!(self.thumbnail));
        fields.insert("title".into(), json!(self.title));
        if let Some(app) = &self.application_id {
            fields.insert("application_id".into(), json!(app));
        }
        fields.insert("clip_created_at".into(), json!(self.created_at));
        fields.insert("clip_participant_ids".into(), json!(self.participants));
        fields
    }

    /// `reserveClipUpload`'s body.
    pub(crate) fn reserve_body(&self, size: u64) -> Value {
        let mut file = Map::new();
        file.insert("filename".into(), json!(self.file_name));
        file.insert("file_size".into(), json!(size));
        file.insert("id".into(), json!("0"));
        file.extend(self.clip_fields());
        file.insert("original_content_type".into(), json!("video/mp4"));
        json!({"content": self.message, "files": [Value::Object(file)]})
    }

    /// `sendClipUpload`'s message body (the nonce and reference filled by the caller).
    pub(crate) fn message_body(
        &self,
        channel: u64,
        size: u64,
        uploaded: &str,
        nonce: &str,
        reference: Option<Value>,
    ) -> Value {
        let mut attachment = Map::new();
        attachment.insert("id".into(), json!("0"));
        attachment.insert("filesize".into(), json!(size));
        attachment.insert("filename".into(), json!(self.file_name));
        attachment.insert("uploaded_filename".into(), json!(uploaded));
        attachment.extend(self.clip_fields());
        let mut body = json!({
            "content": self.message,
            "nonce": nonce,
            "channel_id": channel.to_string(),
            "sticker_ids": [],
            "type": 0,
            "attachments": [Value::Object(attachment)],
        });
        if let Some(reference) = reference {
            body["message_reference"] = reference;
        }
        body
    }
}

/// `shouldRetryWithFFmpeg`: Discord's "not a valid clip" answer (code 50174).
pub(crate) fn not_a_valid_clip(body: &Value) -> bool {
    body["code"].as_u64() == Some(50174)
        || body
            .to_string()
            .to_ascii_lowercase()
            .contains("not a valid clip")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_as_the_original_reads_them() {
        assert!(allowed("a.MP4") && allowed("b.m4v") && !allowed("c.mov"));
        assert_eq!(title_from_name("Epic Moment.mp4"), "Epic Moment");
        assert_eq!(title_from_name(".mp4"), ".mp4");
        assert_eq!(title_from_name("noext"), "noext");
    }

    #[test]
    fn metadata_is_read_after_the_footer() {
        let mut bytes = b"\0\0\0\x20ftypisom".to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(&CLIP_FOOTER);
        bytes.extend_from_slice(
            br#"[{"applicationId":"42","applicationName":"Game","users":["1","2"]}]"#,
        );
        let parsed = parse_metadata(&bytes).unwrap();
        assert_eq!(parsed.application_id.as_deref(), Some("42"));
        assert_eq!(parsed.application_name.as_deref(), Some("Game"));
        assert_eq!(parsed.users, ["1", "2"]);
        assert!(parse_metadata(b"no footer here at all, just bytes").is_none());
        let mut object = CLIP_FOOTER.to_vec();
        object.extend_from_slice(br#"{"version":1}"#);
        assert_eq!(parse_metadata(&object), Some(Parsed::default()));
    }

    #[test]
    fn dates_round_trip_through_the_field() {
        // 2026-11-19 02:20 UTC, three hours east.
        let ms = 1_795_054_800_000;
        let shown = datetime_local(ms, 180);
        assert_eq!(shown, "2026-11-19T05:20");
        assert_eq!(parse_datetime(&shown, 180), Some(ms));
        assert_eq!(iso(ms), "2026-11-19T02:20:00.000Z");
        assert_eq!(parse_datetime("2026-11-19T02:20:00.000Z", 180), Some(ms));
        assert_eq!(parse_datetime("", 0), None);
        assert_eq!(parse_datetime("tomorrow", 0), None);
    }

    #[test]
    fn bodies_carry_the_clip_fields() {
        let options = Options {
            file_name: "clip.mp4".into(),
            participants: vec!["7".into()],
            title: "T".into(),
            spoiler: true,
            created_at: "2026-11-19T02:20:00.000Z".into(),
            message: "hi".into(),
            ..Options::default()
        };
        let reserve = options.reserve_body(10);
        let file = &reserve["files"][0];
        assert_eq!(reserve["content"], "hi");
        assert_eq!(file["id"], "0");
        assert_eq!(file["is_clip"], true);
        assert_eq!(file["is_spoiler"], true);
        assert_eq!(file["clip_participant_ids"], json!(["7"]));
        assert!(file.get("application_id").is_none());
        assert_eq!(file["original_content_type"], "video/mp4");
        let message = options.message_body(5, 10, "up/clip.mp4", "99", None);
        let attachment = &message["attachments"][0];
        assert_eq!(message["channel_id"], "5");
        assert_eq!(message["type"], 0);
        assert_eq!(attachment["filesize"], 10);
        assert_eq!(attachment["uploaded_filename"], "up/clip.mp4");
        assert_eq!(attachment["title"], "T");
        assert!(message.get("message_reference").is_none());
        assert!(not_a_valid_clip(&json!({"code": 50174})));
        assert!(not_a_valid_clip(&json!({"message": "Not a valid clip"})));
        assert!(!not_a_valid_clip(&json!({"code": 50035})));
    }
}
