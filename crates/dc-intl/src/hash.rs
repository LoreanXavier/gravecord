//! `runtimeHashMessageKey` of module 256784: the catalog key of a message name, the first 6 base64 characters of the
//! little-endian XXH64 (seed 0, module 618830) of its UTF-8 bytes. The Language select reads each locale's own name
//! under the key of its code (module 226536, `localizedName`).

const P1: u64 = 0x9E37_79B1_85EB_CA87;
const P2: u64 = 0xC2B2_AE3D_27D4_EB4F;
const P3: u64 = 0x1656_67B1_9E37_79F9;
const P4: u64 = 0x85EB_CA77_C2B2_AE63;
const P5: u64 = 0x27D4_EB2F_1656_67C5;

fn round(acc: u64, input: u64) -> u64 {
    acc.wrapping_add(input.wrapping_mul(P2))
        .rotate_left(31)
        .wrapping_mul(P1)
}

fn merge(acc: u64, value: u64) -> u64 {
    (acc ^ round(0, value)).wrapping_mul(P1).wrapping_add(P4)
}

fn read64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"))
}

fn read32(bytes: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes(
        bytes[..4].try_into().expect("four bytes"),
    ))
}

pub fn xxh64(input: &[u8], seed: u64) -> u64 {
    let mut rest = input;
    let mut hash = if input.len() >= 32 {
        let mut v = [
            seed.wrapping_add(P1).wrapping_add(P2),
            seed.wrapping_add(P2),
            seed,
            seed.wrapping_sub(P1),
        ];
        while rest.len() >= 32 {
            for (lane, chunk) in v.iter_mut().zip(rest.as_chunks::<8>().0) {
                *lane = round(*lane, u64::from_le_bytes(*chunk));
            }
            rest = &rest[32..];
        }
        let mut hash = v[0]
            .rotate_left(1)
            .wrapping_add(v[1].rotate_left(7))
            .wrapping_add(v[2].rotate_left(12))
            .wrapping_add(v[3].rotate_left(18));
        for lane in v {
            hash = merge(hash, lane);
        }
        hash
    } else {
        seed.wrapping_add(P5)
    };
    hash = hash.wrapping_add(input.len() as u64);
    while rest.len() >= 8 {
        hash ^= round(0, read64(rest));
        hash = hash.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        rest = &rest[8..];
    }
    if rest.len() >= 4 {
        hash ^= read32(rest).wrapping_mul(P1);
        hash = hash.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        rest = &rest[4..];
    }
    for byte in rest {
        hash ^= u64::from(*byte).wrapping_mul(P5);
        hash = hash.rotate_left(11).wrapping_mul(P1);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(P2);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(P3);
    hash ^ (hash >> 32)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The catalog key of a message name (`runtimeHashMessageKey`).
pub fn message_key(name: &str) -> String {
    let t = xxh64(name.as_bytes(), 0).to_le_bytes();
    let pick = |index: u8| char::from(ALPHABET[usize::from(index)]);
    [
        pick(t[0] >> 2),
        pick((t[0] & 3) << 4 | t[1] >> 4),
        pick((t[1] & 15) << 2 | t[2] >> 6),
        pick(t[2] & 63),
        pick(t[3] >> 2),
        pick((t[3] & 3) << 4 | t[4] >> 4),
    ]
    .into_iter()
    .collect()
}
