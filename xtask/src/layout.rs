//! The layout-tree diff: every `parity(id)` box against the official element in the DOM dump.

use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// CSS pixels are fractional; a difference under this is float noise, not layout.
const EPSILON: f64 = 0.51;

#[derive(Debug, PartialEq)]
pub struct Mismatch {
    pub id: String,
    pub ours: [f64; 4],
    pub official: Option<[f64; 4]>,
}

struct Dom<'a> {
    strings: &'a [Value],
    document: &'a Value,
}

impl Dom<'_> {
    fn text(&self, index: i64) -> &str {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index))
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn class_of(&self, node: usize) -> &str {
        let attributes = self.document["nodes"]["attributes"][node].as_array();
        for pair in attributes
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .chunks(2)
        {
            if let [name, value] = pair
                && self.text(name.as_i64().unwrap_or(-1)) == "class"
            {
                return self.text(value.as_i64().unwrap_or(-1));
            }
        }
        ""
    }

    fn tag_of(&self, node: usize) -> &str {
        self.text(
            self.document["nodes"]["nodeName"][node]
                .as_i64()
                .unwrap_or(-1),
        )
    }

    /// The first laid out element whose class list starts a class with `prefix`.
    fn find(&self, prefix: &str, tag: Option<&str>, ancestor: Option<&str>) -> Option<[f64; 4]> {
        let nodes = self.document["layout"]["nodeIndex"].as_array()?;
        let bounds = self.document["layout"]["bounds"].as_array()?;
        for (row, node) in nodes.iter().enumerate() {
            let node = node.as_u64()? as usize;
            let class_matches = self
                .class_of(node)
                .split(' ')
                .any(|class| class.starts_with(prefix));
            if class_matches
                && tag.is_none_or(|tag| self.tag_of(node) == tag)
                && ancestor.is_none_or(|prefix| self.has_ancestor(node, prefix))
            {
                let values: Vec<f64> = bounds[row]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_f64)
                    .collect();
                return values.try_into().ok();
            }
        }
        None
    }

    fn has_ancestor(&self, mut node: usize, prefix: &str) -> bool {
        let Some(parents) = self.document["nodes"]["parentIndex"].as_array() else {
            return false;
        };
        for _ in 0..parents.len() {
            let Some(parent) = parents
                .get(node)
                .and_then(Value::as_i64)
                .and_then(|index| usize::try_from(index).ok())
            else {
                return false;
            };
            node = parent;
            if self
                .class_of(node)
                .split(' ')
                .any(|class| class.starts_with(prefix))
            {
                return true;
            }
        }
        false
    }
}

fn differs(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a.iter().zip(b).any(|(a, b)| (a - b).abs() > EPSILON)
}

pub fn compare(
    dom_path: &Path,
    map_path: &Path,
    ours_path: &Path,
) -> Result<(usize, Vec<Mismatch>)> {
    let dump: Value = serde_json::from_slice(&fs::read(dom_path).context("reading the DOM dump")?)?;
    let map: Value =
        serde_json::from_slice(&fs::read(map_path).context("reading the parity map")?)?;
    let ours: Value = serde_json::from_slice(&fs::read(ours_path).context("reading our layout")?)?;
    let dom = Dom {
        strings: dump["snapshot"]["strings"]
            .as_array()
            .context("no strings")?,
        document: &dump["snapshot"]["documents"][0],
    };
    let mut mismatches = Vec::new();
    let entries = map.as_object().context("map is not an object")?;
    for (id, target) in entries {
        let values: Vec<f64> = ours[id]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        let Ok(mine): Result<[f64; 4], _> = values.try_into() else {
            mismatches.push(Mismatch {
                id: format!("{id} (no parity probe)"),
                ours: [0.; 4],
                official: None,
            });
            continue;
        };
        let official = dom.find(
            target["class"].as_str().unwrap_or_default(),
            target["tag"].as_str(),
            target["ancestor"].as_str(),
        );
        if official.is_none_or(|official| differs(&mine, &official)) {
            mismatches.push(Mismatch {
                id: id.clone(),
                ours: mine,
                official,
            });
        }
    }
    Ok((entries.len(), mismatches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dom() -> Value {
        json!({
            "snapshot": {
                "strings": ["class", "foo_abc other", "DIV", "A", "bar_1"],
                "documents": [{
                    "nodes": { "attributes": [[0, 1], [0, 4], [0, 4]], "nodeName": [2, 2, 3], "parentIndex": [-1, -1, 0] },
                    "layout": { "nodeIndex": [0, 1, 2], "bounds": [[1, 2, 3, 4], [5, 6, 7, 8], [9, 9, 9, 9]] },
                }],
            },
        })
    }

    fn with_dom<T>(check: impl FnOnce(&Dom<'_>) -> T) -> T {
        let dump = dom();
        let dom = Dom {
            strings: dump["snapshot"]["strings"].as_array().unwrap(),
            document: &dump["snapshot"]["documents"][0],
        };
        check(&dom)
    }

    #[test]
    fn finds_by_class_prefix() {
        assert_eq!(
            with_dom(|dom| dom.find("foo_", None, None)),
            Some([1., 2., 3., 4.])
        );
        assert_eq!(
            with_dom(|dom| dom.find("other", None, None)),
            Some([1., 2., 3., 4.])
        );
        assert_eq!(with_dom(|dom| dom.find("nope", None, None)), None);
    }

    #[test]
    fn tag_narrows_the_match() {
        assert_eq!(
            with_dom(|dom| dom.find("bar_", Some("A"), None)),
            Some([9., 9., 9., 9.])
        );
    }

    #[test]
    fn ancestor_disambiguates_reused_text_classes() {
        assert_eq!(
            with_dom(|dom| dom.find("bar_", None, Some("foo_"))),
            Some([9., 9., 9., 9.])
        );
        assert_eq!(
            with_dom(|dom| dom.find("bar_", None, Some("missing"))),
            None
        );
    }

    #[test]
    fn half_pixel_noise_is_not_a_difference() {
        assert!(!differs(&[1., 2., 3., 4.], &[1.5, 2., 3., 4.]));
        assert!(differs(&[1., 2., 3., 4.], &[2., 2., 3., 4.]));
    }
}
