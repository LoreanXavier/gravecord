//! A locale's catalog as the pinned build ships it: a webpack chunk whose module is `JSON.parse('{...}')` with the
//! JSON in a single-quoted JavaScript string literal (the shape `tools/dump-strings.mjs` reads). Only the literal is
//! read; nothing is executed.

/// The JSON text of the catalog in a chunk's source.
pub fn catalog_json(source: &str) -> Option<String> {
    let start = source.find("JSON.parse('")? + "JSON.parse('".len();
    let mut units: Vec<u16> = Vec::with_capacity(source.len() - start);
    let mut chars = source[start..].chars();
    let mut buffer = [0u16; 2];
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => return Some(String::from_utf16_lossy(&units)),
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    'n' => units.push(0x0a),
                    'r' => units.push(0x0d),
                    't' => units.push(0x09),
                    'b' => units.push(0x08),
                    'f' => units.push(0x0c),
                    'v' => units.push(0x0b),
                    '0' => units.push(0),
                    'x' => {
                        let hex: String = chars.by_ref().take(2).collect();
                        units.push(u16::from_str_radix(&hex, 16).ok()?);
                    }
                    'u' => {
                        let rest = chars.as_str();
                        if let Some(braced) = rest.strip_prefix('{') {
                            let end = braced.find('}')?;
                            let code = u32::from_str_radix(&braced[..end], 16).ok()?;
                            units
                                .extend_from_slice(char::from_u32(code)?.encode_utf16(&mut buffer));
                            chars = braced[end + 1..].chars();
                        } else {
                            let hex: String = chars.by_ref().take(4).collect();
                            units.push(u16::from_str_radix(&hex, 16).ok()?);
                        }
                    }
                    // A line continuation.
                    '\n' => {}
                    other => units.extend_from_slice(other.encode_utf16(&mut buffer)),
                }
            }
            other => units.extend_from_slice(other.encode_utf16(&mut buffer)),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_escapes_of_a_chunk_literal() {
        let source = r#"push([[1],{2(a){a.exports=JSON.parse('{"a":["A\xe7\u0131k \'x\' \\n \uD83D\uDE00 \u{1F600}"]}')}}]);"#;
        let json = catalog_json(source).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        // `\\n` in the literal is JSON's `\n`.
        assert_eq!(value["a"][0], "A\u{e7}\u{131}k 'x' \n \u{1f600} \u{1f600}");
    }
}
