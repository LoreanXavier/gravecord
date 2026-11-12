//! The popout over a reaction pill (module 30492 `e_` for the text, `eT` for the box, CSS `reactionTooltip_5l5ajC`
//! in module 912842). It names the first three reactors and counts the rest.

use dc_state::User;

/// How long the pointer rests on a pill before the popout opens (`setTimeout(..., 500)` in module 30492).
pub(super) const DELAY_MS: u64 = 500;
/// The most reactors the popout names (`take(3)`).
pub(super) const NAMED: usize = 3;

/// The emoji's name as the popout writes it (`b3` in module 815807): `:name:` for a custom emoji, the first
/// shortcode of a unicode one.
pub(super) fn emoji_label(name: &str, custom: bool) -> String {
    if custom {
        return format!(":{name}:");
    }
    let plain = |text: &str| text.replace('\u{fe0f}', "");
    dc_tokens::emoji_data::EMOJIS
        .iter()
        .find(|emoji| plain(emoji.surrogates) == plain(name))
        .and_then(|emoji| emoji.names.first())
        .map_or_else(|| name.to_owned(), |first| format!(":{first}:"))
}

/// The route's first reactors, as display names.
pub(super) fn names(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(User::from_value)
        .take(NAMED)
        .map(|user| user.display_name().to_owned())
        .collect()
}

/// The sentence for `names` and the reaction's `count` (the `reactionTooltip*` strings): one to three names, then
/// "and N other(s)" for everyone else. Nothing when no name is known.
pub(super) fn text(label: &str, names: &[String], count: usize) -> Option<String> {
    let rest = count.saturating_sub(names.len());
    let n = rest;
    Some(match (names, rest) {
        ([], _) => return None,
        ([a], 0) => dc_intl::t!("Oro30L", emojiName = label, a = a),
        ([a], _) => dc_intl::t!("mXild1", emojiName = label, a = a, n = n),
        ([a, b], 0) => dc_intl::t!("p+0jvt", emojiName = label, a = a, b = b),
        ([a, b], _) => dc_intl::t!("UWGs2n", emojiName = label, a = a, b = b, n = n),
        ([a, b, c, ..], 0) => dc_intl::t!("bbPMcR", emojiName = label, a = a, b = b, c = c),
        ([a, b, c, ..], _) => {
            dc_intl::t!("UnXdX/", emojiName = label, a = a, b = b, c = c, n = n)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn one_name() {
        assert_eq!(
            text(":joy:", &list(&["ada"]), 1).as_deref(),
            Some(":joy: reacted by ada")
        );
        assert_eq!(
            text(":joy:", &list(&["ada"]), 2).as_deref(),
            Some(":joy: reacted by ada and 1 other")
        );
    }

    #[test]
    fn two_and_three_names() {
        assert_eq!(
            text(":a:", &list(&["x", "y"]), 2).as_deref(),
            Some(":a: reacted by x and y")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y"]), 5).as_deref(),
            Some(":a: reacted by x, y, and 3 others")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y", "z"]), 3).as_deref(),
            Some(":a: reacted by x, y, and z")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y", "z"]), 9).as_deref(),
            Some(":a: reacted by x, y, z, and 6 others")
        );
    }

    #[test]
    fn nobody_known_means_no_popout() {
        assert_eq!(text(":a:", &[], 4), None);
    }

    #[test]
    fn labels() {
        assert_eq!(emoji_label("blob", true), ":blob:");
        assert_eq!(emoji_label("\u{1f602}", false), ":joy:");
    }
}
