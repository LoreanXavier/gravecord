//! The interface languages: the official message catalogs, the current locale, and the official formatter.
//!
//! The official client (`@discord/intl`, module 95498) keeps one `MessageLoader` per catalog (module 802249 is the main
//! one; 94 loaders in all, the others per feature, their keys unique across them) and an `IntlManager` (module 461217)
//! with the current locale. `LocaleStore` (module 773669) takes the locale from the settings proto's
//! `localization.locale` and calls `setLocale` (module 449275 `Av`), which loads the locale's catalog chunk of every
//! loader and re-renders: no restart. A key missing in the locale falls back to en-US, then to an empty message
//! (`MessageLoader.get`, module 965413).
//!
//! Here en-US is compiled in (`data/en-US.json.zst`, from `tools/gen-intl.py`); the other locales are installed at run
//! time from the pinned build's own catalog chunks ([`chunk_files`], [`catalog_json`], [`install`]). Lookups:
//! - [`t`] / `t!("key")`: a message by its catalog key (the hashed key the bundle uses, `t.intl.string(t.t.hPBScv)`),
//!   as plain text.
//! - `t!("key", name = value)` / [`format`]: with placeholders, plurals and selects (`formatToPlainString`).
//! - [`tr`]: a literal en-US text, looked up through the compile-time table of literal en-US messages ([`keys_of`]),
//!   so existing English literals adopt without naming their key.
//! - [`string`]: `intl.string`, the message reserialized (rich-text tags as markdown).

mod ast;
mod chunk;
mod chunks;
mod format;
mod hash;
mod locale_data;
mod reverse;

pub use ast::Node;
pub use chunk::catalog_json;
pub use format::{Arg, Args};
pub use hash::message_key;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

/// The default locale of every loader (`DEFAULT_LOCALE`, module 461217).
pub const DEFAULT_LOCALE: &str = "en-US";

struct Message {
    nodes: Vec<Node>,
    /// The plain text of a message that reads no value, formatted once.
    text: Option<Box<str>>,
}

struct Catalog {
    messages: HashMap<Box<str>, Message>,
}

impl Catalog {
    /// The messages of one or more catalog JSONs (a locale has one per loader), merged.
    fn parse(locale: &str, jsons: &[&str]) -> Result<Catalog, String> {
        let formatters = locale_data::formatters(locale);
        let mut messages = HashMap::new();
        for json in jsons {
            let value: serde_json::Value =
                serde_json::from_str(json).map_err(|error| format!("{locale}: {error}"))?;
            let object = value
                .as_object()
                .ok_or_else(|| format!("{locale}: a catalog is not an object"))?;
            messages.reserve(object.len());
            for (key, value) in object {
                let Some(nodes) = ast::parse(value) else {
                    continue;
                };
                let text = ast::needs_no_values(&nodes)
                    .then(|| format::plain(&nodes, &[], formatters).into_boxed_str());
                messages.insert(key.as_str().into(), Message { nodes, text });
            }
        }
        Ok(Catalog { messages })
    }

    fn get(&self, key: &str) -> Option<&Message> {
        self.messages.get(key)
    }
}

fn en_us() -> &'static Catalog {
    static EN_US: OnceLock<Catalog> = OnceLock::new();
    EN_US.get_or_init(|| {
        let bytes = zstd::decode_all(&include_bytes!("../data/en-US.json.zst")[..])
            .expect("the en-US catalog decompresses");
        let json = String::from_utf8(bytes).expect("the en-US catalog is UTF-8");
        Catalog::parse(DEFAULT_LOCALE, &[&json]).expect("the en-US catalog parses")
    })
}

/// Loads the compiled-in en-US catalog now (it is otherwise read on the first lookup by key).
pub fn preload() {
    en_us();
}

/// Every locale the loaders know, en-US first.
pub fn locales() -> impl Iterator<Item = &'static str> {
    let mut seen: Vec<&'static str> = vec![DEFAULT_LOCALE];
    for (locale, _, _) in chunks::CHUNKS {
        if !seen.contains(locale) {
            seen.push(locale);
        }
    }
    seen.into_iter()
}

fn known(code: &str) -> Option<&'static str> {
    locales().find(|known| *known == code)
}

/// The asset files of the pinned build that hold a locale's catalogs, one per loader, module 802249's first
/// (`MODULE_INDEX.tsv`). Every one of them is loaded on a locale change (module 449275 `Av`,
/// `loadAllMessagesInLocale`).
pub fn chunk_files(code: &str) -> impl Iterator<Item = &'static str> {
    chunks::CHUNKS
        .iter()
        .filter(move |(locale, _, _)| *locale == code)
        .map(|(_, _, file)| *file)
}

struct State {
    locale: &'static str,
    catalog: Option<&'static Catalog>,
}

fn state() -> &'static RwLock<State> {
    static STATE: OnceLock<RwLock<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        RwLock::new(State {
            locale: DEFAULT_LOCALE,
            catalog: None,
        })
    })
}

