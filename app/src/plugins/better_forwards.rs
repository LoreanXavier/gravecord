//! BetterForwards' `forward_only` (Equicord `src/equicordplugins/betterForwards/index.tsx`, GPL-3.0-or-later): the
//! attachments and embeds a forward keeps, waiting here by the send's nonce until `net.rs` builds that send's
//! `message_reference` (`forward_only: {attachment_ids, embed_indices}`, as module 138399 sends `forwardOptions`).

use std::collections::HashMap;
use std::sync::Mutex;

/// The attachment ids and embed indices a forward keeps.
type Kept = (Vec<String>, Vec<usize>);

static FORWARD_ONLY: Mutex<Option<HashMap<String, Kept>>> = Mutex::new(None);

/// Keeps the choice for the send with this nonce.
pub(crate) fn set(nonce: &str, attachment_ids: Vec<String>, embed_indices: Vec<usize>) {
    FORWARD_ONLY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(nonce.to_owned(), (attachment_ids, embed_indices));
}

/// The choice of the send with this nonce, once.
pub(crate) fn take(nonce: &str) -> Option<(Vec<String>, Vec<usize>)> {
    FORWARD_ONLY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()?
        .remove(nonce)
}

/// The text a failed forward is sent again as (`sendForward`): the source channel and the text, each line after the
/// preface, then the attachments' links five to a message, the note after the last.
pub(crate) fn resend_texts(
    preface: &str,
    source_channel: u64,
    content: &str,
    attachment_urls: &[String],
    note: &str,
) -> Vec<String> {
    let line = format!("\n{preface} ");
    let prefix = format!(
        "{line}*Forwarded from <#{source_channel}>*{line}{}",
        content.trim().replace('\n', &line)
    );
    let suffix = if note.trim().is_empty() {
        String::new()
    } else {
        format!("\n{}", note.trim())
    };
    if attachment_urls.is_empty() {
        return vec![prefix + &suffix];
    }
    attachment_urls
        .chunks(5)
        .enumerate()
        .map(|(index, group)| {
            let mut text = if index == 0 {
                format!("{prefix}{line}Attachments:{line}")
            } else {
                line.clone()
            };
            text.push_str(&group.join(&line));
            if (index + 1) * 5 >= attachment_urls.len() {
                text.push_str(&suffix);
            }
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resends_read_as_the_original_writes_them() {
        assert_eq!(
            resend_texts(">", 7, "hi\nthere", &[], "note"),
            ["\n> *Forwarded from <#7>*\n> hi\n> there\nnote"]
        );
        let urls: Vec<String> = (1..=6).map(|n| format!("u{n}")).collect();
        let texts = resend_texts("-#", 7, "x", &urls, "");
        assert_eq!(texts.len(), 2);
        assert_eq!(
            texts[0],
            "\n-# *Forwarded from <#7>*\n-# x\n-# Attachments:\n-# u1\n-# u2\n-# u3\n-# u4\n-# u5"
        );
        assert_eq!(texts[1], "\n-# u6");
        set("n1", vec!["0".into()], vec![]);
        assert_eq!(take("n1"), Some((vec!["0".to_owned()], vec![])));
        assert_eq!(take("n1"), None);
    }
}
