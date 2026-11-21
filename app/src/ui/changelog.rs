//! What's New changelog modal (module 837601 + 256311 + 559868 + 883600).
//!
//! Renders the four official modal states from module `837601`:
//! - Loading (`!loaded && changelog == null`)
//! - Out-of-date (`V9ospk`: "Your client is out of date...", `BddRzS` "Okay")
//! - Failed to load (`O1iRT8`: "Failed to load", `BddRzS` "Okay")
//! - Loaded (`LRmNAl` "What's New", `Fb8xx2` formatted date subtitle, `HzBchE` scrollable
//!   markdown + optional 432x240 YouTube/image media banner, social links footer Twitter `/84uiD`,
//!   Facebook `h0or/l`, Instagram `5uVPyf`, `EjVVI7` "Follow us for more updates!").
//!
//! CDN endpoints (`256311` / `559868` / `883600`):
//! - `GET https://cdn.discordapp.com/changelogs/config_0.json?x={bucket}`
//! - `GET https://cdn.discordapp.com/changelogs/0/{id}/{locale}.json?x={bucket}`
//!
//! Marks the latest seen changelog in local storage (`lastChangeLogDate`, `lastSeenChangelogId`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, img, prelude::*, px, rgb, rgba};
use serde_json::Value;
use std::borrow::Cow;

pub(super) const CONFIG_KEY: &str = "changelog:config";
pub(super) const ENTRY_KEY_PREFIX: &str = "changelog:entry:";

const TWITTER_URL: &str = "https://twitter.com/discord";
const FACEBOOK_URL: &str = "https://www.facebook.com/discord/";
const INSTAGRAM_URL: &str = "https://www.instagram.com/discord/";
const LAST_SEEN_ID_KEY: &str = "lastSeenChangelogId";
const LAST_SEEN_DATE_KEY: &str = "lastChangeLogDate";

/// Asset type matching module `883600` (`0 = YOUTUBE_VIDEO_ID`, `1 = IMAGE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangelogAssetKind {
    YoutubeVideo,
    Image,
}

/// Parsed changelog entry from CDN `{id}/{locale}.json` (module `883600`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangelogEntry {
    pub id: String,
    pub date: String,
    pub locale: String,
    pub content: String,
    pub asset_kind: Option<ChangelogAssetKind>,
    pub asset: Option<String>,
}

/// Modal state for module `837601`.
#[derive(Clone, Debug)]
pub(super) enum ChangelogModal {
    Loading,
    OutOfDate,
    Failed,
    Loaded(ChangelogEntry),
}

/// Five-minute cache bucket (`Math.floor(Date.now() / 60000 / 5)` in module `256311`).
pub fn cache_bucket(now_unix_secs: i64) -> i64 {
    now_unix_secs.max(0) / 300
}

/// CDN URL for desktop (`platform = 0`) changelog index (`256311`).
pub fn config_url(now_unix_secs: i64) -> String {
    format!(
        "https://cdn.discordapp.com/changelogs/config_0.json?x={}",
        cache_bucket(now_unix_secs)
    )
}

/// CDN URL for a specific changelog entry (`256311`).
pub fn entry_url(id: &str, locale: &str, now_unix_secs: i64) -> String {
    format!(
        "https://cdn.discordapp.com/changelogs/0/{id}/{locale}.json?x={}",
        cache_bucket(now_unix_secs)
    )
}

/// Resolves the latest eligible changelog ID from `config_0.json`.
/// Returns `Ok(Some(id))` if found, `Ok(None)` when config is non-empty but no entry matches
/// (`clientTooOld` in module `837601`), or `Err(())` when config is empty/malformed.
pub fn pick_latest_id(config: &Value) -> Result<Option<String>, ()> {
    let Some(map) = config.as_object() else {
        return Err(());
    };
    if map.is_empty() {
        return Err(());
    }
    let mut best: Option<(String, String)> = None;
    for (id, entry) in map {
        let min_ver = entry["min_version"].as_str().unwrap_or("");
        if min_ver.contains("999999") {
            continue;
        }
        let date = entry["date"]
            .as_str()
            .or_else(|| entry["publish_date"].as_str())
            .unwrap_or(id)
            .to_owned();
        if best.as_ref().is_none_or(|(b_date, _)| date > *b_date) {
            best = Some((date, id.clone()));
        }
    }
    Ok(best.map(|(_, id)| id))
}

