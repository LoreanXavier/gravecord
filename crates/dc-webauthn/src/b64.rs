//! base64url without padding, the encoding of every binary field in WebAuthn's JSON (module 293731 `r`:
//! `btoa(...)` with `+` to `-`, `/` to `_` and the `=` dropped).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The decoding of `PublicKeyCredential.parseRequestOptionsFromJSON`: padding is tolerated, the standard alphabet is not.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors_without_padding() {
        let vectors: [(&[u8], &str); 7] = [
            (b"", ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"fooba", "Zm9vYmE"),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (bytes, text) in vectors {
            assert_eq!(encode(bytes), text);
            assert_eq!(decode(text).as_deref(), Some(bytes));
        }
    }

    #[test]
    fn the_url_alphabet_replaces_plus_and_slash() {
        assert_eq!(encode(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(decode("-__-").as_deref(), Some(&[0xfb, 0xff, 0xfe][..]));
        assert_eq!(decode("+//+"), None);
    }

    #[test]
    fn padding_is_tolerated_and_garbage_is_refused() {
        assert_eq!(decode("Zg==").as_deref(), Some(&b"f"[..]));
        assert_eq!(decode("Zm9vYg=").as_deref(), Some(&b"foob"[..]));
        assert_eq!(decode("Z"), None);
        assert_eq!(decode("a b"), None);
    }
}
