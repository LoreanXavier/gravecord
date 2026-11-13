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
const FIELD_SAFETY: u64 = 21;
const SAFETY_IGNORE_SPEEDBUMP_DISABLED: u64 = 2;

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

/// `safety_settings.ignore_profile_speedbump_disabled`: an ignored user's profile shows without asking first.
pub fn ignore_speedbump_disabled(settings_base64: &str) -> bool {
    let Ok(settings) = STANDARD.decode(settings_base64) else {
        return false;
    };
    fields(&settings)
        .find(|(number, _)| *number == FIELD_SAFETY)
        .and_then(|(_, safety)| varint_of(safety, SAFETY_IGNORE_SPEEDBUMP_DISABLED))
        .is_some_and(|value| value != 0)
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// A partial `PreloadedUserSettings` holding one varint field inside nested messages, as `ob(ProtoClass, changes)`
/// writes only what changed. The PATCH body of `/users/@me/settings-proto/1` carries it base64-encoded.
pub fn encode_varint_at(path: &[u64], field: u64, value: u64) -> String {
    let mut message = Vec::new();
    push_varint(&mut message, field << 3 | WIRE_VARINT);
    push_varint(&mut message, value);
    for number in path.iter().rev() {
        let mut outer = Vec::new();
        push_varint(&mut outer, number << 3 | WIRE_BYTES);
        push_varint(&mut outer, message.len() as u64);
        outer.extend_from_slice(&message);
        message = outer;
    }
    STANDARD.encode(message)
}

/// "Don't show this again" on an ignored profile (`ignoreProfileSpeedbumpDisabled` set to true).
pub fn ignore_speedbump_disabled_patch() -> String {
    encode_varint_at(&[FIELD_SAFETY], SAFETY_IGNORE_SPEEDBUMP_DISABLED, 1)
}

/// A client preference that lives in the account's proto settings: where it is, as the nested message numbers from
/// `PreloadedUserSettings` (`dumps/PROTOS`) and the varint field inside the last one. The wrappers (`BoolValue`,
/// `UInt32Value`) keep their value in field 1.
pub struct ProtoPref {
    /// The settings page row's `data-nav-anchor-key`.
    pub key: &'static str,
    pub path: &'static [u64],
    pub field: u64,
    /// A `StringValue` preference (`render_spoilers`): the strings the account holds, in the order the page's options
    /// list them; the value of the preference is the index. Empty for a varint preference.
    pub strings: &'static [&'static str],
    /// The page's switch is the opposite of the stored boolean: "Use my Discord activity to personalize Sponsored
    /// Content" is on while `drops_opted_out` is off (`CO`/`CL` in module 158935).
    pub invert: bool,
}

const fn pref(key: &'static str, path: &'static [u64], field: u64) -> ProtoPref {
    ProtoPref {
        key,
        path,
        field,
        strings: &[],
        invert: false,
    }
}

/// A preference whose switch shows the opposite of the stored boolean.
const fn inverted(key: &'static str, path: &'static [u64], field: u64) -> ProtoPref {
    ProtoPref {
        key,
        path,
        field,
        strings: &[],
        invert: true,
    }
}

const fn string_pref(
    key: &'static str,
    path: &'static [u64],
    field: u64,
    strings: &'static [&'static str],
) -> ProtoPref {
    ProtoPref {
        key,
        path,
        field,
        strings,
        invert: false,
    }
}

/// The locale codes of the language select, in its order (module 296437, the `enabled` entries): the strings
/// `localization.locale` holds.
pub const LOCALES: &[&str] = &[
    "en-US", "en-GB", "zh-CN", "zh-TW", "cs", "da", "nl", "fr", "de", "el", "hu", "it", "ja", "ko",
    "no", "hr", "pl", "pt-BR", "ru", "es-ES", "es-419", "sv-SE", "tr", "bg", "uk", "fi", "ro",
    "lt", "th", "vi", "hi",
];

/// `text_and_images` is message 6 and `appearance` 13 of `PreloadedUserSettings`.
pub const PROTO_PREFS: &[ProtoPref] = &[
    // text_and_images: use_rich_chat_input 2, use_thread_sidebar 3, view_image_descriptions 7,
    // inline_attachment_media 9, inline_embed_media 10, gif_auto_play 11, render_embeds 12, render_reactions 13,
    // animate_emoji 14, animate_stickers 15, enable_tts_command 16, message_display_compact 17,
    // convert_emoticons 21, use_legacy_chat_input 24, include_stickers_in_autocomplete 28.
    pref("chat_text_box_previews", &[6, 2], 1),
    pref("chat_threads_split_view", &[6, 3], 1),
    pref("chat_inline_media_image_descriptions", &[6, 7], 1),
    pref("chat_inline_media_uploads", &[6, 9], 1),
    pref("chat_inline_media_links", &[6, 10], 1),
    pref("animate_gifs", &[6, 11], 1),
    pref("chat_embeds_render_embeds", &[6, 12], 1),
    pref("chat_emoji_render_reactions", &[6, 13], 1),
    pref("animate_emojis", &[6, 14], 1),
    pref("animate_stickers", &[6, 15], 1),
    pref("text_to_speech_command", &[6, 16], 1),
    pref("appearance_message_display_mode", &[6, 17], 1),
    pref("chat_emoji_convert_emoticons", &[6, 21], 1),
    pref("enable_legacy_chat_input", &[6, 24], 1),
    pref("chat_stickers_autocomplete", &[6, 28], 1),
    // render_spoilers 4 (`StringValue`: module 652215 `ALWAYS`, `ON_CLICK`, `IF_MODERATOR`; the page lists On click,
    // Always, On servers I moderate), is_cross_dm_search_enabled 36, include_game_mentions_in_autocomplete 39,
    // display_compact_avatars 41.
    string_pref(
        "chat_spoilers_show_spoilers",
        &[6, 4],
        1,
        &["ON_CLICK", "ALWAYS", "IF_MODERATOR"],
    ),
    pref("message_search_default_dm_search_behavior", &[6, 36], 1),
    pref("chat_game_mentions_autocomplete", &[6, 39], 1),
    pref("appearance_display_compact_avatars", &[6, 41], 1),
    // notifications (7): notify_friends_on_go_live 2, enable_friend_online_notifications 12,
    // enable_friend_anniversary_notifications 14, enable_profile_updates_notifications 16,
    // enable_upcoming_server_event_notifications 22, notify_friends_on_profile_update 24,
    // notify_friends_on_come_online 25, notify_server_members_on_go_live 26.
    pref("go_live_notifications", &[7, 2], 1),
    pref("friend_online_notifications", &[7, 12], 1),
    pref("friend_anniversary_notifications", &[7, 14], 1),
    pref("profile_updates_notifications", &[7, 16], 1),
    pref("upcoming_server_event_notifications", &[7, 22], 1),
    pref("notify_friends_on_profile_update_setting", &[7, 24], 1),
    pref(
        "activity_privacy_notify_friends_online_setting",
        &[7, 25],
        1,
    ),
    pref(
        "activity_privacy_notify_server_members_on_go_live_setting",
        &[7, 26],
        1,
    ),
    // privacy (8): allow_activity_party_privacy_friends 1, allow_activity_party_privacy_voice_channel 2;
    // status (11): show_current_game 3.
    pref("activity_privacy_friends_join_setting", &[8, 1], 1),
    pref("activity_privacy_voice_join_setting", &[8, 2], 1),
    pref("activity_privacy_setting", &[11, 3], 1),
    // appearance: developer_mode 2, timestamp_hour_cycle 9.
    // localization (12): locale 1, a `StringValue`.
    string_pref("language_select_setting", &[12, 1], 1, LOCALES),
    pref("developer_mode", &[13], 2),
    pref("time_format_setting", &[13], 9),
    // ui_density 12: COMPACT 1, COZY 2 (Spacious), DEFAULT 4 (the page's radio values, `eJ.NS`).
    pref("appearance_ui_density", &[13], 12),
    // Enums that sit directly in their message (module 253932): privacy.profile_visibility 32, text_and_images.
    // dm_spam_filter_v2 27, notifications.reaction_notifications 7 (ENABLED 0, ONLY_DMS 1, DISABLED 2).
    pref("profile_privacy_setting", &[8], 32),
    pref("dm_spam_setting", &[6], 27),
    pref("reaction_notifications", &[7], 7),
    // voice_and_video (5).afk_timeout 6, a UInt32Value of seconds: how long the desktop must be idle before the phone is
    // notified (`mobile_notification_delay`, module 158935 `vV`).
    pref("mobile_notification_delay", &[5, 6], 1),
    // appearance.client_theme_settings (3).background_gradient_preset_id (2, a UInt32Value): the Nitro colour theme,
    // `BackgroundGradientPresetId` of module 873298 (MINT_APPLE 0 ... HEXAGON 23).
    pref("appearance_client_theme_preset", &[13, 3, 2], 1),
    // clips (19): allow_voice_recording 1, a BoolValue (`U.Q$`, module 253932).
    pref("clips_allow_voice_recording_setting", &[19, 1], 1),
    // privacy (8): drops_opted_out 18 and quests_3p_data_opted_out 30 are "opted out" booleans, so their switches
    // read the opposite (`CO`, `Cb` in module 158935); hide_friend_request_notes 33 likewise (`y9`).
    inverted("data_usage_quests_setting", &[8, 18], 1),
    inverted("data_usage_quests_3p_setting", &[8, 30], 1),
    // swp_message_promotion_opted_out 35 (`pinot_member_setting`, module 410310).
    inverted("pinot_member_setting", &[8, 35], 1),
    inverted("friend_requests_notes_setting", &[8, 33], 1),
    // text_and_images: view_nsfw_commands 23 and view_nsfw_guilds 20, both BoolValues (`U.Qe`, `U.Kg`).
    pref("age_restricted_dm_setting", &[6, 23], 1),
    pref("age_restricted_ios_setting", &[6, 20], 1),
    // privacy: default_guilds_activity_restricted_v2 29, an enum at the message's own level.
    pref("activity_sharing_per_guild_default_setting", &[8], 29),
    // privacy: family_center_enabled_v2 21, a BoolValue: "Enable Family Center tab" of the Family Center panel's
    // Settings tab (`familyCenterEnabledV2`, module 253932).
    pref("family_center_enabled", &[8, 21], 1),
];

