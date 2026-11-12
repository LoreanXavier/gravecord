//! `AuthenticatorAttestationResponse.getPublicKey()` and `getPublicKeyAlgorithm()`: the credential public key of an
//! attestation's authenticator data (a COSE_Key) as DER SubjectPublicKeyInfo, for the algorithms a browser covers
//! (ES256, RS256 and the other RSA ones, EdDSA). Anything else has no SPKI (`null` in the browser).

/// One CBOR item, as far as a COSE_Key needs.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Int(i64),
    Bytes(Vec<u8>),
    Map(Vec<(Item, Item)>),
    Other,
}

fn argument(data: &[u8], at: &mut usize, info: u8) -> Option<u64> {
    let width = match info {
        0..=23 => return Some(u64::from(info)),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return None,
    };
    let end = at.checked_add(width)?;
    let slice = data.get(*at..end)?;
    *at = end;
    Some(
        slice
            .iter()
            .fold(0u64, |all, byte| (all << 8) | u64::from(*byte)),
    )
}

fn read(data: &[u8], at: &mut usize, depth: u8) -> Option<Item> {
    if depth > 4 {
        return None;
    }
    let head = *data.get(*at)?;
    *at += 1;
    let (major, info) = (head >> 5, head & 31);
    let value = argument(data, at, info)?;
    Some(match major {
        0 => Item::Int(i64::try_from(value).ok()?),
        1 => Item::Int(-1 - i64::try_from(value).ok()?),
        2 | 3 => {
            let end = at.checked_add(usize::try_from(value).ok()?)?;
            let slice = data.get(*at..end)?;
            *at = end;
            if major == 2 {
                Item::Bytes(slice.to_vec())
            } else {
                Item::Other
            }
        }
        5 => {
            let mut pairs = Vec::new();
            for _ in 0..value.min(32) {
                let key = read(data, at, depth + 1)?;
                let item = read(data, at, depth + 1)?;
                pairs.push((key, item));
            }
            Item::Map(pairs)
        }
        _ => Item::Other,
    })
}

/// The COSE_Key of the attested credential data: authenticator data is the RP id hash (32), flags (1), the counter
/// (4), then, when flag AT (0x40) is set, the AAGUID (16), the credential id length (2), the id and the key.
fn cose_key(authenticator_data: &[u8]) -> Option<Vec<(Item, Item)>> {
    if authenticator_data.len() < 55 || authenticator_data[32] & 0x40 == 0 {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([
        authenticator_data[53],
        authenticator_data[54],
    ]));
    let mut at = 55usize.checked_add(length)?;
    match read(authenticator_data, &mut at, 0)? {
        Item::Map(pairs) => Some(pairs),
        _ => None,
    }
}

fn find(pairs: &[(Item, Item)], label: i64) -> Option<&Item> {
    pairs
        .iter()
        .find(|(key, _)| *key == Item::Int(label))
        .map(|(_, item)| item)
}

fn number(pairs: &[(Item, Item)], label: i64) -> Option<i64> {
    match find(pairs, label)? {
        Item::Int(value) => Some(*value),
        _ => None,
    }
}

fn octets(pairs: &[(Item, Item)], label: i64) -> Option<&[u8]> {
    match find(pairs, label)? {
        Item::Bytes(bytes) => Some(bytes),
        _ => None,
    }
}

/// `getPublicKeyAlgorithm()`: the COSE `alg` of the credential key.
pub fn algorithm(authenticator_data: &[u8]) -> Option<i64> {
    number(&cose_key(authenticator_data)?, 3)
}

/// DER: tag, length, content.
fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = content.len();
    if length < 128 {
        out.push(length as u8);
    } else {
        let bytes: Vec<u8> = length
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
    out.extend_from_slice(content);
    out
}

/// A positive DER INTEGER from big-endian magnitude bytes.
fn der_uint(bytes: &[u8]) -> Vec<u8> {
    let trimmed: Vec<u8> = bytes
        .iter()
        .copied()
        .skip_while(|byte| *byte == 0)
        .collect();
    let mut body = Vec::with_capacity(trimmed.len() + 1);
    if trimmed.first().is_none_or(|byte| byte & 0x80 != 0) {
        body.push(0);
    }
    body.extend(trimmed);
    der(0x02, &body)
}

fn spki(algorithm_identifier: &[u8], key: &[u8]) -> Vec<u8> {
    let mut bits = vec![0u8];
    bits.extend_from_slice(key);
    der(0x30, &[algorithm_identifier, &der(0x03, &bits)].concat())
}