fn installed() -> &'static RwLock<HashMap<&'static str, &'static Catalog>> {
    static INSTALLED: OnceLock<RwLock<HashMap<&'static str, &'static Catalog>>> = OnceLock::new();
    INSTALLED.get_or_init(Default::default)
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Changes whenever what a lookup returns may have changed: the locale, or its catalog arriving. Views that keep
/// formatted text key it by this.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The current locale (`intl.currentLocale`).
pub fn locale() -> &'static str {
    state().read().map_or(DEFAULT_LOCALE, |state| state.locale)
}

fn current() -> (&'static str, Option<&'static Catalog>) {
    state().read().map_or((DEFAULT_LOCALE, None), |state| {
        (state.locale, state.catalog)
    })
}

/// `intl.setLocale`: a locale the loader does not know falls back to en-US. True when the locale changed.
pub fn set_locale(code: &str) -> bool {
    let code = known(code).unwrap_or(DEFAULT_LOCALE);
    let catalog = installed()
        .read()
        .ok()
        .and_then(|map| map.get(code).copied());
    let Ok(mut state) = state().write() else {
        return false;
    };
    if state.locale == code {
        return false;
    }
    state.locale = code;
    state.catalog = catalog;
    GENERATION.fetch_add(1, Ordering::Relaxed);
    true
}

/// Whether a locale's catalog is ready (en-US always is).
pub fn is_loaded(code: &str) -> bool {
    code == DEFAULT_LOCALE || installed().read().is_ok_and(|map| map.contains_key(code))
}

/// Installs a locale's catalogs from their JSON (what [`catalog_json`] reads out of each chunk). They are kept for the
/// rest of the run, as the official loaders keep every locale they loaded.
pub fn install(code: &str, jsons: &[&str]) -> Result<(), String> {
    let Some(code) = known(code).filter(|code| *code != DEFAULT_LOCALE) else {
        return Err(format!("{code} is not a locale of the loader"));
    };
    if is_loaded(code) {
        return Ok(());
    }
    let catalog: &'static Catalog = Box::leak(Box::new(Catalog::parse(code, jsons)?));
    if let Ok(mut map) = installed().write() {
        map.entry(code).or_insert(catalog);
    }
    if let Ok(mut state) = state().write()
        && state.locale == code
    {
        state.catalog = Some(catalog);
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// `MessageLoader.get`: the locale's message, else en-US's, else none.
fn message(key: &str) -> Option<(&'static str, &'static Message)> {
    let (locale, catalog) = current();
    if let Some(found) = catalog.and_then(|catalog| catalog.get(key)) {
        return Some((locale, found));
    }
    let found = en_us().get(key);
    if found.is_none() {
        debug_once(|| format!("message {key} has no value in {locale} nor in en-US"));
    }
    found.map(|found| (DEFAULT_LOCALE, found))
}

/// A message as plain text, by its catalog key (`formatToPlainString` without values). A message that needs values
/// formats with none (its placeholders print nothing); use [`format`] for those.
pub fn t(key: &str) -> &'static str {
    match message(key) {
        Some((
            _,
            Message {
                text: Some(text), ..
            },
        )) => text,
        Some((locale, message)) => {
            debug_once(|| format!("message {key} needs values, formatted without"));
            leak_cached(locale, key, || {
                format::plain(&message.nodes, &[], locale_data::formatters(locale))
            })
        }
        None => "",
    }
}

fn leak_cached(locale: &'static str, key: &str, make: impl FnOnce() -> String) -> &'static str {
    static CACHE: OnceLock<RwLock<HashMap<(&'static str, String), &'static str>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let id = (locale, key.to_owned());
    if let Some(found) = cache.read().ok().and_then(|map| map.get(&id).copied()) {
        return found;
    }
    let made: &'static str = Box::leak(make().into_boxed_str());
    match cache.write() {
        Ok(mut map) => map.entry(id).or_insert(made),
        Err(_) => made,
    }
}

/// `intl.format` to plain text (`formatToPlainString`): placeholders, numbers, plurals and selects filled from `values`.
pub fn format(key: &str, values: &Args<'_>) -> String {
    match message(key) {
        Some((locale, message)) => {
            format::plain(&message.nodes, values, locale_data::formatters(locale))
        }
        None => String::new(),
    }
}

/// `intl.string`: the message reserialized, rich-text tags as markdown and placeholders as `{name}`.
pub fn string(key: &str) -> Cow<'static, str> {
    match message(key) {
        Some((_, message)) => match (&message.text, ast::literal(&message.nodes)) {
            (Some(text), Some(_)) => Cow::Borrowed(&**text),
            _ => Cow::Owned(format::reserialize(&message.nodes)),
        },
        None => Cow::Borrowed(""),
    }
}

/// A number as `Intl.NumberFormat` writes it in the current locale (`intl.data.formatNumber`).
pub fn number(value: f64) -> String {
    locale_data::formatters(locale()).number(value, None)
}

/// Whether the en-US catalog has the key.
pub fn has_key(key: &str) -> bool {
    en_us().get(key).is_some()
}

/// The en-US text of a key, when it is literal text.
pub fn en_us_text(key: &str) -> Option<&'static str> {
    en_us().get(key).and_then(|message| message.text.as_deref())
}

/// The keys of the literal en-US messages whose text is `text`, in catalog order.
pub fn keys_of(text: &str) -> impl Iterator<Item = &'static str> {
    let table = reverse::REVERSE;
    let start = table.partition_point(|(known, _)| *known < text);
    table[start..]
        .iter()
        .take_while(move |(known, _)| *known == text)
        .map(|(_, key)| *key)
}

