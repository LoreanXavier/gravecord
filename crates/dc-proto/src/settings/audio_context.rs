//! `audio_context_settings` (field 16 of `PreloadedUserSettings`, `AudioContextSettings`): the volume and local mutes
//! this account chose for other people, kept in the account so every client of it hears them the same. Two maps from
//! a user id (`fixed64`) to an `AudioContextSetting`: `user` (1, their voice) and `stream` (2, their Go Live sound).
//! An entry is `muted` (1, bool), `volume` (2, float, the `MediaEngineStore` percent: 100 is unity for a voice, 18 for
//! a stream), `modified_at` (3, fixed64, ms) and `soundboard_muted` (4, bool). protobuf-ts writes a scalar only when
//! it is not its default (module 873298), so an entry with the default volume and no mute is not written at all.
//!
//! The writes are those of module 386741 (`C`): an entry is created at the context's default, changed, stamped, and
//! dropped again when it is back to the default; a map over 300 entries loses its oldest.

use super::{
    STANDARD, WIRE_BYTES, WIRE_FIXED32, WIRE_FIXED64, WIRE_VARINT, push_bytes_field, push_varint,
    raw_fields, whole_message_patch, with_encoded,
};
use base64::Engine;

pub const FIELD_AUDIO_CONTEXT_SETTINGS: u64 = 16;
/// The most entries one map keeps (module 386741).
pub const MAX_ENTRIES: usize = 300;

/// The two maps of `AudioContextSettings`, as `MediaEngineStore` names its contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioContext {
    /// `user` (1), the `default` context: a person's voice.
    User,
    /// `stream` (2): the sound of a person's Go Live stream.
    Stream,
}

impl AudioContext {
    fn field(self) -> u64 {
        match self {
            Self::User => 1,
            Self::Stream => 2,
        }
    }

    /// The volume an entry is created with and dropped at (module 947320: `USER` 100, `STREAM` 18).
    pub fn default_volume(self) -> f32 {
        match self {
            Self::User => 100.,
            Self::Stream => 18.,
        }
    }
}

/// One `AudioContextSetting`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub muted: bool,
    pub volume: f32,
    pub modified_at: u64,
    pub soundboard_muted: bool,
}

impl Entry {
    /// `oP.create({muted: false, volume: T(context)})`.
    pub fn new(context: AudioContext) -> Self {
        Self {
            muted: false,
            volume: context.default_volume(),
            modified_at: 0,
            soundboard_muted: false,
        }
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let mut entry = Self {
            muted: false,
            volume: 0.,
            modified_at: 0,
            soundboard_muted: false,
        };
        let mut rest = bytes;
        while !rest.is_empty() {
            let (number, wire, value, tail) = next(rest)?;
            rest = tail;
            match (number, wire, value) {
                (1, WIRE_VARINT, Value::Varint(on)) => entry.muted = on != 0,
                (2, WIRE_FIXED32, Value::Fixed32(bits)) => entry.volume = f32::from_bits(bits),
                (3, WIRE_FIXED64, Value::Fixed64(ms)) => entry.modified_at = ms,
                (4, WIRE_VARINT, Value::Varint(on)) => entry.soundboard_muted = on != 0,
                _ => {}
            }
        }
        Some(entry)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.muted {
            push_varint(&mut out, 1 << 3 | WIRE_VARINT);
            out.push(1);
        }
        if self.volume != 0. {
            push_varint(&mut out, 2 << 3 | WIRE_FIXED32);
            out.extend_from_slice(&self.volume.to_bits().to_le_bytes());
        }
        if self.modified_at != 0 {
            push_varint(&mut out, 3 << 3 | WIRE_FIXED64);
            out.extend_from_slice(&self.modified_at.to_le_bytes());
        }
        if self.soundboard_muted {
            push_varint(&mut out, 4 << 3 | WIRE_VARINT);
            out.push(1);
        }
        out
    }

    /// Back to what an absent entry means: nothing to keep (`volume !== T(n) || muted || soundboardMuted`).
    fn is_default(&self, context: AudioContext) -> bool {
        self.volume == context.default_volume() && !self.muted && !self.soundboard_muted
    }
}

enum Value<'a> {
    Varint(u64),
    Fixed32(u32),
    Fixed64(u64),
    Bytes(&'a [u8]),
}

