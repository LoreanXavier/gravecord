//! Pair kerning from a font's GPOS table. The browser shapes a name with the font's `kern` feature on (CSS keeps kerning
//! when `letter-spacing` is set), while the text system lays a letter-spaced name out one cluster at a time, so the
//! styled-name layout applies the pairs itself: lookup type 2 (pair adjustment, formats 1 and 2), also behind a type 9
//! extension, reading the first glyph's x advance change.

use std::sync::Arc;

/// The pair-adjustment subtables of the `kern` feature, one list per lookup, in lookup order.
pub struct Kerning {
    table: Arc<Vec<u8>>,
    lookups: Vec<Vec<usize>>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn i16_at(data: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The index of `glyph` in a coverage table, if it is covered.
fn coverage_index(data: &[u8], table: usize, glyph: u16) -> Option<usize> {
    let count = usize::from(u16_at(data, table + 2)?);
    match u16_at(data, table)? {
        1 => {
            let (mut low, mut high) = (0, count);
            while low < high {
                let middle = (low + high) / 2;
                let value = u16_at(data, table + 4 + 2 * middle)?;
                match value.cmp(&glyph) {
                    std::cmp::Ordering::Equal => return Some(middle),
                    std::cmp::Ordering::Less => low = middle + 1,
                    std::cmp::Ordering::Greater => high = middle,
                }
            }
            None
        }
        2 => {
            for range in 0..count {
                let at = table + 4 + 6 * range;
                let (start, end) = (u16_at(data, at)?, u16_at(data, at + 2)?);
                if (start..=end).contains(&glyph) {
                    return Some(usize::from(u16_at(data, at + 4)?) + usize::from(glyph - start));
                }
            }
            None
        }
        _ => None,
    }
}

/// The class of `glyph` in a class definition table (0 when it is not listed).
fn class_of(data: &[u8], table: usize, glyph: u16) -> Option<u16> {
    match u16_at(data, table)? {
        1 => {
            let start = u16_at(data, table + 2)?;
            let count = u16_at(data, table + 4)?;
            if glyph >= start && glyph - start < count {
                u16_at(data, table + 6 + 2 * usize::from(glyph - start))
            } else {
                Some(0)
            }
        }
        2 => {
            let count = usize::from(u16_at(data, table + 2)?);
            for range in 0..count {
                let at = table + 4 + 6 * range;
                let (start, end) = (u16_at(data, at)?, u16_at(data, at + 2)?);
                if (start..=end).contains(&glyph) {
                    return u16_at(data, at + 4);
                }
            }
            Some(0)
        }
        _ => None,
    }
}

/// Bytes of one value record with these format bits (two per set bit among the eight value fields).
fn record_size(format: u16) -> usize {
    2 * (format & 0x00FF).count_ones() as usize
}

/// The x advance inside a value record of `format` at `at`, 0 when the record has none.
fn x_advance(data: &[u8], at: usize, format: u16) -> Option<i16> {
    if format & 0x4 == 0 {
        return Some(0);
    }
    let before = (format & 0x3).count_ones() as usize;
    i16_at(data, at + 2 * before)
}

impl Kerning {
    /// Reads the `kern` feature's pair lookups out of a GPOS table; no table or no such feature is no kerning.
    pub fn new(gpos: &[u8]) -> Self {
        Self {
            table: Arc::new(gpos.to_vec()),
            lookups: Self::collect(gpos).unwrap_or_default(),
        }
    }

    fn collect(data: &[u8]) -> Option<Vec<Vec<usize>>> {
        let features = usize::from(u16_at(data, 6)?);
        let lookup_list = usize::from(u16_at(data, 8)?);
        let mut wanted: Vec<u16> = Vec::new();
        let feature_count = usize::from(u16_at(data, features)?);
        for record in 0..feature_count {
            let at = features + 2 + 6 * record;
            if data.get(at..at + 4)? != b"kern" {
                continue;
            }
            let feature = features + usize::from(u16_at(data, at + 4)?);
            let count = usize::from(u16_at(data, feature + 2)?);
            for index in 0..count {
                let lookup = u16_at(data, feature + 4 + 2 * index)?;
                if !wanted.contains(&lookup) {
                    wanted.push(lookup);
                }
            }
        }
        wanted.sort_unstable();
        let mut lookups = Vec::new();
        for index in wanted {
            let lookup =
                lookup_list + usize::from(u16_at(data, lookup_list + 2 + 2 * usize::from(index))?);
            let kind = u16_at(data, lookup)?;
            let count = usize::from(u16_at(data, lookup + 4)?);
            let mut subtables = Vec::new();
            for sub in 0..count {
                let at = lookup + usize::from(u16_at(data, lookup + 6 + 2 * sub)?);
                match kind {
                    2 => subtables.push(at),
                    9 if u16_at(data, at + 2)? == 2 => {
                        subtables.push(at + u32_at(data, at + 4)? as usize);
                    }
                    _ => {}
                }
            }
            if !subtables.is_empty() {
                lookups.push(subtables);
            }
        }
        Some(lookups)
    }

    /// How much the advance of `left` changes when `right` follows it, in font units.
    pub fn pair(&self, left: u16, right: u16) -> i32 {
        let mut total = 0;
        for subtables in &self.lookups {
            for subtable in subtables {
                if let Some(Some(value)) = self.subtable_pair(*subtable, left, right) {
                    total += i32::from(value);
                    break;
                }
            }
        }
        total
    }

    /// `Some(None)` when the subtable does not cover the pair, so the next one is tried.
    fn subtable_pair(&self, at: usize, left: u16, right: u16) -> Option<Option<i16>> {
        let data = self.table.as_slice();
        let format = u16_at(data, at)?;
        let index = coverage_index(data, at + usize::from(u16_at(data, at + 2)?), left);
        let Some(index) = index else {
            return Some(None);
        };
        let (first, second) = (u16_at(data, at + 4)?, u16_at(data, at + 6)?);
        let record = record_size(first) + record_size(second);
        match format {
            1 => {
                let set = at + usize::from(u16_at(data, at + 10 + 2 * index)?);
                let count = usize::from(u16_at(data, set)?);
                for pair in 0..count {
                    let entry = set + 2 + (2 + record) * pair;
                    if u16_at(data, entry)? == right {
                        return Some(Some(x_advance(data, entry + 2, first)?));
                    }
                }
                Some(None)
            }
            2 => {
                let class_one = usize::from(class_of(
                    data,
                    at + usize::from(u16_at(data, at + 8)?),
                    left,
                )?);
                let class_two = usize::from(class_of(
                    data,
                    at + usize::from(u16_at(data, at + 10)?),
                    right,
                )?);
                let columns = usize::from(u16_at(data, at + 14)?);
                let entry = at + 16 + record * (class_one * columns + class_two);
                Some(Some(x_advance(data, entry, first)?))
            }
            _ => Some(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A format 1 pair table by hand: glyph 5 followed by glyph 9 closes up by 40 units.
    #[test]
    fn a_hand_built_pair_table_kerns() {
        let mut gpos = vec![0u8; 0];
        let push16 = |bytes: &mut Vec<u8>, value: u16| bytes.extend(value.to_be_bytes());
        // Header: version 1.0, script list 0, feature list at 10, lookup list at 24.
        push16(&mut gpos, 1);
        push16(&mut gpos, 0);
        push16(&mut gpos, 0);
        push16(&mut gpos, 10);
        push16(&mut gpos, 24);
        // Feature list at 10: one record `kern` -> feature at +8 (offset 8 from the list), whose table follows.
        push16(&mut gpos, 1);
        gpos.extend(b"kern");
        push16(&mut gpos, 8);
        // Feature table at 18: no params, one lookup (index 0).
        push16(&mut gpos, 0);
        push16(&mut gpos, 1);
        push16(&mut gpos, 0);
        // Lookup list at 24: one lookup at +4.
        push16(&mut gpos, 1);
        push16(&mut gpos, 4);
        // Lookup at 28: type 2, flag 0, one subtable at +8.
        push16(&mut gpos, 2);
        push16(&mut gpos, 0);
        push16(&mut gpos, 1);
        push16(&mut gpos, 8);
        // Subtable at 36: format 1, coverage at +14, value format 4 (x advance), none, one pair set at +12.
        push16(&mut gpos, 1);
        push16(&mut gpos, 18);
        push16(&mut gpos, 4);
        push16(&mut gpos, 0);
        push16(&mut gpos, 1);
        push16(&mut gpos, 12);
        // The pair set follows the subtable header (offset 48), then the coverage (offset 54).
        let mut table = gpos;
        // Pair set (48): one record, glyph 9, value -40.
        push16(&mut table, 1);
        push16(&mut table, 9);
        table.extend((-40i16).to_be_bytes());
        // Coverage at 36+18 = 54: format 1, one glyph, 5.
        push16(&mut table, 1);
        push16(&mut table, 1);
        push16(&mut table, 5);
        let kerning = Kerning::new(&table);
        assert_eq!(kerning.pair(5, 9), -40);
        assert_eq!(kerning.pair(5, 8), 0);
        assert_eq!(kerning.pair(6, 9), 0);
    }

    /// The captured fonts' names are narrower than their plain advances by the kerning: the widths measured in the
    /// official client's DOM dumps are the reference ("onugeccen" at 16px with the class's letter-spacing).
    #[test]
    fn kerning_matches_the_widths_the_official_client_measured() {
        let Some(cache) = std::env::var_os("LOCALAPPDATA").map(|dir| {
            std::path::PathBuf::from(dir)
                .join("gravecord-capture")
                .join("fonts")
        }) else {
            return;
        };
        // (file, plain advances plus letter-spacing at 16px, the width in the DOM dump)
        let cases = [
            ("33d4f12a85e1f736", 82.35, 80.28),
            ("69c735ca5c604de7", 88.13, 86.22),
            ("8f20cb550d739cea", 79.26, 77.58),
            ("4841dace333e9054", 86.10, 84.69),
            ("e6f5f44abb520735", 64.78, 63.98),
        ];
        for (hash, plain, measured) in cases {
            let Ok(data) = std::fs::read(cache.join(format!("{hash}.ttf"))) else {
                continue;
            };
            let face = ttf_parser::Face::parse(&data, 0).unwrap();
            let gpos = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"GPOS"))
                .unwrap();
            let kerning = Kerning::new(gpos);
            let glyphs: Vec<u16> = "onugeccen"
                .chars()
                .map(|c| face.glyph_index(c).unwrap().0)
                .collect();
            let units: i32 = glyphs
                .windows(2)
                .map(|pair| kerning.pair(pair[0], pair[1]))
                .sum();
            let width = plain + units as f64 * 16. / f64::from(face.units_per_em());
            assert!(
                (width - measured).abs() < 0.05,
                "{hash}: {width} against {measured}"
            );
        }
    }
}
