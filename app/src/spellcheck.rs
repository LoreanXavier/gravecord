//! Spell checking on the system checker (`ISpellCheckerFactory` / `ISpellChecker`), as the desktop client's Chromium
//! does on Windows. The web side is module 334604 (`Spellchecker`: which languages, how a misspelling and its
//! suggestions reach the menu) and module 638128 (`SpellcheckStore`: the switch and the learned words, kept on the
//! device); module 413756 is the menu section, module 787564 decides which text the editor leaves unchecked.
//!
//! COM lives on one worker thread. Callers send jobs and read the answer from a channel, so the interface thread never
//! waits on the checker. A check that is still queued when a newer one arrives is dropped (its channel closes without an
//! answer), which is how a text change cancels the check of the old text.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// Suggestions the menu shows: `uz(e = 5)` of module 334604 slices the cached corrections to five.
pub const MAX_SUGGESTIONS: usize = 5;

/// Key of the device-local record, `SpellcheckStore` of module 638128 (`a.w.set("SpellcheckStore", ...)`).
const STORE_KEY: &str = "SpellcheckStore";

// ---------------------------------------------------------------------------------------------------------------
// The store: `SpellcheckStore` (module 638128). The switch and the learned words persist on the device.
// ---------------------------------------------------------------------------------------------------------------

