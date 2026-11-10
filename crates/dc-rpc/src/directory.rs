//! Where the server learns about an application: its name (`GET /oauth2/applications/{id}/rpc`, module 90924 `G`)
//! and its asset list (`GET /oauth2/applications/{id}/assets`), plus external image links
//! (`POST /applications/{id}/external-assets`), with the caches and the key lookup of module 913659 (`RG`).

use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the host provides: the REST routes and the account the server speaks for.
pub trait Directory: Send + Sync + 'static {
    /// `GET /oauth2/applications/{id}/rpc`; `None` when the application does not exist (or the request failed).
    fn application(&self, id: &str) -> BoxFuture<'_, Option<Value>>;
    /// `GET /oauth2/applications/{id}/assets`: `[{id, type, name}]`.
    fn assets(&self, id: &str) -> BoxFuture<'_, Option<Vec<Value>>>;
    /// `POST /applications/{id}/external-assets` with `{urls}`: `[{url, external_asset_path}]`.
    fn external_assets(&self, id: &str, urls: Vec<String>) -> BoxFuture<'_, Option<Vec<Value>>>;
    /// The current user as `READY` carries it (module 639621), or `None` when logged out.
    fn current_user(&self) -> Option<Value>;
    /// `allowActivityPartyPrivacyFriends` and `allowActivityPartyPrivacyVoiceChannel` (module 253932).
    fn party_privacy(&self) -> (bool, bool) {
        (true, true)
    }
}

/// An application's asset list is fetched again once it is older than this (`R`, 36e5 ms).
const ASSETS_MAX_AGE: Duration = Duration::from_secs(3600);

struct Assets {
    at: Instant,
    by_name: HashMap<String, String>,
}

/// `ApplicationStore` (the applications seen), `ApplicationAssetsStore` and the external asset map `S`.
#[derive(Default)]
pub struct Cache {
    applications: Mutex<HashMap<String, Value>>,
    assets: Mutex<HashMap<String, Assets>>,
    external: Mutex<HashMap<String, String>>,
}

fn is_http(key: &str) -> bool {
    key.starts_with("http:") || key.starts_with("https:")
}

impl Cache {
    /// The application, from the store or from the server (`uM` fetches only what the store does not hold).
    pub async fn application(&self, dir: &dyn Directory, id: &str) -> Option<Value> {
        let known = self.applications.lock().expect("cache").get(id).cloned();
        if known.is_some() {
            return known;
        }
        let fetched = dir.application(id).await?;
        self.applications
            .lock()
            .expect("cache")
            .insert(id.to_owned(), fetched.clone());
        Some(fetched)
    }

    async fn asset_ids(
        &self,
        dir: &dyn Directory,
        id: &str,
        fresh: bool,
    ) -> HashMap<String, String> {
        if !fresh {
            let known = self
                .assets
                .lock()
                .expect("cache")
                .get(id)
                .filter(|known| known.at.elapsed() <= ASSETS_MAX_AGE)
                .map(|known| known.by_name.clone());
            if let Some(known) = known {
                return known;
            }
        }
        let listed = dir.assets(id).await.unwrap_or_default();
        // `keyBy(assets, "name")`: the last asset of a name wins.
        let by_name: HashMap<String, String> = listed
            .iter()
            .filter_map(|asset| {
                Some((
                    asset["name"].as_str()?.to_owned(),
                    asset["id"].as_str()?.to_owned(),
                ))
            })
            .collect();
        self.assets.lock().expect("cache").insert(
            id.to_owned(),
            Assets {
                at: Instant::now(),
                by_name: by_name.clone(),
            },
        );
        by_name
    }