/// The next field of `bytes`: its number, wire type, value and what follows it.
fn next(bytes: &[u8]) -> Option<(u64, u64, Value<'_>, &[u8])> {
    let mut reader = super::Reader { bytes };
    let tag = reader.varint()?;
    let (number, wire) = (tag >> 3, tag & 7);
    let value = match wire {
        WIRE_VARINT => Value::Varint(reader.varint()?),
        WIRE_FIXED32 => Value::Fixed32(u32::from_le_bytes(reader.take(4)?.try_into().ok()?)),
        WIRE_FIXED64 => Value::Fixed64(u64::from_le_bytes(reader.take(8)?.try_into().ok()?)),
        WIRE_BYTES => {
            let length = usize::try_from(reader.varint()?).ok()?;
            Value::Bytes(reader.take(length)?)
        }
        _ => return None,
    };
    Some((number, wire, value, reader.bytes))
}

/// Both maps, in the order the message lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioContextSettings {
    pub user: Vec<(u64, Entry)>,
    pub stream: Vec<(u64, Entry)>,
}

impl AudioContextSettings {
    pub fn map(&self, context: AudioContext) -> &Vec<(u64, Entry)> {
        match context {
            AudioContext::User => &self.user,
            AudioContext::Stream => &self.stream,
        }
    }

    fn map_mut(&mut self, context: AudioContext) -> &mut Vec<(u64, Entry)> {
        match context {
            AudioContext::User => &mut self.user,
            AudioContext::Stream => &mut self.stream,
        }
    }

    pub fn get(&self, context: AudioContext, user: u64) -> Option<&Entry> {
        self.map(context)
            .iter()
            .find(|(id, _)| *id == user)
            .map(|(_, entry)| entry)
    }

    /// The `AudioContextSettings` message itself (without the field 16 tag).
    pub fn decode(message: &[u8]) -> Self {
        let mut settings = Self::default();
        let mut rest = message;
        while !rest.is_empty() {
            let Some((number, _, value, tail)) = next(rest) else {
                break;
            };
            rest = tail;
            let (context, Value::Bytes(pair)) = (number, value) else {
                continue;
            };
            let context = match context {
                1 => AudioContext::User,
                2 => AudioContext::Stream,
                _ => continue,
            };
            // A map entry: key 1 (fixed64), value 2 (the message).
            let (mut key, mut entry) = (None, None);
            let mut fields = pair;
            while !fields.is_empty() {
                let Some((number, _, value, tail)) = next(fields) else {
                    break;
                };
                fields = tail;
                match (number, value) {
                    (1, Value::Fixed64(id)) => key = Some(id),
                    (2, Value::Bytes(bytes)) => entry = Entry::decode(bytes),
                    _ => {}
                }
            }
            if let Some(key) = key {
                let entry = entry.unwrap_or(Entry {
                    muted: false,
                    volume: 0.,
                    modified_at: 0,
                    soundboard_muted: false,
                });
                let map = settings.map_mut(context);
                map.retain(|(id, _)| *id != key);
                map.push((key, entry));
            }
        }
        settings
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for context in [AudioContext::User, AudioContext::Stream] {
            for (id, entry) in self.map(context) {
                let mut pair = Vec::new();
                push_varint(&mut pair, 1 << 3 | WIRE_FIXED64);
                pair.extend_from_slice(&id.to_le_bytes());
                push_bytes_field(&mut pair, 2, &entry.encode());
                push_bytes_field(&mut out, context.field(), &pair);
            }
        }
        out
    }

    /// The write of module 386741 for one person: the entry (created at the default when there is none) takes
    /// `change`, is stamped `now_ms`, is dropped when it is back to the default, and the map keeps its newest 300.
    pub fn update(
        &mut self,
        context: AudioContext,
        user: u64,
        now_ms: u64,
        change: impl FnOnce(&mut Entry),
    ) {
        let map = self.map_mut(context);
        let index = match map.iter().position(|(id, _)| *id == user) {
            Some(index) => index,
            None => {
                map.push((user, Entry::new(context)));
                map.len() - 1
            }
        };
        let entry = &mut map[index].1;
        change(entry);
        entry.modified_at = now_ms;
        if entry.is_default(context) {
            map.remove(index);
        }
        if map.len() > MAX_ENTRIES {
            let mut order: Vec<(u64, u64)> = map
                .iter()
                .map(|(id, entry)| (entry.modified_at, *id))
                .collect();
            order.sort_unstable();
            let drop: Vec<u64> = order[..map.len() - MAX_ENTRIES]
                .iter()
                .map(|(_, id)| *id)
                .collect();
            map.retain(|(id, _)| !drop.contains(id));
        }
    }
}

