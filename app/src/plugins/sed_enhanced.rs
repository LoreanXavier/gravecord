//! SedEnhanced (Equicord `src/equicordplugins/sedEnhanced`, GPL-3.0-or-later): the wider pattern of the send path's
//! `s/find/replace` rule. The original's `sedRegex`,
//! `^s(?<sep>[/|$#@!])(?<match>(?!\1)(?:(?![^\\]\1).)*.|)\1(?<replace>(?!\1)(?:(?![^\\]\1).)*.|)\1?(?<modes>[rgmisudyv]*)$`,
//! needs back references, so it is matched here by hand with the same backtracking order: any of `/|$#@!` separates
//! (not one after a backslash), the last separator may be left out, and the modes `rgmisudyv` follow. JavaScript's
//! `.` stops at line ends, so neither part spans lines.

/// The parts of a sed message, as written (escapes kept).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Sed {
    pub pattern: String,
    pub replace: String,
    pub modes: String,
}

const SEPARATORS: [char; 6] = ['/', '|', '$', '#', '@', '!'];
const MODES: &str = "rgmisudyv";

fn line_end(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Where `(?!\1)(?:(?![^\\]\1).)*.|` starting at `start` may end, in the order the regex tries them: the longest
/// part first, then shorter ones that end before an escaped separator, then the empty part.
fn part_ends(chars: &[char], start: usize, sep: char, need_sep: bool) -> Vec<usize> {
    let mut ends = Vec::new();
    if start < chars.len() && chars[start] != sep && !line_end(chars[start]) {
        // The loop takes a character while it is no line end and not a non-backslash followed by the separator.
        let mut stop = start;
        while stop < chars.len()
            && !line_end(chars[stop])
            && !(chars[stop] != '\\' && chars.get(stop + 1) == Some(&sep))
        {
            stop += 1;
        }
        // The final `.`: the loop gives back characters until what follows fits.
        let last = if stop < chars.len() && !line_end(chars[stop]) {
            stop + 1
        } else {
            stop
        };
        for end in (start + 1..=last).rev() {
            if !need_sep || chars.get(end) == Some(&sep) {
                ends.push(end);
            }
        }
    }
    ends.push(start);
    ends
}

/// `content.match(sedRegex)?.groups`.
pub(crate) fn parse(content: &str) -> Option<Sed> {
    let chars: Vec<char> = content.chars().collect();
    if chars.first() != Some(&'s') {
        return None;
    }
    let sep = *chars.get(1)?;
    if !SEPARATORS.contains(&sep) {
        return None;
    }
    // Whether everything from an index on is modes.
    let mut modes_from = vec![true; chars.len() + 1];
    for index in (0..chars.len()).rev() {
        modes_from[index] = modes_from[index + 1] && MODES.contains(chars[index]);
    }
    let text = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    for pattern_end in part_ends(&chars, 2, sep, true) {
        if chars.get(pattern_end) != Some(&sep) {
            continue;
        }
        let replace_start = pattern_end + 1;
        for replace_end in part_ends(&chars, replace_start, sep, false) {
            // `\1?` is greedy: with the separator first, then without.
            let tails = [
                (chars.get(replace_end) == Some(&sep)).then_some(replace_end + 1),
                Some(replace_end),
            ];
            for modes_start in tails.into_iter().flatten() {
                if modes_from[modes_start] {
                    return Some(Sed {
                        pattern: text(2, pattern_end),
                        replace: text(replace_start, replace_end),
                        modes: text(modes_start, chars.len()),
                    });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(content: &str) -> Option<(String, String, String)> {
        parse(content).map(|sed| (sed.pattern, sed.replace, sed.modes))
    }

    fn some(pattern: &str, replace: &str, modes: &str) -> Option<(String, String, String)> {
        Some((pattern.into(), replace.into(), modes.into()))
    }

    #[test]
    fn the_wider_pattern_matches_as_the_original() {
        assert_eq!(parts("s/cat/dog"), some("cat", "dog", ""));
        assert_eq!(parts("s/cat/dog/"), some("cat", "dog", ""));
        assert_eq!(parts("s/cat/dog/gi"), some("cat", "dog", "gi"));
        assert_eq!(parts("s|a/b|c|r"), some("a/b", "c", "r"));
        assert_eq!(parts("s#x#y#g"), some("x", "y", "g"));
        // Without a closing separator the modes are part of the replacement (the greedy part takes them).
        assert_eq!(parts("s/a/bg"), some("a", "bg", ""));
        // An escaped separator stays inside the part, escape and all.
        assert_eq!(parts("s/a\\/b/c/"), some("a\\/b", "c", ""));
        assert_eq!(parts("s//x"), some("", "x", ""));
        assert_eq!(parts("s/a//"), some("a", "", ""));
        assert_eq!(parts("s/a/"), some("a", "", ""));
        assert_eq!(parts("s/a/b/q"), None);
        assert_eq!(parts("s%a%b"), None);
        assert_eq!(parts("s/a"), None);
        assert_eq!(parts("s/a\nb/c"), None);
        assert_eq!(parts("x/a/b"), None);
    }
}
