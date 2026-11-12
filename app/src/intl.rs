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
            // The locale keeps showing en-US; the next change of locale tries again.
            Err(error) => tracing::warn!(locale = code, %error, "interface catalog unavailable"),
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

#[cfg(test)]
mod tests {
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
                        for (index, part) in argument.split('"').enumerate() {
                            if index % 2 == 1 {
                                out.push((path.display().to_string(), part.to_owned()));
                            }
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

    #[test]
    fn locale_stacks_follow_the_css() {
        assert_eq!(super::primary_stack("ru"), super::primary_stack("en-US"));
        assert!(super::primary_stack("ja").contains(&"MS PGothic"));
        assert_eq!(super::primary_stack("zh-TW")[0], "Microsoft JhengHei");
        assert!(super::primary_stack("ko").contains(&"Malgun Gothic"));
    }
}
