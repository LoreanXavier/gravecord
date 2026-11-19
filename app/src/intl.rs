//! The interface language at run time: which locale shows, and fetching its catalog.
//!
//! `LocaleStore` (module 773669) takes the locale from the settings proto's `localization.locale` (READY, the cached
//! proto, `USER_SETTINGS_PROTO_UPDATE`), and before any of those from the system language matched to the available
//! locales (module 792996 `initialLocale`, module 226536 `kt`). `setLocale` (module 449275) then loads the locale's
//! catalog chunk and everything re-renders in it, without a restart. Here the chunk is the pinned build's own asset,
//! fetched once from the asset host like the fonts and kept on disk (`intl/<asset>.json`); en-US is compiled in.
//!
//! The account's choice is also kept in `storage.json` (`intl.locale`), standing in for the official client's cached
//! settings proto, so the next start shows the language before READY.

use gpui::App;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const STORAGE_KEY: &str = "intl.locale";

/// The locale shown until the account's arrives: last run's choice, else the system language.
pub fn startup_locale() -> &'static str {
    static STARTUP: OnceLock<&'static str> = OnceLock::new();
    STARTUP.get_or_init(|| {
        let locales = dc_proto::settings::LOCALES;
        if let Some(saved) = crate::storage::get(STORAGE_KEY)
            && let Some(found) = saved
                .as_str()
                .and_then(|code| locales.iter().copied().find(|known| *known == code))
        {
            return found;
        }
        let system = sys_locale::get_locale().unwrap_or_default();
        dc_intl::match_locale(&system, locales, dc_intl::DEFAULT_LOCALE)
    })
}

/// `initialLocale` (module 792996): the locale the client started in. Unit tests start in en-US and read no storage.
pub fn initial_locale() -> &'static str {
    if cfg!(test) {
        dc_intl::DEFAULT_LOCALE
    } else {
        startup_locale()
    }
}

/// Keeps the account's locale for the next start.
pub fn remember(code: &'static str) {
    static LAST: Mutex<Option<&'static str>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if *last == Some(code) {
        return;
    }
    *last = Some(code);
    if crate::storage::get(STORAGE_KEY)
        .as_ref()
        .and_then(|value| value.as_str())
        != Some(code)
    {
        crate::storage::set(STORAGE_KEY, serde_json::Value::from(code));
    }
}

/// `--font-primary` after "gg sans", per locale (`952007.*.css`: `:root` and the `:root:lang(...)` overrides). The
/// generic `sans-serif` is left out; what no family covers goes to the system fallback, as in Chromium.
fn primary_stack(code: &str) -> &'static [&'static str] {
    const DEFAULT: &[&str] = &["Noto Sans", "Helvetica Neue", "Helvetica", "Arial"];
    match code {
        "ko" => &[
            "Apple SD Gothic Neo",
            "NanumBarunGothic",
            "맑은 고딕",
            "Malgun Gothic",
            "Gulim",
            "굴림",
            "Dotum",
            "돋움",
            "Noto Sans",
            "Helvetica Neue",
            "Helvetica",
            "Arial",
        ],
        "ja" => &[
            "Hiragino Sans",
            "ヒラギノ角ゴ ProN W3",
            "Hiragino Kaku Gothic ProN",
            "メイリオ",
            "Meiryo",
            "Osaka",
            "MS PGothic",
            "Noto Sans",
            "Helvetica Neue",
            "Helvetica",
            "Arial",
        ],
        "zh-CN" => &[
            "Microsoft YaHei New",
            "微软雅黑",
            "Microsoft Yahei",
            "Microsoft JhengHei",
            "宋体",
            "SimSun",
            "Noto Sans",
            "Helvetica Neue",
            "Helvetica",
            "Arial",
        ],
        "zh-TW" => &[
            "Microsoft JhengHei",
            "微軟正黑體",
            "Microsoft JhengHei UI",
            "Microsoft YaHei",
            "微軟雅黑",
            "宋体",
            "SimSun",
            "Noto Sans",
            "Helvetica Neue",
            "Helvetica",
            "Arial",
        ],
        // bg, el, ru and uk repeat the default stack.
        _ => DEFAULT,
    }
}

