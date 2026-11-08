//! `FrecencyUserSettings` (settings-proto type 2), only the emoji maps: field 6 `emoji_frecency` and field 13
//! `emoji_reaction_frecency`, each an `EmojiFrecency { map<string, FrecencyItem> emojis = 1 }`. `FrecencyItem` is
//! `total_uses` (uint32, 1), `recent_uses` (repeated uint64, 2), `frecency` (int32, 3), `score` (int32, 4).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub const EMOJI_FRECENCY: u64 = 6;
pub const EMOJI_REACTION_FRECENCY: u64 = 13;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub total_uses: u32,
    /// Unix milliseconds of the latest uses.
    pub recent_uses: Vec<u64>,
    pub frecency: i32,
    pub score: i32,
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Skipped,
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = self.bytes.split_first()?;
            self.bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        if length > self.bytes.len() {
            return None;
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Some(head)
    }

    fn next_field(&mut self) -> Option<(u64, Value<'a>)> {
        if self.bytes.is_empty() {
            return None;
        }
        let tag = self.varint()?;
        let value = match tag & 7 {
            0 => Value::Varint(self.varint()?),
            1 => {
                self.take(8)?;
                Value::Skipped
            }
            2 => {
                let length = usize::try_from(self.varint()?).ok()?;
                Value::Bytes(self.take(length)?)
            }
            5 => {
                self.take(4)?;
                Value::Skipped
            }
            _ => return None,
        };
        Some((tag >> 3, value))
    }
}

fn fields(bytes: &[u8]) -> impl Iterator<Item = (u64, Value<'_>)> {
    let mut reader = Reader { bytes };
    std::iter::from_fn(move || reader.next_field())
}

fn packed_varints(bytes: &[u8]) -> Vec<u64> {
    let mut reader = Reader { bytes };
    std::iter::from_fn(|| {
        (!reader.bytes.is_empty())
            .then(|| reader.varint())
            .flatten()
    })
    .collect()
}

fn item(bytes: &[u8]) -> Item {
    let mut out = Item::default();
    for (number, value) in fields(bytes) {
        match (number, value) {
            (1, Value::Varint(total)) => out.total_uses = total as u32,
            (2, Value::Varint(time)) => out.recent_uses.push(time),
            (2, Value::Bytes(packed)) => out.recent_uses.extend(packed_varints(packed)),
            (3, Value::Varint(frecency)) => out.frecency = frecency as i32,
            (4, Value::Varint(score)) => out.score = score as i32,
            _ => {}
        }
    }
    out
}

/// The `emojis` map of one `EmojiFrecency` field, keyed by emoji name or custom emoji id.
pub fn emoji_map(settings_base64: &str, field: u64) -> Vec<(String, Item)> {
    let Ok(settings) = STANDARD.decode(settings_base64) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (number, value) in fields(&settings) {
        let (true, Value::Bytes(frecency)) = (number == field, value) else {
            continue;
        };
        for (entry_number, entry) in fields(frecency) {
            let (1, Value::Bytes(entry)) = (entry_number, entry) else {
                continue;
            };
            let mut key = String::new();
            let mut value = Item::default();
            for (part, payload) in fields(entry) {
                match (part, payload) {
                    (1, Value::Bytes(text)) => key = String::from_utf8_lossy(text).into_owned(),
                    (2, Value::Bytes(message)) => value = item(message),
                    _ => {}
                }
            }
            if !key.is_empty() {
                out.push((key, value));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn bytes(number: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((number << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn reads_reaction_frecency_entries() {
        let mut uses = Vec::new();
        uses.extend(varint(1_762_500_000_000));
        uses.extend(varint(1_762_600_000_000));
        let mut item = varint(1 << 3);
        item.extend(varint(7));
        item.extend(bytes(2, &uses));
        let mut entry = bytes(1, b"eyes");
        entry.extend(bytes(2, &item));
        let map = bytes(1, &entry);
        let settings = bytes(EMOJI_REACTION_FRECENCY, &map);
        let read = emoji_map(&STANDARD.encode(&settings), EMOJI_REACTION_FRECENCY);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].0, "eyes");
        assert_eq!(read[0].1.total_uses, 7);
        assert_eq!(
            read[0].1.recent_uses,
            [1_762_500_000_000, 1_762_600_000_000]
        );
        assert!(emoji_map(&STANDARD.encode(&settings), EMOJI_FRECENCY).is_empty());
    }
}