/// `notifications.quiet_mode`, read from the merged account settings rather than a settings-page row.
pub fn notification_quiet_mode(settings: &[u8]) -> bool {
    fields(settings)
        .find(|(number, _)| *number == 7)
        .and_then(|(_, notifications)| fields(notifications).find(|(number, _)| *number == 5))
        .and_then(|(_, wrapper)| varint_of(wrapper, 1))
        .is_some_and(|value| value != 0)
}

/// The varint at `field` of the message found by following `path` from the top: `None` when a message on the way is
/// missing (the account never set it), zero when the message is there without the field (proto3 leaves defaults out).
pub fn varint_at(settings_base64: &str, path: &[u64], field: u64) -> Option<u64> {
    let settings = STANDARD.decode(settings_base64).ok()?;
    let mut current: &[u8] = &settings;
    for number in path {
        current = fields(current)
            .find(|(known, _)| known == number)
            .map(|(_, payload)| payload)?;
    }
    Some(varint_of(current, field).unwrap_or(0))
}

/// What the account holds for a preference: the varint, or for a string preference the index of its string.
pub fn pref_value(settings_base64: &str, pref: &ProtoPref) -> Option<u64> {
    if pref.strings.is_empty() {
        return varint_at(settings_base64, pref.path, pref.field);
    }
    let settings = STANDARD.decode(settings_base64).ok()?;
    let mut current: &[u8] = &settings;
    for number in pref.path {
        current = fields(current)
            .find(|(known, _)| known == number)
            .map(|(_, payload)| payload)?;
    }
    let text = fields(current).find(|(number, _)| *number == pref.field);
    Some(text.map_or(0, |(_, bytes)| {
        pref.strings
            .iter()
            .position(|known| known.as_bytes() == bytes)
            .unwrap_or(0) as u64
    }))
}

/// The PATCH body value that sets one preference on its own. The server swaps in whole any top-level message it is
/// sent, so a live change goes through `set_pref`, which carries the rest of that message along.
pub fn pref_patch(pref: &ProtoPref, value: u64) -> String {
    encode_varint_at(pref.path, pref.field, value)
}

/// The theme, which the Appearance page's swatches set; kept here so a test and the harness can write it back.
/// "Don't show this again" on an ignored profile (`ignoreProfileSpeedbumpDisabled`), written with the rest of the
/// safety settings the account holds: a message with this one field would replace the whole safety message.
pub const IGNORE_SPEEDBUMP_DISABLED: ProtoPref = pref(
    "ignore_profile_speedbump_disabled",
    &[FIELD_SAFETY],
    SAFETY_IGNORE_SPEEDBUMP_DISABLED,
);
pub const THEME: ProtoPref = pref("appearance_theme", &[13], 1);
/// `appearance.client_theme_settings.background_gradient_preset_id`: the Nitro colour theme (see `PROTO_PREFS`).
pub const CLIENT_THEME_PRESET: ProtoPref = pref("appearance_client_theme_preset", &[13, 3, 2], 1);

/// The fields of `bytes` as (number, the field's own bytes, tag included), in order. Stops at a malformed one.
fn raw_fields(bytes: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let mut reader = Reader { bytes: rest };
        let Some(tag) = reader.varint() else { break };
        let ok = match tag & 7 {
            WIRE_VARINT => reader.varint().is_some(),
            WIRE_FIXED64 => reader.take(8).is_some(),
            WIRE_BYTES => reader
                .varint()
                .and_then(|length| usize::try_from(length).ok())
                .and_then(|length| reader.take(length))
                .is_some(),
            WIRE_FIXED32 => reader.take(4).is_some(),
            _ => false,
        };
        if !ok {
            break;
        }
        let used = rest.len() - reader.bytes.len();
        out.push((tag >> 3, &rest[..used]));
        rest = reader.bytes;
    }
    out
}

/// `message` with the varint `field` of the nested message at `path` set, everything else kept byte for byte.
fn with_varint(message: &[u8], path: &[u64], field: u64, value: u64) -> Vec<u8> {
    let mut encoded = Vec::new();
    push_varint(&mut encoded, field << 3 | WIRE_VARINT);
    push_varint(&mut encoded, value);
    with_encoded(message, path, field, &encoded)
}

/// `message` with the field at `path` replaced by `encoded` (tag and payload), everything else kept byte for byte.
fn with_encoded(message: &[u8], path: &[u64], field: u64, encoded: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let Some((head, tail)) = path.split_first() else {
        for (number, raw) in raw_fields(message) {
            if number != field {
                out.extend_from_slice(raw);
            }
        }
        out.extend_from_slice(encoded);
        return out;
    };
    let inner = fields(message)
        .find(|(number, _)| number == head)
        .map_or(&[][..], |(_, payload)| payload);
    let rebuilt = with_encoded(inner, tail, field, encoded);
    for (number, raw) in raw_fields(message) {
        if number != *head {
            out.extend_from_slice(raw);
        }
    }
    push_varint(&mut out, head << 3 | WIRE_BYTES);
    push_varint(&mut out, rebuilt.len() as u64);
    out.extend_from_slice(&rebuilt);
    out
}

/// The account's settings as bytes, from the base64 of READY or an update.
pub fn decode(settings_base64: &str) -> Vec<u8> {
    STANDARD.decode(settings_base64).unwrap_or_default()
}

/// Setting one preference: the PATCH body value, and the settings as they are from now on. The server replaces a
/// top-level message it is sent as a whole, so the patch is the whole of `appearance` or `text_and_images`, built from
/// `current`; sending only the changed field would wipe the rest of its message (the theme, the other switches).
pub fn set_pref(current: &[u8], pref: &ProtoPref, value: u64) -> (String, Vec<u8>) {
    let updated = if pref.strings.is_empty() {
        with_varint(current, pref.path, pref.field, value)
    } else {
        let text = pref
            .strings
            .get(value as usize)
            .copied()
            .unwrap_or_default();
        let mut encoded = Vec::new();
        push_bytes_field(&mut encoded, pref.field, text.as_bytes());
        with_encoded(current, pref.path, pref.field, &encoded)
    };
    let top = pref.path.first().copied().unwrap_or(0);
    whole_message_patch(updated, top)
}

/// `message` with the field `field` of the nested message at `path` left out, everything else kept byte for byte. A
/// message on the way that is not there leaves `message` as it is.
fn without_field(message: &[u8], path: &[u64], field: u64) -> Vec<u8> {
    let Some((head, tail)) = path.split_first() else {
        return raw_fields(message)
            .into_iter()
            .filter(|(number, _)| *number != field)
            .flat_map(|(_, raw)| raw.iter().copied())
            .collect();
    };
    let Some(inner) = fields(message)
        .find(|(number, _)| number == head)
        .map(|(_, payload)| payload)
    else {
        return message.to_vec();
    };
    let rebuilt = without_field(inner, tail, field);
    let mut out = Vec::new();
    for (number, raw) in raw_fields(message) {
        if number != *head {
            out.extend_from_slice(raw);
        }
    }
    push_varint(&mut out, head << 3 | WIRE_BYTES);
    push_varint(&mut out, rebuilt.len() as u64);
    out.extend_from_slice(&rebuilt);
    out
}

/// Takes a preference out of the account's settings, as choosing a default theme does with the colour theme: the
/// wrapper message that held the value (`background_gradient_preset_id`) is left out of its parent and the parent
/// stays, empty if it held nothing else. Like `set_pref` it returns the PATCH body value (the whole top-level message)
/// and the settings from now on.
pub fn clear_pref(current: &[u8], pref: &ProtoPref) -> (String, Vec<u8>) {
    let updated = match pref.path.split_last() {
        Some((wrapper, parent)) => without_field(current, parent, *wrapper),
        None => without_field(current, &[], pref.field),
    };
    let top = pref.path.first().copied().unwrap_or(pref.field);
    whole_message_patch(updated, top)
}

/// `appearance.client_theme_settings.custom_user_theme_settings` (module 873298 `CustomUserThemeSettings`): the
/// account's own colour theme. A path only, for taking it out (`clear_pref`); it is not a varint.
pub const CUSTOM_THEME_PREF: ProtoPref = pref("appearance_custom_theme", &[13, 3, 4], 1);

/// `CustomUserThemeSettings`: `colors` 1 (hex strings), `gradient_color_stops` 2 (floats, always empty from the
/// editor), `gradient_angle` 3 and `base_mix` 4 (int32).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomTheme {
    pub colors: Vec<String>,
    pub angle: u32,
    pub base_mix: u32,
}

/// The account's custom theme, if it holds one with at least one colour (`customUserThemeSettings?.colors.length`).
pub fn custom_theme_in(settings: &[u8]) -> Option<CustomTheme> {
    let appearance = fields(settings)
        .find(|(number, _)| *number == FIELD_APPEARANCE)?
        .1;
    let client = fields(appearance).find(|(number, _)| *number == 3)?.1;
    let custom = fields(client).find(|(number, _)| *number == 4)?.1;
    let colors: Vec<String> = fields(custom)
        .filter(|(number, _)| *number == 1)
        .filter_map(|(_, text)| String::from_utf8(text.to_vec()).ok())
        .collect();
    if colors.is_empty() {
        return None;
    }
    Some(CustomTheme {
        colors,
        angle: varint_of(custom, 3).unwrap_or(0) as u32,
        base_mix: varint_of(custom, 4).unwrap_or(0) as u32,
    })
}

/// Applying a custom theme the way `u_` of module 817281 does: `appearance.theme` and the whole of
/// `client_theme_settings` are written together (the colour theme preset goes, the custom theme comes), the rest of
/// `appearance` stays. Returns the PATCH body value (the whole `appearance` message) and the settings from now on.
pub fn set_custom_theme(current: &[u8], theme: u64, custom: &CustomTheme) -> (String, Vec<u8>) {
    let mut message = Vec::new();
    for colour in &custom.colors {
        push_bytes_field(&mut message, 1, colour.as_bytes());
    }
    if custom.angle != 0 {
        push_varint(&mut message, 3 << 3 | WIRE_VARINT);
        push_varint(&mut message, u64::from(custom.angle));
    }
    if custom.base_mix != 0 {
        push_varint(&mut message, 4 << 3 | WIRE_VARINT);
        push_varint(&mut message, u64::from(custom.base_mix));
    }
    let mut client = Vec::new();
    push_bytes_field(&mut client, 4, &message);
    let mut encoded = Vec::new();
    push_bytes_field(&mut encoded, 3, &client);
    let with_theme = with_varint(current, &[FIELD_APPEARANCE], APPEARANCE_THEME, theme);
    let updated = with_encoded(&with_theme, &[FIELD_APPEARANCE], 3, &encoded);
    whole_message_patch(updated, FIELD_APPEARANCE)
}