/// The account's `audio_context_settings`, empty when it has none.
pub fn read(settings: &[u8]) -> AudioContextSettings {
    super::fields(settings)
        .find(|(number, _)| *number == FIELD_AUDIO_CONTEXT_SETTINGS)
        .map(|(_, message)| AudioContextSettings::decode(message))
        .unwrap_or_default()
}

/// `settings` with `audio_context_settings` replaced by `audio`: the PATCH body value (the whole field 16 message, as
/// the server replaces a top-level message it is sent) and the settings as they are from now on. Every other field is
/// kept byte for byte.
pub fn write(settings: &[u8], audio: &AudioContextSettings) -> (String, Vec<u8>) {
    let mut encoded = Vec::new();
    push_bytes_field(&mut encoded, FIELD_AUDIO_CONTEXT_SETTINGS, &audio.encode());
    let updated = with_encoded(settings, &[], FIELD_AUDIO_CONTEXT_SETTINGS, &encoded);
    let (patch, updated) = whole_message_patch(updated, FIELD_AUDIO_CONTEXT_SETTINGS);
    (patch, updated)
}

/// The PATCH value as bytes (the field 16 tag and message), for checks.
pub fn patch_bytes(patch: &str) -> Vec<u8> {
    STANDARD.decode(patch).unwrap_or_default()
}

