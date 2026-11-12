//! The official formatter, ported: `bindFormatValuesWithBuilder` of module 133275 driving the `StringBuilder` of module
//! 138259 (`formatToPlainString`), and `InternalIntlMessage.reserialize` of module 251277 (`intl.string`).

use crate::ast::Node;
use crate::locale_data::Formatters;
use std::borrow::Cow;
use std::fmt::Write as _;

/// A value for a message's placeholder.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg<'a> {
    Str(Cow<'a, str>),
    Num(f64),
    /// A moment for Date and Time nodes, written by the caller (`Intl.DateTimeFormat` is not ported): a Date node
    /// prints `date`, a Time node `time`, anything else `date`.
    When {
        date: Cow<'a, str>,
        time: Cow<'a, str>,
    },
}

impl Arg<'_> {
    /// JavaScript's `String(value)`: what an Argument node prints and what Select and Plural compare option keys with.
    fn js_string(&self) -> Cow<'_, str> {
        match self {
            Arg::Str(text) | Arg::When { date: text, .. } => Cow::Borrowed(text),
            Arg::Num(number) => Cow::Owned(js_number(*number)),
        }
    }

    /// JavaScript's `Number(value)`, as Plural and Number nodes take it.
    fn number(&self) -> Option<f64> {
        match self {
            Arg::Num(number) => Some(*number),
            Arg::Str(text) => text.trim().parse().ok(),
            Arg::When { .. } => None,
        }
    }
}

/// `Number.prototype.toString` for the values messages are given: integers print without a fraction.
pub(crate) fn js_number(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned();
    }
    if number == number.trunc() && number.abs() < 1e21 {
        return format!("{}", number as i128);
    }
    format!("{number}")
}

macro_rules! arg_from_number {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Arg<'_> {
            fn from(value: $ty) -> Self {
                Arg::Num(value as f64)
            }
        }
    )*};
}
arg_from_number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

/// `String(true)`: a select over a boolean picks its `true` / `false` option.
impl From<bool> for Arg<'_> {
    fn from(value: bool) -> Self {
        Arg::Str(Cow::Borrowed(if value { "true" } else { "false" }))
    }
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Arg::Str(Cow::Borrowed(value))
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(value: &'a String) -> Self {
        Arg::Str(Cow::Borrowed(value))
    }
}

impl From<String> for Arg<'_> {
    fn from(value: String) -> Self {
        Arg::Str(Cow::Owned(value))
    }
}

impl<'a> From<Cow<'a, str>> for Arg<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        Arg::Str(value)
    }
}

pub type Args<'a> = [(&'a str, Arg<'a>)];

fn value_of<'v>(values: &'v Args<'v>, name: &str) -> Option<&'v Arg<'v>> {
    values
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| value)
}

fn missing(name: &str) {
    // Module 133275 throws "No value for variable '<name>' was provided for the localized message".
    crate::debug_once(|| format!("no value for variable '{name}'"));
}

/// `formatToPlainString`: literals and values, rich-text tags reduced to their children.
pub fn plain(nodes: &[Node], values: &Args<'_>, formatters: &Formatters) -> String {
    let mut out = String::new();
    bind(&mut out, nodes, values, formatters, None);
    out
}

fn bind(
    out: &mut String,
    nodes: &[Node],
    values: &Args<'_>,
    formatters: &Formatters,
    plural_value: Option<f64>,
) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Pound => {
                if let Some(value) = plural_value {
                    out.push_str(&formatters.number(value, None));
                }
            }
            Node::Argument(name) => match value_of(values, name) {
                Some(value) => out.push_str(&value.js_string()),
                None => missing(name),
            },
            Node::Number(name, style) => match value_of(values, name) {
                Some(value) => match value.number() {
                    // `integer` of DEFAULT_FORMAT_CONFIG (module 333519): `maximumFractionDigits: 0`.
                    Some(number) => {
                        let fraction = (style.as_deref() == Some("integer")).then_some(0);
                        out.push_str(&formatters.number(number, fraction));
                    }
                    None => out.push_str(&value.js_string()),
                },
                None => missing(name),
            },
            Node::Date(name, _) | Node::Time(name, _) => match value_of(values, name) {
                // Intl.DateTimeFormat is not ported: the caller passes the date already written.
                Some(Arg::When { time, .. }) if matches!(node, Node::Time(..)) => {
                    out.push_str(time)
                }
                Some(value) => out.push_str(&value.js_string()),
                None => missing(name),
            },
            Node::Tag { name, children, .. } => {
                // A `$` tag is a rich-text tag: the string builder pushes its children. A hook (a name without `$`)
                // is a function of the caller's in the official client; here its children stand for its result.
                if !name.starts_with('$') && value_of(values, name).is_none() {
                    crate::debug_once(|| format!("tag hook '{name}' formatted as its children"));
                }
                bind(out, children, values, formatters, plural_value);
            }
            Node::Select(name, options) => {
                let Some(value) = value_of(values, name) else {
                    missing(name);
                    continue;
                };
                let key = value.js_string();
                let chosen = options
                    .iter()
                    .find(|(option, _)| **option == *key)
                    .or_else(|| options.iter().find(|(option, _)| &**option == "other"));
                // The official call passes no `currentPluralValue` into a select's option.
                if let Some((_, nodes)) = chosen {
                    bind(out, nodes, values, formatters, None);
                }
            }
            Node::Plural {
                name,
                options,
                offset,
                ordinal,
            } => {
                let Some(value) = value_of(values, name) else {
                    missing(name);
                    continue;
                };
                let exact = format!("={}", value.js_string());
                let number = value.number().unwrap_or(f64::NAN);
                let shifted = number - offset;
                let chosen = options
                    .iter()
                    .find(|(option, _)| **option == *exact)
                    .or_else(|| {
                        let category = formatters.plural_category(shifted, *ordinal);
                        options.iter().find(|(option, _)| &**option == category)
                    })
                    .or_else(|| options.iter().find(|(option, _)| &**option == "other"));
                if let Some((_, nodes)) = chosen {
                    bind(out, nodes, values, formatters, Some(shifted));
                }
            }
        }
    }
}