/// `privacy.friend_source_flags` (a `UInt32Value`): who may send the account a friend request. The bits are
/// `MUTUAL_FRIENDS` 2, `MUTUAL_GUILDS` 4 and `NO_RELATION` 8 (module 652215); all three is "Everyone".
pub const FRIEND_SOURCE_FLAGS: ProtoPref = pref("friend_source_flags", &[8, 11], 1);
/// `default_guilds_restricted_v2` (27) and `default_message_request_restricted` (17), both `BoolValue`s: direct
/// messages and message requests off for servers that have no entry of their own.
pub const DM_DEFAULT_RESTRICTED: ProtoPref = pref("permissions_dms_default", &[8, 27], 1);
pub const REQUESTS_DEFAULT_RESTRICTED: ProtoPref =
    pref("permissions_requests_default", &[8, 17], 1);
/// The sensitive content filters (`text_and_images`): `explicit_content_settings` 29 and `gore_content_settings` 32,
/// each with `*_guilds` 1, `*_friend_dm` 2 and `*_non_friend_dm` 3 (`ExplicitContentRedaction`: SHOW 1, BLUR 2,
/// BLOCK 3, unset 0). Indexed by kind (Mature Sexual Media, Graphic Media), then row (friends' DMs, other DMs, servers).
pub const CONTENT_REDACTION: [[ProtoPref; 3]; 2] = [
    [
        pref("explicit_content_friend_dm", &[6, 29], 2),
        pref("explicit_content_non_friend_dm", &[6, 29], 3),
        pref("explicit_content_guilds", &[6, 29], 1),
    ],
    [
        pref("gore_content_friend_dm", &[6, 32], 2),
        pref("gore_content_non_friend_dm", &[6, 32], 3),
        pref("gore_content_guilds", &[6, 32], 1),
    ],
];
/// `text_and_images.explicit_content_filter` (19, a `UInt32Value`): DISABLED 0, NON_FRIENDS 1, FRIENDS_AND_NON_FRIENDS 2.
/// It decides the defaults the filters show while the account has not chosen.
pub const EXPLICIT_CONTENT_FILTER: ProtoPref = pref("explicit_content_filter", &[6, 19], 1);
pub const FRIEND_MUTUAL_FRIENDS: u64 = 2;
pub const FRIEND_MUTUAL_GUILDS: u64 = 4;
pub const FRIEND_NO_RELATION: u64 = 8;
pub const FRIEND_EVERYONE: u64 = 14;

/// A repeated fixed64 field (the guild lists of `privacy`): `restricted_guild_ids` 3 and
/// `message_request_restricted_guild_ids` 16 (direct messages and message requests off per server),
/// `activity_restricted_guild_ids` 13 (activity not shared per server) and `activity_joining_restricted_guild_ids` 15.
pub struct IdList {
    pub path: &'static [u64],
    pub field: u64,
}

pub const DM_RESTRICTED_GUILDS: IdList = IdList {
    path: &[8],
    field: 3,
};
pub const MESSAGE_REQUEST_RESTRICTED_GUILDS: IdList = IdList {
    path: &[8],
    field: 16,
};
pub const ACTIVITY_RESTRICTED_GUILDS: IdList = IdList {
    path: &[8],
    field: 13,
};

/// The varint at `field` of the message found by following `path` through raw settings bytes.
pub fn varint_in(settings: &[u8], path: &[u64], field: u64) -> Option<u64> {
    let mut current = settings;
    for number in path {
        current = fields(current)
            .find(|(known, _)| known == number)
            .map(|(_, payload)| payload)?;
    }
    Some(varint_of(current, field).unwrap_or(0))
}

/// The ids of a repeated fixed64 field, packed or not (a proto3 reader accepts both).
pub fn ids_in(settings: &[u8], list: &IdList) -> Vec<u64> {
    let mut current = settings;
    for number in list.path {
        let Some((_, payload)) = fields(current).find(|(known, _)| known == number) else {
            return Vec::new();
        };
        current = payload;
    }
    fields(current)
        .filter(|(number, _)| *number == list.field)
        .flat_map(|(_, bytes)| fixed64_list(bytes))
        .collect()
}

/// Replaces a guild list: the PATCH body value (the whole top-level message, as `set_pref`) and the new settings.
/// The ids are written packed, as `ob(ProtoClass, ...)` encodes a repeated fixed64.
pub fn set_ids(current: &[u8], list: &IdList, ids: &[u64]) -> (String, Vec<u8>) {
    let mut encoded = Vec::new();
    if !ids.is_empty() {
        let packed: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
        push_bytes_field(&mut encoded, list.field, &packed);
    }
    let updated = with_encoded(current, list.path, list.field, &encoded);
    let top = list.path.first().copied().unwrap_or(0);
    whole_message_patch(updated, top)
}

fn whole_message_patch(updated: Vec<u8>, top: u64) -> (String, Vec<u8>) {
    let part: Vec<u8> = raw_fields(&updated)
        .into_iter()
        .filter(|(number, _)| *number == top)
        .flat_map(|(_, raw)| raw.iter().copied())
        .collect();
    (STANDARD.encode(part), updated)
}

/// UserContentSettings.dismissed_contents: byte bitset, indexed as module 460288.
pub fn content_dismissed(current: &[u8], content: u16) -> bool {
    fields(current)
        .find(|(number, _)| *number == 4)
        .and_then(|(_, message)| fields(message).find(|(number, _)| *number == 1))
        .and_then(|(_, bits)| bits.get(usize::from(content / 8)))
        .is_some_and(|byte| byte & (1 << (content % 8)) != 0)
}

/// Carries the entire user_content message, sharing set_pref's whole-message patch construction.
pub fn dismiss_content(current: &[u8], content: u16) -> (String, Vec<u8>) {
    let message = fields(current)
        .find(|(number, _)| *number == 4)
        .map_or(&[][..], |(_, payload)| payload);
    let mut bits = fields(message)
        .find(|(number, _)| *number == 1)
        .map_or_else(Vec::new, |(_, payload)| payload.to_vec());
    let index = usize::from(content / 8);
    bits.resize(bits.len().max(index + 1), 0);
    bits[index] |= 1 << (content % 8);
    let mut updated_message: Vec<u8> = raw_fields(message)
        .into_iter()
        .filter(|(number, _)| *number != 1)
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect();
    push_bytes_field(&mut updated_message, 1, &bits);
    let mut updated: Vec<u8> = raw_fields(current)
        .into_iter()
        .filter(|(number, _)| *number != 4)
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect();
    push_bytes_field(&mut updated, 4, &updated_message);
    whole_message_patch(updated, 4)
}

/// An update from the gateway folded into `current`: a full one replaces everything, a partial one only the top-level
/// messages it carries.
pub fn merge_update(current: &mut Vec<u8>, update: &[u8], partial: bool) {
    if !partial {
        *current = update.to_vec();
        return;
    }
    let carried = raw_fields(update);
    let mut merged: Vec<u8> = raw_fields(current)
        .into_iter()
        .filter(|(number, _)| carried.iter().all(|(other, _)| other != number))
        .flat_map(|(_, raw)| raw.iter().copied())
        .collect();
    for (_, raw) in carried {
        merged.extend_from_slice(raw);
    }
    *current = merged;
}

/// `status` is message 11 of `PreloadedUserSettings` (`StatusSettings`): status 1 (a `StringValue`), custom_status 2,
/// show_current_game 3, status_expires_at_ms 4 (fixed64), status_created_at_ms 5 (a `UInt64Value`).
const FIELD_STATUS: u64 = 11;

/// `CustomStatus`: text 1, emoji_id 2 (fixed64), emoji_name 3, expires_at_ms 4 (fixed64), created_at_ms 5 (fixed64).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomStatus {
    pub text: String,
    pub emoji_id: u64,
    pub emoji_name: String,
    pub expires_at_ms: u64,
}

/// The account's status as the proto settings hold it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusState {
    /// `online`, `idle`, `dnd` or `invisible`; `None` while the account never chose one.
    pub status: Option<String>,
    /// When the chosen status lapses, milliseconds since the epoch; zero for no end.
    pub expires_at_ms: u64,
    pub custom: Option<CustomStatus>,
}