    /// `RG`: the asset ids for the image keys a game sent (large, small, invite cover). A link becomes
    /// `mp:<external path>`; a key is looked up by name in the application's assets, which are fetched once more
    /// when a key is missing. A key that stays unknown comes back `None`.
    pub async fn resolve_assets(
        &self,
        dir: &dyn Directory,
        application: &str,
        keys: [Option<&str>; 3],
    ) -> [Option<String>; 3] {
        let mut out: [Option<String>; 3] = [None, None, None];
        let links: Vec<String> = keys
            .iter()
            .flatten()
            .filter(|key| is_http(key))
            .map(|key| (*key).to_owned())
            .collect();
        if !links.is_empty() {
            let unknown: Vec<String> = {
                let known = self.external.lock().expect("cache");
                links
                    .iter()
                    .filter(|link| !known.contains_key(*link))
                    .cloned()
                    .collect()
            };
            if !unknown.is_empty()
                && let Some(found) = dir.external_assets(application, unknown).await
            {
                let mut known = self.external.lock().expect("cache");
                for entry in found {
                    if let (Some(url), Some(path)) =
                        (entry["url"].as_str(), entry["external_asset_path"].as_str())
                    {
                        known.insert(url.to_owned(), path.to_owned());
                    }
                }
            }
            let known = self.external.lock().expect("cache");
            let mut resolved = 0;
            for (index, key) in keys.iter().enumerate() {
                if let Some(path) = key
                    .filter(|key| is_http(key))
                    .and_then(|key| known.get(key))
                {
                    out[index] = Some(format!("mp:{path}"));
                    resolved += 1;
                }
            }
            // The official check compares with the length of the whole list, so it holds only when all three keys
            // were links.
            if resolved == keys.len() {
                return out;
            }
        }
        let mut names = self.asset_ids(dir, application, false).await;
        let mut retried = false;
        loop {
            let mut missing = false;
            for (index, key) in keys.iter().enumerate() {
                let Some(key) = key else { continue };
                if out[index].is_some() {
                    continue;
                }
                match names.get(*key) {
                    Some(id) => out[index] = Some(id.clone()),
                    None => missing = true,
                }
            }
            if !missing || retried {
                return out;
            }
            retried = true;
            names = self.asset_ids(dir, application, true).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Fake {
        asset_fetches: AtomicUsize,
        assets: Mutex<Vec<Value>>,
    }

    impl Directory for Fake {
        fn application(&self, _: &str) -> BoxFuture<'_, Option<Value>> {
            Box::pin(async { Some(json!({"id": "1", "name": "Game"})) })
        }
        fn assets(&self, _: &str) -> BoxFuture<'_, Option<Vec<Value>>> {
            self.asset_fetches.fetch_add(1, Ordering::SeqCst);
            let listed = self.assets.lock().unwrap().clone();
            Box::pin(async move { Some(listed) })
        }
        fn external_assets(&self, _: &str, urls: Vec<String>) -> BoxFuture<'_, Option<Vec<Value>>> {
            Box::pin(async move {
                Some(
                    urls.iter()
                        .map(|url| json!({"url": url, "external_asset_path": "external/abc"}))
                        .collect(),
                )
            })
        }
        fn current_user(&self) -> Option<Value> {
            None
        }
    }

    #[tokio::test]
    async fn a_key_becomes_the_asset_id_and_the_list_is_cached() {
        let fake = Fake::default();
        *fake.assets.lock().unwrap() = vec![json!({"id": "77", "name": "logo", "type": 1})];
        let cache = Cache::default();
        let one = cache
            .resolve_assets(&fake, "1", [Some("logo"), None, None])
            .await;
        assert_eq!(one, [Some("77".to_owned()), None, None]);
        cache
            .resolve_assets(&fake, "1", [Some("logo"), None, None])
            .await;
        assert_eq!(fake.asset_fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_unknown_key_refetches_the_list_once_and_stays_unresolved() {
        let fake = Fake::default();
        let cache = Cache::default();
        let out = cache
            .resolve_assets(&fake, "1", [Some("nope"), None, None])
            .await;
        assert_eq!(out, [None, None, None]);
        assert_eq!(fake.asset_fetches.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_link_becomes_a_media_proxy_path() {
        let fake = Fake::default();
        *fake.assets.lock().unwrap() = vec![json!({"id": "77", "name": "logo", "type": 1})];
        let cache = Cache::default();
        let out = cache
            .resolve_assets(&fake, "1", [Some("https://a.b/c.png"), Some("logo"), None])
            .await;
        assert_eq!(
            out,
            [
                Some("mp:external/abc".to_owned()),
                Some("77".to_owned()),
                None
            ]
        );
    }

    #[tokio::test]
    async fn the_application_is_fetched_once() {
        let fake = Fake::default();
        let cache = Cache::default();
        assert_eq!(cache.application(&fake, "1").await.unwrap()["name"], "Game");
        assert!(cache.applications.lock().unwrap().contains_key("1"));
    }
}
