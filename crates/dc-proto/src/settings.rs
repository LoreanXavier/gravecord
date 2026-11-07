//! `PreloadedUserSettings` from READY's `user_settings_proto`. Field numbers are in `dumps/PROTOS/`.
//! Only what the UI reads so far is decoded; unknown fields are skipped by wire type.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const FIELD_APPEARANCE: u64 = 13;
const APPEARANCE_THEME: u64 = 1;
const FIELD_GUILD_FOLDERS: u64 = 14;
const FOLDERS_FOLDER: u64 = 1;
const FOLDERS_POSITIONS: u64 = 2;
const FOLDER_GUILD_IDS: u64 = 1;

const WIRE_VARINT: u64 = 0;
const WIRE_FIXED64: u64 = 1;
const WIRE_BYTES: u64 = 2;
const WIRE_FIXED32: u64 = 5;

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

    /// The next field as its number and, for length-delimited fields, its bytes.
    fn field(&mut self) -> Option<(u64, Option<&'a [u8]>)> {
        let tag = self.varint()?;
        let (number, wire) = (tag >> 3, tag & 7);
        let payload = match wire {
            WIRE_VARINT => {
                self.varint()?;
                None
            }
            WIRE_FIXED64 => {
                self.take(8)?;
                None
            }
            WIRE_BYTES => {
                let length = usize::try_from(self.varint()?).ok()?;
                Some(self.take(length)?)
            }
            WIRE_FIXED32 => {
                self.take(4)?;
                None
            }
            _ => return None,
        };
        Some((number, payload))
    }
}

fn fields(bytes: &[u8]) -> impl Iterator<Item = (u64, &[u8])> {
    let mut reader = Reader { bytes };
    std::iter::from_fn(move || {
        while !reader.bytes.is_empty() {
            let (number, payload) = reader.field()?;
            if let Some(payload) = payload {
                return Some((number, payload));
            }
        }
        None
    })
}

fn fixed64_list(bytes: &[u8]) -> Vec<u64> {
    let (chunks, _) = bytes.as_chunks::<8>();
    chunks
        .iter()
        .map(|chunk| u64::from_le_bytes(*chunk))
        .collect()
}

/// The varint value of field `number` at the top level of `bytes`.
fn varint_of(bytes: &[u8], number: u64) -> Option<u64> {
    let mut reader = Reader { bytes };
    while !reader.bytes.is_empty() {
        let tag = reader.varint()?;
        let (field, wire) = (tag >> 3, tag & 7);
        match wire {
            WIRE_VARINT => {
                let value = reader.varint()?;
                if field == number {
                    return Some(value);
                }
            }
            WIRE_FIXED64 => {
                reader.take(8)?;
            }
            WIRE_BYTES => {
                let length = usize::try_from(reader.varint()?).ok()?;
                reader.take(length)?;
            }
            WIRE_FIXED32 => {
                reader.take(4)?;
            }
            _ => return None,
        }
    }
    None
}

/// `appearance.theme`: the `discord_users.v1.Theme` enum, UNSET 0, DARK 1, LIGHT 2, DARKER 3, MIDNIGHT 4.
pub fn theme(settings_base64: &str) -> Option<u64> {
    let settings = STANDARD.decode(settings_base64).ok()?;
    let (_, appearance) = fields(&settings).find(|(number, _)| *number == FIELD_APPEARANCE)?;
    varint_of(appearance, APPEARANCE_THEME)
}

/// Guild ids in sidebar order, from `guild_folders`. Folders win over the legacy flat list.
pub fn guild_order(settings_base64: &str) -> Option<Vec<u64>> {
    let settings = STANDARD.decode(settings_base64).ok()?;
    let (_, folders) = fields(&settings).find(|(number, _)| *number == FIELD_GUILD_FOLDERS)?;
    let mut order = Vec::new();
    let mut positions = Vec::new();
    for (number, payload) in fields(folders) {
        match number {
            FOLDERS_FOLDER => {
                for (inner, ids) in fields(payload) {
                    if inner == FOLDER_GUILD_IDS {
                        order.extend(fixed64_list(ids));
                    }
                }
            }
            FOLDERS_POSITIONS => positions = fixed64_list(payload),
            _ => {}
        }
    }
    Some(if order.is_empty() { positions } else { order })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_is_read_from_appearance() {
        // appearance (13) holding theme (1) = DARKER (3), after an unrelated varint field.
        let appearance = [0x10, 0x01, 0x08, 0x03];
        let mut settings = vec![(13 << 3) | 2, appearance.len() as u8];
        settings.extend_from_slice(&appearance);
        assert_eq!(theme(&STANDARD.encode(&settings)), Some(3));
        assert_eq!(theme(&STANDARD.encode([])), None);
    }

    fn bytes_field(number: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![(number << 3) | 2, payload.len() as u8];
        out.extend_from_slice(payload);
        out
    }

    fn ids(values: &[u64]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn encoded(message: &[u8]) -> String {
        STANDARD.encode(message)
    }

    #[test]
    fn reads_folders_in_order() {
        let first = bytes_field(1, &bytes_field(1, &ids(&[7, 8])));
        let second = bytes_field(1, &bytes_field(1, &ids(&[9])));
        let folders = [first, second].concat();
        let message = bytes_field(14, &folders);
        assert_eq!(guild_order(&encoded(&message)), Some(vec![7, 8, 9]));
    }

    #[test]
    fn falls_back_to_positions() {
        let message = bytes_field(14, &bytes_field(2, &ids(&[3, 4])));
        assert_eq!(guild_order(&encoded(&message)), Some(vec![3, 4]));
    }

    #[test]
    fn skips_other_fields_of_every_wire_type() {
        let mut message = vec![0x08, 0xac, 0x02]; // field 1, varint
        message.extend_from_slice(&[0x11, 1, 2, 3, 4, 5, 6, 7, 8]); // field 2, fixed64
        message.extend_from_slice(&[0x1d, 1, 2, 3, 4]); // field 3, fixed32
        message.extend(bytes_field(4, &[9, 9]));
        message.extend(bytes_field(
            14,
            &bytes_field(1, &bytes_field(1, &ids(&[5]))),
        ));
        assert_eq!(guild_order(&encoded(&message)), Some(vec![5]));
    }

    #[test]
    fn missing_or_broken_settings_are_none() {
        assert_eq!(guild_order(&encoded(&[])), None);
        assert_eq!(guild_order("not base64!"), None);
        assert_eq!(guild_order(&encoded(&[0x72, 200])), None);
    }

    #[test]
    fn a_runaway_varint_stops() {
        assert_eq!(guild_order(&encoded(&[0xff; 12])), None);
    }
}
