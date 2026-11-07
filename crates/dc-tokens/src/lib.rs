//! Design tokens of the pinned build, resolved per theme by `tools/dump-tokens.mjs`.

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

#[cfg(test)]
mod tests {
    use super::Theme;

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
