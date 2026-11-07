//! External Term Format, as the pinned client speaks it. Port of the packer and unpacker in module 868.
//!
//! Packer options: `{string: binary, key: binary, array: list, null: atom, undefined: null, useLegacyAtoms: true}`.
//! Unpacker options: `{binary: utf8, string: array, nil: array, atomTable: {nil, null, true, false}}`.
//!
//! JSON has the same limits here as in the original: integers that need more than 4 bytes decode to decimal strings,
//! which is what keeps snowflakes exact.

use serde_json::{Map, Number, Value};
use thiserror::Error;

const VERSION: u8 = 131;
const NEW_FLOAT: u8 = 70;
const BIT_BINARY: u8 = 77;
const ATOM: u8 = 100;
const SMALL_INTEGER: u8 = 97;
const INTEGER: u8 = 98;
const SMALL_TUPLE: u8 = 104;
const LARGE_TUPLE: u8 = 105;
const NIL: u8 = 106;
const STRING: u8 = 107;
const LIST: u8 = 108;
const BINARY: u8 = 109;
const SMALL_BIG: u8 = 110;
const LARGE_BIG: u8 = 111;
const SMALL_ATOM: u8 = 115;
const MAP: u8 = 116;
const ATOM_UTF8: u8 = 118;
const SMALL_ATOM_UTF8: u8 = 119;

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
/// Far deeper than any payload; stops a hostile frame from overflowing the stack.
const MAX_DEPTH: usize = 128;

#[derive(Debug, Error, PartialEq)]
pub enum EtfError {
    #[error("unexpected end of data")]
    Truncated,
    #[error("unexpected ETF tag {0}")]
    UnexpectedTag(u8),
    #[error("nesting too deep")]
    TooDeep,
    #[error("length larger than the data")]
    BadLength,
}

pub fn pack(value: &Value) -> Vec<u8> {
    let mut out = vec![VERSION];
    pack_value(value, &mut out);
    out
}

fn pack_atom(name: &str, out: &mut Vec<u8>) {
    out.push(SMALL_ATOM);
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
}

fn pack_binary(text: &str, out: &mut Vec<u8>) {
    out.push(BINARY);
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn pack_integer(magnitude: u64, negative: bool, out: &mut Vec<u8>) {
    if magnitude < 256 && !negative {
        out.extend_from_slice(&[SMALL_INTEGER, magnitude as u8]);
    } else if magnitude < 0x8000_0000 {
        let signed = if negative {
            -(magnitude as i32)
        } else {
            magnitude as i32
        };
        out.push(INTEGER);
        out.extend_from_slice(&signed.to_be_bytes());
    } else if magnitude <= MAX_SAFE_INTEGER {
        let high = magnitude >> 32;
        let length = match high {
            0 => 4,
            1..=0xff => 5,
            0x100..=0xffff => 6,
            _ => 7,
        };
        out.extend_from_slice(&[SMALL_BIG, length, negative as u8]);
        out.extend_from_slice(&magnitude.to_le_bytes()[..length as usize]);
    } else {
        let value = magnitude as f64 * if negative { -1.0 } else { 1.0 };
        pack_float(value, out);
    }
}

fn pack_float(value: f64, out: &mut Vec<u8>) {
    out.push(NEW_FLOAT);
    out.extend_from_slice(&value.to_be_bytes());
}

fn pack_number(number: &Number, out: &mut Vec<u8>) {
    if let Some(value) = number.as_u64() {
        pack_integer(value, false, out);
    } else if let Some(value) = number.as_i64() {
        pack_integer(value.unsigned_abs(), value < 0, out);
    } else {
        let value = number.as_f64().unwrap_or(0.0);
        // JS has one number type, so 2.0 is an integer there.
        if value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER as f64 {
            pack_integer(value.abs() as u64, value < 0.0, out);
        } else {
            pack_float(value, out);
        }
    }
}

fn pack_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => pack_atom("nil", out),
        Value::Bool(true) => pack_atom("true", out),
        Value::Bool(false) => pack_atom("false", out),
        Value::Number(number) => pack_number(number, out),
        Value::String(text) => pack_binary(text, out),
        Value::Array(items) if items.is_empty() => out.push(NIL),
        Value::Array(items) => {
            out.push(LIST);
            out.extend_from_slice(&(items.len() as u32).to_be_bytes());
            for item in items {
                pack_value(item, out);
            }
            out.push(NIL);
        }
        Value::Object(map) => {
            out.push(MAP);
            out.extend_from_slice(&(map.len() as u32).to_be_bytes());
            for (key, item) in map {
                pack_binary(key, out);
                pack_value(item, out);
            }
        }
    }
}