/// Parses a `{id}/{locale}.json` payload into [`ChangelogEntry`] (module `883600`).
pub fn parse_changelog_entry(id: &str, body: &Value) -> Option<ChangelogEntry> {
    let content = body["content"].as_str()?.to_owned();
    let date = body["date"].as_str().unwrap_or("").to_owned();
    let locale = body["locale"].as_str().unwrap_or("en-US").to_owned();
    let asset = body["asset"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let asset_kind = asset.as_ref().map(|_| match body["asset_type"].as_u64() {
        Some(0) => ChangelogAssetKind::YoutubeVideo,
        _ => ChangelogAssetKind::Image,
    });
    Some(ChangelogEntry {
        id: id.to_owned(),
        date,
        locale,
        content,
        asset_kind,
        asset,
    })
}

/// Persists `lastSeenChangelogId` and `lastChangeLogDate` (`559868`).
pub fn mark_changelog_seen(id: &str, date: &str) {
    crate::storage::set(LAST_SEEN_ID_KEY, serde_json::json!(id));
    crate::storage::set(LAST_SEEN_DATE_KEY, serde_json::json!(date));
}

/// Splits changelog markdown body into styled blocks (headers, bullets, paragraphs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangelogBlock {
    Heading { text: String, tag: Option<String> },
    Bullet(String),
    Paragraph(String),
}

pub fn parse_markdown_blocks(content: &str) -> Vec<ChangelogBlock> {
    let mut out = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let trimmed = rest.trim_start_matches('#').trim();
            // Discord changelogs use `{added marginTop}` / `{fixed}` / `{improved}` / `{progress}` tags.
            let (text, tag) = if let Some((head, brace)) = trimmed.split_once('{') {
                let tag_word = brace
                    .trim_end_matches('}')
                    .split_whitespace()
                    .next()
                    .map(str::to_owned);
                (head.trim().to_owned(), tag_word)
            } else {
                (trimmed.to_owned(), None)
            };
            if !text.is_empty() {
                out.push(ChangelogBlock::Heading { text, tag });
            }
        } else if let Some(item) = line.strip_prefix("* ").or_else(|| line.strip_prefix("- ")) {
            out.push(ChangelogBlock::Bullet(
                item.trim().replace("**", "").replace("__", ""),
            ));
        } else {
            out.push(ChangelogBlock::Paragraph(
                line.replace("**", "").replace("__", ""),
            ));
        }
    }
    out
}

fn format_changelog_date(date: &str) -> String {
    let formatted = dc_intl::format(
        "Fb8xx2",
        &[(
            "date",
            Arg::When {
                date: Cow::Borrowed(date),
                time: Cow::Borrowed(""),
            },
        )],
    );
    if formatted.trim().is_empty() {
        date.to_owned()
    } else {
        formatted
    }
}

#[cfg(feature = "capture")]
fn demo_changelog_entry() -> ChangelogEntry {
    ChangelogEntry {
        id: "2026-11-18".into(),
        date: "November 18, 2026".into(),
        locale: "en-US".into(),
        content: "# New Features {added}\n* Faster voice and screen share frame pacing across high-refresh displays.\n* Favorite GIFs directly from the message hover overlay and expanded lightbox.\n# Improvements {improved}\n* Popout device menus now adapt cleanly to long Windows audio device names.\n* Profile effects and animated nameplates now respect hover and click animations.\n# Bug Fixes {fixed}\n* Fixed Backup & Restore file picker blocking the main window.".into(),
        asset_kind: Some(ChangelogAssetKind::YoutubeVideo),
        asset: Some("dQw4w9WgXcQ".into()),
    }
}

