use dc_state::Message;
use serde_json::json;

#[test]
fn attachment_titles_preserve_the_original_download_name_and_final_extension() {
    for (filename, title, expected) in [
        ("package.tar.gz", Some("Archive"), "Archive.gz"),
        (".gitignore", Some("Rules"), "Rules"),
        ("README", Some("Notes"), "Notes"),
        ("picture.png", Some(""), ".png"),
        ("ölçüm.pdf", Some("Ölçüm"), "Ölçüm.pdf"),
        ("file.", Some("Name"), "Name."),
        ("package.zip", None, "package.zip"),
    ] {
        let message = Message::from_value(&json!({
            "id": "1", "channel_id": "2", "type": 0, "content": "",
            "timestamp": "2026-11-15T12:00:00Z",
            "author": {"id": "3", "username": "Reference"},
            "attachments": [{"id": "4", "filename": filename, "title": title,
                "url": "https://example.invalid/original-file", "size": 75497472}],
        }))
        .expect("attachment packet");
        let attachment = &message.attachments[0];
        assert_eq!(attachment.title.as_deref(), title);
        assert_eq!(attachment.display_name(), expected);
        assert_eq!(attachment.filename, filename);
        assert_eq!(attachment.url, "https://example.invalid/original-file");
    }
}
