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
        self.attribute_of(node, "class")
    }

    fn attribute_of(&self, node: usize, attribute: &str) -> &str {
        let attributes = self.document["nodes"]["attributes"][node].as_array();
        for pair in attributes
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .chunks(2)
        {
            if let [name, value] = pair
                && self.text(name.as_i64().unwrap_or(-1)) == attribute
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
    fn find(
        &self,
        prefix: &str,
        tag: Option<&str>,
        ancestor: Option<&str>,
        ancestor_id: Option<&str>,
        attribute: Option<(&str, &str)>,
    ) -> Option<[f64; 4]> {
        let nodes = self.document["layout"]["nodeIndex"].as_array()?;
        let bounds = self.document["layout"]["bounds"].as_array()?;
        for (row, node) in nodes.iter().enumerate() {
            let node = node.as_u64()? as usize;
            let class_matches = self
                .class_of(node)
                .split(' ')
                .any(|class| class.starts_with(prefix));
            if class_matches
                && attribute.is_none_or(|(name, value)| self.attribute_of(node, name) == value)
                && tag.is_none_or(|tag| self.tag_of(node) == tag)
                && ancestor.is_none_or(|prefix| self.has_ancestor(node, prefix, false))
                && ancestor_id.is_none_or(|id| self.has_ancestor(node, id, true))
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

    fn has_ancestor(&self, mut node: usize, value: &str, by_id: bool) -> bool {
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
            if if by_id {
                self.attribute_of(node, "id") == value
            } else {
                self.class_of(node)
                    .split(' ')
                    .any(|class| class.starts_with(value))
            } {
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
            target["ancestor_id"].as_str(),
            target["attribute"].as_str().zip(target["value"].as_str()),
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
            with_dom(|dom| dom.find("foo_", None, None, None, None)),
            Some([1., 2., 3., 4.])
        );
        assert_eq!(
            with_dom(|dom| dom.find("other", None, None, None, None)),
            Some([1., 2., 3., 4.])
        );
        assert_eq!(
            with_dom(|dom| dom.find("nope", None, None, None, None)),
            None
        );
    }

    #[test]
    fn tag_narrows_the_match() {
        assert_eq!(
            with_dom(|dom| dom.find("bar_", Some("A"), None, None, None)),
            Some([9., 9., 9., 9.])
        );
    }

    #[test]
    fn ancestor_disambiguates_reused_text_classes() {
        assert_eq!(
            with_dom(|dom| dom.find("bar_", None, Some("foo_"), None, None)),
            Some([9., 9., 9., 9.])
        );
        assert_eq!(
            with_dom(|dom| dom.find("bar_", None, Some("missing"), None, None)),
            None
        );
    }

    #[test]
    fn ancestor_id_selects_the_right_group_with_reused_classes() {
        let dump = json!({"strings":["class","foo_abc","DIV","A","bar_1","id","target-extra","target"],
            "nodes":{"attributes":[[0,1,5,6],[0,4],[0,4],[0,1,5,7],[0,4]],
                "nodeName":[2,2,3,2,3],"parentIndex":[-1,-1,0,-1,3]},
            "layout":{"nodeIndex":[0,1,2,3,4],"bounds":[[1,2,3,4],[5,6,7,8],[9,9,9,9],[20,20,20,20],[31,32,33,34]]}
        });
        let dom = Dom {
            strings: dump["strings"].as_array().unwrap(),
            document: &dump,
        };
        assert_eq!(
            dom.find("bar_", Some("A"), Some("foo_"), Some("target"), None),
            Some([31., 32., 33., 34.])
        );
        assert_eq!(dom.find("bar_", None, None, Some("missing"), None), None);
    }

    #[test]
    fn half_pixel_noise_is_not_a_difference() {
        assert!(!differs(&[1., 2., 3., 4.], &[1.5, 2., 3., 4.]));
        assert!(differs(&[1., 2., 3., 4.], &[2., 2., 3., 4.]));
    }

    #[test]
    fn accessible_name_disambiguates_identical_button_classes() {
        let dump = json!({"strings":["class","button_abc","BUTTON","aria-label","Listen Along"],
            "nodes":{"attributes":[[0,1],[0,1,3,4]],"nodeName":[2,2],"parentIndex":[-1,-1]},
            "layout":{"nodeIndex":[0,1],"bounds":[[730,552,212,32],[950,552,32,32]]}});
        let dom = Dom {
            strings: dump["strings"].as_array().unwrap(),
            document: &dump,
        };
        assert_eq!(
            dom.find(
                "button_",
                Some("BUTTON"),
                None,
                None,
                Some(("aria-label", "Listen Along"))
            ),
            Some([950., 552., 32., 32.])
        );
        assert!(
            dom.find("button_", None, None, None, Some(("aria-label", "Missing")))
                .is_none()
        );
    }
}
