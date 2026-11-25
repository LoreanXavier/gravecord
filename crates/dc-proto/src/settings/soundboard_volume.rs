//! `voice_and_video.soundboard_settings.volume` (5, 9, 1: a float, 0 to 100): the Soundboard Volume the account keeps
//! (`SoundboardStore` reads `getSetting()?.volume ?? 100`, module 209932; `updateSetting({volume})` writes it).

use super::{
    FIELD_VOICE_AND_VIDEO, WIRE_FIXED32, fields, push_varint, raw_fields, whole_message_patch,
    with_encoded,
};

const SOUNDBOARD_SETTINGS: u64 = 9;
const VOLUME: u64 = 1;

/// The account's soundboard volume, `None` when it has none (the volume is then 100).
pub fn read(settings: &[u8]) -> Option<f32> {
    let voice = fields(settings)
        .find(|(number, _)| *number == FIELD_VOICE_AND_VIDEO)?
        .1;
    let soundboard = fields(voice)
        .find(|(number, _)| *number == SOUNDBOARD_SETTINGS)?
        .1;
    // The float is the one fixed32 field of the message: read it from its raw bytes.
    raw_fields(soundboard)
        .into_iter()
        .find(|(number, raw)| *number == VOLUME && raw.len() == 5)
        .map(|(_, raw)| f32::from_le_bytes([raw[1], raw[2], raw[3], raw[4]]))
        .or(Some(0.))
}

/// `settings` with the soundboard volume set: the PATCH body value (the whole `voice_and_video` message) and the
/// settings from now on. A volume of 0 is the float's default and is not written, as protobuf-ts leaves it out.
pub fn write(settings: &[u8], volume: f32) -> (String, Vec<u8>) {
    let mut encoded = Vec::new();
    if volume != 0. {
        push_varint(&mut encoded, VOLUME << 3 | WIRE_FIXED32);
        encoded.extend_from_slice(&volume.to_le_bytes());
    }
    let updated = if encoded.is_empty() {
        // `soundboardSettings: {volume: 0}` is an empty message, but the message itself is set.
        let mut empty = Vec::new();
        push_varint(&mut empty, SOUNDBOARD_SETTINGS << 3 | super::WIRE_BYTES);
        empty.push(0);
        with_encoded(
            settings,
            &[FIELD_VOICE_AND_VIDEO],
            SOUNDBOARD_SETTINGS,
            &empty,
        )
    } else {
        with_encoded(
            settings,
            &[FIELD_VOICE_AND_VIDEO, SOUNDBOARD_SETTINGS],
            VOLUME,
            &encoded,
        )
    };
    whole_message_patch(updated, FIELD_VOICE_AND_VIDEO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_round_trips_and_the_other_voice_settings_stay() {
        // voice_and_video with afk_timeout (6) set, and appearance (13) beside it.
        let settings = vec![0x2a, 0x04, 0x32, 0x02, 0x08, 0x3c, 0x6a, 0x02, 0x08, 0x01];
        assert_eq!(read(&settings), None);
        let (patch, updated) = write(&settings, 42.5);
        assert_eq!(read(&updated), Some(42.5));
        let patch = super::super::decode(&patch);
        // The PATCH is the whole voice_and_video message: afk_timeout kept, the soundboard settings added.
        let voice = fields(&patch).next().unwrap().1;
        assert!(voice.starts_with(&[0x32, 0x02, 0x08, 0x3c]));
        assert!(updated.ends_with(&[0x6a, 0x02, 0x08, 0x01]) || updated.starts_with(&[0x6a, 0x02]));
        let (_, again) = write(&updated, 80.);
        assert_eq!(read(&again), Some(80.));
        let (_, silent) = write(&again, 0.);
        assert_eq!(read(&silent), Some(0.));
    }
}