pub fn unpack(bytes: &[u8]) -> Result<Value, EtfError> {
    let mut reader = Reader {
        bytes,
        at: usize::from(bytes.first() == Some(&VERSION)),
    };
    reader.value(0)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], EtfError> {
        let end = self.at.checked_add(count).ok_or(EtfError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(EtfError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, EtfError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<usize, EtfError> {
        let raw = self.take(2)?;
        Ok(u16::from_be_bytes([raw[0], raw[1]]) as usize)
    }

    fn u32(&mut self) -> Result<usize, EtfError> {
        let raw = self.take(4)?;
        Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
    }

    /// A count of items, each at least one byte, can never exceed what is left.
    fn count(&self, count: usize) -> Result<usize, EtfError> {
        if count > self.bytes.len() - self.at {
            return Err(EtfError::BadLength);
        }
        Ok(count)
    }

    fn atom(&mut self, length: usize, utf8: bool) -> Result<Value, EtfError> {
        let raw = self.take(length)?;
        Ok(match raw {
            b"nil" | b"null" => Value::Null,
            b"true" => Value::Bool(true),
            b"false" => Value::Bool(false),
            _ if utf8 => Value::String(String::from_utf8_lossy(raw).into_owned()),
            _ => Value::String(raw.iter().map(|&byte| byte as char).collect()),
        })
    }

    fn items(&mut self, count: usize, depth: usize) -> Result<Vec<Value>, EtfError> {
        let count = self.count(count)?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(self.value(depth + 1)?);
        }
        Ok(items)
    }

    fn big(&mut self, length: usize) -> Result<Value, EtfError> {
        if length == 0 {
            self.take(1)?;
            return Ok(Value::from(0));
        }
        if length == 1 && self.bytes.get(self.at + 1) == Some(&0) {
            self.take(2)?;
            return Ok(Value::from(0));
        }
        let negative = self.u8()? == 1;
        let digits = self.take(length)?;
        if length <= 4 {
            let magnitude = digits
                .iter()
                .rev()
                .fold(0u64, |sum, &byte| (sum << 8) | byte as u64);
            return Ok(match (negative, magnitude) {
                (false, _) => Value::from(magnitude),
                (true, 0) => Value::from(0),
                (true, small) if small < 0x8000_0000 => Value::from(-(small as i64)),
                (true, large) => Value::String(format!("-{large}")),
            });
        }
        let text = little_endian_to_decimal(digits);
        Ok(Value::String(if negative {
            format!("-{text}")
        } else {
            text
        }))
    }

    fn value(&mut self, depth: usize) -> Result<Value, EtfError> {
        if depth > MAX_DEPTH {
            return Err(EtfError::TooDeep);
        }
        let tag = self.u8()?;
        match tag {
            NEW_FLOAT => {
                let raw: [u8; 8] = self.take(8)?.try_into().map_err(|_| EtfError::Truncated)?;
                let value = f64::from_be_bytes(raw);
                Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
            }
            SMALL_INTEGER => Ok(Value::from(self.u8()?)),
            INTEGER => {
                let raw: [u8; 4] = self.take(4)?.try_into().map_err(|_| EtfError::Truncated)?;
                Ok(Value::from(i32::from_be_bytes(raw)))
            }
            ATOM => {
                let length = self.u16()?;
                self.atom(length, false)
            }
            ATOM_UTF8 => {
                let length = self.u16()?;
                self.atom(length, true)
            }
            SMALL_ATOM => {
                let length = self.u8()? as usize;
                self.atom(length, false)
            }
            SMALL_ATOM_UTF8 => {
                let length = self.u8()? as usize;
                self.atom(length, true)
            }
            SMALL_TUPLE => {
                let count = self.u8()? as usize;
                Ok(Value::Array(self.items(count, depth)?))
            }
            LARGE_TUPLE => {
                let count = self.u32()?;
                Ok(Value::Array(self.items(count, depth)?))
            }
            LIST => {
                let count = self.u32()?;
                let mut items = self.items(count, depth)?;
                if self.bytes.get(self.at) == Some(&NIL) {
                    self.at += 1;
                } else {
                    items.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            NIL => Ok(Value::Array(Vec::new())),
            STRING => {
                let length = self.u16()?;
                Ok(Value::Array(
                    self.take(length)?
                        .iter()
                        .map(|&byte| Value::from(byte))
                        .collect(),
                ))
            }
            BINARY => {
                let length = self.u32()?;
                Ok(Value::String(
                    String::from_utf8_lossy(self.take(length)?).into_owned(),
                ))
            }
            BIT_BINARY => {
                let length = self.u32()?;
                let bits = self.u8()?;
                let mut raw = self.take(length)?.to_vec();
                if let Some(last) = raw.last_mut() {
                    *last = last.checked_shr(8 - bits as u32).unwrap_or(0);
                }
                Ok(Value::Array(raw.into_iter().map(Value::from).collect()))
            }
            SMALL_BIG => {
                let length = self.u8()? as usize;
                self.big(length)
            }
            LARGE_BIG => {
                let length = self.u32()?;
                self.big(length)
            }
            MAP => {
                let count = self.u32()?;
                let count = self.count(count)?;
                let mut map = Map::with_capacity(count);
                for _ in 0..count {
                    let key = key_text(self.value(depth + 1)?);
                    map.insert(key, self.value(depth + 1)?);
                }
                Ok(Value::Object(map))
            }
            other => Err(EtfError::UnexpectedTag(other)),
        }
    }
}

/// JS coerces object keys with `String(key)`.
fn key_text(key: Value) -> String {
    match key {
        Value::String(text) => text,
        Value::Array(items) => items.iter().map(key_text_ref).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

fn key_text_ref(key: &Value) -> String {
    key_text(key.clone())
}

fn little_endian_to_decimal(digits: &[u8]) -> String {
    let mut decimal = vec![0u8];
    for &byte in digits.iter().rev() {
        let mut carry = byte as u32;
        for digit in decimal.iter_mut() {
            let sum = *digit as u32 * 256 + carry;
            *digit = (sum % 10) as u8;
            carry = sum / 10;
        }
        while carry > 0 {
            decimal.push((carry % 10) as u8);
            carry /= 10;
        }
    }
    decimal
        .iter()
        .rev()
        .map(|digit| (b'0' + digit) as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn packs_atoms_the_way_the_client_does() {
        assert_eq!(pack(&json!(null)), [131, 115, 3, b'n', b'i', b'l']);
        assert_eq!(pack(&json!(true)), [131, 115, 4, b't', b'r', b'u', b'e']);
        assert_eq!(
            pack(&json!(false)),
            [131, 115, 5, b'f', b'a', b'l', b's', b'e']
        );
    }

    #[test]
    fn packs_integers_by_size() {
        assert_eq!(pack(&json!(255)), [131, 97, 255]);
        assert_eq!(pack(&json!(256)), [131, 98, 0, 0, 1, 0]);
        assert_eq!(pack(&json!(-1)), [131, 98, 255, 255, 255, 255]);
        assert_eq!(
            pack(&json!(0x8000_0000u64)),
            [131, 110, 4, 0, 0, 0, 0, 0x80]
        );
        assert_eq!(
            pack(&json!(0x1_0000_0000u64)),
            [131, 110, 5, 0, 0, 0, 0, 0, 1]
        );
        assert_eq!(
            pack(&json!(-0x8000_0000i64)),
            [131, 110, 4, 1, 0, 0, 0, 0x80]
        );
    }

    #[test]
    fn integers_past_the_js_safe_range_become_floats() {
        let packed = pack(&json!(1u64 << 53));
        assert_eq!(packed[1], NEW_FLOAT);
        assert_eq!(unpack(&packed).unwrap(), json!(9007199254740992.0));
    }

    #[test]
    fn packs_strings_keys_and_lists_as_the_client_does() {
        assert_eq!(pack(&json!("hi")), [131, 109, 0, 0, 0, 2, b'h', b'i']);
        assert_eq!(pack(&json!([])), [131, 106]);
        assert_eq!(pack(&json!([1])), [131, 108, 0, 0, 0, 1, 97, 1, 106]);
        assert_eq!(
            pack(&json!({"a": 1})),
            [131, 116, 0, 0, 0, 1, 109, 0, 0, 0, 1, b'a', 97, 1]
        );
    }

    #[test]
    fn round_trips_a_payload_in_key_order() {
        let value =
            json!({"op": 2, "d": {"token": "x", "list": [1, "two", null, true, 1.5], "empty": []}});
        assert_eq!(
            unpack(&pack(&value)).unwrap().to_string(),
            value.to_string()
        );
    }

    #[test]
    fn snowflakes_decode_to_exact_strings() {
        // 1554855651514122281 little endian, as the gateway sends ids.
        let id: u64 = 1554855651514122281;
        let mut bytes = vec![131, 110, 8, 0];
        bytes.extend_from_slice(&id.to_le_bytes());
        assert_eq!(unpack(&bytes).unwrap(), json!("1554855651514122281"));
    }

    #[test]
    fn big_integer_widths_follow_the_client() {
        assert_eq!(unpack(&[110, 0, 0]).unwrap(), json!(0));
        assert_eq!(unpack(&[110, 1, 0, 0]).unwrap(), json!(0));
        assert_eq!(unpack(&[110, 4, 0, 1, 0, 0, 0]).unwrap(), json!(1));
        assert_eq!(unpack(&[110, 4, 1, 1, 0, 0, 0]).unwrap(), json!(-1));
        assert_eq!(
            unpack(&[110, 4, 1, 0, 0, 0, 0x80]).unwrap(),
            json!("-2147483648")
        );
        assert_eq!(
            unpack(&[110, 5, 0, 0, 0, 0, 0, 1]).unwrap(),
            json!("4294967296")
        );
        assert_eq!(
            unpack(&[110, 5, 1, 0, 0, 0, 0, 1]).unwrap(),
            json!("-4294967296")
        );
    }

    #[test]
    fn nineteen_byte_integers_stay_exact() {
        let mut bytes = vec![110, 9, 0];
        bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(unpack(&bytes).unwrap(), json!("18446744073709551616"));
    }

    #[test]
    fn nil_is_an_empty_array_and_atoms_map_to_json() {
        assert_eq!(unpack(&[106]).unwrap(), json!([]));
        assert_eq!(unpack(&[119, 3, b'n', b'i', b'l']).unwrap(), json!(null));
        assert_eq!(
            unpack(&[119, 4, b'n', b'u', b'l', b'l']).unwrap(),
            json!(null)
        );
        assert_eq!(
            unpack(&[100, 0, 4, b't', b'r', b'u', b'e']).unwrap(),
            json!(true)
        );
        assert_eq!(unpack(&[115, 2, b'o', b'k']).unwrap(), json!("ok"));
    }

    #[test]
    fn string_ext_decodes_to_bytes() {
        assert_eq!(unpack(&[107, 0, 2, 1, 200]).unwrap(), json!([1, 200]));
    }

    #[test]
    fn improper_list_tail_is_kept() {
        assert_eq!(
            unpack(&[108, 0, 0, 0, 1, 97, 1, 97, 2]).unwrap(),
            json!([1, 2])
        );
    }

    #[test]
    fn map_keys_are_coerced_to_text() {
        assert_eq!(
            unpack(&[116, 0, 0, 0, 1, 97, 5, 97, 1]).unwrap(),
            json!({"5": 1})
        );
        assert_eq!(
            unpack(&[116, 0, 0, 0, 1, 119, 2, b'o', b'p', 97, 1]).unwrap(),
            json!({"op": 1})
        );
    }

    #[test]
    fn rejects_truncated_and_hostile_input() {
        assert_eq!(unpack(&[]), Err(EtfError::Truncated));
        assert_eq!(unpack(&[131, 109, 0, 0, 0, 9, 1]), Err(EtfError::Truncated));
        assert_eq!(unpack(&[108, 255, 255, 255, 255]), Err(EtfError::BadLength));
        assert_eq!(unpack(&[116, 255, 255, 255, 255]), Err(EtfError::BadLength));
        assert_eq!(unpack(&[200]), Err(EtfError::UnexpectedTag(200)));
        let mut nested = Vec::new();
        for _ in 0..2000 {
            nested.extend_from_slice(&[108, 0, 0, 0, 1]);
        }
        assert_eq!(unpack(&nested), Err(EtfError::TooDeep));
    }
}