impl Root {
    /// Opens the What's New changelog modal (module `837601`) and fetches `config_0.json`.
    pub(super) fn open_changelog_modal(&mut self, cx: &mut Context<Self>) {
        self.changelog_modal = Some(ChangelogModal::Loading);
        let now = chrono::Utc::now().timestamp();
        let _ = self.requests.send(Request::Fetch {
            key: CONFIG_KEY.to_owned(),
            route: config_url(now),
        });
        cx.notify();
    }

    /// Closes the changelog modal; marks it seen if loaded (module `837601` unmount).
    pub(super) fn close_changelog_modal(&mut self) {
        if let Some(ChangelogModal::Loaded(entry)) = self.changelog_modal.take() {
            mark_changelog_seen(&entry.id, &entry.date);
        }
    }

    /// Handles CDN responses for `CONFIG_KEY` and `ENTRY_KEY_PREFIX`.
    pub(super) fn handle_changelog_response(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        if self.changelog_modal.is_none() {
            return;
        }
        if key == CONFIG_KEY {
            match pick_latest_id(body) {
                Ok(Some(id)) => {
                    let now = chrono::Utc::now().timestamp();
                    let _ = self.requests.send(Request::Fetch {
                        key: format!("{ENTRY_KEY_PREFIX}{id}"),
                        route: entry_url(&id, "en-US", now),
                    });
                }
                Ok(None) => {
                    self.changelog_modal = Some(ChangelogModal::OutOfDate);
                }
                Err(()) => {
                    self.changelog_modal = Some(ChangelogModal::Failed);
                }
            }
            cx.notify();
            return;
        }
        if let Some(id) = key.strip_prefix(ENTRY_KEY_PREFIX) {
            if let Some(entry) = parse_changelog_entry(id, body) {
                if entry.asset_kind == Some(ChangelogAssetKind::Image)
                    && let Some(url) = entry.asset.as_ref()
                {
                    self.request_image(format!("changelog-img:{id}"), url.clone());
                } else if entry.asset_kind == Some(ChangelogAssetKind::YoutubeVideo)
                    && let Some(vid) = entry.asset.as_ref()
                {
                    self.request_image(
                        format!("changelog-yt:{vid}"),
                        format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"),
                    );
                }
                self.changelog_modal = Some(ChangelogModal::Loaded(entry));
            } else {
                self.changelog_modal = Some(ChangelogModal::Failed);
            }
            cx.notify();
        }
    }

