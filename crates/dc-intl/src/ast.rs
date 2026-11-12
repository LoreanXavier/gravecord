//! A message as the catalogs hold it: the compressed FormatJS AST of module 724707 (`compressFormatJsToAst`). A message
//! is a string, or an array whose items are strings (literals) or nodes `[type, ...]` with the types of
//! `FormatJsNodeType`: Argument 1, Number 2, Date 3, Time 4, Select 5, Plural 6, Pound 7, Tag 8.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A literal, `"text"`.
    Text(Box<str>),
    /// `[1, name]`.
    Argument(Box<str>),
    /// `[2, name, style]`.
    Number(Box<str>, Option<Box<str>>),
    /// `[3, name, style]`.
    Date(Box<str>, Option<Box<str>>),
    /// `[4, name, style]`.
    Time(Box<str>, Option<Box<str>>),
    /// `[5, name, {option: nodes}]`, options in catalog order.
    Select(Box<str>, Vec<(Box<str>, Vec<Node>)>),
    /// `[6, name, {option: nodes}, offset, "cardinal" | "ordinal"]`.
    Plural {
        name: Box<str>,
        options: Vec<(Box<str>, Vec<Node>)>,
        offset: f64,
        ordinal: bool,
    },
    /// `[7]`, the `#` of a plural option.
    Pound,
    /// `[8, name, children, control]`: `$b`, `$i`, `$link` ... or a hook name.
    Tag {
        name: Box<str>,
        children: Vec<Node>,
        control: Option<Vec<Node>>,
    },
}

/// The nodes of a message, from its catalog value. `None` for a value of another shape.
pub fn parse(value: &Value) -> Option<Vec<Node>> {
    match value {
        Value::String(text) => Some(vec![Node::Text(text.as_str().into())]),
        Value::Array(items) => items.iter().map(node).collect(),
        _ => None,
    }
}

fn name_of(items: &[Value], index: usize) -> Option<Box<str>> {
    Some(items.get(index)?.as_str()?.into())
}

fn style_of(items: &[Value]) -> Option<Box<str>> {
    items.get(2).and_then(Value::as_str).map(Into::into)
}

fn options_of(value: Option<&Value>) -> Option<Vec<(Box<str>, Vec<Node>)>> {
    value?
        .as_object()?
        .iter()
        .map(|(option, nodes)| Some((option.as_str().into(), parse(nodes)?)))
        .collect()
}

fn node(value: &Value) -> Option<Node> {
    let items = match value {
        Value::String(text) => return Some(Node::Text(text.as_str().into())),
        Value::Array(items) => items,
        _ => return None,
    };
    let kind = items.first()?.as_u64()?;
    Some(match kind {
        1 => Node::Argument(name_of(items, 1)?),
        2 => Node::Number(name_of(items, 1)?, style_of(items)),
        3 => Node::Date(name_of(items, 1)?, style_of(items)),
        4 => Node::Time(name_of(items, 1)?, style_of(items)),
        5 => Node::Select(name_of(items, 1)?, options_of(items.get(2))?),
        6 => Node::Plural {
            name: name_of(items, 1)?,
            options: options_of(items.get(2))?,
            offset: items.get(3).and_then(Value::as_f64).unwrap_or(0.0),
            ordinal: items.get(4).and_then(Value::as_str) == Some("ordinal"),
        },
        7 => Node::Pound,
        8 => Node::Tag {
            name: name_of(items, 1)?,
            children: parse(items.get(2)?)?,
            control: match items.get(3) {
                None | Some(Value::Null) => None,
                Some(control) => Some(parse(control)?),
            },
        },
        _ => return None,
    })
}

/// The text of a message made of literals only.
pub fn literal(nodes: &[Node]) -> Option<String> {
    let mut text = String::new();
    for node in nodes {
        let Node::Text(part) = node else {
            return None;
        };
        text.push_str(part);
    }
    Some(text)
}

/// Whether formatting the message reads no value: literals and `$` rich-text tags around literals.
pub fn needs_no_values(nodes: &[Node]) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(_) => true,
        Node::Tag {
            name,
            children,
            control,
        } => {
            name.starts_with('$')
                && needs_no_values(children)
                && control.as_deref().is_none_or(needs_no_values)
        }
        _ => false,
    })
}