/// `getPublicKey()`: SubjectPublicKeyInfo of the credential key, when it is one a browser reports.
pub fn public_key_spki(authenticator_data: &[u8]) -> Option<Vec<u8>> {
    let key = cose_key(authenticator_data)?;
    match number(&key, 1)? {
        // EC2 on P-256: id-ecPublicKey with prime256v1 and the uncompressed point.
        2 if number(&key, -1) == Some(1) => {
            let (x, y) = (octets(&key, -2)?, octets(&key, -3)?);
            if x.len() != 32 || y.len() != 32 {
                return None;
            }
            let identifier = der(
                0x30,
                &[
                    der(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]),
                    der(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]),
                ]
                .concat(),
            );
            Some(spki(&identifier, &[&[4][..], x, y].concat()))
        }
        // RSA: rsaEncryption with a NULL parameter and RSAPublicKey { n, e }.
        3 => {
            let (n, e) = (octets(&key, -1)?, octets(&key, -2)?);
            let identifier = der(
                0x30,
                &[
                    der(
                        0x06,
                        &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01],
                    ),
                    vec![0x05, 0x00],
                ]
                .concat(),
            );
            let public = der(0x30, &[der_uint(n), der_uint(e)].concat());
            Some(spki(&identifier, &public))
        }
        // OKP on Ed25519: id-Ed25519, the 32 byte key.
        1 if number(&key, -1) == Some(6) => {
            let x = octets(&key, -2)?;
            if x.len() != 32 {
                return None;
            }
            let identifier = der(0x30, &der(0x06, &[0x2b, 0x65, 0x70]));
            Some(spki(&identifier, x))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Authenticator data with attested credential data around a COSE_Key.
    fn authenticator_data(cose: &[u8]) -> Vec<u8> {
        let mut data = vec![7u8; 32];
        data.push(0x45);
        data.extend([0, 0, 0, 9]);
        data.extend([0u8; 16]);
        data.extend([0, 3]);
        data.extend([1, 2, 3]);
        data.extend_from_slice(cose);
        data
    }

    fn ec_key(x: u8, y: u8) -> Vec<u8> {
        // {1: 2, 3: -7, -1: 1, -2: x*32, -3: y*32}
        let mut key = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        key.extend([x; 32]);
        key.extend([0x22, 0x58, 0x20]);
        key.extend([y; 32]);
        key
    }

    #[test]
    fn an_es256_key_becomes_a_91_byte_spki() {
        let data = authenticator_data(&ec_key(0xaa, 0xbb));
        assert_eq!(algorithm(&data), Some(-7));
        let spki = public_key_spki(&data).unwrap();
        assert_eq!(spki.len(), 91);
        assert_eq!(
            spki[..27],
            [
                0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
                0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00, 0x04
            ]
        );
        assert!(spki[27..59].iter().all(|byte| *byte == 0xaa));
        assert!(spki[59..].iter().all(|byte| *byte == 0xbb));
    }

    #[test]
    fn an_rsa_key_is_der_with_a_padded_modulus() {
        // {1: 3, 3: -257, -1: n (3 bytes, high bit set), -2: 010001}
        let mut cose = vec![
            0xa4, 0x01, 0x03, 0x03, 0x39, 0x01, 0x00, 0x20, 0x43, 0x80, 0x01, 0x02,
        ];
        cose.extend([0x21, 0x43, 0x01, 0x00, 0x01]);
        let data = authenticator_data(&cose);
        assert_eq!(algorithm(&data), Some(-257));
        let spki = public_key_spki(&data).unwrap();
        // SEQ { SEQ { OID, NULL }, BIT STRING { 00, SEQ { INT 80 01 02 with its 00, INT 01 00 01 } } }
        let expected: Vec<u8> = vec![
            0x30, 0x1f, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01,
            0x01, 0x05, 0x00, 0x03, 0x0e, 0x00, 0x30, 0x0b, 0x02, 0x04, 0x00, 0x80, 0x01, 0x02,
            0x02, 0x03, 0x01, 0x00, 0x01,
        ];
        assert_eq!(spki.len(), expected.len());
        assert_eq!(spki, expected);
    }

    #[test]
    fn an_ed25519_key_is_44_bytes() {
        // {1: 1, 3: -8, -1: 6, -2: x*32}
        let mut cose = vec![0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20];
        cose.extend([9u8; 32]);
        let data = authenticator_data(&cose);
        assert_eq!(algorithm(&data), Some(-8));
        let spki = public_key_spki(&data).unwrap();
        assert_eq!(spki.len(), 44);
        assert_eq!(
            spki[..12],
            [
                0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00
            ]
        );
    }

    #[test]
    fn data_without_a_credential_has_no_key() {
        let mut data = authenticator_data(&ec_key(1, 2));
        data[32] = 0x05;
        assert_eq!(public_key_spki(&data), None);
        assert_eq!(algorithm(&data), None);
        assert_eq!(public_key_spki(&[0u8; 10]), None);
        assert_eq!(public_key_spki(&authenticator_data(&[0xa1, 0x01])), None);
    }

    #[test]
    fn long_der_lengths_use_the_long_form() {
        assert_eq!(der(0x04, &[0u8; 200])[..3], [0x04, 0x81, 200]);
        assert_eq!(der(0x04, &[0u8; 300])[..4], [0x04, 0x82, 0x01, 0x2c]);
    }
}