struct Store {
    enabled: bool,
    learned: HashSet<String>,
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        let saved = crate::storage::get(STORE_KEY);
        let enabled = saved
            .as_ref()
            .and_then(|value| value.get("enabled"))
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let learned = saved
            .as_ref()
            .and_then(|value| value.get("learnedWords"))
            .and_then(|value| value.as_array())
            .map(|words| {
                words
                    .iter()
                    .filter_map(|word| word.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Mutex::new(Store { enabled, learned })
    })
}

fn save(store: &Store) {
    let mut words: Vec<&String> = store.learned.iter().collect();
    words.sort();
    crate::storage::set(
        STORE_KEY,
        serde_json::json!({"enabled": store.enabled, "learnedWords": words}),
    );
}

/// `isEnabled`.
pub fn is_enabled() -> bool {
    store().lock().map_or(true, |store| store.enabled)
}

/// `SPELLCHECK_TOGGLE`.
pub fn toggle() {
    if let Ok(mut store) = store().lock() {
        store.enabled = !store.enabled;
        save(&store);
    }
}

/// `SPELLCHECK_LEARN_WORD`: the word is kept lower-cased.
pub fn learn_word(word: &str) {
    if let Ok(mut store) = store().lock() {
        store.learned.insert(word.to_lowercase());
        save(&store);
    }
}

/// `SPELLCHECK_UNLEARN_WORD`.
pub fn unlearn_word(word: &str) {
    if let Ok(mut store) = store().lock() {
        store.learned.remove(&word.to_lowercase());
        save(&store);
    }
}

/// `hasLearnedWord`.
pub fn has_learned_word(word: &str) -> bool {
    store()
        .lock()
        .is_ok_and(|store| store.learned.contains(&word.to_lowercase()))
}

/// `findLearnedWordIn`: the first learned word that stands alone in `text` (letters, digits and `_` do not touch it).
pub fn find_learned_word_in(text: &str) -> Option<String> {
    let store = store().lock().ok()?;
    if text.is_empty() || store.learned.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let mut found: Vec<&String> = store.learned.iter().collect();
    found.sort();
    found
        .into_iter()
        .find(|word| contains_whole_word(&lower, word))
        .cloned()
}

fn contains_whole_word(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Which text is checked
// ---------------------------------------------------------------------------------------------------------------

/// The stretches of `text` the editor leaves to the checker. The official editor (module 787564) sets `spellCheck`
/// to false on: the language marker of a code fence (`codeBlockLang`/`codeBlockSyntax` leaves), every line of a code
/// block that names a language, and a slash command's name (`leaf.commandName`); mentions, emoji and timestamps are
/// void elements the browser never checks, and in plain text they are the `<...>` tokens. Inline code, links and a
/// code block without a language stay checked there, so they stay checked here.
pub fn checkable_segments(text: &str) -> Vec<Range<usize>> {
    let mut skip: Vec<Range<usize>> = Vec::new();
    // Code fences, line by line.
    let mut offset = 0;
    let mut open: Option<(usize, bool, usize)> = None; // (fence line start, has language, first body byte)
    let mut fences: Vec<(Range<usize>, bool)> = Vec::new();
    for line in text.split_inclusive('\n') {
        let end = offset + line.len();
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if let Some(rest) = trimmed.strip_prefix("```") {
            match open.take() {
                None => open = Some((offset, !rest.trim().is_empty(), end)),
                Some((start, has_language, body)) => {
                    // The opening fence line (with its language) and the closing line carry no checked text.
                    fences.push((start..body, true));
                    if has_language {
                        fences.push((body..offset, true));
                    }
                    fences.push((offset..end, true));
                }
            }
        }
        offset = end;
    }
    skip.extend(fences.into_iter().map(|(range, _)| range));
    // A leading slash command: its name is not checked.
    if text.starts_with('/') {
        let end = text.find(char::is_whitespace).unwrap_or(text.len());
        skip.push(0..end);
    }
    // `<@id>`, `<@!id>`, `<@&id>`, `<#id>`, `<:name:id>`, `<a:name:id>`, `</name:id>`, `<t:unix:style>`, `<id:...>`.
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'<'
            && let Some(close) = text[at..].find('>')
            && is_token(&text[at + 1..at + close])
        {
            skip.push(at..at + close + 1);
            at += close + 1;
        } else {
            at += 1;
        }
    }
    skip.sort_by_key(|range| range.start);
    let mut out = Vec::new();
    let mut from = 0;
    for range in skip {
        if range.start > from {
            out.push(from..range.start);
        }
        from = from.max(range.end);
    }
    if from < text.len() {
        out.push(from..text.len());
    }
    out
}

fn is_token(inner: &str) -> bool {
    if inner.is_empty() || inner.len() > 120 || inner.contains(char::is_whitespace) {
        return false;
    }
    let first = inner.as_bytes()[0];
    matches!(first, b'@' | b'#' | b'/' | b':')
        || inner.starts_with("a:")
        || inner.starts_with("t:")
        || inner.starts_with("id:")
}

/// The ranges that were marked in `old`, moved to where the same text stands in `new`; a range the edit touched is
/// dropped (Chromium removes a marker the edit overlaps and marks it again once the new check is back).
pub fn shift_ranges(old: &str, new: &str, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let old_changed_end = old.len() - suffix;
    let delta = new.len() as isize - old.len() as isize;
    ranges
        .iter()
        .filter_map(|range| {
            if range.end < prefix {
                Some(range.clone())
            } else if range.start > old_changed_end {
                let start = (range.start as isize + delta) as usize;
                let end = (range.end as isize + delta) as usize;
                Some(start..end)
            } else {
                None
            }
        })
        .filter(|range| new.is_char_boundary(range.start) && new.is_char_boundary(range.end))
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Languages: `applyLanguages` of module 334604
// ---------------------------------------------------------------------------------------------------------------

/// `I(e)`: `xx-YY` with the region upper-cased; `no` stands for `nb`.
fn normalize(tag: &str) -> Option<String> {
    let tag = tag.replace('_', "-");
    let mut parts = tag.split('-');
    let language = parts.next()?.to_lowercase();
    let language = if language == "no" {
        "nb".to_owned()
    } else {
        language
    };
    let region =
        parts.find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()));
    Some(match region {
        Some(region) => format!("{language}-{}", region.to_uppercase()),
        None => language,
    })
}