fn fixed64_of(bytes: &[u8], number: u64) -> Option<u64> {
    let mut reader = Reader { bytes };
    while !reader.bytes.is_empty() {
        let tag = reader.varint()?;
        let (field, wire) = (tag >> 3, tag & 7);
        match wire {
            WIRE_VARINT => {
                reader.varint()?;
            }
            WIRE_FIXED64 => {
                let chunk = reader.take(8)?;
                if field == number {
                    return Some(u64::from_le_bytes(chunk.try_into().ok()?));
                }
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

fn string_of(bytes: &[u8], number: u64) -> Option<String> {
    fields(bytes)
        .find(|(field, _)| *field == number)
        .map(|(_, text)| String::from_utf8_lossy(text).into_owned())
}

/// The status part of the settings in `settings` (raw bytes), `None` when the account has none.
pub fn read_status(settings: &[u8]) -> Option<StatusState> {
    let (_, message) = fields(settings).find(|(number, _)| *number == FIELD_STATUS)?;
    let custom = fields(message)
        .find(|(number, _)| *number == 2)
        .map(|(_, custom)| CustomStatus {
            text: string_of(custom, 1).unwrap_or_default(),
            emoji_id: fixed64_of(custom, 2).unwrap_or(0),
            emoji_name: string_of(custom, 3).unwrap_or_default(),
            expires_at_ms: fixed64_of(custom, 4).unwrap_or(0),
        })
        .filter(|custom| {
            !custom.text.is_empty() || !custom.emoji_name.is_empty() || custom.emoji_id != 0
        });
    Some(StatusState {
        status: fields(message)
            .find(|(number, _)| *number == 1)
            .and_then(|(_, wrapper)| string_of(wrapper, 1)),
        expires_at_ms: fixed64_of(message, 4).unwrap_or(0),
        custom,
    })
}

fn push_bytes_field(out: &mut Vec<u8>, number: u64, payload: &[u8]) {
    push_varint(out, number << 3 | WIRE_BYTES);
    push_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn push_fixed64_field(out: &mut Vec<u8>, number: u64, value: u64) {
    push_varint(out, number << 3 | WIRE_FIXED64);
    out.extend_from_slice(&value.to_le_bytes());
}

/// `settings` with the status message rebuilt: the fields in `drop` are removed from it and `add` is appended; the
/// rest of it (Show current activity) is kept byte for byte. The patch is the whole status message, as the server
/// replaces a top-level message it is sent.
fn with_status_fields(settings: &[u8], drop: &[u64], add: &[u8]) -> (String, Vec<u8>) {
    let existing = fields(settings)
        .find(|(number, _)| *number == FIELD_STATUS)
        .map_or(&[][..], |(_, message)| message);
    let mut message: Vec<u8> = raw_fields(existing)
        .into_iter()
        .filter(|(number, _)| !drop.contains(number))
        .flat_map(|(_, raw)| raw.iter().copied())
        .collect();
    message.extend_from_slice(add);
    let mut part = Vec::new();
    push_bytes_field(&mut part, FIELD_STATUS, &message);
    let mut updated: Vec<u8> = raw_fields(settings)
        .into_iter()
        .filter(|(number, _)| *number != FIELD_STATUS)
        .flat_map(|(_, raw)| raw.iter().copied())
        .collect();
    updated.extend_from_slice(&part);
    (STANDARD.encode(part), updated)
}

/// `updateStatus` (module 827827): the status, when it lapses (zero for never) and when it was chosen. Returns the
/// PATCH body value and the settings as they are from now on.
pub fn set_status(
    current: &[u8],
    status: &str,
    expires_at_ms: u64,
    now_ms: u64,
) -> (String, Vec<u8>) {
    let mut add = Vec::new();
    let mut wrapper = Vec::new();
    push_bytes_field(&mut wrapper, 1, status.as_bytes());
    push_bytes_field(&mut add, 1, &wrapper);
    push_fixed64_field(&mut add, 4, expires_at_ms);
    let mut created = Vec::new();
    push_varint(&mut created, 1 << 3 | WIRE_VARINT);
    push_varint(&mut created, now_ms);
    push_bytes_field(&mut add, 5, &created);
    with_status_fields(current, &[1, 4, 5], &add)
}

/// Saving the custom status, or clearing it with `None`.
pub fn set_custom_status(
    current: &[u8],
    custom: Option<&CustomStatus>,
    now_ms: u64,
) -> (String, Vec<u8>) {
    let mut add = Vec::new();
    if let Some(custom) = custom {
        let mut body = Vec::new();
        push_bytes_field(&mut body, 1, custom.text.as_bytes());
        if custom.emoji_id != 0 {
            push_fixed64_field(&mut body, 2, custom.emoji_id);
        }
        if !custom.emoji_name.is_empty() {
            push_bytes_field(&mut body, 3, custom.emoji_name.as_bytes());
        }
        push_fixed64_field(&mut body, 4, custom.expires_at_ms);
        push_fixed64_field(&mut body, 5, now_ms);
        push_bytes_field(&mut add, 2, &body);
    }
    with_status_fields(current, &[2], &add)
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

/// One `GuildFolder` of `guild_folders.folders` (`discord_users.v1`): `guild_ids` 1 (packed fixed64), `id` 2
/// (`Int64Value`), `name` 3 (`StringValue`), `color` 4 (`UInt64Value`). A guild outside any folder is an entry
/// without an id (module 711014 `S`, `folderId: void 0`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildFolderEntry {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<u64>,
    pub guild_ids: Vec<u64>,
}

/// The `guild_folders` message of `settings`: its folder entries in order and the legacy `guild_positions` list.
pub fn guild_folders(settings: &[u8]) -> (Vec<GuildFolderEntry>, Vec<u64>) {
    let Some((_, message)) = fields(settings).find(|(number, _)| *number == FIELD_GUILD_FOLDERS)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut entries = Vec::new();
    let mut positions = Vec::new();
    for (number, payload) in fields(message) {
        match number {
            FOLDERS_FOLDER => {
                let mut entry = GuildFolderEntry::default();
                for (inner, bytes) in fields(payload) {
                    match inner {
                        FOLDER_GUILD_IDS => entry.guild_ids.extend(fixed64_list(bytes)),
                        2 => entry.id = Some(varint_of(bytes, 1).unwrap_or(0)),
                        3 => entry.name = Some(string_of(bytes, 1).unwrap_or_default()),
                        4 => entry.color = Some(varint_of(bytes, 1).unwrap_or(0)),
                        _ => {}
                    }
                }
                entries.push(entry);
            }
            FOLDERS_POSITIONS => positions.extend(fixed64_list(payload)),
            _ => {}
        }
    }
    (entries, positions)
}

fn wrapper_field(out: &mut Vec<u8>, number: u64, inner: &[u8]) {
    push_bytes_field(out, number, inner);
}

/// A stored folder entry as read before a rewrite: its id, its guild ids and the fields of it this client has no
/// model for, as the raw bytes they came in.
struct KeptFolder {
    id: Option<u64>,
    guild_ids: Vec<u64>,
    unknown: Vec<u8>,
}

fn kept_folder_parts(message: &[u8]) -> Vec<KeptFolder> {
    raw_fields(message)
        .into_iter()
        .filter(|(number, _)| *number == FOLDERS_FOLDER)
        .map(|(_, raw)| {
            let payload = fields(raw).next().map_or(&[][..], |(_, payload)| payload);
            let mut kept = KeptFolder {
                id: None,
                guild_ids: Vec::new(),
                unknown: Vec::new(),
            };
            for (number, bytes) in raw_fields(payload) {
                match number {
                    FOLDER_GUILD_IDS => {
                        let inner = fields(bytes).next().map_or(&[][..], |(_, inner)| inner);
                        kept.guild_ids.extend(fixed64_list(inner));
                    }
                    2 => {
                        let inner = fields(bytes).next().map_or(&[][..], |(_, inner)| inner);
                        kept.id = Some(varint_of(inner, 1).unwrap_or(0));
                    }
                    3 | 4 => {}
                    _ => kept.unknown.extend_from_slice(bytes),
                }
            }
            kept
        })
        .collect()
}

/// Replaces the folders of `guild_folders` (`updateAsync("guildFolders", ...)`, module 817281 `um`): every entry is
/// written again, an id as `Int64Value`, a colour as `UInt64Value`, a name as `StringValue` (only when not empty).
/// `guild_positions` is carried over untouched. Returns the PATCH body value (the whole `guild_folders` message, as
/// the server replaces a top-level message it is sent) and the new settings.
pub fn set_guild_folders(current: &[u8], folders: &[GuildFolderEntry]) -> (String, Vec<u8>) {
    let existing = fields(current)
        .find(|(number, _)| *number == FIELD_GUILD_FOLDERS)
        .map_or(&[][..], |(_, message)| message);
    let kept = kept_folder_parts(existing);
    let listed: std::collections::HashSet<u64> = folders
        .iter()
        .flat_map(|entry| entry.guild_ids.iter().copied())
        .collect();
    let mut message = Vec::new();
    for entry in folders {
        let mut body = Vec::new();
        let mut guild_ids = entry.guild_ids.clone();
        let old = kept.iter().find(|old| match (entry.id, old.id) {
            (Some(id), Some(old_id)) => id == old_id,
            (None, None) => {
                entry.guild_ids.len() == 1 && old.guild_ids.first() == entry.guild_ids.first()
            }
            _ => false,
        });
        // An entry without an id holds one guild in the model; the rest it held stay, unless they moved elsewhere.
        if let (None, Some(old)) = (entry.id, old) {
            guild_ids.extend(
                old.guild_ids
                    .iter()
                    .skip(1)
                    .filter(|guild| !listed.contains(guild)),
            );
        }
        if !guild_ids.is_empty() {
            let packed: Vec<u8> = guild_ids.iter().flat_map(|id| id.to_le_bytes()).collect();
            push_bytes_field(&mut body, FOLDER_GUILD_IDS, &packed);
        }
        let varint_wrapper = |value: u64| {
            let mut inner = Vec::new();
            if value != 0 {
                push_varint(&mut inner, 1 << 3 | WIRE_VARINT);
                push_varint(&mut inner, value);
            }
            inner
        };
        if let Some(id) = entry.id {
            wrapper_field(&mut body, 2, &varint_wrapper(id));
        }
        if let Some(name) = entry.name.as_deref().filter(|name| !name.is_empty()) {
            let mut inner = Vec::new();
            push_bytes_field(&mut inner, 1, name.as_bytes());
            wrapper_field(&mut body, 3, &inner);
        }
        if let Some(color) = entry.color {
            wrapper_field(&mut body, 4, &varint_wrapper(color));
        }
        if let Some(old) = old {
            body.extend_from_slice(&old.unknown);
        }
        push_bytes_field(&mut message, FOLDERS_FOLDER, &body);
    }
    for (number, raw) in raw_fields(existing) {
        if number == FOLDERS_POSITIONS {
            message.extend_from_slice(raw);
        }
    }
    let mut encoded = Vec::new();
    push_bytes_field(&mut encoded, FIELD_GUILD_FOLDERS, &message);
    let updated = with_encoded(current, &[], FIELD_GUILD_FOLDERS, &encoded);
    whole_message_patch(updated, FIELD_GUILD_FOLDERS)
}

/// `favorites` is message 15 of `PreloadedUserSettings` (module 873298, `Favorites`).
const FIELD_FAVORITES: u64 = 15;

/// `FavoriteChannelType`: a channel the list points at, or a category the account made.
pub const FAVORITE_REFERENCE: u64 = 1;
pub const FAVORITE_CATEGORY: u64 = 2;

/// `FavoriteChannel` (module 873298): nickname 1, type 2, position 3, parent_id 4 (fixed64, 0 for none), channel_type
/// 5 (a `UInt32Value`), collapsed 6.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FavoriteChannel {
    pub nickname: String,
    pub kind: u64,
    pub position: u64,
    pub parent_id: u64,
    pub channel_type: Option<u64>,
    pub collapsed: bool,
}

/// `Favorites`: `favorite_channels` 1 (a map from channel id, fixed64), muted 2, guild_visible 3 (a `BoolValue`),
/// auto_add_joined_threads 4, prune_inactive_threads 5 (a `BoolValue`), prune_inactive_threads_days 6,
/// prune_all_favorited_threads 7.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Favorites {
    pub channels: Vec<(u64, FavoriteChannel)>,
    pub muted: bool,
    pub guild_visible: Option<bool>,
    pub auto_add_joined_threads: bool,
    pub prune_inactive_threads: Option<bool>,
    pub prune_inactive_threads_days: u64,
    pub prune_all_favorited_threads: bool,
}

impl Favorites {
    /// `favoriteGuildEnabled`: whether the Favorites server shows in the rail.
    pub fn guild_enabled(&self) -> bool {
        self.guild_visible == Some(true)
    }

    pub fn is_favorite(&self, channel: u64) -> bool {
        self.channels.iter().any(|(id, _)| *id == channel)
    }

    /// `k`: the position after the largest one.
    fn next_position(&self) -> u64 {
        self.channels
            .iter()
            .map(|(_, known)| known.position)
            .max()
            .map_or(1, |largest| largest + 1)
    }

    /// Adds a channel (`z` of module 146092): a reference with no nickname, outside any category, at the end; the
    /// first favourite also turns the Favorites server on. False when it already is one, or the list is full (200).
    pub fn add_channel(&mut self, channel: u64, channel_type: Option<u64>) -> bool {
        if self.is_favorite(channel) || self.channels.len() >= 200 {
            return false;
        }
        if !self.guild_enabled() {
            self.guild_visible = Some(true);
        }
        let position = self.next_position();
        self.channels.push((
            channel,
            FavoriteChannel {
                kind: FAVORITE_REFERENCE,
                position,
                channel_type,
                ..FavoriteChannel::default()
            },
        ));
        true
    }

    /// Removes a favourite (`Q`); the channels of a removed category fall back outside any category.
    pub fn remove_channel(&mut self, channel: u64) -> bool {
        let Some(at) = self.channels.iter().position(|(id, _)| *id == channel) else {
            return false;
        };
        let (_, removed) = self.channels.remove(at);
        if removed.kind == FAVORITE_CATEGORY {
            for (_, known) in &mut self.channels {
                if known.parent_id == channel {
                    known.parent_id = 0;
                }
            }
        }
        true
    }
}

fn bool_value(bytes: &[u8]) -> bool {
    varint_of(bytes, 1) == Some(1)
}

/// The account's favourites in `settings` (raw bytes).
pub fn read_favorites(settings: &[u8]) -> Favorites {
    let mut favorites = Favorites::default();
    let Some((_, message)) = fields(settings).find(|(number, _)| *number == FIELD_FAVORITES) else {
        return favorites;
    };
    for (number, payload) in fields(message) {
        match number {
            1 => {
                let Some(id) = fixed64_of(payload, 1) else {
                    continue;
                };
                let value = fields(payload)
                    .find(|(inner, _)| *inner == 2)
                    .map_or(&[][..], |(_, value)| value);
                favorites.channels.push((
                    id,
                    FavoriteChannel {
                        nickname: string_of(value, 1).unwrap_or_default(),
                        kind: varint_of(value, 2).unwrap_or(0),
                        position: varint_of(value, 3).unwrap_or(0),
                        parent_id: fixed64_of(value, 4).unwrap_or(0),
                        channel_type: fields(value)
                            .find(|(inner, _)| *inner == 5)
                            .map(|(_, wrapper)| varint_of(wrapper, 1).unwrap_or(0)),
                        collapsed: varint_of(value, 6) == Some(1),
                    },
                ));
            }
            3 => favorites.guild_visible = Some(bool_value(payload)),
            5 => favorites.prune_inactive_threads = Some(bool_value(payload)),
            _ => {}
        }
    }
    favorites.muted = varint_of(message, 2) == Some(1);
    favorites.auto_add_joined_threads = varint_of(message, 4) == Some(1);
    favorites.prune_inactive_threads_days = varint_of(message, 6).unwrap_or(0);
    favorites.prune_all_favorited_threads = varint_of(message, 7) == Some(1);
    favorites
}

/// Replaces the favourites (`updateAsync("favorites", ...)`, module 146092 `K`): the whole message is written, as the
/// server replaces a top-level message it is sent. Returns the PATCH body value and the new settings.
pub fn set_favorites(current: &[u8], favorites: &Favorites) -> (String, Vec<u8>) {
    let existing = fields(current)
        .find(|(number, _)| *number == FIELD_FAVORITES)
        .map_or(&[][..], |(_, message)| message);
    // What the rewrite has no model for stays: unknown fields of the message, of a map entry and of a favourite.
    let mut unknown_top: Vec<u8> = Vec::new();
    let mut unknown_of: std::collections::HashMap<u64, (Vec<u8>, Vec<u8>)> =
        std::collections::HashMap::new();
    for (number, raw) in raw_fields(existing) {
        if number == 1 {
            let payload = fields(raw).next().map_or(&[][..], |(_, payload)| payload);
            let Some(id) = fixed64_of(payload, 1) else {
                continue;
            };
            let (mut entry_extra, mut value_extra) = (Vec::new(), Vec::new());
            for (inner, bytes) in raw_fields(payload) {
                match inner {
                    1 => {}
                    2 => {
                        let value = fields(bytes).next().map_or(&[][..], |(_, value)| value);
                        for (field, part) in raw_fields(value) {
                            if !(1..=6).contains(&field) {
                                value_extra.extend_from_slice(part);
                            }
                        }
                    }
                    _ => entry_extra.extend_from_slice(bytes),
                }
            }
            unknown_of.insert(id, (entry_extra, value_extra));
        } else if !(2..=7).contains(&number) {
            unknown_top.extend_from_slice(raw);
        }
    }
    let mut message = Vec::new();
    for (id, known) in &favorites.channels {
        let mut value = Vec::new();
        if !known.nickname.is_empty() {
            push_bytes_field(&mut value, 1, known.nickname.as_bytes());
        }
        if known.kind != 0 {
            push_varint(&mut value, 2 << 3 | WIRE_VARINT);
            push_varint(&mut value, known.kind);
        }
        if known.position != 0 {
            push_varint(&mut value, 3 << 3 | WIRE_VARINT);
            push_varint(&mut value, known.position);
        }
        if known.parent_id != 0 {
            push_fixed64_field(&mut value, 4, known.parent_id);
        }
        if let Some(channel_type) = known.channel_type {
            let mut wrapper = Vec::new();
            if channel_type != 0 {
                push_varint(&mut wrapper, 1 << 3 | WIRE_VARINT);
                push_varint(&mut wrapper, channel_type);
            }
            push_bytes_field(&mut value, 5, &wrapper);
        }
        if known.collapsed {
            push_varint(&mut value, 6 << 3 | WIRE_VARINT);
            push_varint(&mut value, 1);
        }
        let mut entry = Vec::new();
        push_fixed64_field(&mut entry, 1, *id);
        if let Some((_, value_extra)) = unknown_of.get(id) {
            value.extend_from_slice(value_extra);
        }
        push_bytes_field(&mut entry, 2, &value);
        if let Some((entry_extra, _)) = unknown_of.get(id) {
            entry.extend_from_slice(entry_extra);
        }
        push_bytes_field(&mut message, 1, &entry);
    }
    let flag = |out: &mut Vec<u8>, number: u64, on: bool| {
        if on {
            push_varint(out, number << 3 | WIRE_VARINT);
            push_varint(out, 1);
        }
    };
    let wrapped = |out: &mut Vec<u8>, number: u64, value: Option<bool>| {
        if let Some(value) = value {
            let mut inner = Vec::new();
            flag(&mut inner, 1, value);
            push_bytes_field(out, number, &inner);
        }
    };
    flag(&mut message, 2, favorites.muted);
    wrapped(&mut message, 3, favorites.guild_visible);
    flag(&mut message, 4, favorites.auto_add_joined_threads);
    wrapped(&mut message, 5, favorites.prune_inactive_threads);
    if favorites.prune_inactive_threads_days != 0 {
        push_varint(&mut message, 6 << 3 | WIRE_VARINT);
        push_varint(&mut message, favorites.prune_inactive_threads_days);
    }
    flag(&mut message, 7, favorites.prune_all_favorited_threads);
    message.extend_from_slice(&unknown_top);
    let mut encoded = Vec::new();
    push_bytes_field(&mut encoded, FIELD_FAVORITES, &message);
    let updated = with_encoded(current, &[], FIELD_FAVORITES, &encoded);
    whole_message_patch(updated, FIELD_FAVORITES)
}

/// `voice_and_video` is message 5 of `PreloadedUserSettings`; its oneof `video_background_filter_desktop` is `blur` 1
/// (`VideoFilterBackgroundBlur`: `use_blur` 1), `preset_option` 2 (uint32) and `custom_asset` 3 (`VideoFilterAsset`:
/// `id` 1 fixed64, `asset_hash` 2), `dumps/PROTOS/discord_protos.discord_users.v1.VoiceAndVideoSettings.json`.
pub const FIELD_VOICE_AND_VIDEO: u64 = 5;
const VIDEO_FILTER_FIELDS: [u64; 3] = [1, 2, 3];

/// The value of `video_background_filter_desktop`, as module 965162 `R4` writes it (`oneofKind` and its member).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoBackgroundFilter {
    Blur {
        use_blur: bool,
    },
    /// A preset number. A oneof member is on the wire whenever it is set, so 0 (Cybercity) is told from no choice.
    Preset(u32),
    Custom {
        id: u64,
        asset_hash: String,
    },
}

/// The oneof as the account holds it, `None` when no member is on the wire. When several are (a proto reader takes
/// the last one), the last wins.
pub fn video_background_in(settings: &[u8]) -> Option<VideoBackgroundFilter> {
    let message = fields(settings)
        .find(|(number, _)| *number == FIELD_VOICE_AND_VIDEO)?
        .1;
    let mut reader = Reader { bytes: message };
    let mut found = None;
    while !reader.bytes.is_empty() {
        let tag = reader.varint()?;
        let (field, wire) = (tag >> 3, tag & 7);
        match wire {
            WIRE_VARINT => {
                let value = reader.varint()?;
                if field == 2 {
                    found = Some(VideoBackgroundFilter::Preset(value as u32));
                }
            }
            WIRE_FIXED64 => {
                reader.take(8)?;
            }
            WIRE_BYTES => {
                let length = usize::try_from(reader.varint()?).ok()?;
                let payload = reader.take(length)?;
                match field {
                    1 => {
                        found = Some(VideoBackgroundFilter::Blur {
                            use_blur: varint_of(payload, 1).is_some_and(|value| value != 0),
                        });
                    }
                    3 => {
                        found = Some(VideoBackgroundFilter::Custom {
                            id: fixed64_of(payload, 1).unwrap_or(0),
                            asset_hash: string_of(payload, 2).unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            }
            WIRE_FIXED32 => {
                reader.take(4)?;
            }
            _ => return None,
        }
    }
    found
}

/// Sets the oneof (`t.videoBackgroundFilterDesktop = R4(e)` in module 352430 `_C`): the other members go, the rest of
/// `voice_and_video` stays byte for byte. Returns the PATCH body value (the whole `voice_and_video` message, as the
/// server replaces a top-level message it is sent) and the settings from now on.
pub fn set_video_background(
    current: &[u8],
    filter: Option<&VideoBackgroundFilter>,
) -> (String, Vec<u8>) {
    let existing = fields(current)
        .find(|(number, _)| *number == FIELD_VOICE_AND_VIDEO)
        .map_or(&[][..], |(_, message)| message);
    let mut message: Vec<u8> = raw_fields(existing)
        .into_iter()
        .filter(|(number, _)| !VIDEO_FILTER_FIELDS.contains(number))
        .flat_map(|(_, raw)| raw.iter().copied())
        .collect();
    match filter {
        None => {}
        Some(VideoBackgroundFilter::Blur { use_blur }) => {
            let mut blur = Vec::new();
            if *use_blur {
                push_varint(&mut blur, 1 << 3 | WIRE_VARINT);
                push_varint(&mut blur, 1);
            }
            push_bytes_field(&mut message, 1, &blur);
        }
        Some(VideoBackgroundFilter::Preset(id)) => {
            push_varint(&mut message, 2 << 3 | WIRE_VARINT);
            push_varint(&mut message, u64::from(*id));
        }
        Some(VideoBackgroundFilter::Custom { id, asset_hash }) => {
            let mut asset = Vec::new();
            if *id != 0 {
                push_fixed64_field(&mut asset, 1, *id);
            }
            if !asset_hash.is_empty() {
                push_bytes_field(&mut asset, 2, asset_hash.as_bytes());
            }
            push_bytes_field(&mut message, 3, &asset);
        }
    }
    let mut encoded = Vec::new();
    push_bytes_field(&mut encoded, FIELD_VOICE_AND_VIDEO, &message);
    let updated = with_encoded(current, &[], FIELD_VOICE_AND_VIDEO, &encoded);
    whole_message_patch(updated, FIELD_VOICE_AND_VIDEO)
}

#[cfg(test)]
mod favorites_tests {
    use super::*;

    /// A varint field this client has no name for.
    fn stranger(number: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, number << 3 | WIRE_VARINT);
        push_varint(&mut out, value);
        out
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn a_rewrite_of_the_favorites_keeps_the_fields_it_does_not_know() {
        // favorites { channel 7 { type 1, position 1, unknown 9 = 33 } + entry-level unknown 3 = 4, unknown 12 = 5 }
        let mut value = Vec::new();
        push_varint(&mut value, 2 << 3 | WIRE_VARINT);
        push_varint(&mut value, 1);
        push_varint(&mut value, 3 << 3 | WIRE_VARINT);
        push_varint(&mut value, 1);
        value.extend(stranger(9, 33));
        let mut entry = Vec::new();
        push_fixed64_field(&mut entry, 1, 7);
        push_bytes_field(&mut entry, 2, &value);
        entry.extend(stranger(3, 4));
        let mut message = Vec::new();
        push_bytes_field(&mut message, 1, &entry);
        message.extend(stranger(12, 5));
        let mut settings = Vec::new();
        push_bytes_field(&mut settings, FIELD_FAVORITES, &message);

        let mut favorites = read_favorites(&settings);
        assert!(favorites.add_channel(8, Some(0)));
        let (_, rewritten) = set_favorites(&settings, &favorites);
        let (_, kept) = fields(&rewritten)
            .find(|(number, _)| *number == FIELD_FAVORITES)
            .unwrap();
        assert!(contains(kept, &stranger(12, 5)), "message level field kept");
        assert!(
            contains(kept, &stranger(9, 33)),
            "favourite level field kept"
        );
        assert!(
            contains(kept, &stranger(3, 4)),
            "map entry level field kept"
        );
        let read = read_favorites(&rewritten);
        assert!(read.is_favorite(7) && read.is_favorite(8));
    }

    #[test]
    fn a_favorite_is_written_as_the_official_message() {
        let mut favorites = Favorites::default();
        assert!(favorites.add_channel(0x0102_0304_0506_0708, Some(2)));
        assert!(!favorites.add_channel(0x0102_0304_0506_0708, Some(2)));
        let (patch, settings) = set_favorites(&[], &favorites);
        let bytes = decode(&patch);
        // favorites (15) { favorite_channels (1) { key fixed64 (1), value (2) { type 1, position 1, channel_type
        // (5) { 2 } } }, guild_visible (3) { true } }
        let expected: Vec<u8> = vec![
            0x7a, 25, 0x0a, 19, 0x09, 8, 7, 6, 5, 4, 3, 2, 1, 0x12, 8, 0x10, 1, 0x18, 1, 0x2a, 2,
            0x08, 2, 0x1a, 2, 0x08, 1,
        ];
        assert_eq!(bytes, expected);
        assert_eq!(settings, bytes);
        let read = read_favorites(&settings);
        assert_eq!(read, favorites);
        assert!(read.guild_enabled());
    }

    #[test]
    fn removing_a_favorite_and_keeping_the_rest_of_the_settings() {
        let mut other = Vec::new();
        push_bytes_field(&mut other, 77, b"keep");
        let mut favorites = Favorites::default();
        favorites.add_channel(5, Some(0));
        favorites.add_channel(6, None);
        let (_, with_two) = set_favorites(&other, &favorites);
        let mut read = read_favorites(&with_two);
        assert_eq!(read.channels.len(), 2);
        assert_eq!(read.channels[1].1.position, 2);
        assert_eq!(read.channels[0].1.channel_type, Some(0));
        assert!(read.remove_channel(5));
        let (_, one) = set_favorites(&with_two, &read);
        assert_eq!(read_favorites(&one).channels.len(), 1);
        assert!(raw_fields(&one).iter().any(|(number, _)| *number == 77));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dismissing_content_preserves_other_bits_and_whole_messages() {
        let mut content = Vec::new();
        super::push_bytes_field(&mut content, 1, &[1]);
        super::push_bytes_field(&mut content, 42, b"preserve nested field");
        let mut current = Vec::new();
        super::push_bytes_field(&mut current, 4, &content);
        super::push_bytes_field(&mut current, 77, b"preserve top field");
        let (_, current) = super::dismiss_content(&current, 98);
        let (patch, current) = super::dismiss_content(&current, 133);
        for id in [0, 98, 133] {
            assert!(super::content_dismissed(&current, id));
        }
        assert!(!super::content_dismissed(&current, 132));
        assert!(!super::content_dismissed(&[], 133));
        assert!(super::fields(&current).any(|(n, p)| n == 77 && p == b"preserve top field"));
        let patch = super::decode(&patch);
        let top: Vec<_> = super::fields(&patch).collect();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, 4);
        assert!(super::fields(top[0].1).any(|(n, p)| n == 42 && p == b"preserve nested field"));
        assert_eq!(super::dismiss_content(&current, 133).1, current);
    }
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
    fn the_speedbump_patch_round_trips() {
        let patch = ignore_speedbump_disabled_patch();
        assert_eq!(
            STANDARD.decode(&patch).unwrap(),
            vec![0xaa, 0x01, 0x02, 0x10, 0x01]
        );
        assert!(ignore_speedbump_disabled(&patch));
        assert!(!ignore_speedbump_disabled(&encoded(&[])));
    }

    #[test]
    fn a_string_preference_is_written_as_its_string_and_read_back_as_its_index() {
        let spoilers = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "chat_spoilers_show_spoilers")
            .expect("spoilers");
        let mut current = Vec::new();
        push_bytes_field(&mut current, 13, &[0x08, 0x03]);
        let (patch, updated) = set_pref(&current, spoilers, 1);
        // text_and_images (6) holds render_spoilers (4), a StringValue whose field 1 is "ALWAYS".
        let part = STANDARD.decode(&patch).unwrap();
        assert!(part.windows(6).any(|window| window == b"ALWAYS"));
        assert_eq!(pref_value(&patch, spoilers), Some(1));
        assert_eq!(pref_value(&STANDARD.encode(&updated), spoilers), Some(1));
        assert_eq!(theme(&STANDARD.encode(&updated)), Some(3));
        let (_, back) = set_pref(&updated, spoilers, 2);
        assert_eq!(pref_value(&STANDARD.encode(&back), spoilers), Some(2));
    }

    #[test]
    fn ui_density_is_a_field_of_appearance() {
        let density = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "appearance_ui_density")
            .expect("density");
        let mut current = Vec::new();
        push_bytes_field(&mut current, 13, &[0x08, 0x03]);
        let (patch, _) = set_pref(&current, density, 4);
        assert_eq!(varint_at(&patch, &[13], 12), Some(4));
        assert_eq!(theme(&patch), Some(3));
    }

    #[test]
    fn a_runaway_varint_stops() {
        assert_eq!(guild_order(&encoded(&[0xff; 12])), None);
    }

    #[test]
    fn every_pref_round_trips_through_a_patch() {
        for pref in PROTO_PREFS.iter().filter(|pref| pref.strings.is_empty()) {
            let patch = pref_patch(pref, 2);
            assert_eq!(
                varint_at(&patch, pref.path, pref.field),
                Some(2),
                "{}",
                pref.key
            );
        }
    }

    #[test]
    fn a_wrapper_without_its_value_reads_as_zero_and_a_missing_one_as_unset() {
        // text_and_images (6) holding an empty render_embeds (12): BoolValue{} is false.
        let embeds = bytes_field(6, &bytes_field(12, &[]));
        assert_eq!(varint_at(&encoded(&embeds), &[6, 12], 1), Some(0));
        assert_eq!(varint_at(&encoded(&embeds), &[6, 13], 1), None);
        assert_eq!(varint_at(&encoded(&[]), &[6, 12], 1), None);
    }

    #[test]
    fn pref_keys_and_places_are_unique() {
        for (index, pref) in PROTO_PREFS.iter().enumerate() {
            for other in &PROTO_PREFS[index + 1..] {
                assert_ne!(pref.key, other.key);
                assert!(
                    pref.path != other.path || pref.field != other.field,
                    "{}",
                    pref.key
                );
            }
        }
    }

    #[test]
    fn setting_a_pref_keeps_the_rest_of_its_message_and_sends_the_whole_message() {
        // appearance (13) holds the theme DARKER and an unrelated fixed32 field 7.
        let appearance = [0x08, 0x03, 0x3d, 1, 2, 3, 4];
        let mut current = bytes_field(13, &appearance);
        current.extend(bytes_field(4, &[9, 9]));
        let hour = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "time_format_setting")
            .unwrap();
        let (patch, updated) = set_pref(&current, hour, 2);
        let patch = STANDARD.decode(patch).unwrap();
        // The patch is the appearance message alone, theme and all, with the hour cycle added.
        assert_eq!(raw_fields(&patch).len(), 1);
        assert_eq!(varint_at(&STANDARD.encode(&patch), &[13], 1), Some(3));
        assert_eq!(
            varint_at(&STANDARD.encode(&patch), hour.path, hour.field),
            Some(2)
        );
        // The other top-level message is untouched in the new settings.
        assert!(
            raw_fields(&updated)
                .iter()
                .any(|(number, raw)| *number == 4 && raw[2..] == [9, 9])
        );
        // Setting it again replaces the value instead of stacking another.
        let (_, again) = set_pref(&updated, hour, 1);
        assert_eq!(
            varint_at(&STANDARD.encode(&again), hour.path, hour.field),
            Some(1)
        );
        assert_eq!(
            raw_fields(&again)
                .iter()
                .filter(|(number, _)| *number == 13)
                .count(),
            1
        );
    }

    #[test]
    fn a_pref_in_a_missing_message_creates_it() {
        let reactions = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "chat_emoji_render_reactions")
            .unwrap();
        let (patch, updated) = set_pref(&[], reactions, 0);
        assert_eq!(varint_at(&patch, reactions.path, reactions.field), Some(0));
        assert_eq!(STANDARD.encode(&updated), patch);
    }

    #[test]
    fn the_colour_theme_is_saved_as_the_gradient_preset_of_the_client_theme() {
        let preset = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "appearance_client_theme_preset")
            .unwrap();
        // SUNSET is 8: appearance (13) { client_theme_settings (3) { background_gradient_preset_id (2) { value (1) } } }.
        let (patch, updated) = set_pref(&[], preset, 8);
        assert_eq!(varint_at(&patch, &[13, 3, 2], 1), Some(8));
        assert_eq!(
            updated,
            bytes_field(13, &bytes_field(3, &bytes_field(2, &[8, 8])))
        );
        assert_eq!(STANDARD.encode(&updated), patch);
    }

    #[test]
    fn a_custom_theme_is_written_with_the_theme_and_replaces_the_colour_theme() {
        // appearance { theme 3, client_theme_settings { preset { 8 }, other 5 }, other 9 }
        let mut client = bytes_field(2, &[8, 8]);
        client.extend_from_slice(&[0x28, 5]);
        let mut appearance = vec![8, 3];
        appearance.extend(bytes_field(3, &client));
        appearance.extend_from_slice(&[0x48, 1]);
        let current = bytes_field(13, &appearance);
        let custom = CustomTheme {
            colors: vec!["#5865f2".to_owned(), "#ff0000".to_owned()],
            angle: 90,
            base_mix: 74,
        };
        let (patch, updated) = set_custom_theme(&current, 2, &custom);
        assert_eq!(STANDARD.encode(&updated), patch);
        assert_eq!(custom_theme_in(&updated), Some(custom));
        // The theme is the new one, the preset and the old message are gone, the rest of `appearance` stays.
        assert_eq!(varint_at(&patch, &[13], 1), Some(2));
        assert_eq!(varint_at(&patch, &[13, 3, 2], 1), None);
        assert!(!updated.windows(2).any(|pair| pair == [0x28, 5]));
        assert_eq!(varint_at(&patch, &[13], 9), Some(1));
        // Taking it out again leaves an empty client theme message.
        let (_, cleared) = clear_pref(&updated, &CUSTOM_THEME_PREF);
        assert_eq!(custom_theme_in(&cleared), None);
        // Nothing saved reads as none, and zero angle and mix are left out of the encoding (proto3 defaults).
        let plain = CustomTheme {
            colors: vec!["#5865f2".to_owned()],
            angle: 0,
            base_mix: 0,
        };
        let (_, written) = set_custom_theme(&[], 3, &plain);
        assert_eq!(custom_theme_in(&written), Some(plain));
        assert_eq!(custom_theme_in(&[]), None);
    }

    #[test]
    fn choosing_a_default_theme_clears_the_saved_colour_theme() {
        let preset = &CLIENT_THEME_PRESET;
        // appearance { theme 3, client_theme_settings { background_gradient_preset_id { value 8 }, other 5 } }
        let mut client = bytes_field(2, &[8, 8]);
        client.extend_from_slice(&[0x28, 5]);
        let mut appearance = vec![8, 3];
        appearance.extend(bytes_field(3, &client));
        let current = bytes_field(13, &appearance);
        let (patch, updated) = clear_pref(&current, preset);
        // The preset is gone, the theme and the other field of the same message stay, and the patch is the whole
        // `appearance` message.
        assert_eq!(varint_at(&patch, &[13, 3, 2], 1), None);
        assert_eq!(varint_at(&patch, &[13], 1), Some(3));
        assert_eq!(varint_at(&patch, &[13, 3], 5), Some(5));
        assert_eq!(STANDARD.encode(&updated), patch);
        // Nothing saved: nothing changes, and a message that is not there is not made.
        let (again, same) = clear_pref(&updated, preset);
        assert_eq!(again, patch);
        assert_eq!(same, updated);
        let (empty_patch, empty) = clear_pref(&[], preset);
        assert!(empty.is_empty());
        assert!(empty_patch.is_empty());
    }

    #[test]
    fn the_theme_is_the_first_varint_of_appearance_with_the_enum_of_the_definition() {
        // `PreloadedUserSettings.appearance` is 13 and its `theme` 1: UNSET 0, DARK 1, LIGHT 2, DARKER 3, MIDNIGHT 4
        // (module 873298).
        assert_eq!((THEME.path, THEME.field), (&[13][..], 1));
        for value in 1..=4 {
            let (patch, updated) = set_pref(&[], &THEME, value);
            assert_eq!(STANDARD.encode(bytes_field(13, &[8, value as u8])), patch);
            assert_eq!(updated, bytes_field(13, &[8, value as u8]));
            assert_eq!(theme(&patch), Some(value));
        }
    }

    #[test]
    fn updates_fold_into_the_settings() {
        let mut current = bytes_field(6, &bytes_field(12, &[8, 1]));
        current.extend(bytes_field(13, &[8, 3]));
        let update = bytes_field(6, &bytes_field(13, &[8, 0]));
        merge_update(&mut current, &update, true);
        let encoded = STANDARD.encode(&current);
        assert_eq!(varint_at(&encoded, &[6, 13], 1), Some(0));
        assert_eq!(varint_at(&encoded, &[6, 12], 1), None);
        assert_eq!(varint_at(&encoded, &[13], 1), Some(3));
        merge_update(&mut current, &update, false);
        assert_eq!(current, update);
    }

    #[test]
    fn a_status_change_keeps_the_custom_status() {
        let custom = CustomStatus {
            text: "busy".into(),
            emoji_id: 0,
            emoji_name: "x".into(),
            expires_at_ms: 0,
        };
        let (_, first) = set_custom_status(&[], Some(&custom), 5);
        let (patch, second) = set_status(&first, "idle", 1_000, 7);
        let read = read_status(&second).expect("status");
        assert_eq!(read.status.as_deref(), Some("idle"));
        assert_eq!(read.expires_at_ms, 1_000);
        assert_eq!(read.custom, Some(custom));
        // The patch is the whole top-level message, as the server swaps it in.
        let patched = read_status(&STANDARD.decode(patch).unwrap()).expect("patch");
        assert_eq!(patched, read);
    }

    #[test]
    fn clearing_the_custom_status_leaves_the_status() {
        let custom = CustomStatus {
            text: "hi".into(),
            ..CustomStatus::default()
        };
        let (_, with) = set_custom_status(&[], Some(&custom), 1);
        let (_, with) = set_status(&with, "dnd", 0, 2);
        let (_, without) = set_custom_status(&with, None, 3);
        let read = read_status(&without).expect("status");
        assert_eq!(read.status.as_deref(), Some("dnd"));
        assert_eq!(read.custom, None);
        assert_eq!(read_status(&[]), None);
    }

    #[test]
    fn other_top_level_messages_survive_a_status_write() {
        let mut appearance = Vec::new();
        push_bytes_field(&mut appearance, 13, &[0x08, 0x03]);
        let (_, updated) = set_status(&appearance, "online", 0, 1);
        assert_eq!(theme(&STANDARD.encode(&updated)), Some(3));
    }

    #[test]
    fn a_guild_list_is_written_packed_and_read_back() {
        let mut current = bytes_field(8, &[0x10, 0x01]);
        current.extend(bytes_field(13, &[8, 3]));
        let (patch, updated) = set_ids(&current, &DM_RESTRICTED_GUILDS, &[10, 20]);
        assert_eq!(ids_in(&updated, &DM_RESTRICTED_GUILDS), vec![10, 20]);
        // The patch is the whole of privacy (8), the rest of it kept.
        let part = STANDARD.decode(&patch).unwrap();
        assert_eq!(raw_fields(&part).len(), 1);
        assert_eq!(ids_in(&part, &DM_RESTRICTED_GUILDS), vec![10, 20]);
        assert_eq!(varint_in(&part, &[8], 2), Some(1));
        // Another list of the same message stays as it was; emptying removes the field.
        let (_, other) = set_ids(&updated, &ACTIVITY_RESTRICTED_GUILDS, &[5]);
        assert_eq!(ids_in(&other, &DM_RESTRICTED_GUILDS), vec![10, 20]);
        assert_eq!(ids_in(&other, &ACTIVITY_RESTRICTED_GUILDS), vec![5]);
        let (_, emptied) = set_ids(&other, &DM_RESTRICTED_GUILDS, &[]);
        assert!(ids_in(&emptied, &DM_RESTRICTED_GUILDS).is_empty());
        assert_eq!(ids_in(&emptied, &ACTIVITY_RESTRICTED_GUILDS), vec![5]);
        // A snowflake keeps all 64 bits.
        let big = 1_568_692_564_637_524_109u64;
        let (_, wide) = set_ids(&[], &MESSAGE_REQUEST_RESTRICTED_GUILDS, &[big]);
        assert_eq!(ids_in(&wide, &MESSAGE_REQUEST_RESTRICTED_GUILDS), vec![big]);
    }

    #[test]
    fn friend_source_flags_are_a_wrapper_in_privacy() {
        let (patch, updated) = set_pref(&[], &FRIEND_SOURCE_FLAGS, FRIEND_EVERYONE);
        assert_eq!(
            varint_in(&updated, &[8, 11], 1),
            Some(FRIEND_EVERYONE),
            "{patch}"
        );
        assert_eq!(
            FRIEND_MUTUAL_FRIENDS | FRIEND_MUTUAL_GUILDS | FRIEND_NO_RELATION,
            14
        );
    }

    #[test]
    fn inverted_preferences_name_their_rows() {
        let quests = PROTO_PREFS
            .iter()
            .find(|pref| pref.key == "data_usage_quests_setting")
            .unwrap();
        assert!(quests.invert);
        assert_eq!((quests.path, quests.field), (&[8u64, 18][..], 1));
        assert!(
            !PROTO_PREFS
                .iter()
                .find(|pref| pref.key == "developer_mode")
                .unwrap()
                .invert
        );
    }
}

#[cfg(test)]
mod guild_folder_tests {
    use super::*;

    fn folder(
        id: Option<u64>,
        name: Option<&str>,
        color: Option<u64>,
        ids: &[u64],
    ) -> GuildFolderEntry {
        GuildFolderEntry {
            id,
            name: name.map(str::to_owned),
            color,
            guild_ids: ids.to_vec(),
        }
    }

    #[test]
    fn folders_round_trip_through_the_patch() {
        let entries = vec![
            folder(None, None, None, &[7]),
            folder(Some(1_234_567), Some("Games"), Some(0x5865F2), &[8, 9]),
            folder(Some(42), None, None, &[10]),
        ];
        let (patch, updated) = set_guild_folders(&[], &entries);
        let (read, positions) = guild_folders(&updated);
        assert_eq!(read, entries);
        assert!(positions.is_empty());
        // The PATCH value is the whole guild_folders message and nothing else.
        let sent = decode(&patch);
        let top: Vec<_> = fields(&sent).collect();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, 14);
        assert_eq!(guild_folders(&sent).0, entries);
    }

    #[test]
    fn exact_bytes_of_one_folder() {
        let (patch, _) = set_guild_folders(&[], &[folder(Some(5), Some("ab"), Some(3), &[1])]);
        let expected = vec![
            0x72, 0x1a, // guild_folders (14), 26 bytes
            0x0a, 0x18, // folders[0], 24 bytes
            0x0a, 0x08, 1, 0, 0, 0, 0, 0, 0, 0, // guild_ids packed
            0x12, 0x02, 0x08, 0x05, // id: Int64Value 5
            0x1a, 0x04, 0x0a, 0x02, b'a', b'b', // name: StringValue
            0x22, 0x02, 0x08, 0x03, // color: UInt64Value 3
        ];
        assert_eq!(STANDARD.decode(patch).unwrap(), expected);
    }

    #[test]
    fn other_settings_and_positions_survive() {
        let mut current = Vec::new();
        push_bytes_field(&mut current, 77, b"other");
        let mut old = Vec::new();
        let packed: Vec<u8> = [3u64, 4].iter().flat_map(|id| id.to_le_bytes()).collect();
        push_bytes_field(&mut old, 2, &packed);
        push_bytes_field(&mut current, 14, &old);
        let (patch, updated) = set_guild_folders(&current, &[folder(None, None, None, &[3])]);
        assert!(fields(&updated).any(|(n, p)| n == 77 && p == b"other"));
        let (entries, positions) = guild_folders(&updated);
        assert_eq!(entries.len(), 1);
        assert_eq!(positions, vec![3, 4]);
        // The patch carries only the guild_folders message.
        assert!(fields(&decode(&patch)).all(|(n, _)| n == 14));
    }

    #[test]
    fn an_empty_name_is_not_written_and_reads_back_as_none() {
        let (_, updated) = set_guild_folders(&[], &[folder(Some(9), Some(""), None, &[1, 2])]);
        let (entries, _) = guild_folders(&updated);
        assert_eq!(entries[0].name, None);
        assert_eq!(entries[0].id, Some(9));
    }
}