/// Whether `settings` carries field 16 at all.
pub fn present(settings: &[u8]) -> bool {
    raw_fields(settings)
        .iter()
        .any(|(number, _)| *number == FIELD_AUDIO_CONTEXT_SETTINGS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One map entry as the service writes it: key, then the setting.
    fn pair(out: &mut Vec<u8>, context: u64, id: u64, volume: f32, at: u64, muted: bool) {
        let mut entry = Vec::new();
        if muted {
            entry.extend_from_slice(&[0x08, 1]);
        }
        entry.push(0x15);
        entry.extend_from_slice(&volume.to_le_bytes());
        entry.push(0x19);
        entry.extend_from_slice(&at.to_le_bytes());
        let mut map = vec![0x09];
        map.extend_from_slice(&id.to_le_bytes());
        map.push(0x12);
        map.push(entry.len() as u8);
        map.extend_from_slice(&entry);
        out.push((context << 3 | 2) as u8);
        out.push(map.len() as u8);
        out.extend_from_slice(&map);
    }

    fn sample() -> Vec<u8> {
        let mut message = Vec::new();
        pair(&mut message, 1, 11, 199.526, 1_700_000_000_001, false);
        pair(&mut message, 1, 12, 100., 1_700_000_000_002, true);
        pair(&mut message, 2, 13, 51.148, 1_700_000_000_003, false);
        message
    }

    #[test]
    fn the_message_reads_and_writes_back_byte_for_byte() {
        let message = sample();
        let read = AudioContextSettings::decode(&message);
        assert_eq!(read.user.len(), 2);
        assert_eq!(read.stream.len(), 1);
        assert_eq!(read.get(AudioContext::User, 11).unwrap().volume, 199.526);
        assert!(read.get(AudioContext::User, 12).unwrap().muted);
        assert_eq!(read.get(AudioContext::Stream, 13).unwrap().volume, 51.148);
        assert_eq!(read.encode(), message);
    }

    #[test]
    fn a_new_volume_changes_only_its_entry_and_the_rest_of_the_settings_stays() {
        // appearance (13) and guild_folders (14) around the audio settings.
        let mut settings = vec![0x6a, 0x02, 0x08, 0x01];
        push_bytes_field(&mut settings, 16, &sample());
        settings.extend_from_slice(&[0x72, 0x00]);
        let mut audio = read(&settings);
        audio.update(AudioContext::User, 99, 1_800_000_000_000, |entry| {
            entry.volume = 150.
        });
        let (patch, updated) = write(&settings, &audio);
        // The PATCH is the whole field 16: the people's entries with the new one last, then the streams' (protobuf-ts
        // writes the `user` map before the `stream` map).
        let mut expected = Vec::new();
        pair(&mut expected, 1, 11, 199.526, 1_700_000_000_001, false);
        pair(&mut expected, 1, 12, 100., 1_700_000_000_002, true);
        pair(&mut expected, 1, 99, 150., 1_800_000_000_000, false);
        pair(&mut expected, 2, 13, 51.148, 1_700_000_000_003, false);
        let mut field = Vec::new();
        push_bytes_field(&mut field, 16, &expected);
        assert_eq!(patch_bytes(&patch), field);
        // Everything else is kept.
        assert!(updated.starts_with(&[0x6a, 0x02, 0x08, 0x01, 0x72, 0x00]));
        assert_eq!(read(&updated), audio);
    }

    #[test]
    fn an_entry_back_at_the_default_is_dropped() {
        let mut audio = AudioContextSettings::decode(&sample());
        audio.update(AudioContext::User, 12, 5, |entry| entry.muted = false);
        assert!(audio.get(AudioContext::User, 12).is_none());
        // A stream's default is 18, not 100.
        audio.update(AudioContext::Stream, 13, 5, |entry| entry.volume = 18.);
        assert!(audio.get(AudioContext::Stream, 13).is_none());
        audio.update(AudioContext::Stream, 14, 5, |entry| entry.volume = 100.);
        assert_eq!(audio.get(AudioContext::Stream, 14).unwrap().volume, 100.);
        // A soundboard mute alone keeps the entry, at the default volume.
        audio.update(AudioContext::User, 15, 6, |entry| {
            entry.soundboard_muted = true
        });
        let kept = audio.get(AudioContext::User, 15).unwrap();
        assert_eq!(
            (kept.volume, kept.muted, kept.soundboard_muted),
            (100., false, true)
        );
    }

    #[test]
    fn a_full_map_forgets_its_oldest_entry() {
        let mut audio = AudioContextSettings::default();
        for id in 0..MAX_ENTRIES as u64 {
            audio.update(AudioContext::User, id + 1, 1000 + id, |entry| {
                entry.volume = 50.
            });
        }
        audio.update(AudioContext::User, 9999, 1, |entry| entry.volume = 60.);
        assert_eq!(audio.user.len(), MAX_ENTRIES);
        // The new entry was stamped 1, the oldest of all: it is the one that goes.
        assert!(audio.get(AudioContext::User, 9999).is_none());
        audio.update(AudioContext::User, 8888, 5000, |entry| entry.volume = 60.);
        assert!(audio.get(AudioContext::User, 1).is_none());
        assert!(audio.get(AudioContext::User, 8888).is_some());
    }

    #[test]
    fn settings_without_the_field_read_empty() {
        assert_eq!(
            read(&[0x6a, 0x02, 0x08, 0x01]),
            AudioContextSettings::default()
        );
        assert!(!present(&[0x6a, 0x02, 0x08, 0x01]));
    }

    /// The account settings of a recorded READY read and write back unchanged:
    /// `GRAVECORD_REPLAY_READY=<gateway.jsonl> cargo test -p dc-proto recorded -- --ignored`.
    #[test]
    #[ignore]
    fn recorded_settings_write_back_unchanged() {
        let Ok(path) = std::env::var("GRAVECORD_REPLAY_READY") else {
            return;
        };
        let text = std::fs::read_to_string(path).unwrap();
        let proto = text
            .lines()
            .find_map(|line| {
                let start = line.find("\"t\":\"READY\"")?;
                let _ = start;
                let key = "\"user_settings_proto\":\"";
                let at = line.find(key)? + key.len();
                let end = line[at..].find('"')? + at;
                Some(line[at..end].to_owned())
            })
            .unwrap();
        let settings = super::super::decode(&proto);
        let message = super::super::fields(&settings)
            .find(|(number, _)| *number == FIELD_AUDIO_CONTEXT_SETTINGS)
            .unwrap()
            .1
            .to_vec();
        let audio = AudioContextSettings::decode(&message);
        assert!(!audio.user.is_empty());
        assert_eq!(audio.encode(), message);
        let (_, updated) = write(&settings, &audio);
        assert_eq!(read(&updated), audio);
    }
}