/// The checker languages for the account locale followed by the system's languages: each candidate maps to itself
/// when the system has it, else to the first available language of the same family; duplicates collapse.
pub fn resolve_languages(candidates: &[String], available: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for candidate in candidates {
        let Some(normalized) = normalize(candidate) else {
            continue;
        };
        let exact = available
            .iter()
            .find(|tag| normalize(tag).as_deref() == Some(normalized.as_str()));
        let family = normalized.split('-').next().unwrap_or_default();
        let by_family = available
            .iter()
            .find(|tag| normalize(tag).is_some_and(|tag| tag.split('-').next() == Some(family)));
        if let Some(tag) = exact.or(by_family)
            && !out.contains(tag)
        {
            out.push(tag.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------------------------------------------

/// The system-dictionary jobs are the module's API and are exercised by its tests; the app keeps learned words in the
/// store (as the official client does) and does not touch the system dictionary.
#[cfg_attr(not(test), allow(dead_code))]
enum Job {
    Languages(Sender<Vec<String>>),
    Use(Vec<String>, Sender<Vec<String>>),
    Check {
        text: String,
        segments: Vec<Range<usize>>,
        reply: Sender<Vec<Range<usize>>>,
    },
    Suggest(String, Sender<Vec<String>>),
    Add(String, Sender<bool>),
    Remove(String, Sender<bool>),
    Ignore(String, Sender<bool>),
}

/// The handle to the worker thread.
pub struct Service {
    jobs: Sender<Job>,
}

/// The one service of the process; the worker starts on first use.
pub fn service() -> &'static Service {
    static SERVICE: OnceLock<Service> = OnceLock::new();
    SERVICE.get_or_init(|| {
        let (jobs, queue) = mpsc::channel();
        std::thread::Builder::new()
            .name("spellcheck".into())
            .spawn(move || worker(queue))
            .ok();
        Service { jobs }
    })
}

#[cfg_attr(not(test), allow(dead_code))]
impl Service {
    /// Language tags the system has a checker for.
    pub fn supported_languages(&self) -> Receiver<Vec<String>> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Languages(reply));
        answer
    }

    /// Picks the languages checks and suggestions use; the answer is the ones the system accepted.
    pub fn use_languages(&self, tags: Vec<String>) -> Receiver<Vec<String>> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Use(tags, reply));
        answer
    }

    /// Checks `text`: the byte ranges of the misspelled words. A newer check cancels this one while it still waits.
    pub fn check(&self, text: String) -> Receiver<Vec<Range<usize>>> {
        let (reply, answer) = mpsc::channel();
        let segments = checkable_segments(&text);
        let _ = self.jobs.send(Job::Check {
            text,
            segments,
            reply,
        });
        answer
    }

    /// Suggestions for a word, best first, at most [`MAX_SUGGESTIONS`].
    pub fn suggest(&self, word: &str) -> Receiver<Vec<String>> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Suggest(word.to_owned(), reply));
        answer
    }

    /// `ISpellChecker::Add`: the word goes into the system user dictionary.
    pub fn add_to_user_dictionary(&self, word: &str) -> Receiver<bool> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Add(word.to_owned(), reply));
        answer
    }

    /// `ISpellChecker2::Remove`: takes a word back out of the user dictionary.
    pub fn remove_from_user_dictionary(&self, word: &str) -> Receiver<bool> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Remove(word.to_owned(), reply));
        answer
    }

    /// `ISpellChecker::Ignore`: the word is accepted until the checker is dropped (the session).
    pub fn ignore_for_session(&self, word: &str) -> Receiver<bool> {
        let (reply, answer) = mpsc::channel();
        let _ = self.jobs.send(Job::Ignore(word.to_owned(), reply));
        answer
    }
}

/// Drops every queued check but the newest.
fn coalesce(pending: Vec<Job>) -> Vec<Job> {
    let last_check = pending
        .iter()
        .rposition(|job| matches!(job, Job::Check { .. }));
    pending
        .into_iter()
        .enumerate()
        .filter(|(index, job)| !matches!(job, Job::Check { .. }) || Some(*index) == last_check)
        .map(|(_, job)| job)
        .collect()
}