/// `formatToPlainString` keeping where the rich-text tags were: the text in runs, each with the innermost tag around
/// it (`$b` for bold, `$link`, ...), for callers that style them as the React builder does.
pub fn runs(
    nodes: &[Node],
    values: &Args<'_>,
    formatters: &Formatters,
) -> Vec<(String, Option<Box<str>>)> {
    let mut out: Vec<(String, Option<Box<str>>)> = Vec::new();
    collect_runs(&mut out, nodes, values, formatters, None);
    out.retain(|(text, _)| !text.is_empty());
    out
}

fn collect_runs(
    out: &mut Vec<(String, Option<Box<str>>)>,
    nodes: &[Node],
    values: &Args<'_>,
    formatters: &Formatters,
    tag: Option<&str>,
) {
    for node in nodes {
        match node {
            // A hook (a tag without `$`) given a value prints that value in its run: the caller's stand-in for the
            // element the official hook returns.
            Node::Tag { name, .. }
                if !name.starts_with('$') && value_of(values, name).is_some() =>
            {
                if let Some(value) = value_of(values, name) {
                    out.push((value.js_string().into_owned(), Some(name.clone())));
                }
            }
            Node::Tag { name, children, .. } => {
                collect_runs(out, children, values, formatters, Some(&**name));
            }
            other => {
                let mut text = String::new();
                bind(
                    &mut text,
                    std::slice::from_ref(other),
                    values,
                    formatters,
                    None,
                );
                match out.last_mut() {
                    Some((last, last_tag)) if last_tag.as_deref() == tag => last.push_str(&text),
                    _ => out.push((text, tag.map(Into::into))),
                }
            }
        }
    }
}

/// `InternalIntlMessage.reserialize`: the message back as ICU source, its rich-text tags as markdown.
pub fn reserialize(nodes: &[Node]) -> String {
    let mut out = String::new();
    write_nodes(&mut out, nodes);
    out
}

fn write_nodes(out: &mut String, nodes: &[Node]) {
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Argument(name) => {
                let _ = write!(out, "{{{name}}}");
            }
            Node::Date(name, style) => write_typed(out, name, "date", style.as_deref()),
            Node::Time(name, style) => write_typed(out, name, "time", style.as_deref()),
            Node::Number(name, style) => write_typed(out, name, "number", style.as_deref()),
            Node::Plural {
                name,
                options,
                offset,
                ordinal,
            } => {
                let kind = if *ordinal { "selectordinal" } else { "plural" };
                let _ = write!(out, "{{{name}, {kind},");
                if *offset != 0.0 {
                    let _ = write!(out, " offset:{}", js_number(*offset));
                }
                write_options(out, options);
            }
            Node::Pound => out.push('#'),
            Node::Select(name, options) => {
                let _ = write!(out, "{{{name}, select,");
                write_options(out, options);
            }
            Node::Tag {
                name,
                children,
                control,
            } => match &**name {
                "$b" => wrap(out, "**", children, "**"),
                "$i" => wrap(out, "*", children, "*"),
                "$code" => wrap(out, "`", children, "`"),
                "$p" => wrap(out, "", children, "\n\n"),
                "$link" => {
                    out.push('[');
                    write_nodes(out, children);
                    out.push_str("](");
                    if let Some(control) = control {
                        write_nodes(out, control);
                    }
                    out.push(')');
                }
                _ => {
                    out.push_str("$[");
                    write_nodes(out, children);
                    let _ = write!(out, "]({name})");
                }
            },
        }
    }
}

fn write_typed(out: &mut String, name: &str, kind: &str, style: Option<&str>) {
    let _ = write!(out, "{{{name}, {kind}");
    if let Some(style) = style {
        let _ = write!(out, ", {style}");
    }
    out.push('}');
}

fn write_options(out: &mut String, options: &[(Box<str>, Vec<Node>)]) {
    for (option, nodes) in options {
        let _ = write!(out, " {option} {{");
        write_nodes(out, nodes);
        out.push('}');
    }
    out.push('}');
}

fn wrap(out: &mut String, open: &str, children: &[Node], close: &str) {
    out.push_str(open);
    write_nodes(out, children);
    out.push_str(close);
}
