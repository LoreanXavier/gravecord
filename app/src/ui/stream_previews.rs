//! The preview picture of a stream this client does not watch (`ApplicationStreamPreviewStore`, module 384200, and
//! `fetchStreamPreview` through module 450149): `GET /streams/<key>/preview` answers `{url}`. A picture is kept for
//! two minutes; a failure waits what the answer says, or ten seconds for each failure in a row, and after five
//! failures with no picture the stream is left alone. The tile draws the picture behind Watch Stream
//! (`image_hELJz2`: the whole tile, `object-fit: contain`).

use super::Root;
use dc_voice::streams::StreamKey;
use gpui::Context;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(super) const KEY_PREFIX: &str = "stream-preview:";

/// How long a fetched picture stands (`expires: Date.now() + 12e4`).
const KEPT_MS: f64 = 120_000.;

#[derive(Default)]
pub(super) struct Previews {
    /// The address, when one was answered, and when it stops standing.
    known: HashMap<String, (Option<String>, f64)>,
    fails: HashMap<String, u32>,
    loading: HashSet<String>,
}

impl Previews {
    /// `shouldFetchPreview`.
    pub fn should_fetch(&self, key: &str, now: f64) -> bool {
        if self.loading.contains(key) {
            return false;
        }
        match self.known.get(key) {
            None => self.fails.get(key).copied().unwrap_or(0) < 5,
            Some((_, expires)) => now > *expires,
        }
    }

    /// `STREAM_PREVIEW_FETCH_START`.
    pub fn start(&mut self, key: &str) {
        *self.fails.entry(key.to_owned()).or_default() += 1;
        self.loading.insert(key.to_owned());
    }

    /// `STREAM_PREVIEW_FETCH_SUCCESS`.
    pub fn success(&mut self, key: &str, url: String, now: f64) {
        self.known
            .insert(key.to_owned(), (Some(url), now + KEPT_MS));
        self.fails.insert(key.to_owned(), 0);
        self.loading.remove(key);
    }

    /// `STREAM_PREVIEW_FETCH_FAIL`: the last picture stays; `retry_after` is in milliseconds.
    pub fn fail(&mut self, key: &str, retry_after: Option<f64>, now: f64) {
        let url = self.known.get(key).and_then(|(url, _)| url.clone());
        let wait = retry_after
            .unwrap_or_else(|| 10_000. * f64::from(self.fails.get(key).copied().unwrap_or(0)));
        self.known.insert(key.to_owned(), (url, now + wait));
        self.loading.remove(key);
    }

    /// `getPreviewURL`.
    pub fn url(&self, key: &str) -> Option<&str> {
        self.known.get(key).and_then(|(url, _)| url.as_deref())
    }

    /// A voice state without `self_stream` forgets the stream.
    pub fn forget(&mut self, key: &str) {
        self.known.remove(key);
        self.fails.remove(key);
    }

    pub fn clear(&mut self) {
        self.known.clear();
        self.fails.clear();
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64() * 1000.)
}

/// The key of `owner`'s stream in a call or a server's voice channel.
pub(super) fn key_of(guild: u64, channel: u64, owner: u64) -> StreamKey {
    if guild != 0 {
        StreamKey::Guild {
            guild,
            channel,
            owner,
        }
    } else {
        StreamKey::Call { channel, owner }
    }
}

impl Root {
    /// Asks for the preview of a stream whose tile shows Watch Stream, when the store says it is time, and for the
    /// picture of an address already known.
    pub(super) fn stream_preview_want(&mut self, key: StreamKey) {
        let key = key.to_string();
        if let Some(url) = self.stream_previews.url(&key).map(str::to_owned) {
            self.request_image(url.clone(), url);
        }
        let now = now_ms();
        if self.stream_previews.should_fetch(&key, now) {
            self.stream_previews.start(&key);
            let _ = self.requests.send(crate::net::Request::Fetch {
                key: format!("{KEY_PREFIX}{key}"),
                // `query: {version: Date.now()}`.
                route: format!("/streams/{key}/preview?version={}", now as u64),
            });
        }
    }

    pub(super) fn stream_preview_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = key.strip_prefix(KEY_PREFIX) else {
            return;
        };
        let now = now_ms();
        match body["url"].as_str() {
            Some(url) => self.stream_previews.success(key, url.to_owned(), now),
            None => {
                let retry = body["body"]["retry_after"]
                    .as_f64()
                    .map(|seconds| seconds * 1000.);
                self.stream_previews.fail(key, retry, now);
            }
        }
        cx.notify();
    }

    /// The preview picture of `owner`'s stream, once fetched.
    pub(super) fn stream_preview_image(
        &self,
        guild: u64,
        channel: u64,
        owner: u64,
    ) -> Option<std::sync::Arc<gpui::Image>> {
        let key = key_of(guild, channel, owner).to_string();
        let url = self.stream_previews.url(&key)?;
        self.images.get(url).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_stands_for_two_minutes() {
        let mut store = Previews::default();
        assert!(store.should_fetch("k", 0.));
        store.start("k");
        assert!(!store.should_fetch("k", 0.));
        store.success("k", "https://x/p.jpg".to_owned(), 1000.);
        assert_eq!(store.url("k"), Some("https://x/p.jpg"));
        assert!(!store.should_fetch("k", 120_999.));
        assert!(store.should_fetch("k", 121_001.));
    }

    #[test]
    fn failures_wait_longer_each_time_and_keep_the_last_picture() {
        let mut store = Previews::default();
        store.start("k");
        store.success("k", "u".to_owned(), 0.);
        store.start("k");
        store.fail("k", None, 200_000.);
        assert_eq!(store.url("k"), Some("u"));
        // One failure in a row: ten seconds.
        assert!(!store.should_fetch("k", 209_999.));
        assert!(store.should_fetch("k", 210_001.));
        store.start("k");
        store.fail("k", Some(3000.), 300_000.);
        assert!(store.should_fetch("k", 303_001.));
    }

    #[test]
    fn a_stream_that_ended_is_forgotten() {
        let mut store = Previews::default();
        store.start("k");
        store.success("k", "u".to_owned(), 0.);
        store.forget("k");
        assert_eq!(store.url("k"), None);
        assert!(store.should_fetch("k", 1.));
    }
}
