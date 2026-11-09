//! Emoticon conversion (`chat_emoji_convert_emoticons`): a typed `:)` followed by a space becomes the emoji. The
//! table is module 132565 of the pinned build, entry for entry; the emoji are looked up by name.

use dc_tokens::emoji_by_name;

pub(super) const EMOTICONS: &[(&str, &str)] = &[
    (">:(", "angry"),
    (">:-(", "angry"),
    (">=(", "angry"),
    (">=-(", "angry"),
    (":\")", "blush"),
    (":-\")", "blush"),
    ("=\")", "blush"),
    ("=-\")", "blush"),
    ("</3", "broken_heart"),
    ("<\\3", "broken_heart"),
    (":-\\", "confused"),
    (":-/", "confused"),
    ("=-\\", "confused"),
    ("=-/", "confused"),
    (":'(", "cry"),
    (":'-(", "cry"),
    (":,(", "cry"),
    (":,-(", "cry"),
    ("='(", "cry"),
    ("='-(", "cry"),
    ("=,(", "cry"),
    ("=,-(", "cry"),
    (":(", "slight_frown"),
    (":-(", "slight_frown"),
    ("=(", "slight_frown"),
    ("=-(", "slight_frown"),
    ("<3", "heart"),
    ("♡", "heart"),
    ("]:(", "imp"),
    ("]:-(", "imp"),
    ("]=(", "imp"),
    ("]=-(", "imp"),
    ("o:)", "innocent"),
    ("O:)", "innocent"),
    ("o:-)", "innocent"),
    ("O:-)", "innocent"),
    ("0:)", "innocent"),
    ("0:-)", "innocent"),
    ("o=)", "innocent"),
    ("O=)", "innocent"),
    ("o=-)", "innocent"),
    ("O=-)", "innocent"),
    ("0=)", "innocent"),
    ("0=-)", "innocent"),
    (":'D", "joy"),
    (":'-D", "joy"),
    (":,D", "joy"),
    (":,-D", "joy"),
    ("='D", "joy"),
    ("='-D", "joy"),
    ("=,D", "joy"),
    ("=,-D", "joy"),
    (":*", "kissing"),
    (":-*", "kissing"),
    ("=*", "kissing"),
    ("=-*", "kissing"),
    ("x-)", "laughing"),
    ("X-)", "laughing"),
    (":|", "neutral_face"),
    (":-|", "neutral_face"),
    ("=|", "neutral_face"),
    ("=-|", "neutral_face"),
    (":o", "open_mouth"),
    (":-o", "open_mouth"),
    (":O", "open_mouth"),
    (":-O", "open_mouth"),
    ("=o", "open_mouth"),
    ("=-o", "open_mouth"),
    ("=O", "open_mouth"),
    ("=-O", "open_mouth"),
    (":@", "rage"),
    (":-@", "rage"),
    ("=@", "rage"),
    ("=-@", "rage"),
    (":D", "smile"),
    (":-D", "smile"),
    ("=D", "smile"),
    ("=-D", "smile"),
    (":')", "smiling_face_with_tear"),
    (":'-)", "smiling_face_with_tear"),
    (":,)", "smiling_face_with_tear"),
    (":,-)", "smiling_face_with_tear"),
    ("=')", "smiling_face_with_tear"),
    ("='-)", "smiling_face_with_tear"),
    ("=,)", "smiling_face_with_tear"),
    ("=,-)", "smiling_face_with_tear"),
    (":)", "slight_smile"),
    (":-)", "slight_smile"),
    ("=)", "slight_smile"),
    ("=-)", "slight_smile"),
    ("]:)", "smiling_imp"),
    ("]:-)", "smiling_imp"),
    ("]=)", "smiling_imp"),
    ("]=-)", "smiling_imp"),
    (":,'(", "sob"),
    (":,'-(", "sob"),
    (";(", "sob"),
    (";-(", "sob"),
    ("=,'(", "sob"),
    ("=,'-(", "sob"),
    (":P", "stuck_out_tongue"),
    (":-P", "stuck_out_tongue"),
    ("=P", "stuck_out_tongue"),
    ("=-P", "stuck_out_tongue"),
    ("8-)", "sunglasses"),
    ("B-)", "sunglasses"),
    (",:(", "sweat"),
    (",:-(", "sweat"),
    (",=(", "sweat"),
    (",=-(", "sweat"),
    (",:)", "sweat_smile"),
    (",:-)", "sweat_smile"),
    (",=)", "sweat_smile"),
    (",=-)", "sweat_smile"),
    (":s", "unamused"),
    (":-S", "unamused"),
    (":z", "unamused"),
    (":-Z", "unamused"),
    (":$", "unamused"),
    (":-$", "unamused"),
    ("=s", "unamused"),
    ("=-S", "unamused"),
    ("=z", "unamused"),
    ("=-Z", "unamused"),
    ("=$", "unamused"),
    ("=-$", "unamused"),
    (";)", "wink"),
    (";-)", "wink"),
];

/// The text with the emoticon before its trailing space replaced, or `None` when there is nothing to convert.
/// An emoticon counts at the start of the text or after whitespace, as a whole word.
pub(super) fn convert(text: &str) -> Option<String> {
    let body = text.strip_suffix(' ')?;
    let start = body.rfind(char::is_whitespace).map_or(0, |at| {
        at + body[at..].chars().next().map_or(1, char::len_utf8)
    });
    let word = &body[start..];
    let (_, name) = EMOTICONS.iter().find(|(emoticon, _)| *emoticon == word)?;
    let emoji = emoji_by_name(name)?;
    Some(format!("{}{} ", &body[..start], emoji.surrogates))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_emoticon_becomes_its_emoji() {
        let smile = emoji_by_name("slight_smile").unwrap().surrogates;
        assert_eq!(
            convert(":) ").as_deref(),
            Some(format!("{smile} ").as_str())
        );
        assert_eq!(
            convert("hello ;) ").as_deref(),
            Some(format!("hello {} ", emoji_by_name("wink").unwrap().surrogates).as_str())
        );
    }

    #[test]
    fn only_a_whole_word_converts() {
        assert_eq!(convert("a:) "), None);
        assert_eq!(convert(":)"), None);
        assert_eq!(convert("word "), None);
    }

    #[test]
    fn every_name_in_the_table_is_a_known_emoji() {
        for (emoticon, name) in EMOTICONS {
            assert!(emoji_by_name(name).is_some(), "{emoticon} -> {name}");
        }
    }
}
