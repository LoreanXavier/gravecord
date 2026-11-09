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

pub const GUILD_AND_CHANNEL_FRECENCY: u64 = 12;

/// The `guild_and_channels` map of `GuildAndChannelFrecency` (field 12): guild or channel id to its uses. The key
/// arrives as a fixed64 or a varint depending on the writer, so both are read.
pub fn id_map(settings_base64: &str, field: u64) -> Vec<(u64, Item)> {
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
            let mut key = None;
            let mut value = Item::default();
            let mut reader = Reader { bytes: entry };
            while !reader.bytes.is_empty() {
                let Some(tag) = reader.varint() else { break };
                match (tag >> 3, tag & 7) {
                    (1, 0) => key = reader.varint(),
                    (1, 1) => {
                        key = reader
                            .take(8)
                            .and_then(|bytes| bytes.try_into().ok())
                            .map(u64::from_le_bytes)
                    }
                    (2, 2) => {
                        let Some(length) = reader
                            .varint()
                            .and_then(|length| usize::try_from(length).ok())
                        else {
                            break;
                        };
                        if let Some(message) = reader.take(length) {
                            value = item(message);
                        }
                    }
                    (_, 0) => {
                        reader.varint();
                    }
                    (_, 1) => {
                        reader.take(8);
                    }
                    (_, 2) => {
                        let length = reader
                            .varint()
                            .and_then(|length| usize::try_from(length).ok())
                            .unwrap_or(0);
                        reader.take(length);
                    }
                    (_, 5) => {
                        reader.take(4);
                    }
                    _ => break,
                }
            }
            if let Some(key) = key {
                out.push((key, value));
            }
        }
    }
    out
}

pub const FAVORITE_GIFS: u64 = 2;

/// `FavoriteGIF` (module 803805): `format` 1 (1 image, 2 video), `src` 2, `width` 3, `height` 4, `order` 5, kept
/// under the GIF's page URL in `FavoriteGIFs.gifs` (field 1, a map), `hide_tooltip` is field 2.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FavoriteGif {
    pub url: String,
    pub format: u32,
    pub src: String,
    pub width: u32,
    pub height: u32,
    pub order: u32,
}

/// The favourite GIFs of `settings_base64`, oldest favourite first (by `order`).
pub fn favorite_gifs(settings_base64: &str) -> Vec<FavoriteGif> {
    let Ok(settings) = STANDARD.decode(settings_base64) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (number, value) in fields(&settings) {
        let (true, Value::Bytes(message)) = (number == FAVORITE_GIFS, value) else {
            continue;
        };
        for (entry_number, entry) in fields(message) {
            let (1, Value::Bytes(entry)) = (entry_number, entry) else {
                continue;
            };
            let mut gif = FavoriteGif::default();
            for (part, payload) in fields(entry) {
                match (part, payload) {
                    (1, Value::Bytes(text)) => gif.url = String::from_utf8_lossy(text).into_owned(),
                    (2, Value::Bytes(message)) => {
                        for (field, value) in fields(message) {
                            match (field, value) {
                                (1, Value::Varint(format)) => gif.format = format as u32,
                                (2, Value::Bytes(text)) => {
                                    gif.src = String::from_utf8_lossy(text).into_owned()
                                }
                                (3, Value::Varint(width)) => gif.width = width as u32,
                                (4, Value::Varint(height)) => gif.height = height as u32,
                                (5, Value::Varint(order)) => gif.order = order as u32,
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !gif.url.is_empty() {
                out.push(gif);
            }
        }
    }
    out.sort_by_key(|gif| gif.order);
    out
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn push_field(out: &mut Vec<u8>, number: u64, payload: &[u8]) {
    push_varint(out, number << 3 | 2);
    push_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn push_number(out: &mut Vec<u8>, number: u64, value: u32) {
    if value != 0 {
        push_varint(out, number << 3);
        push_varint(out, u64::from(value));
    }
}

/// The PATCH body that sets the favourites: the whole `favorite_gifs` message, which the server replaces as one.
pub fn favorite_gifs_patch(gifs: &[FavoriteGif], hide_tooltip: bool) -> String {
    let mut message = Vec::new();
    for gif in gifs {
        let mut value = Vec::new();
        push_number(&mut value, 1, gif.format);
        if !gif.src.is_empty() {
            push_field(&mut value, 2, gif.src.as_bytes());
        }
        push_number(&mut value, 3, gif.width);
        push_number(&mut value, 4, gif.height);
        push_number(&mut value, 5, gif.order);
        let mut entry = Vec::new();
        push_field(&mut entry, 1, gif.url.as_bytes());
        push_field(&mut entry, 2, &value);
        push_field(&mut message, 1, &entry);
    }
    if hide_tooltip {
        push_varint(&mut message, 2 << 3);
        push_varint(&mut message, 1);
    }
    let mut settings = Vec::new();
    push_field(&mut settings, FAVORITE_GIFS, &message);
    STANDARD.encode(settings)
}

/// Favouriting adds the GIF at the end (`order` one past the largest); favouriting it again removes it
/// (`toggle` in module 497685). The result keeps the list in `order`.
pub fn toggle_favorite_gif(mut gifs: Vec<FavoriteGif>, gif: FavoriteGif) -> Vec<FavoriteGif> {
    if let Some(at) = gifs.iter().position(|known| known.url == gif.url) {
        gifs.remove(at);
    } else {
        let order = gifs.iter().map(|known| known.order).max().unwrap_or(0) + 1;
        gifs.push(FavoriteGif { order, ..gif });
    }
    gifs
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
    fn reads_channel_frecency_with_fixed64_keys() {
        let mut item = varint(1 << 3);
        item.extend(varint(3));
        let mut entry = vec![(1 << 3) | 1];
        entry.extend(1_568_692_565_136_375_900u64.to_le_bytes());
        entry.extend(bytes(2, &item));
        let settings = bytes(GUILD_AND_CHANNEL_FRECENCY, &bytes(1, &entry));
        let read = id_map(&STANDARD.encode(&settings), GUILD_AND_CHANNEL_FRECENCY);
        assert_eq!(
            read,
            [(
                1_568_692_565_136_375_900,
                Item {
                    total_uses: 3,
                    ..Item::default()
                }
            )]
        );
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

    #[test]
    fn favorite_gifs_round_trip_and_toggle() {
        let gif = FavoriteGif {
            url: "https://klipy.com/gifs/wave".to_owned(),
            format: 1,
            src: "https://static.klipy.com/wave.gif".to_owned(),
            width: 220,
            height: 124,
            order: 0,
        };
        let one = toggle_favorite_gif(Vec::new(), gif.clone());
        assert_eq!(one[0].order, 1);
        let other = FavoriteGif {
            url: "https://klipy.com/gifs/nod".to_owned(),
            ..gif.clone()
        };
        let two = toggle_favorite_gif(one, other.clone());
        assert_eq!(two[1].order, 2);
        let patch = favorite_gifs_patch(&two, true);
        let read = favorite_gifs(&patch);
        assert_eq!(read, two);
        let back = toggle_favorite_gif(read, gif);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].url, other.url);
        assert!(favorite_gifs(&favorite_gifs_patch(&[], false)).is_empty());
    }
}