/// The fallback families of the interface font in the current locale, for the root text style.
pub fn font_fallbacks() -> gpui::FontFallbacks {
    static MADE: Mutex<Option<(&'static str, gpui::FontFallbacks)>> = Mutex::new(None);
    let code = dc_intl::locale();
    let Ok(mut made) = MADE.lock() else {
        return gpui::FontFallbacks::default();
    };
    match &*made {
        Some((known, fallbacks)) if *known == code => fallbacks.clone(),
        _ => {
            let fallbacks = gpui::FontFallbacks::from_fonts(
                primary_stack(code)
                    .iter()
                    .map(|family| (*family).to_owned())
                    .collect(),
            );
            *made = Some((code, fallbacks.clone()));
            fallbacks
        }
    }
}

/// The error of the last catalog load that failed (module 449275's `error`), cleared when a load starts or succeeds.
fn failed() -> &'static Mutex<Option<String>> {
    static FAILED: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    FAILED.get_or_init(Mutex::default)
}

/// Why the interface catalog could not be loaded, while that is so: module 638421 then shows "An error occurred
/// loading {locale}" with this message in place of the app.
pub fn load_error() -> Option<String> {
    failed().lock().ok().and_then(|failed| failed.clone())
}

fn set_load_error(error: Option<String>) {
    if let Ok(mut failed) = failed().lock() {
        *failed = error;
    }
}

fn loading() -> &'static Mutex<Vec<&'static str>> {
    static LOADING: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    LOADING.get_or_init(Mutex::default)
}

