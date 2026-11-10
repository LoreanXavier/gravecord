//! Shortens a path to fit a width, the way Windows path fields do: the drive and the last folders stay, the middle
//! becomes an ellipsis.

const ELLIPSIS: &str = "\u{2026}";

/// Returns `path` unchanged when `fits` accepts it. Otherwise the drive (or first folder) is kept, as many of the last
/// folders as fit follow an ellipsis, and when not even the last name fits whole, the start of the path is cut instead.
/// `fits` says whether a candidate string is narrow enough to show.
pub fn elide_path(path: &str, fits: &dyn Fn(&str) -> bool) -> String {
    if fits(path) {
        return path.to_owned();
    }
    let parts: Vec<&str> = path.split(['\\', '/']).collect();
    if parts.len() >= 3 {
        let head = parts[0];
        for tail_count in (1..parts.len() - 1).rev() {
            let tail = parts[parts.len() - tail_count..].join("\\");
            let candidate = format!("{head}\\{ELLIPSIS}\\{tail}");
            if candidate.chars().count() < path.chars().count() && fits(&candidate) {
                return candidate;
            }
        }
    }
    elide_start(path, fits)
}

/// Keeps the longest end of `text` that fits after a leading ellipsis; the ellipsis alone when nothing else does.
pub fn elide_start(text: &str, fits: &dyn Fn(&str) -> bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let build = |keep: usize| -> String {
        let mut out = String::from(ELLIPSIS);
        out.extend(&chars[chars.len() - keep..]);
        out
    };
    // `fits` grows stricter as more characters are kept, so the best count is found by bisection.
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(&build(middle)) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    build(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight pixels a character.
    fn within(limit: usize) -> impl Fn(&str) -> bool {
        move |text: &str| text.chars().count() <= limit
    }

    #[test]
    fn a_path_that_fits_is_unchanged() {
        let path = r"C:\Users\x\gravecord";
        assert_eq!(elide_path(path, &within(40)), path);
    }

    #[test]
    fn the_middle_goes_and_the_drive_and_the_end_stay() {
        let path = r"C:\Users\someone\AppData\Local\Programs\gravecord";
        let shown = elide_path(path, &within(34));
        assert!(shown.starts_with(r"C:\"), "{shown}");
        assert!(shown.contains('\u{2026}'), "{shown}");
        assert!(shown.ends_with(r"Programs\gravecord"), "{shown}");
        assert!(shown.chars().count() <= 34, "{shown}");
    }

    #[test]
    fn more_room_keeps_more_folders() {
        let path = r"C:\Users\someone\AppData\Local\Programs\gravecord";
        let narrow = elide_path(path, &within(30));
        let wide = elide_path(path, &within(40));
        assert!(wide.chars().count() > narrow.chars().count());
        assert!(wide.ends_with(r"Local\Programs\gravecord"), "{wide}");
    }

    #[test]
    fn a_last_name_too_long_for_the_room_loses_its_start() {
        let path = r"C:\a\averyveryverylongfoldername";
        let shown = elide_path(path, &within(12));
        assert_eq!(shown.chars().count(), 12);
        assert!(shown.starts_with('\u{2026}'));
        assert!(shown.ends_with("foldername"));
    }

    #[test]
    fn nothing_fits_leaves_only_the_ellipsis() {
        assert_eq!(elide_path(r"C:\a\b", &|_| false), "\u{2026}");
        assert_eq!(elide_path("", &|_| true), "");
    }

    #[test]
    fn unicode_names_are_cut_on_characters() {
        let path = r"C:\Kullanıcılar\Şükrü\Belgeler\gravecord";
        let shown = elide_path(path, &within(22));
        assert!(shown.chars().count() <= 22, "{shown}");
        assert!(shown.ends_with("gravecord"), "{shown}");
    }
}