fn worker(queue: Receiver<Job>) {
    let mut engine = platform::Engine::new();
    while let Ok(first) = queue.recv() {
        let mut pending = vec![first];
        while let Ok(more) = queue.try_recv() {
            pending.push(more);
        }
        for job in coalesce(pending) {
            match job {
                Job::Languages(reply) => {
                    let _ = reply.send(engine.supported());
                }
                Job::Use(tags, reply) => {
                    let _ = reply.send(engine.select(&tags));
                }
                Job::Check {
                    text,
                    segments,
                    reply,
                } => {
                    let _ = reply.send(engine.check(&text, &segments));
                }
                Job::Suggest(word, reply) => {
                    let mut found = engine.suggest(&word);
                    found.truncate(MAX_SUGGESTIONS);
                    let _ = reply.send(found);
                }
                Job::Add(word, reply) => {
                    let _ = reply.send(engine.add(&word));
                }
                Job::Remove(word, reply) => {
                    let _ = reply.send(engine.remove(&word));
                }
                Job::Ignore(word, reply) => {
                    let _ = reply.send(engine.ignore(&word));
                }
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::ops::Range;
    use windows::Win32::Globalization::{
        CORRECTIVE_ACTION_DELETE, GetUserDefaultLocaleName, GetUserPreferredUILanguages,
        ISpellChecker, ISpellChecker2, ISpellCheckerFactory, MUI_LANGUAGE_NAME,
        SpellCheckerFactory,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, IEnumString,
    };
    use windows::core::{Interface, PCWSTR, PWSTR};

    /// The locales the system reports: the default locale, then the preferred languages (what the page reads as
    /// `navigator.languages`).
    pub fn system_languages() -> Vec<String> {
        let mut out = Vec::new();
        // SAFETY: the buffers outlive the calls and the lengths passed are their sizes.
        unsafe {
            let mut name = [0u16; 85];
            let length = GetUserDefaultLocaleName(&mut name);
            if length > 1 {
                out.push(String::from_utf16_lossy(&name[..length as usize - 1]));
            }
            let mut count = 0u32;
            let mut size = 0u32;
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut size).is_ok()
                && size > 0
            {
                let mut buffer = vec![0u16; size as usize];
                if GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut size,
                )
                .is_ok()
                {
                    for tag in buffer
                        .split(|unit| *unit == 0)
                        .filter(|tag| !tag.is_empty())
                    {
                        out.push(String::from_utf16_lossy(tag));
                    }
                }
            }
        }
        out
    }

    pub struct Engine {
        factory: Option<ISpellCheckerFactory>,
        checkers: HashMap<String, ISpellChecker>,
        /// The tags checks use, in order.
        active: Vec<String>,
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Reads and frees every string of an `IEnumString`.
    fn strings(list: &IEnumString) -> Vec<String> {
        let mut out = Vec::new();
        loop {
            let mut item = [PWSTR::null()];
            let mut fetched = 0u32;
            // SAFETY: `item` and `fetched` are valid for the call; each returned string is freed once.
            unsafe {
                let status = list.Next(&mut item, Some(&mut fetched));
                if status.0 != 0 || fetched == 0 || item[0].is_null() {
                    break;
                }
                out.push(item[0].to_string().unwrap_or_default());
                CoTaskMemFree(Some(item[0].0 as *const _));
            }
        }
        out
    }

    impl Engine {
        pub fn new() -> Self {
            // SAFETY: plain COM start-up of this thread; the factory is only used on it.
            let factory = unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance::<_, ISpellCheckerFactory>(
                    &SpellCheckerFactory,
                    None,
                    CLSCTX_INPROC_SERVER,
                )
                .ok()
            };
            Self {
                factory,
                checkers: HashMap::new(),
                active: Vec::new(),
            }
        }

        pub fn supported(&mut self) -> Vec<String> {
            let Some(factory) = &self.factory else {
                return Vec::new();
            };
            // SAFETY: `factory` is a live interface of this thread.
            unsafe {
                factory
                    .SupportedLanguages()
                    .map(|list| strings(&list))
                    .unwrap_or_default()
            }
        }

        fn checker(&mut self, tag: &str) -> Option<ISpellChecker> {
            if let Some(known) = self.checkers.get(tag) {
                return Some(known.clone());
            }
            let factory = self.factory.as_ref()?;
            let tag_wide = wide(tag);
            // SAFETY: `tag_wide` is NUL-terminated and outlives the calls.
            let created = unsafe {
                if !factory
                    .IsSupported(PCWSTR(tag_wide.as_ptr()))
                    .ok()?
                    .as_bool()
                {
                    return None;
                }
                factory.CreateSpellChecker(PCWSTR(tag_wide.as_ptr())).ok()?
            };
            self.checkers.insert(tag.to_owned(), created.clone());
            Some(created)
        }

        pub fn select(&mut self, tags: &[String]) -> Vec<String> {
            let mut applied = Vec::new();
            for tag in tags {
                if self.checker(tag).is_some() && !applied.contains(tag) {
                    applied.push(tag.clone());
                }
            }
            self.active = applied.clone();
            applied
        }

        /// A word is misspelled only when every active language flags it (a word valid in any language is fine).
        pub fn check(&mut self, text: &str, segments: &[Range<usize>]) -> Vec<Range<usize>> {
            let active = self.active.clone();
            let checkers: Vec<ISpellChecker> =
                active.iter().filter_map(|tag| self.checker(tag)).collect();
            if checkers.is_empty() {
                return Vec::new();
            }
            let mut out = Vec::new();
            for segment in segments {
                let piece = &text[segment.clone()];
                if piece.trim().is_empty() {
                    continue;
                }
                let units: Vec<u16> = piece.encode_utf16().collect();
                let mut common: Option<Vec<Range<usize>>> = None;
                for checker in &checkers {
                    let found = errors(checker, &units);
                    common = Some(match common {
                        None => found,
                        Some(previous) => previous
                            .into_iter()
                            .filter(|range| {
                                found
                                    .iter()
                                    .any(|other| other.start < range.end && range.start < other.end)
                            })
                            .collect(),
                    });
                }
                for range in common.unwrap_or_default() {
                    if let (Some(start), Some(end)) = (
                        utf16_to_byte(piece, range.start),
                        utf16_to_byte(piece, range.end),
                    ) {
                        out.push(segment.start + start..segment.start + end);
                    }
                }
            }
            out
        }

        pub fn suggest(&mut self, word: &str) -> Vec<String> {
            let active = self.active.clone();
            let word_wide = wide(word);
            let mut lists: Vec<Vec<String>> = Vec::new();
            for tag in active {
                if let Some(checker) = self.checker(&tag) {
                    // SAFETY: `word_wide` is NUL-terminated and outlives the call.
                    let found = unsafe { checker.Suggest(PCWSTR(word_wide.as_ptr())) }
                        .map(|list| strings(&list))
                        .unwrap_or_default();
                    lists.push(found);
                }
            }
            // Interleave the languages' lists, best first, without repeats.
            let mut out: Vec<String> = Vec::new();
            let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
            for position in 0..longest {
                for list in &lists {
                    if let Some(item) = list.get(position)
                        && !out.contains(item)
                    {
                        out.push(item.clone());
                    }
                }
            }
            out
        }

        fn first_checker(&mut self) -> Option<ISpellChecker> {
            let tag = self.active.first()?.clone();
            self.checker(&tag)
        }

        pub fn add(&mut self, word: &str) -> bool {
            let Some(checker) = self.first_checker() else {
                return false;
            };
            let word_wide = wide(word);
            // SAFETY: `word_wide` is NUL-terminated and outlives the call.
            unsafe { checker.Add(PCWSTR(word_wide.as_ptr())) }.is_ok()
        }

        pub fn remove(&mut self, word: &str) -> bool {
            let Some(checker) = self.first_checker() else {
                return false;
            };
            let Ok(checker) = checker.cast::<ISpellChecker2>() else {
                return false;
            };
            let word_wide = wide(word);
            // SAFETY: `word_wide` is NUL-terminated and outlives the call.
            unsafe { checker.Remove(PCWSTR(word_wide.as_ptr())) }.is_ok()
        }

        pub fn ignore(&mut self, word: &str) -> bool {
            let Some(checker) = self.first_checker() else {
                return false;
            };
            let word_wide = wide(word);
            // SAFETY: `word_wide` is NUL-terminated and outlives the call.
            unsafe { checker.Ignore(PCWSTR(word_wide.as_ptr())) }.is_ok()
        }
    }

    /// The misspelled stretches (UTF-16 offsets) one checker finds. A repeated word the checker would delete is not
    /// a misspelling.
    fn errors(checker: &ISpellChecker, units: &[u16]) -> Vec<Range<usize>> {
        let mut text = units.to_vec();
        text.push(0);
        let mut out = Vec::new();
        // SAFETY: `text` is NUL-terminated and outlives the enumeration.
        unsafe {
            let Ok(list) = checker.Check(PCWSTR(text.as_ptr())) else {
                return out;
            };
            loop {
                let mut error = None;
                if list.Next(&mut error).0 != 0 {
                    break;
                }
                let Some(error) = error else { break };
                let (Ok(start), Ok(length), Ok(action)) =
                    (error.StartIndex(), error.Length(), error.CorrectiveAction())
                else {
                    continue;
                };
                if action == CORRECTIVE_ACTION_DELETE || length == 0 {
                    continue;
                }
                out.push(start as usize..(start + length) as usize);
            }
        }
        out
    }

    fn utf16_to_byte(text: &str, target: usize) -> Option<usize> {
        let mut units = 0;
        for (at, c) in text.char_indices() {
            if units == target {
                return Some(at);
            }
            units += c.len_utf16();
            if units > target {
                return None;
            }
        }
        (units == target).then_some(text.len())
    }
}

#[cfg(not(windows))]
mod platform {
    use std::ops::Range;

    pub fn system_languages() -> Vec<String> {
        Vec::new()
    }

    /// Without a system checker nothing is flagged.
    pub struct Engine;

    impl Engine {
        pub fn new() -> Self {
            Self
        }
        pub fn supported(&mut self) -> Vec<String> {
            Vec::new()
        }
        pub fn select(&mut self, _: &[String]) -> Vec<String> {
            Vec::new()
        }
        pub fn check(&mut self, _: &str, _: &[Range<usize>]) -> Vec<Range<usize>> {
            Vec::new()
        }
        pub fn suggest(&mut self, _: &str) -> Vec<String> {
            Vec::new()
        }
        pub fn add(&mut self, _: &str) -> bool {
            false
        }
        pub fn remove(&mut self, _: &str) -> bool {
            false
        }
        pub fn ignore(&mut self, _: &str) -> bool {
            false
        }
    }
}

/// The languages the checks use: the account locale (the system locale stands in for it) and the system's languages,
/// resolved against what the system can check, as `applyLanguages` does (the `enableElectronMultilangSpellcheck` path).
/// Applied once, on the first call.
pub fn ensure_languages() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let service = service();
        let available = service
            .supported_languages()
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap_or_default();
        let candidates = platform::system_languages();
        let mut chosen = resolve_languages(&candidates, &available);
        if chosen.is_empty() {
            chosen = resolve_languages(&["en-US".to_owned()], &available);
        }
        let _ = service.use_languages(chosen);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// One test at a time: a newer check cancels a queued one, and the tests share the worker.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait<T>(answer: Receiver<T>) -> Option<T> {
        answer.recv_timeout(Duration::from_secs(10)).ok()
    }

    /// The English checker, or `None` (the test then skips) when the system has none.
    fn english() -> Option<&'static Service> {
        let service = service();
        let available = wait(service.supported_languages())?;
        let tag = available
            .iter()
            .find(|tag| tag.starts_with("en-"))
            .cloned()?;
        let applied = wait(service.use_languages(vec![tag]))?;
        (!applied.is_empty()).then_some(service)
    }

    #[test]
    fn resolves_the_locale_then_the_family() {
        let available = vec!["en-US".to_owned(), "tr-TR".to_owned(), "de-DE".to_owned()];
        let got = resolve_languages(
            &[
                "tr".to_owned(),
                "en_GB".to_owned(),
                "tr-TR".to_owned(),
                "fr-FR".to_owned(),
            ],
            &available,
        );
        assert_eq!(got, vec!["tr-TR".to_owned(), "en-US".to_owned()]);
        assert_eq!(normalize("no"), Some("nb".to_owned()));
        assert_eq!(normalize("pt_br"), Some("pt-BR".to_owned()));
    }

    #[test]
    fn segments_skip_what_the_official_editor_leaves_unchecked() {
        let text = "hello <@123> world <:cat:456> and <t:1:R> end";
        let parts: Vec<&str> = checkable_segments(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(parts, vec!["hello ", " world ", " and ", " end"]);

        // A code block with a language is left alone; one without is checked; the fences never are.
        let text = "teh\n```rust\nlet x = wrold;\n```\n```\nteh again\n```\nend";
        let parts: Vec<&str> = checkable_segments(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(parts, vec!["teh\n", "teh again\n", "end"]);

        // The slash command's name is skipped, its arguments are not; inline code stays checked.
        let text = "/giphy teh `wrold`";
        let parts: Vec<&str> = checkable_segments(text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(parts, vec![" teh `wrold`"]);
    }

    #[test]
    fn ranges_follow_an_edit_and_drop_when_touched() {
        let old = "aa bbb cc";
        let ranges = vec![0..2, 3..6, 7..9];
        let new = "aa XXbbb cc";
        // The edit touches the second word: it drops, the others keep their words.
        assert_eq!(shift_ranges(old, new, &ranges), vec![0..2, 9..11]);
        let new = "aaa bbb cc";
        assert_eq!(shift_ranges(old, new, &ranges), vec![4..7, 8..10]);
    }

    #[test]
    fn learned_words_match_whole_words_only() {
        assert!(contains_whole_word("a zorp b", "zorp"));
        assert!(!contains_whole_word("zorpy", "zorp"));
        assert!(contains_whole_word("(zorp)", "zorp"));
    }

    #[test]
    fn flags_a_misspelled_word_and_suggests_the_right_one() {
        let _serial = serial();
        let Some(service) = english() else {
            eprintln!("skipped: no English spell checker installed");
            return;
        };
        let text = "This is a smple sentence with helo wrold in it.".to_owned();
        let found = wait(service.check(text.clone())).expect("check answered");
        let words: Vec<&str> = found.iter().map(|range| &text[range.clone()]).collect();
        assert!(words.contains(&"smple"), "{words:?}");
        assert!(words.contains(&"wrold"), "{words:?}");
        assert!(!words.contains(&"sentence"), "{words:?}");
        let suggestions = wait(service.suggest("wrold")).expect("suggest answered");
        assert!(suggestions.len() <= MAX_SUGGESTIONS);
        assert!(
            suggestions.iter().any(|word| word == "world"),
            "{suggestions:?}"
        );
    }

    #[test]
    fn a_newer_check_cancels_the_queued_older_one() {
        let (first, _answer_one) = mpsc::channel();
        let (second, _answer_two) = mpsc::channel();
        let (flag, _) = mpsc::channel();
        let job = |reply| Job::Check {
            text: String::new(),
            segments: Vec::new(),
            reply,
        };
        let kept = coalesce(vec![job(first), Job::Add("x".into(), flag), job(second)]);
        assert_eq!(kept.len(), 2);
        assert!(matches!(kept[0], Job::Add(..)));
        assert!(matches!(kept[1], Job::Check { .. }));
    }

    #[test]
    fn a_made_up_word_can_be_added_removed_and_ignored() {
        let _serial = serial();
        let Some(service) = english() else {
            eprintln!("skipped: no English spell checker installed");
            return;
        };
        let word = "qzxvbnmplk";
        let text = format!("one {word} two");
        let flagged = |service: &Service| {
            wait(service.check(text.clone())).is_some_and(|found| !found.is_empty())
        };
        assert!(flagged(service), "the made-up word is flagged at first");
        assert_eq!(wait(service.ignore_for_session(word)), Some(true));
        assert!(!flagged(service), "ignored words are accepted");
        // Ignore lasts for the session only; `Add` and `Remove` leave the user dictionary as it was.
        assert_eq!(wait(service.add_to_user_dictionary(word)), Some(true));
        assert!(!flagged(service));
        assert_eq!(wait(service.remove_from_user_dictionary(word)), Some(true));
    }

    #[test]
    fn urls_are_left_alone_by_the_system_checker() {
        let _serial = serial();
        let Some(service) = english() else {
            eprintln!("skipped: no English spell checker installed");
            return;
        };
        let text = "see https://exampel.com/helo for more".to_owned();
        let found = wait(service.check(text.clone())).expect("check answered");
        let words: Vec<&str> = found.iter().map(|range| &text[range.clone()]).collect();
        assert!(words.is_empty(), "flagged inside a link: {words:?}");
    }
}
