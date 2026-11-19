//! GifProviderSwitcher (Vencord `src/plugins/gifProviderSwitcher`, formerly TenorGifSearch; GPL-3.0-or-later): the
//! GIF picker's search, trending results, category tiles and shares go to Tenor or Giphy (`provider`, Tenor by
//! default) instead of Discord's Klipy, the search field reads "Search Tenor", and each GIF tile carries its source's
//! badge (Tenor, Giphy or Klipy, by its id or page's host) at its bottom left. Not part of the pinned client.
//!
//! GAP: the provider choice beside the picker's search field and several providers at once (`provider` is chosen in
//! the plugin's settings, one at a time; the original's first non-Klipy one is the one asked), the search suggestions
//! (the app's picker shows none), and the `/tenor` command (the app has no GIF integration commands yet).

use crate::plugins::State;
use crate::plugins::gif_provider::Provider;

/// The backup's (and the registry's) name of the plugin.
const ID: &str = "TenorGifSearch";

fn provider_of(state: &State) -> Option<Option<Provider>> {
    if !state.is_on(ID) {
        return None;
    }
    let key = |name: &str| state.value(ID, name);
    Some(Provider::from_name(
        key("provider").as_str().unwrap_or("tenor"),
    ))
}

/// `shouldReplace` and `provider`: Tenor or Giphy while the plugin is on and not set to Klipy.
pub(in crate::ui) fn gif_provider() -> Option<Provider> {
    if crate::plugins::bypassed() {
        return None;
    }
    crate::plugins::with(provider_of).flatten()
}

/// `renderProviderBadge`: the label of a GIF's source, while the plugin is on.
pub(in crate::ui) fn gif_provider_badge(id: &str, url: &str) -> Option<&'static str> {
    if crate::plugins::bypassed() || crate::plugins::with(provider_of).is_none() {
        return None;
    }
    let by_id = id.split(':').next().unwrap_or_default();
    let host = url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    [("tenor", "Tenor"), ("giphy", "Giphy"), ("klipy", "Klipy")]
        .iter()
        .find(|(name, _)| {
            by_id == *name
                || host == format!("{name}.com")
                || host.ends_with(&format!(".{name}.com"))
        })
        .map(|(_, label)| *label)
}

/// The search field's placeholder ("Klipy" turned into the provider's name).
pub(in crate::ui) fn gif_provider_placeholder() -> Option<String> {
    gif_provider().map(|provider| format!("Search {}", provider.label()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn the_provider_and_badges_follow_the_original() {
        let mut state = State::default();
        assert_eq!(provider_of(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(provider_of(&state), Some(Some(Provider::Tenor)));
        state.set_value(ID, "provider", &Value::from("klipy"));
        assert_eq!(provider_of(&state), Some(None));
    }
}
