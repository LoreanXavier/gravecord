//! Design tokens of the pinned build, resolved per theme by `tools/dump-tokens.mjs`.

mod emoji;
mod generated;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    Darker,
    Midnight,
}

impl Theme {
    fn table(self) -> &'static [(&'static str, u32)] {
        match self {
            Self::Dark => generated::DARK,
            Self::Light => generated::LIGHT,
            Self::Darker => generated::DARKER,
            Self::Midnight => generated::MIDNIGHT,
        }
    }

    /// `0xRRGGBBAA` of a token. Unknown names are a programming error: they come from code, not from data.
    pub fn color(self, name: &str) -> u32 {
        let table = self.table();
        let found = table.binary_search_by(|(key, _)| (*key).cmp(name));
        match found {
            Ok(index) => table[index].1,
            Err(_) => panic!("unknown design token {name}"),
        }
    }
}

/// The asset file of a unicode emoji. Variation selectors are not part of the file names.
pub fn emoji_file(text: &str) -> Option<&'static str> {
    let codepoints: Vec<String> = text
        .chars()
        .filter(|character| *character != '\u{fe0f}')
        .map(|character| format!("{:x}", character as u32))
        .collect();
    let key = codepoints.join("-");
    let index = emoji::EMOJI_FILES
        .binary_search_by(|(known, _)| (*known).cmp(key.as_str()))
        .ok()?;
    Some(emoji::EMOJI_FILES[index].1)
}

#[cfg(test)]
mod tests {
    use super::{Theme, emoji_file};

    #[test]
    fn emoji_map_to_the_captured_assets() {
        assert_eq!(emoji_file("\u{1f44d}"), Some("a4faf6864a96a042.svg"));
        assert_eq!(emoji_file("\u{1f525}"), Some("a7bd71d6389d0dfe.svg"));
        assert_eq!(emoji_file("not an emoji"), None);
    }

    #[test]
    fn variation_selectors_are_ignored() {
        assert_eq!(emoji_file("\u{2764}\u{fe0f}"), emoji_file("\u{2764}"));
    }

    #[test]
    fn values_match_the_captured_dom() {
        assert_eq!(Theme::Darker.color("--background-base-lowest"), 0x121214ff);
        assert_eq!(Theme::Darker.color("--background-base-lower"), 0x1a1a1eff);
        assert_eq!(Theme::Darker.color("--text-default"), 0xefeff1ff);
    }

    #[test]
    fn themes_differ() {
        let name = "--background-base-lowest";
        assert_ne!(Theme::Light.color(name), Theme::Darker.color(name));
    }

    #[test]
    #[should_panic(expected = "unknown design token")]
    fn unknown_token_panics() {
        Theme::Dark.color("--nope");
    }
}
