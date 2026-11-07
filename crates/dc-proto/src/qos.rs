//! `discord_protos.qos_token.v1.QosToken`, hand-encoded: two length-delimited fields.
//! `derived` is READY's `qos_token`, kept verbatim between sessions.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

fn varint(mut value: usize, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// IDENTIFY's `qos_token`. Field 1 is always written, empty when the user is inactive, as the original does.
pub fn encode_token(is_active: bool, derived_base64: Option<&str>) -> String {
    let mut out = vec![0x0a];
    if is_active {
        out.extend_from_slice(&[2, 0x08, 1]);
    } else {
        out.push(0);
    }
    if let Some(derived) = derived_base64.and_then(|text| STANDARD.decode(text).ok()) {
        out.push(0x12);
        varint(derived.len(), &mut out);
        out.extend_from_slice(&derived);
    }
    STANDARD.encode(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_connect_token_has_only_the_empty_client_field() {
        assert_eq!(encode_token(false, None), "CgA=");
    }

    #[test]
    fn active_flag_is_field_one_of_the_client_message() {
        assert_eq!(
            STANDARD.decode(encode_token(true, None)).unwrap(),
            [0x0a, 2, 8, 1]
        );
    }

    #[test]
    fn keeps_the_recorded_prefix_when_derived_data_exists() {
        let derived = STANDARD.encode([7u8; 99]);
        let token = STANDARD
            .decode(encode_token(false, Some(&derived)))
            .unwrap();
        assert_eq!(&token[..4], [0x0a, 0, 0x12, 99]);
        assert_eq!(token.len(), 4 + 99);
    }
}