/// What [`tr`] does with a literal in a locale.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// No literal en-US message has this text: it stays as written.
    Unknown,
    /// The text of the message (or the literal, when the locale lacks it).
    One(&'a str),
    /// Several keys share the text and their translations differ: the translation most of them have (on a tie, the
    /// earliest key's in catalog order). A call site that means one of them names its key with [`t`] instead.
    Ambiguous(&'a str),
}

fn resolve<'a>(text: &'a str, catalog: Option<&'static Catalog>) -> Resolved<'a> {
    let mut keys = keys_of(text).peekable();
    if keys.peek().is_none() {
        return Resolved::Unknown;
    }
    let translation = |key: &str| -> &'a str {
        catalog
            .and_then(|catalog| catalog.get(key))
            .and_then(|message| message.text.as_deref())
            .unwrap_or(text)
    };
    let translations: Vec<&'a str> = keys.map(translation).collect();
    if translations.iter().all(|found| *found == translations[0]) {
        return Resolved::One(translations[0]);
    }
    // The translation most of the keys share; on a tie, the one of the earliest key.
    let mut best = translations[0];
    let mut best_count = 0;
    for candidate in &translations {
        let count = translations
            .iter()
            .filter(|found| *found == candidate)
            .count();
        if count > best_count {
            best = candidate;
            best_count = count;
        }
    }
    Resolved::Ambiguous(best)
}

/// An en-US literal in the current locale, found through the literal en-US messages with that exact text. The literal
/// stays as written when no message has it (logged once in a debug build) or the locale's catalog is not loaded yet.
pub fn tr(text: &str) -> &str {
    let (locale, catalog) = current();
    if locale == DEFAULT_LOCALE {
        if cfg!(debug_assertions) && keys_of(text).next().is_none() {
            debug_once(|| format!("tr: no en-US message reads {text:?}"));
        }
        return text;
    }
    match resolve(text, catalog) {
        Resolved::Unknown => {
            debug_once(|| format!("tr: no en-US message reads {text:?}"));
            text
        }
        Resolved::One(found) => found,
        Resolved::Ambiguous(found) => {
            debug_once(|| {
                let keys: Vec<_> = keys_of(text).collect();
                format!(
                    "tr: {text:?} is {keys:?}, translated differently in {locale}; using the most common"
                )
            });
            found
        }
    }
}

/// The name of a locale in the current language (module 226536 `getAvailableLocales`, `localizedName`).
pub fn localized_locale_name(code: &str) -> &'static str {
    t(&message_key(code))
}

/// The locale a system language maps to (module 226536 `kt`): itself, its language, `es`/`nb`/`nn` aliases, Chinese
/// Traditional script, else the first available locale of the same language, else `default`.
pub fn match_locale<'a>(tag: &str, available: &[&'a str], default: &'a str) -> &'a str {
    let find = |code: &str| available.iter().copied().find(|known| *known == code);
    if let Some(found) = find(tag) {
        return found;
    }
    let parts: Vec<&str> = tag.split('-').collect();
    if let Some(found) = find(parts[0]) {
        return found;
    }
    if parts.len() == 1 {
        let alias = match parts[0] {
            "es" => Some("es-ES"),
            "nb" | "nn" => Some("no"),
            _ => None,
        };
        if let Some(found) = alias.and_then(find) {
            return found;
        }
    }
    if parts[0] == "zh" && parts.len() > 1 && parts[1] == "Hant" {
        return find("zh-TW").unwrap_or(default);
    }
    available
        .iter()
        .copied()
        .find(|known| known.split('-').next() == Some(parts[0]))
        .unwrap_or(default)
}

/// A line in the log, once per distinct text, in a debug build only.
pub(crate) fn debug_once(make: impl FnOnce() -> String) {
    if !cfg!(debug_assertions) {
        return;
    }
    static SEEN: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let line = make();
    let Ok(mut seen) = SEEN.get_or_init(Default::default).lock() else {
        return;
    };
    if seen.insert(line.clone()) {
        tracing::warn!(target: "dc_intl", "{line}");
    }
}

/// `t!("key")` is [`t`]; `t!("key", name = value, ...)` is [`format`] with those values.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::t($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::format($key, &[$((stringify!($name), $crate::Arg::from($value))),+])
    };
}

#[cfg(test)]
mod tests;