#[cfg(test)]
mod folder_rewrite_tests {
    use super::*;

    fn ids(list: &[u64]) -> Vec<u8> {
        list.iter().flat_map(|id| id.to_le_bytes()).collect()
    }

    fn stranger(number: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, number << 3 | WIRE_VARINT);
        push_varint(&mut out, value);
        out
    }

    fn stored(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut message = Vec::new();
        for entry in entries {
            push_bytes_field(&mut message, FOLDERS_FOLDER, entry);
        }
        let mut settings = Vec::new();
        push_bytes_field(&mut settings, FIELD_GUILD_FOLDERS, &message);
        settings
    }

    #[test]
    fn a_rewrite_keeps_unknown_folder_fields_and_the_guilds_of_an_entry_without_an_id() {
        // Folder 5 {guilds 1 2, unknown 9 = 77}; an entry without an id holding guilds 3 4 5.
        let mut folder = Vec::new();
        push_bytes_field(&mut folder, FOLDER_GUILD_IDS, &ids(&[1, 2]));
        let mut id = Vec::new();
        push_varint(&mut id, 1 << 3 | WIRE_VARINT);
        push_varint(&mut id, 5);
        push_bytes_field(&mut folder, 2, &id);
        folder.extend(stranger(9, 77));
        let mut loose = Vec::new();
        push_bytes_field(&mut loose, FOLDER_GUILD_IDS, &ids(&[3, 4, 6]));
        let settings = stored(&[folder, loose]);

        // The model holds the loose entry as its first guild only, and the user drags 1 out of the folder.
        let model = [
            GuildFolderEntry {
                id: Some(5),
                guild_ids: vec![2],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                guild_ids: vec![3],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                guild_ids: vec![1],
                ..GuildFolderEntry::default()
            },
        ];
        let (_, rewritten) = set_guild_folders(&settings, &model);
        let (entries, _) = guild_folders(&rewritten);
        assert_eq!(entries[0].id, Some(5));
        assert_eq!(entries[0].guild_ids, vec![2]);
        assert_eq!(entries[1].guild_ids, vec![3, 4, 6]);
        assert_eq!(entries[2].guild_ids, vec![1]);
        let (_, message) = fields(&rewritten)
            .find(|(number, _)| *number == FIELD_GUILD_FOLDERS)
            .unwrap();
        let kept = kept_folder_parts(message);
        assert!(
            kept[0].unknown == stranger(9, 77),
            "the unknown field of folder 5"
        );
    }

    #[test]
    fn a_guild_that_moved_is_not_listed_twice() {
        let mut loose = Vec::new();
        push_bytes_field(&mut loose, FOLDER_GUILD_IDS, &ids(&[3, 4]));
        let settings = stored(&[loose]);
        let model = [
            GuildFolderEntry {
                guild_ids: vec![3],
                ..GuildFolderEntry::default()
            },
            GuildFolderEntry {
                guild_ids: vec![4],
                ..GuildFolderEntry::default()
            },
        ];
        let (_, rewritten) = set_guild_folders(&settings, &model);
        let (entries, _) = guild_folders(&rewritten);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].guild_ids, vec![3]);
        assert_eq!(entries[1].guild_ids, vec![4]);
    }
}

