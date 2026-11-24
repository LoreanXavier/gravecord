//! The popout over a reaction pill (module 30492 `e_` for the text, `eT` for the box, CSS `reactionTooltip_5l5ajC`
//! in module 912842). It names the first three reactors and counts the rest.

use dc_state::User;

/// How long the pointer rests on a pill before the popout opens (`setTimeout(..., 500)` in module 30492).
pub(super) const DELAY_MS: u64 = 500;
/// The same for a super reaction's pill (`t?750:500`).
pub(super) const BURST_DELAY_MS: u64 = 750;
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

/// The sentence for `names` and the reaction's `count` (the `reactionTooltip*` strings, or their super reaction
/// forms for a `burst` pill): one to three names, then "and N other(s)" for everyone else. Nothing when no name is
/// known.
pub(super) fn text(label: &str, names: &[String], count: usize, burst: bool) -> Option<String> {
    let n = count.saturating_sub(names.len());
    Some(match (names, n, burst) {
        ([], _, _) => return None,
        ([a], 0, false) => dc_intl::t!("Oro30L", emojiName = label, a = a),
        ([a], _, false) => dc_intl::t!("mXild1", emojiName = label, a = a, n = n),
        ([a, b], 0, false) => dc_intl::t!("p+0jvt", emojiName = label, a = a, b = b),
        ([a, b], _, false) => dc_intl::t!("UWGs2n", emojiName = label, a = a, b = b, n = n),
        ([a, b, c, ..], 0, false) => {
            dc_intl::t!("bbPMcR", emojiName = label, a = a, b = b, c = c)
        }
        ([a, b, c, ..], _, false) => {
            dc_intl::t!("UnXdX/", emojiName = label, a = a, b = b, c = c, n = n)
        }
        ([a], 0, true) => dc_intl::t!("z4q3+w", emojiName = label, a = a),
        ([a], _, true) => dc_intl::t!("u/03eN", emojiName = label, a = a, n = n),
        ([a, b], 0, true) => dc_intl::t!("R2HykW", emojiName = label, a = a, b = b),
        ([a, b], _, true) => dc_intl::t!("T4EYUu", emojiName = label, a = a, b = b, n = n),
        ([a, b, c, ..], 0, true) => dc_intl::t!("sNl6XR", emojiName = label, a = a, b = b, c = c),
        ([a, b, c, ..], _, true) => {
            dc_intl::t!("M8bwl+", emojiName = label, a = a, b = b, c = c, n = n)
        }
    })
}

/// The line under a super reaction's popout (module 30492 `eT`): what the account did, or may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BurstPrompt {
    /// "You've sent this Super Reaction", with the Nitro mark for a subscriber.
    Sent { premium: bool },
    /// "Add Super Reaction", for a subscriber.
    Add,
    /// The Nitro upsell line, for everyone else.
    Upsell,
}

impl BurstPrompt {
    pub(super) fn of(me_burst: bool, premium: bool) -> Self {
        if me_burst {
            Self::Sent { premium }
        } else if premium {
            Self::Add
        } else {
            Self::Upsell
        }
    }
}

/// The text the reaction's emoji shows when it has no picture (module 565645): a unicode emoji the client has no file
/// for is drawn as text (`emoji emoji-text`, its name), and a picture that failed to load leaves its `alt` (the
/// emoji's name) in the box, as a broken image does. While a picture is on its way the box stays empty.
pub(super) fn emoji_text_fallback(
    name: &str,
    custom: bool,
    has_file: bool,
    failed: bool,
) -> Option<&str> {
    ((!custom && !has_file) || failed).then_some(name)
}

impl super::Root {
    /// A reaction pill's 20px emoji (`.reaction .emoji`), or its text when the picture is missing (`.emoji-text`:
    /// 20px, `--text-default`, clipped to the box).
    pub(super) fn reaction_emoji(
        &self,
        reaction: &dc_state::Reaction,
        image: Option<std::sync::Arc<gpui::Image>>,
    ) -> gpui::AnyElement {
        use gpui::{IntoElement, ParentElement, Styled, div, img, px};
        let key = match reaction.emoji_id {
            Some(id) => format!("emoji-id:{id}"),
            None => format!("emoji:{}", reaction.name),
        };
        let custom = reaction.emoji_id.is_some();
        let fallback = image
            .is_none()
            .then(|| {
                emoji_text_fallback(
                    &reaction.name,
                    custom,
                    custom || dc_tokens::emoji_file(&reaction.name).is_some(),
                    self.images.retries.failures(&key) > 0,
                )
            })
            .flatten();
        div()
            .size(px(20.))
            .flex_none()
            .overflow_hidden()
            .children(image.map(|image| img(image).size(px(20.))))
            .children(fallback.map(|text| {
                div()
                    .text_size(px(20.))
                    .line_height(px(20.))
                    .whitespace_nowrap()
                    .text_color(super::color("--text-default"))
                    .child(text.to_owned())
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn a_missing_emoji_picture_leaves_its_name() {
        // A unicode emoji without a file is text from the start.
        assert_eq!(emoji_text_fallback("x", false, false, false), Some("x"));
        // A picture on its way leaves the box empty; a failed one shows its alt.
        assert_eq!(emoji_text_fallback("\u{1f499}", false, true, false), None);
        assert_eq!(
            emoji_text_fallback("\u{1f499}", false, true, true),
            Some("\u{1f499}")
        );
        assert_eq!(emoji_text_fallback("blob", true, true, true), Some("blob"));
        assert_eq!(emoji_text_fallback("blob", true, true, false), None);
    }

    #[test]
    fn one_name() {
        assert_eq!(
            text(":joy:", &list(&["ada"]), 1, false).as_deref(),
            Some(":joy: reacted by ada")
        );
        assert_eq!(
            text(":joy:", &list(&["ada"]), 2, false).as_deref(),
            Some(":joy: reacted by ada and 1 other")
        );
    }

    #[test]
    fn two_and_three_names() {
        assert_eq!(
            text(":a:", &list(&["x", "y"]), 2, false).as_deref(),
            Some(":a: reacted by x and y")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y"]), 5, false).as_deref(),
            Some(":a: reacted by x, y, and 3 others")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y", "z"]), 3, false).as_deref(),
            Some(":a: reacted by x, y, and z")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y", "z"]), 9, false).as_deref(),
            Some(":a: reacted by x, y, z, and 6 others")
        );
    }

    #[test]
    fn super_reactions_have_their_own_sentences() {
        assert_eq!(
            text(":a:", &list(&["x"]), 1, true).as_deref(),
            Some(":a: Super reacted by x")
        );
        assert_eq!(
            text(":a:", &list(&["x", "y"]), 4, true).as_deref(),
            Some(":a: Super reacted by x, y, and 2 others")
        );
        assert_eq!(
            BurstPrompt::of(true, false),
            BurstPrompt::Sent { premium: false }
        );
        assert_eq!(BurstPrompt::of(false, true), BurstPrompt::Add);
        assert_eq!(BurstPrompt::of(false, false), BurstPrompt::Upsell);
    }

    #[test]
    fn nobody_known_means_no_popout() {
        assert_eq!(text(":a:", &[], 4, false), None);
    }

    #[test]
    fn labels() {
        assert_eq!(emoji_label("blob", true), ":blob:");
        assert_eq!(emoji_label("\u{1f602}", false), ":joy:");
    }
}
