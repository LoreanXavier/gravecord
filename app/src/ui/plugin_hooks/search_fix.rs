//! SearchFix (Equicord `src/equicordplugins/searchFix`, GPL-3.0-or-later): a search page past offset 5000 (the most
//! the server accepts, "We dropped the magnifying glass!") is asked for in the other sort order from offset 0
//! (`main`: `offset > 5000 - 5000` always holds there). The plugin card carries its note. Not part of the pinned
//! client.

const ID: &str = "SearchFix";
/// The card's `settingsAboutComponent`.
pub(in crate::ui) const NOTE: &str = "This fix isn't perfect, so you may have to reload the search bar to fix issues. \
Discord only allows a max offset of 5000 (this is what causes the magnifying glass error). This means that you can \
only see precisely 5000 messages into the past, and 5000 messages into the future (when sorting by old). This plugin \
just jumps to the opposite sorting method to try get around Discord's restriction, but if there is a large search \
result, and you try to view a message that is unobtainable with both methods of sorting, the plugin will simply show \
offset 0 (either newest or oldest message depending on the sorting method).";

/// `main`: the sort order (oldest first) and offset the request carries.
fn fixed(oldest_first: bool, offset: u64) -> (bool, u64) {
    if offset > 5000 {
        (!oldest_first, 0)
    } else {
        (oldest_first, offset)
    }
}

/// The search request's sort order and offset with the plugin's fix (as they are while it is off).
pub(in crate::ui) fn search_fix(oldest_first: bool, offset: u64) -> (bool, u64) {
    if crate::plugins::bypassed() || !crate::plugins::on(ID) {
        return (oldest_first, offset);
    }
    fixed(oldest_first, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_5000_the_order_flips_from_the_start() {
        assert_eq!(fixed(false, 5000), (false, 5000));
        assert_eq!(fixed(false, 5025), (true, 0));
        assert_eq!(fixed(true, 9000), (false, 0));
    }
}
