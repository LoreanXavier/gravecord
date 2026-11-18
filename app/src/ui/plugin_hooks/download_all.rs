//! DownloadAllAttachments (Equicord `src/equicordplugins/downloadAllAttachments`, GPL-3.0-or-later): a hover bar button
//! on a message with attachments that saves all of them at once, one file as it is and several as a stored ZIP named
//! `attachments-<message id>.zip`, then says how it went in a toast. Not part of the pinned client. The app has no save
//! dialog (`saveFile`), so the file goes into Downloads like the app's other downloads (`net.rs`, `save_all`).

use super::super::Root;
use crate::plugins::State;
use dc_state::{Attachment, Id, Message};
use gpui::Context;
use serde_json::Value;

const ID: &str = "DownloadAllAttachments";
/// The button's label.
pub(in crate::ui) const DOWNLOAD_ALL: &str = "Download All Attachments";
/// `MAX_ZIP_BYTES`.
const MAX_ZIP_BYTES: u64 = 1024 * 1024 * 1024;
/// The answer's key, then the message id.
const KEY: &str = "plugin-download-all:";

/// The button shows on a message with attachments while the plugin is on.
pub(in crate::ui) fn download_all(message: &Message) -> bool {
    !message.attachments.is_empty()
        && !crate::plugins::bypassed()
        && crate::plugin_on!("DownloadAllAttachments")
}

/// `fetchAttachment`'s sources: the proxy URL, then with `downloadAllFileTypes` the URL.
fn sources(state: &State, attachment: &Attachment) -> Vec<String> {
    let mut list = vec![attachment.proxy_url.clone()];
    if state.value(ID, "downloadAllFileTypes") == Value::Bool(true) {
        list.push(attachment.url.clone());
    }
    list.retain(|source| !source.is_empty());
    list
}

/// `pluralize(n, "attachment")`.
fn plural(count: u64) -> String {
    if count == 1 {
        "1 attachment".to_owned()
    } else {
        format!("{count} attachments")
    }
}

/// The toast after the downloads: its text and whether it is the success one.
fn outcome(saved: u64, total: u64) -> (String, bool) {
    if saved == 0 {
        ("Could not download any attachments.".to_owned(), false)
    } else if saved == total {
        (format!("Saved {}.", plural(saved)), true)
    } else {
        (
            format!(
                "Saved {saved} of {total} attachments. {} failed.",
                total - saved
            ),
            false,
        )
    }
}

impl Root {
    /// `variant: "critical"`: the toast with the critical feedback icon.
    fn critical_toast(&mut self, text: String, cx: &mut Context<Self>) {
        self.put_icon_toast(
            text,
            ("icons/login_error.svg", "--icon-feedback-critical"),
            cx,
        );
    }

    /// `downloadAll`.
    pub(in crate::ui) fn download_all_attachments(
        &mut self,
        message: Id,
        attachments: &[Attachment],
        cx: &mut Context<Self>,
    ) {
        if attachments
            .iter()
            .map(|attachment| attachment.size)
            .sum::<u64>()
            > MAX_ZIP_BYTES
        {
            self.critical_toast(
                "These attachments are too large to zip. Save them one by one instead.".to_owned(),
                cx,
            );
            return;
        }
        let names = crate::plugins::zip::unique_filenames(
            &attachments
                .iter()
                .map(|attachment| attachment.filename.clone())
                .collect::<Vec<_>>(),
        );
        let files = crate::plugins::with(|state| {
            attachments
                .iter()
                .zip(names)
                .map(|(attachment, name)| (sources(state, attachment), name))
                .collect()
        });
        let _ = self.requests.send(crate::net::Request::DownloadAll {
            files,
            archive: format!("attachments-{message}.zip"),
            key: format!("{KEY}{message}"),
        });
    }

    /// The outcome's toast; true when `key` was this plugin's.
    pub(in crate::ui) fn download_all_answered(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if !key.starts_with(KEY) {
            return false;
        }
        let (text, success) = outcome(
            body["saved"].as_u64().unwrap_or(0),
            body["total"].as_u64().unwrap_or(0),
        );
        if success {
            self.show_success_toast(text, cx);
        } else {
            self.critical_toast(text, cx);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sources_size_check_and_toasts_follow_the_original() {
        let attachment = Attachment {
            url: "https://cdn/a.exe".to_owned(),
            proxy_url: "https://media/a.exe".to_owned(),
            ..Attachment::default()
        };
        let mut state = State::default();
        state.set_enabled(ID, true);
        assert_eq!(sources(&state, &attachment), ["https://media/a.exe"]);
        state.set_value(ID, "downloadAllFileTypes", &json!(true));
        assert_eq!(
            sources(&state, &attachment),
            ["https://media/a.exe", "https://cdn/a.exe"]
        );
        assert_eq!(MAX_ZIP_BYTES, 1 << 30);
        assert_eq!(outcome(1, 1), ("Saved 1 attachment.".to_owned(), true));
        assert_eq!(outcome(3, 3), ("Saved 3 attachments.".to_owned(), true));
        assert_eq!(
            outcome(2, 3),
            ("Saved 2 of 3 attachments. 1 failed.".to_owned(), false)
        );
        assert_eq!(
            outcome(0, 3),
            ("Could not download any attachments.".to_owned(), false)
        );
    }
}