    /// Renders module `837601` What's New Changelog Modal.
    pub(super) fn changelog_modal_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.changelog_modal.as_ref()?;
        let card: AnyElement = match modal {
            ChangelogModal::Loading => div()
                .id("changelog-loading-modal")
                .w(px(480.))
                .h(px(220.))
                .rounded(px(12.))
                .bg(color("--background-surface-high"))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("LRmNAl")),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("ZTNur7")),
                )
                .into_any_element(),
            ChangelogModal::OutOfDate | ChangelogModal::Failed => {
                let title_key = if matches!(modal, ChangelogModal::OutOfDate) {
                    "V9ospk"
                } else {
                    "O1iRT8"
                };
                div()
                    .id("changelog-status-modal")
                    .w(px(440.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(18.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(700.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t(title_key)),
                            )
                            .child(
                                div()
                                    .id("changelog-status-close")
                                    .size(px(28.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_changelog_modal();
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    .child(
                        div().flex().justify_end().child(
                            div()
                                .id("changelog-status-ok")
                                .h(px(38.))
                                .px(px(20.))
                                .rounded(px(8.))
                                .bg(color("--control-primary-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_changelog_modal();
                                    cx.notify();
                                }))
                                .child(dc_intl::t("BddRzS")),
                        ),
                    )
                    .into_any_element()
            }
            ChangelogModal::Loaded(entry) => {
                let subtitle = format_changelog_date(&entry.date);
                let blocks = parse_markdown_blocks(&entry.content);
                let media_el: Option<AnyElement> =
                    entry.asset.as_ref().map(|asset| match entry.asset_kind {
                        Some(ChangelogAssetKind::YoutubeVideo) => {
                            let url = format!("https://youtu.be/{asset}");
                            let thumb = self.images.get(&format!("changelog-yt:{asset}")).cloned();
                            div()
                                .id("changelog-youtube-banner")
                                .w(px(432.))
                                .h(px(200.))
                                .rounded(px(8.))
                                .overflow_hidden()
                                .relative()
                                .bg(color("--background-base-lowest"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                                .children(thumb.map(|pic| {
                                    img(pic).size_full().object_fit(gpui::ObjectFit::Cover)
                                }))
                                .child(
                                    div()
                                        .size(px(52.))
                                        .rounded(px(26.))
                                        .bg(rgba(0x000000aa))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("clips_play", 24., "--white")),
                                )
                                .into_any_element()
                        }
                        _ => {
                            let pic = self
                                .images
                                .get(&format!("changelog-img:{}", entry.id))
                                .cloned();
                            div()
                                .w(px(432.))
                                .h(px(200.))
                                .rounded(px(8.))
                                .overflow_hidden()
                                .bg(color("--background-base-lowest"))
                                .children(
                                    pic.map(|p| {
                                        img(p).size_full().object_fit(gpui::ObjectFit::Cover)
                                    }),
                                )
                                .into_any_element()
                        }
                    });

                div()
                    .id("changelog-loaded-modal")
                    .w(px(480.))
                    .max_h(px(620.))
                    .rounded(px(12.))
                    .bg(color("--background-surface-high"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    // Header: `LRmNAl` "What's New" + `Fb8xx2` formatted date + close X
                    .child(
                        div()
                            .px(px(24.))
                            .pt(px(20.))
                            .pb(px(14.))
                            .border_b_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_start()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("LRmNAl")),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-muted"))
                                            .child(subtitle),
                                    ),
                            )
                            .child(
                                div()
                                    .id("changelog-close")
                                    .size(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_changelog_modal();
                                        cx.notify();
                                    }))
                                    .child(icon("modal_close", 16., "--icon-subtle")),
                            ),
                    )
                    // Scrollable content region (`HzBchE`: "Changelog content")
                    .child(
                        div()
                            .id("changelog-scroll-body")
                            .aria_label(dc_intl::t("HzBchE"))
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .px(px(24.))
                            .py(px(16.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .children(media_el)
                            .children(blocks.into_iter().map(|block| {
                                match block {
                                    ChangelogBlock::Heading { text, tag } => {
                                        let accent = match tag.as_deref() {
                                            Some("added") => rgb(0x23a55a),
                                            Some("fixed") => rgb(0xf23f43),
                                            Some("improved") => rgb(0x5865f2),
                                            Some("progress") => rgb(0xf0b232),
                                            _ => color("--text-strong"),
                                        };
                                        div()
                                            .pt(px(4.))
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(accent)
                                                    .child(SharedString::from(text.to_uppercase())),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h(px(1.))
                                                    .bg(color("--border-subtle")),
                                            )
                                            .into_any_element()
                                    }
                                    ChangelogBlock::Bullet(text) => div()
                                        .flex()
                                        .items_start()
                                        .gap(px(8.))
                                        .pl(px(4.))
                                        .child(
                                            div()
                                                .mt(px(7.))
                                                .size(px(5.))
                                                .rounded(px(3.))
                                                .flex_none()
                                                .bg(color("--text-muted")),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(14.))
                                                .line_height(px(20.))
                                                .text_color(color("--text-default"))
                                                .child(SharedString::from(text)),
                                        )
                                        .into_any_element(),
                                    ChangelogBlock::Paragraph(text) => div()
                                        .text_size(px(14.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-default"))
                                        .child(SharedString::from(text))
                                        .into_any_element(),
                                }
                            })),
                    )
                    // Footer (`actionBarInput`: Twitter `/84uiD`, Facebook `h0or/l`, Instagram `5uVPyf`, `EjVVI7`)
                    .child(
                        div()
                            .px(px(24.))
                            .py(px(14.))
                            .bg(color("--background-base-lower"))
                            .border_t_1()
                            .border_color(color("--border-subtle"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .id("changelog-social-twitter")
                                            .aria_label(dc_intl::t("/84uiD"))
                                            .size(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.open_url(TWITTER_URL);
                                            }))
                                            .child(icon("link_x", 16., "--icon-subtle")),
                                    )
                                    .child(
                                        div()
                                            .id("changelog-social-facebook")
                                            .aria_label(dc_intl::t("h0or/l"))
                                            .size(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.open_url(FACEBOOK_URL);
                                            }))
                                            .child(icon("link_facebook", 16., "--icon-subtle")),
                                    )
                                    .child(
                                        div()
                                            .id("changelog-social-instagram")
                                            .aria_label(dc_intl::t("5uVPyf"))
                                            .size(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.open_url(INSTAGRAM_URL);
                                            }))
                                            .child(icon("link_instagram", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(color("--text-muted"))
                                    .child(dc_intl::t("EjVVI7")),
                            ),
                    )
                    .into_any_element()
            }
        };

        Some(
            gpui::deferred(
                div()
                    .id("changelog-backdrop")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_changelog_modal();
                        cx.notify();
                    }))
                    .child(card),
            )
            .with_priority(6)
            .into_any_element(),
        )
    }

    /// Capture helper: `changelog [loaded|loading|out-of-date|failed|close]`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_changelog(&mut self, mode: &str, cx: &mut Context<Self>) -> String {
        match mode.trim() {
            "loading" => {
                self.changelog_modal = Some(ChangelogModal::Loading);
                cx.notify();
                "changelog: loading".into()
            }
            "out-of-date" => {
                self.changelog_modal = Some(ChangelogModal::OutOfDate);
                cx.notify();
                "changelog: out-of-date".into()
            }
            "failed" => {
                self.changelog_modal = Some(ChangelogModal::Failed);
                cx.notify();
                "changelog: failed".into()
            }
            "close" => {
                self.close_changelog_modal();
                cx.notify();
                "changelog: closed".into()
            }
            _ => {
                self.changelog_modal = Some(ChangelogModal::Loaded(demo_changelog_entry()));
                cx.notify();
                "changelog: loaded".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pick_latest_id_selects_newest_and_detects_out_of_date() {
        let cfg = json!({
            "2026-10-01": { "date": "2026-10-01" },
            "2026-11-18": { "date": "2026-11-18" },
        });
        assert_eq!(pick_latest_id(&cfg), Ok(Some("2026-11-18".into())));

        let old_cfg = json!({
            "future": { "date": "2027-01-01", "min_version": "999999.0" }
        });
        assert_eq!(pick_latest_id(&old_cfg), Ok(None));
        assert_eq!(pick_latest_id(&json!({})), Err(()));
    }

    #[test]
    fn parses_changelog_entry_and_markdown_blocks() {
        let entry = parse_changelog_entry(
            "2026-11-18",
            &json!({
                "date": "2026-11-18",
                "locale": "en-US",
                "content": "# Added {added}\n* Item one\nSummary line",
                "asset_type": 0,
                "asset": "abc123"
            }),
        )
        .expect("entry");
        assert_eq!(entry.asset_kind, Some(ChangelogAssetKind::YoutubeVideo));
        let blocks = parse_markdown_blocks(&entry.content);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(
            &blocks[0],
            ChangelogBlock::Heading { tag: Some(t), .. } if t == "added"
        ));
    }
}