#[cfg(test)]
mod video_background_tests {
    use super::*;

    /// A `voice_and_video` with `afk_timeout` (6, a `UInt32Value` of 600) and the given oneof bytes.
    fn settings_with(oneof: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();
        let mut afk = Vec::new();
        push_varint(&mut afk, 1 << 3 | WIRE_VARINT);
        push_varint(&mut afk, 600);
        push_bytes_field(&mut message, 6, &afk);
        message.extend_from_slice(oneof);
        let mut settings = Vec::new();
        // An unrelated top-level message that must survive.
        push_bytes_field(&mut settings, FIELD_APPEARANCE, &[0x08, 0x02]);
        push_bytes_field(&mut settings, FIELD_VOICE_AND_VIDEO, &message);
        settings
    }

    #[test]
    fn every_choice_round_trips_and_preset_zero_is_not_nothing() {
        let choices = [
            None,
            Some(VideoBackgroundFilter::Blur { use_blur: true }),
            Some(VideoBackgroundFilter::Preset(0)),
            Some(VideoBackgroundFilter::Preset(3)),
            Some(VideoBackgroundFilter::Preset(10)),
            Some(VideoBackgroundFilter::Custom {
                id: 1_234_567_890_123_456_789,
                asset_hash: "v_0123abcd".to_owned(),
            }),
        ];
        for choice in &choices {
            let (patch, updated) = set_video_background(&settings_with(&[]), choice.as_ref());
            assert_eq!(&video_background_in(&updated), choice, "{choice:?}");
            // The patch is the whole voice_and_video message and carries the same choice and afk_timeout.
            let part = STANDARD.decode(&patch).unwrap();
            assert_eq!(&video_background_in(&part), choice);
            assert_eq!(varint_in(&part, &[5, 6], 1), Some(600));
            assert!(fields(&part).all(|(number, _)| number == FIELD_VOICE_AND_VIDEO));
            // The rest of the settings stays.
            assert_eq!(varint_in(&updated, &[FIELD_APPEARANCE], 1), Some(2));
        }
        // Preset 0 is on the wire as tag 0x10 with value 0.
        let (patch, _) = set_video_background(&[], Some(&VideoBackgroundFilter::Preset(0)));
        assert_eq!(
            STANDARD.decode(patch).unwrap(),
            vec![0x2a, 0x02, 0x10, 0x00]
        );
        // Blur is `{useBlur: true}`.
        let (patch, _) =
            set_video_background(&[], Some(&VideoBackgroundFilter::Blur { use_blur: true }));
        assert_eq!(
            STANDARD.decode(patch).unwrap(),
            vec![0x2a, 0x04, 0x0a, 0x02, 0x08, 0x01]
        );
    }

    #[test]
    fn a_new_choice_replaces_the_other_members_and_the_last_member_wins() {
        let (_, blurred) = set_video_background(
            &settings_with(&[0x10, 0x07]),
            Some(&VideoBackgroundFilter::Blur { use_blur: true }),
        );
        let message = fields(&blurred)
            .find(|(number, _)| *number == FIELD_VOICE_AND_VIDEO)
            .unwrap()
            .1;
        assert!(raw_fields(message).iter().all(|(number, _)| *number != 2));
        // Written by another client with two members: a proto reader keeps the last.
        let both = settings_with(&[0x0a, 0x02, 0x08, 0x01, 0x10, 0x00]);
        assert_eq!(
            video_background_in(&both),
            Some(VideoBackgroundFilter::Preset(0))
        );
        assert_eq!(video_background_in(&settings_with(&[])), None);
        assert_eq!(video_background_in(&[]), None);
    }
}