/// Shows `code` from now on, loading its catalog first if it is not here yet (the frames in between show en-US, as
/// the official loader's fallback does). Called every frame; cheap when nothing changed.
pub fn apply(code: &'static str, cx: &mut App) {
    if dc_intl::set_locale(code) {
        tracing::info!(locale = code, "interface locale");
        cx.refresh_windows();
    }
    if dc_intl::is_loaded(code) {
        return;
    }
    {
        let Ok(mut loading) = loading().lock() else {
            return;
        };
        if loading.contains(&code) {
            return;
        }
        loading.push(code);
    }
    set_load_error(None);
    let slot: Arc<Mutex<Option<Result<(), String>>>> = Arc::default();
    let worker = slot.clone();
    std::thread::spawn(move || {
        let result = load(code);
        if let Ok(mut slot) = worker.lock() {
            *slot = Some(result);
        }
    });
    cx.spawn(async move |cx| {
        let result = loop {
            cx.background_executor()
                .timer(Duration::from_millis(30))
                .await;
            if let Some(done) = slot.lock().ok().and_then(|mut slot| slot.take()) {
                break done;
            }
        };
        match result {
            Ok(()) => tracing::info!(locale = code, "interface catalog ready"),
            // Module 638421's error page; the next change of locale tries again.
            Err(error) => {
                tracing::warn!(locale = code, %error, "interface catalog unavailable");
                set_load_error(Some(error));
            }
        }
        if let Ok(mut loading) = loading().lock() {
            loading.retain(|known| *known != code);
        }
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// The harness's `locale` step: the catalog loaded on this thread before the step's next frame.
#[cfg(feature = "capture")]
pub fn load_now(code: &str) -> String {
    if dc_intl::is_loaded(code) {
        return "catalog ready".to_owned();
    }
    match load(code) {
        Ok(()) => "catalog loaded".to_owned(),
        Err(error) => format!("catalog unavailable: {error}"),
    }
}

fn cache_path(file: &str) -> Option<std::path::PathBuf> {
    let dir = crate::storage::data_dir()?.join("intl");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(file).with_extension("json"))
}

/// The catalogs of `code` installed, one per loader: each from the disk cache, else from the pinned build's chunk on
/// the asset host (fetched together). A catalog that cannot be had leaves its messages in en-US.
fn load(code: &str) -> Result<(), String> {
    load_from(code, cache_path, fetch_all)
}

/// [`load`] with its disk cache and its fetch given, so a test can stand in for both.
fn load_from(
    code: &str,
    cache_path: impl Fn(&str) -> Option<std::path::PathBuf>,
    fetch_all: impl FnOnce(&[&'static str]) -> Result<Vec<Fetched>, String>,
) -> Result<(), String> {
    let files: Vec<&'static str> = dc_intl::chunk_files(code).collect();
    if files.is_empty() {
        return Err(format!("no catalog chunk for {code}"));
    }
    let mut jsons: Vec<Option<String>> = files
        .iter()
        .map(|file| cache_path(file).and_then(|path| std::fs::read_to_string(path).ok()))
        .collect();
    let missing: Vec<&'static str> = files
        .iter()
        .zip(&jsons)
        .filter(|(_, json)| json.is_none())
        .map(|(file, _)| *file)
        .collect();
    let mut failed = 0;
    for (file, fetched) in fetch_all(&missing)? {
        match fetched {
            Ok(json) => {
                if let Some(path) = cache_path(file) {
                    let partial = path.with_extension("part");
                    if std::fs::write(&partial, &json).is_ok() {
                        let _ = std::fs::rename(&partial, &path);
                    }
                }
                if let Some(slot) = files.iter().position(|known| *known == file) {
                    jsons[slot] = Some(json);
                }
            }
            Err(error) => {
                failed += 1;
                tracing::warn!(file, %error, "interface catalog chunk unavailable");
            }
        }
    }
    let have: Vec<&str> = jsons.iter().flatten().map(String::as_str).collect();
    // Without the main catalog (module 802249's, the first) the locale stays en-US.
    if jsons[0].is_none() {
        return Err(format!(
            "{} of {} catalogs unavailable",
            failed,
            files.len()
        ));
    }
    dc_intl::install(code, &have)
}

/// A chunk file and its catalog's JSON, or why it could not be had.
type Fetched = (&'static str, Result<String, String>);

/// The chunks' catalogs, fetched at the same time.
fn fetch_all(files: &[&'static str]) -> Result<Vec<Fetched>, String> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let client = std::sync::Arc::new(
        dc_http::Client::new(
            dc_proto::secrets::Token::new(""),
            dc_proto::identity::Identity::new(),
        )
        .map_err(|error| error.to_string())?,
    );
    Ok(runtime.block_on(async {
        let mut tasks = tokio::task::JoinSet::new();
        for file in files {
            let client = client.clone();
            let file: &'static str = file;
            tasks.spawn(async move {
                let url = format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN);
                let result = match client.fetch_public(&url).await {
                    Ok(bytes) => String::from_utf8(bytes)
                        .map_err(|error| error.to_string())
                        .and_then(|source| {
                            dc_intl::catalog_json(&source)
                                .ok_or_else(|| format!("{file} holds no catalog"))
                        }),
                    Err(error) => Err(error.to_string()),
                };
                (file, result)
            });
        }
        let mut out = Vec::new();
        while let Some(done) = tasks.join_next().await {
            if let Ok(done) = done {
                out.push(done);
            }
        }
        out
    }))
}

/// Keeps the adopted surfaces adopted: a string literal whose text is exactly a literal en-US message must reach the
/// screen through `dc_intl::tr` / `t!` (or be listed in [`guard::ALLOWED`] with its reason).
#[cfg(test)]
mod guard {
    /// The files whose interface text is adopted, relative to `app/src`.
    pub(super) const ADOPTED: &[&str] = &[
        "ui.rs",
        "ui/message_kinds.rs",
        "ui/message_header.rs",
        "ui/unread.rs",
        "ui/jump.rs",
        "ui/reaction_tip.rs",
        "ui/embed.rs",
        "ui/sent_attachment.rs",
        "ui/uploads.rs",
        "ui/upload_error.rs",
        "ui/upload_progress.rs",
        "ui/hover_bar.rs",
        "ui/message_tips.rs",
        "ui/send_state.rs",
        "ui/composer_plus.rs",
        "ui/dm.rs",
        "ui/dm_call.rs",
        "ui/dm_profile.rs",
        "ui/invite_embed.rs",
        "ui/pins.rs",
        "ui/context_menu.rs",
        "ui/menu.rs",
        "ui/guild_menu.rs",
        "ui/rail_folder_menu.rs",
        "ui/guild_navigation.rs",
        "ui/channel_icon.rs",
        "ui/friends.rs",
        "ui/quickswitcher.rs",
        "ui/destructive.rs",
        "ui/modal.rs",
        "ui/settings.rs",
        "ui/settings/account_dialogs.rs",
        "ui/settings/account_nested.rs",
        "ui/settings/account_panels.rs",
        "ui/settings/actions.rs",
        "ui/settings/appearance.rs",
        "ui/settings/billing.rs",
        "ui/settings/client_theme.rs",
        "ui/settings/clips.rs",
        "ui/settings/connection_dialogs.rs",
        "ui/settings/connections.rs",
        "ui/settings/content_filters.rs",
        "ui/settings/controls.rs",
        "ui/settings/custom_theme.rs",
        "ui/settings/gift_inventory_page.rs",
        "ui/settings/gift_redeem.rs",
        "ui/settings/keybinds.rs",
        "ui/settings/mfa.rs",
        "ui/settings/nitro_page.rs",
        "ui/settings/notifications_page.rs",
        "ui/settings/privacy.rs",
        "ui/settings/render.rs",
        "ui/settings/server_boost_page.rs",
        "ui/settings/soundboard.rs",
        "ui/settings/streamer.rs",
        "ui/settings/system.rs",
        "ui/settings/voice.rs",
        "ui/settings/voice_camera.rs",
        "ui/settings/voice_camera/background.rs",
        "ui/settings/voice/extra.rs",
        "ui/profile_editor.rs",
        "ui/profile_editor/panel.rs",
        "ui/profile_editor/widgets.rs",
        "ui/profile_editor/servers.rs",
        "ui/profile_editor/styles.rs",
        "ui/timestamps.rs",
        "ui/full_profile.rs",
        "ui/home.rs",
        "ui/saved.rs",
        "ui/inbox.rs",
        "ui/settings/pages.rs",
        "ui/settings/overlay_page.rs",
        "ui/overlay.rs",
        "ui/overlay_widgets.rs",
        "ui/overlay_panel.rs",
        "ui/overlay_chat.rs",
        "ui/voice_panel.rs",
        "ui/voice_panel/rtc_debug.rs",
        "ui/video_player.rs",
        "ui/picker/nitro_packs.rs",
    ];

    /// Literals that read like a message but are not shown as written, with why: (file, text, reason).
    pub(super) const ALLOWED: &[(&str, &str, &str)] = &[
        // Not in the pinned client: Translate+'s own labels, English in the original whatever the locale.
        (
            "ui/hover_bar.rs",
            "Translate",
            "a plugin's own label, not translated by the original",
        ),
        (
            "ui/menu.rs",
            "Translate",
            "a plugin's own label, not translated by the original",
        ),
        // The Mute/Unmute pairs are looked up by their English text and shown through `tr`.
        (
            "ui/context_menu.rs",
            "Mute Channel",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute Channel",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Mute Category",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute Category",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Mute Thread",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute Thread",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Mute Server",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute Server",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Mute Conversation",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute Conversation",
            "lookup key, shown through tr",
        ),
        (
            "ui/context_menu.rs",
            "Unmute",
            "fallback replace of an English label",
        ),
        // The connection dialogs' feature lists: (glyph, label, note) tables drawn through `tr`.
        (
            "ui/settings/connection_dialogs.rs",
            "Display the Crunchyroll show you\u{2019}re watching on Discord.",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Join voice calls directly from Xbox",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Stream and watch streams on Xbox",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "See what Discord friends are currently doing on your Xbox",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Display the Xbox game you're playing on Discord",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Join voice calls directly from PlayStation",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Only available on PS5",
            "props table",
        ),
        (
            "ui/settings/connection_dialogs.rs",
            "Display the game you're playing on Discord",
            "props table",
        ),
        // Module 338659 `getStats` writes the unknown resolution in English.
        (
            "ui/video_player.rs",
            "Unknown",
            "the official stats text is untranslated",
        ),
    ];

    /// Functions whose literals are lookup keys (sidebar labels the navigation matches) or are translated where they
    /// are drawn (the message menu's items go through `tr` in `menu_row`).
    const KEY_FUNCTIONS: &[&str] = &[
        "destination_of",
        "link_destination",
        "message_items",
        "message_items_saved",
    ];

    /// Calls whose string argument is an id, a key, a pattern or a log line, never interface text.
    const KEY_CALLS: &[&str] = &[
        ".id",
        "parity",
        "parity_with_border",
        ".path",
        "icon",
        "mask_icon",
        "color",
        "hsla",
        "request_image",
        "insert",
        "get",
        "remove",
        "contains",
        "contains_key",
        "starts_with",
        "ends_with",
        "strip_prefix",
        "find",
        "split",
        "replace",
        "replacen",
        "matches!",
        "anchor_probe",
        "group",
        "format!",
        "write!",
        "panic!",
        "assert!",
        "assert_eq!",
        "debug_assert!",
        "concat!",
        "include_str!",
        "position",
        "any",
        "eq",
        "expect",
        "storage::get",
        "storage::set",
        "script_menu_run",
        "run_label",
        "font",
        "log_effect",
    ];

    pub(super) struct Literal {
        pub start: usize,
        pub end: usize,
        pub text: String,
    }

    /// The normal string literals of a source outside comments, raw strings, byte strings and char literals.
    pub(super) fn literals(source: &str) -> Vec<Literal> {
        let bytes = source.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        let ident =
            |at: usize| at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
        while i < bytes.len() {
            let rest = &source[i..];
            if rest.starts_with("//") {
                i += rest.find('\n').unwrap_or(rest.len());
            } else if rest.starts_with("/*") {
                i += rest.find("*/").map_or(rest.len(), |end| end + 2);
            } else if (rest.starts_with("r\"") || rest.starts_with("r#")) && !ident(i) {
                let hashes = rest[1..].bytes().take_while(|b| *b == b'#').count();
                let close = format!("\"{}", "#".repeat(hashes));
                let open = 1 + hashes + 1;
                i += rest[open..]
                    .find(&close)
                    .map_or(rest.len(), |end| open + end + close.len());
            } else if rest.starts_with("b\"") && !ident(i) {
                i += 2;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            } else if bytes[i] == b'\'' {
                // A char literal ('x', '\n', '\u{..}') or a lifetime.
                let close = rest[1..]
                    .char_indices()
                    .take(12)
                    .find(|(_, ch)| *ch == '\'');
                match close {
                    Some((at, _)) if at > 0 && (at <= 4 || rest[1..].starts_with("\\u")) => {
                        i += at + 2
                    }
                    _ => i += 1,
                }
            } else if bytes[i] == b'"' {
                let start = i;
                i += 1;
                let mut text = String::new();
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        let next = source[i + 1..].chars().next().unwrap_or(' ');
                        match next {
                            'n' => text.push('\n'),
                            't' => text.push('\t'),
                            'u' => {
                                let close = source[i..].find('}').unwrap_or(2);
                                let hex = source[i + 3..i + close].to_owned();
                                if let Some(ch) =
                                    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                                {
                                    text.push(ch);
                                }
                                i += close + 1;
                                continue;
                            }
                            '\n' | '\r' => {
                                i += 1;
                                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                                    i += 1;
                                }
                                continue;
                            }
                            other => text.push(other),
                        }
                        i += 1 + next.len_utf8();
                    } else {
                        let ch = source[i..].chars().next().unwrap_or(' ');
                        text.push(ch);
                        i += ch.len_utf8();
                    }
                }
                i += 1;
                out.push(Literal {
                    start,
                    end: i,
                    text,
                });
            } else {
                i += 1;
            }
        }
        out
    }

    /// Where the first `#[cfg(test)]` module starts.
    fn tests_start(source: &str) -> usize {
        source
            .match_indices("#[cfg(test)]")
            .find(|(at, _)| {
                source[at + "#[cfg(test)]".len()..]
                    .trim_start()
                    .starts_with("mod ")
            })
            .map_or(source.len(), |(at, _)| at)
    }

    /// The spans of `const` and `static` items: their literals are drawn through `tr` where they are used.
    fn item_spans(source: &str) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            let trimmed = trimmed
                .strip_prefix("pub(in crate::ui) ")
                .or_else(|| trimmed.strip_prefix("pub(super) "))
                .or_else(|| trimmed.strip_prefix("pub(crate) "))
                .or_else(|| trimmed.strip_prefix("pub "))
                .unwrap_or(trimmed);
            let named = |rest: &str| {
                rest.chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
            };
            if !(trimmed.strip_prefix("const ").is_some_and(named)
                || trimmed.strip_prefix("static ").is_some_and(named))
            {
                continue;
            }
            let start: usize = source.lines().take(index).map(|line| line.len() + 1).sum();
            let end = source[start..]
                .find(";\n")
                .map_or(source.len(), |end| start + end);
            spans.push((start, end));
        }
        spans
    }

    /// Whether the literal sits inside `tr(...)`, `t!(...)`, `format(...)` or `string(...)` of `dc_intl`, possibly
    /// inside an `if`/`else` there.
    fn translated(source: &str, start: usize) -> bool {
        let before = &source[..start];
        let mut depth = 0i32;
        for (at, ch) in before.char_indices().rev() {
            match ch {
                ')' | '}' | ']' => depth += 1,
                '(' | '[' if depth > 0 => depth -= 1,
                '{' if depth > 0 => depth -= 1,
                '(' => {
                    let head = before[..at].trim_end();
                    return [
                        "tr",
                        "t!",
                        "dc_intl::t",
                        "dc_intl::format",
                        "dc_intl::string",
                        "format_runs",
                        "set_placeholder_message",
                    ]
                    .iter()
                    .any(|name| head.ends_with(name));
                }
                '{' => {
                    let head = before[..at].trim_end();
                    let line = head.rsplit(['\n', ';']).next().unwrap_or(head);
                    if !(head.ends_with("else") || line.contains("if ")) {
                        return false;
                    }
                }
                '[' => return false,
                ';' if depth == 0 => return false,
                _ => {}
            }
        }
        false
    }

    /// Why a literal is not interface text, if it is not.
    fn exempt(source: &str, literal: &Literal) -> Option<&'static str> {
        let text = &literal.text;
        if text
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || "_.-".contains(ch))
        {
            return Some("identifier");
        }
        let before = source[..literal.start].trim_end();
        let after = source[literal.end..].trim_start();
        if before.ends_with("==")
            || before.ends_with("!=")
            || after.starts_with("==")
            || after.starts_with("!=")
        {
            return Some("comparison");
        }
        if let Some(head) = before.strip_suffix("Some(")
            && (head.trim_end().ends_with("==") || head.trim_end().ends_with("!="))
        {
            return Some("comparison");
        }
        // A part of a settings text (`Runs`): drawn through `tr` when it is the only part; the parts of a longer
        // text are the fragments of a keyed message, left in English.
        let compact: String = after
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .take(7)
            .collect();
        if compact.starts_with(",false") || compact.starts_with(",true") {
            return Some("runs part");
        }
        if after.starts_with("=>") || after.starts_with('|') || before.ends_with('|') {
            return Some("pattern");
        }
        if [
            ".len",
            ".is_empty",
            ".starts_with",
            ".ends_with",
            ".contains",
            ".split",
            ".eq",
        ]
        .iter()
        .any(|method| after.starts_with(method))
        {
            return Some("method");
        }
        if let Some(head) = before.strip_suffix('(') {
            let head = head.trim_end();
            if KEY_CALLS.iter().any(|call| head.ends_with(call)) {
                return Some("key argument");
            }
        }
        let line = source[..literal.start]
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .trim_start();
        if line.starts_with("#[") || line.starts_with("tracing::") {
            return Some("attribute or log");
        }
        if source[..literal.start].rfind("json!(").is_some_and(|at| {
            let open = &source[at..literal.start];
            open.matches(['(', '[', '{']).count() > open.matches([')', ']', '}']).count()
        }) {
            return Some("json");
        }
        let function = source[..literal.start]
            .rmatch_indices("fn ")
            .next()
            .map(|(at, _)| {
                source[at + 3..]
                    .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                    .next()
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        KEY_FUNCTIONS.contains(&function).then_some("lookup keys")
    }

    /// The literals of `file` that are exact en-US messages and reach the screen untranslated.
    pub(super) fn untranslated(file: &str, source: &str) -> Vec<String> {
        let tests = tests_start(source);
        let items = item_spans(source);
        literals(source)
            .into_iter()
            .filter(|literal| literal.start < tests)
            .filter(|literal| !items.iter().any(|(a, b)| (*a..*b).contains(&literal.start)))
            .filter(|literal| literal.text.chars().any(|ch| ch.is_alphabetic()))
            .filter(|literal| dc_intl::keys_of(&literal.text).next().is_some())
            .filter(|literal| !translated(source, literal.start))
            .filter(|literal| exempt(source, literal).is_none())
            .filter(|literal| {
                !ALLOWED
                    .iter()
                    .any(|(known, text, _)| *known == file && *text == literal.text)
            })
            .map(|literal| {
                let line = source[..literal.start].matches('\n').count() + 1;
                format!("{file}:{line}: {:?}", literal.text)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    /// Every literal of the adopted files that an en-US message reads goes through `tr` / `t!`: a new one added
    /// without fails here, so the adoption does not rot.
    #[test]
    fn adopted_files_draw_no_untranslated_message() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        for file in super::guard::ADOPTED {
            let source = std::fs::read_to_string(src.join(file))
                .unwrap_or_else(|error| panic!("{file}: {error}"));
            found.extend(super::guard::untranslated(file, &source));
        }
        assert!(
            found.is_empty(),
            "untranslated messages (wrap them in dc_intl::tr, or list them in guard::ALLOWED): {found:#?}"
        );
    }

    /// The guard sees a bare message, and passes the translated, compared, keyed and test ones.
    #[test]
    fn the_guard_flags_a_bare_message_only() {
        let source = concat!(
            "fn view() {\n",
            "    div().child(\"Settings\");\n",
            "    div().child(dc_intl::tr(\"Settings\"));\n",
            "    div().child(dc_intl::tr(if on { \"Mute\" } else { \"Unmute\" }));\n",
            "    if label == \"Settings\" {}\n",
            "    div().id(\"Settings\");\n",
            "}\n",
            "const LABELS: [&str; 1] = [\"Settings\"];\n",
            "#[cfg(test)]\nmod tests {\n    const X: &str = \"Settings\";\n    fn t() { div().child(\"Settings\"); }\n}\n",
        );
        let found = super::guard::untranslated("example.rs", source);
        assert_eq!(found, ["example.rs:2: \"Settings\""]);
    }

    /// The string literals inside every `dc_intl::tr(...)` of the app's sources.
    fn adopted_literals() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && path.file_name().is_some_and(|name| name != "intl.rs")
                {
                    let source = std::fs::read_to_string(&path).unwrap_or_default();
                    for (start, _) in source.match_indices("dc_intl::tr(") {
                        let rest = &source[start + "dc_intl::tr(".len()..];
                        // The argument: up to the parenthesis that closes the call.
                        let mut depth = 0;
                        let mut end = rest.len();
                        let mut in_string = false;
                        let mut escaped = false;
                        for (index, ch) in rest.char_indices() {
                            if in_string {
                                match (escaped, ch) {
                                    (true, _) => escaped = false,
                                    (false, '\\') => escaped = true,
                                    (false, '"') => in_string = false,
                                    _ => {}
                                }
                                continue;
                            }
                            match ch {
                                '"' => in_string = true,
                                '(' | '[' | '{' => depth += 1,
                                ')' | ']' | '}' if depth == 0 => {
                                    end = index;
                                    break;
                                }
                                ')' | ']' | '}' => depth -= 1,
                                _ => {}
                            }
                        }
                        let argument = &rest[..end];
                        for literal in super::guard::literals(argument) {
                            out.push((path.display().to_string(), literal.text));
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        out
    }

    #[test]
    fn every_adopted_literal_is_an_en_us_message() {
        let adopted = adopted_literals();
        assert!(adopted.len() > 40, "{} literals", adopted.len());
        let unknown: Vec<String> = adopted
            .iter()
            .filter(|(_, text)| dc_intl::keys_of(text).next().is_none())
            .map(|(file, text)| format!("{file}: {text:?}"))
            .collect();
        assert!(unknown.is_empty(), "no literal en-US message: {unknown:#?}");
    }

    /// The fetch of a normal run, stood in for: the main catalog arrives (the other chunks fail), the locale installs
    /// and its messages read from it; nothing is written to the disk cache.
    #[test]
    fn a_fetched_catalog_installs_its_locale() {
        let key = dc_intl::keys_of("Settings")
            .next()
            .expect("Settings is a message");
        let mut asked = Vec::new();
        let result = super::load_from(
            "lt",
            |_| None,
            |files| {
                asked = files.to_vec();
                Ok(files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        let json = if index == 0 {
                            Ok(format!("{{\"{key}\":[\"Nustatymai\"]}}"))
                        } else {
                            Err("offline".to_owned())
                        };
                        (*file, json)
                    })
                    .collect())
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(asked, dc_intl::chunk_files("lt").collect::<Vec<_>>());
        assert!(dc_intl::is_loaded("lt"));
        assert_eq!(dc_intl::t_in("lt", key), Some("Nustatymai"));
    }

    /// A fetch that fails leaves the locale uninstalled: its frames show en-US (`MessageLoader.get`'s fallback) and a
    /// later choice of the locale tries again.
    #[test]
    fn a_failed_fetch_falls_back_to_english() {
        let result = super::load_from(
            "fi",
            |_| None,
            |files| {
                Ok(files
                    .iter()
                    .map(|file| (*file, Err("offline".to_owned())))
                    .collect())
            },
        );
        assert!(result.is_err(), "{result:?}");
        assert!(!dc_intl::is_loaded("fi"));
        let key = dc_intl::keys_of("Settings")
            .next()
            .expect("Settings is a message");
        assert_eq!(dc_intl::t_in("fi", key), None);
        assert_eq!(dc_intl::en_us_text(key), Some("Settings"));
        let error = super::load_from("fi", |_| None, |_| Err("no network".to_owned()));
        assert_eq!(error, Err("no network".to_owned()));
    }

    #[test]
    fn locale_stacks_follow_the_css() {
        assert_eq!(super::primary_stack("ru"), super::primary_stack("en-US"));
        assert!(super::primary_stack("ja").contains(&"MS PGothic"));
        assert_eq!(super::primary_stack("zh-TW")[0], "Microsoft JhengHei");
        assert!(super::primary_stack("ko").contains(&"Malgun Gothic"));
    }
}
