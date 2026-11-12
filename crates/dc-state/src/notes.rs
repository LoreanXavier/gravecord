//! NoteStore (module 279263): the private note this account keeps on each user.
//!
//! `USER_NOTE_UPDATE {id, note}` (module 225123 passes the payload through) stores `{loading: false, note}` under the
//! user; `USER_NOTE_LOAD_START` marks one as loading until the REST answer arrives; `CONNECTION_OPEN` (READY),
//! `RESET_SOCKET` and `LOGOUT` forget every note.

use crate::{Id, Stores, id, text};
use serde_json::Value;
use std::collections::HashMap;

/// One user's entry: `{loading, note}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    pub loading: bool,
    pub note: Option<String>,
}

#[derive(Default)]
pub struct NoteStore {
    notes: HashMap<Id, Note>,
    /// Counts every change, so a screen can tell a field was replaced from outside.
    pub version: u64,
}

impl NoteStore {
    /// `getNote`.
    pub fn get_note(&self, user: Id) -> Option<&Note> {
        self.notes.get(&user)
    }

    /// The note text, empty when there is none.
    pub fn text_of(&self, user: Id) -> &str {
        self.notes
            .get(&user)
            .and_then(|entry| entry.note.as_deref())
            .unwrap_or_default()
    }

    /// `USER_NOTE_LOAD_START`.
    pub fn load_start(&mut self, user: Id) {
        self.notes.insert(
            user,
            Note {
                loading: true,
                note: None,
            },
        );
        self.version += 1;
    }

    /// The note the REST read (`GET /users/@me/notes/{id}`) or the editor's save settled on.
    pub fn set(&mut self, user: Id, note: Option<String>) {
        self.notes.insert(
            user,
            Note {
                loading: false,
                note,
            },
        );
        self.version += 1;
    }

    fn clear(&mut self) {
        self.notes.clear();
        self.version += 1;
    }
}

impl Stores {
    pub(crate) fn notes_dispatch(&mut self, name: &str, data: &Value) {
        match name {
            "USER_NOTE_UPDATE" => {
                if let Some(user) = id(&data["id"]) {
                    self.notes.set(user, text(&data["note"]));
                }
            }
            "READY" | "RESET_SOCKET" | "LOGOUT" => self.notes.clear(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_note_update_replaces_the_loading_entry_and_ready_forgets_it() {
        let mut stores = Stores::default();
        stores.notes.load_start(42);
        assert!(stores.notes.get_note(42).unwrap().loading);
        stores.dispatch(
            "USER_NOTE_UPDATE",
            &json!({"id": "42", "note": "met at the fair"}),
        );
        let note = stores.notes.get_note(42).unwrap();
        assert!(!note.loading);
        assert_eq!(stores.notes.text_of(42), "met at the fair");
        // Another session cleared it: the payload carries an empty note.
        stores.dispatch("USER_NOTE_UPDATE", &json!({"id": "42", "note": ""}));
        assert_eq!(stores.notes.text_of(42), "");
        stores.dispatch("USER_NOTE_UPDATE", &json!({"id": "7", "note": "x"}));
        let before = stores.notes.version;
        stores.dispatch(
            "READY",
            &json!({"user": {"id": "1", "username": "me", "discriminator": "0"}, "guilds": [], "private_channels": []}),
        );
        assert!(stores.notes.get_note(7).is_none());
        assert!(stores.notes.version > before);
    }
}
