//! The editor's text model, free of any UI: selection, IME marked text, undo, and the UTF-16 offsets operating systems use.
//! Offsets are bytes into UTF-8 and always sit on a character boundary.

use std::borrow::Cow;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edit {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug)]
pub struct TextBuffer {
    text: String,
    selection: Range<usize>,
    /// The end the caret is on when a selection grows with shift.
    reversed: bool,
    marked: Option<Range<usize>>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Edit,
    max_utf16: Option<usize>,
    single_line: bool,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            text: String::new(),
            selection: 0..0,
            reversed: false,
            marked: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: Edit::Other,
            max_utf16: None,
            single_line: false,
        }
    }
}

fn previous_boundary(text: &str, at: usize) -> usize {
    text[..at]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, at: usize) -> usize {
    text[at..]
        .chars()
        .next()
        .map_or(at, |character| at + character.len_utf8())
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

impl TextBuffer {
    pub fn set_max_utf16(&mut self, limit: usize) {
        self.max_utf16 = Some(limit);
    }

    pub fn set_single_line(&mut self, single_line: bool) {
        self.single_line = single_line;
    }

    pub fn is_single_line(&self) -> bool {
        self.single_line
    }

    fn sanitized_text<'a>(&self, text: &'a str, composing: bool) -> Cow<'a, str> {
        if !self.single_line || !text.contains(['\r', '\n']) {
            return Cow::Borrowed(text);
        }
        Cow::Owned(if composing {
            text.replace(['\r', '\n'], "")
        } else {
            text.replace("\r\n", " ").replace(['\r', '\n'], " ")
        })
    }

    fn bounded_text<'a>(&self, range: &Range<usize>, incoming: &'a str) -> &'a str {
        let Some(limit) = self.max_utf16 else {
            return incoming;
        };
        let retained = self.text[..range.start].encode_utf16().count()
            + self.text[range.end..].encode_utf16().count();
        let room = limit.saturating_sub(retained);
        let mut units = 0;
        for (index, character) in incoming.char_indices() {
            units += character.len_utf16();
            if units > room {
                return &incoming[..index];
            }
        }
        incoming
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    pub fn caret(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            selection: self.selection.clone(),
        }
    }

    /// Typing groups into one undo step until it crosses a word boundary or changes kind.
    fn record(&mut self, edit: Edit, starts_word: bool) {
        if self.last_edit != edit || edit == Edit::Other || starts_word {
            self.undo.push(self.snapshot());
        }
        self.redo.clear();
        self.last_edit = edit;
    }

    fn splice(&mut self, range: Range<usize>, new_text: &str) {
        self.text.replace_range(range.clone(), new_text);
        let caret = range.start + new_text.len();
        self.selection = caret..caret;
        self.reversed = false;
    }

    /// Replaces the marked text, or the selection, as committed input.
    pub fn insert(&mut self, new_text: &str) {
        let range = self.marked.take().unwrap_or_else(|| self.selection());
        let clean = self.sanitized_text(new_text, false);
        let new_text = self.bounded_text(&range, &clean);
        let starts_word = new_text.chars().any(char::is_whitespace);
        self.record(Edit::Typing, starts_word);
        self.splice(range, new_text);
    }

    /// Replaces `range` with committed text. An empty range at `None` means the selection.
    pub fn replace(&mut self, range: Option<Range<usize>>, new_text: &str) {
        let range = range
            .or_else(|| self.marked.take())
            .unwrap_or_else(|| self.selection());
        self.marked = None;
        let clean = self.sanitized_text(new_text, false);
        let new_text = self.bounded_text(&range, &clean);
        self.record(Edit::Typing, new_text.chars().any(char::is_whitespace));
        self.splice(range, new_text);
    }

    /// IME composition: `new_text` stays marked until committed. `caret` is inside it, relative to its start.
    pub fn compose(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        caret: Option<Range<usize>>,
    ) {
        let caret = caret.map(|inner| {
            self.sanitized_text(&new_text[..inner.start], true).len()
                ..self.sanitized_text(&new_text[..inner.end], true).len()
        });
        let clean = self.sanitized_text(new_text, true);
        let new_text = clean.as_ref();
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        if self.marked.is_none() {
            self.record(Edit::Typing, true);
        }
        let start = range.start;
        self.text.replace_range(range, new_text);
        self.marked = (!new_text.is_empty()).then(|| start..start + new_text.len());
        self.selection = match caret {
            Some(inner) => start + inner.start..start + inner.end,
            None => {
                let end = start + new_text.len();
                end..end
            }
        };
        self.reversed = false;
    }

    pub fn commit_composition(&mut self) {
        if let Some(range) = self.marked.take() {
            let accepted = self.bounded_text(&range, &self.text[range.clone()]);
            if accepted.len() != range.len() {
                let accepted = accepted.to_owned();
                self.splice(range, &accepted);
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.selection.is_empty() {
            let caret = self.selection.start;
            if caret == 0 {
                return;
            }
            self.selection = previous_boundary(&self.text, caret)..caret;
        }
        self.record(Edit::Deleting, false);
        self.splice(self.selection(), "");
    }

    pub fn delete(&mut self) {
        if self.selection.is_empty() {
            let caret = self.selection.start;
            if caret == self.text.len() {
                return;
            }
            self.selection = caret..next_boundary(&self.text, caret);
        }
        self.record(Edit::Deleting, false);
        self.splice(self.selection(), "");
    }

    pub fn set_caret(&mut self, at: usize) {
        self.selection = at..at;
        self.reversed = false;
        self.last_edit = Edit::Other;
    }

    pub fn select_to(&mut self, at: usize) {
        let anchor = if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        self.reversed = at < anchor;
        self.selection = at.min(anchor)..at.max(anchor);
        self.last_edit = Edit::Other;
    }

    /// A double click, as Chromium does it on Windows: the word at `at` (the character after it, or the last one at the
    /// end) with the spaces that follow it; on a run of spaces, that run.
    pub fn select_word_at(&mut self, at: usize) {
        let text = &self.text;
        if text.is_empty() {
            return;
        }
        let at = at.min(text.len());
        let probe = if at == text.len() {
            previous_boundary(text, at)
        } else {
            at
        };
        let Some(character) = text[probe..].chars().next() else {
            return;
        };
        let (start, end) = if is_word(character) {
            let start = text[..probe]
                .char_indices()
                .rev()
                .find(|(_, c)| !is_word(*c))
                .map_or(0, |(index, c)| index + c.len_utf8());
            let after = &text[probe..];
            let word_end = probe + after.find(|c: char| !is_word(c)).unwrap_or(after.len());
            let rest = &text[word_end..];
            let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            (start, word_end + spaces)
        } else if character == ' ' || character == '\t' {
            let start = text[..probe]
                .char_indices()
                .rev()
                .find(|(_, c)| *c != ' ' && *c != '\t')
                .map_or(0, |(index, c)| index + c.len_utf8());
            let after = &text[probe..];
            (
                start,
                probe + after.len() - after.trim_start_matches([' ', '\t']).len(),
            )
        } else {
            (probe, probe + character.len_utf8())
        };
        self.selection = start..end;
        self.reversed = false;
        self.last_edit = Edit::Other;
    }

    /// A triple click: the line (paragraph) around `at`.
    pub fn select_line_at(&mut self, at: usize) {
        let at = at.min(self.text.len());
        let start = self.text[..at].rfind('\n').map_or(0, |index| index + 1);
        let end = self.text[at..]
            .find('\n')
            .map_or(self.text.len(), |index| at + index);
        self.selection = start..end;
        self.reversed = false;
        self.last_edit = Edit::Other;
    }

    pub fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.reversed = false;
    }

    pub fn left(&self) -> usize {
        if self.selection.is_empty() {
            previous_boundary(&self.text, self.selection.start)
        } else {
            self.selection.start
        }
    }

    pub fn right(&self) -> usize {
        if self.selection.is_empty() {
            next_boundary(&self.text, self.selection.end)
        } else {
            self.selection.end
        }
    }

    pub fn char_left(&self) -> usize {
        previous_boundary(&self.text, self.caret())
    }

    pub fn char_right(&self) -> usize {
        next_boundary(&self.text, self.caret())
    }

    pub fn word_left(&self) -> usize {
        let before = &self.text[..self.caret()];
        let trimmed = before.trim_end_matches(|character| !is_word(character));
        trimmed
            .char_indices()
            .rev()
            .find(|(_, character)| !is_word(*character))
            .map_or(0, |(index, character)| index + character.len_utf8())
    }

    pub fn word_right(&self) -> usize {
        let caret = self.caret();
        let after = &self.text[caret..];
        let skipped = after.len()
            - after
                .trim_start_matches(|character| !is_word(character))
                .len();
        let rest = &after[skipped..];
        caret
            + skipped
            + rest
                .find(|character| !is_word(character))
                .unwrap_or(rest.len())
    }

    pub fn line_start(&self) -> usize {
        self.text[..self.caret()]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }

    pub fn line_end(&self) -> usize {
        let caret = self.caret();
        self.text[caret..]
            .find('\n')
            .map_or(self.text.len(), |index| caret + index)
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(previous);
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(next);
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.selection = snapshot.selection;
        self.reversed = false;
        self.marked = None;
        self.last_edit = Edit::Other;
    }

    pub fn clear(&mut self) {
        let max_utf16 = self.max_utf16;
        let single_line = self.single_line;
        *self = Self::default();
        self.max_utf16 = max_utf16;
        self.single_line = single_line;
    }

    pub fn offset_from_utf16(&self, utf16: usize) -> usize {
        let mut units = 0;
        for (index, character) in self.text.char_indices() {
            if units >= utf16 {
                return index;
            }
            units += character.len_utf16();
        }
        self.text.len()
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..offset].chars().map(char::len_utf16).sum()
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    /// The caret's UTF-16 range inside composed text maps to bytes of that text, not of the buffer.
    pub fn utf16_range_in(text: &str, range: &Range<usize>) -> Range<usize> {
        let byte_at = |utf16: usize| {
            let mut units = 0;
            for (index, character) in text.char_indices() {
                if units >= utf16 {
                    return index;
                }
                units += character.len_utf16();
            }
            text.len()
        };
        byte_at(range.start)..byte_at(range.end)
    }

    pub fn is_reversed(&self) -> bool {
        self.reversed
    }
}

#[cfg(test)]
mod tests {
    use super::TextBuffer;

    #[test]
    fn double_and_triple_clicks_select_as_the_official_composer() {
        let mut buffer = TextBuffer::default();
        buffer.insert("hello brave world");
        // Probed in the official composer: the word and its trailing space; the last word alone.
        buffer.select_word_at(2);
        assert_eq!(buffer.selected_text(), "hello ");
        buffer.select_word_at(6);
        assert_eq!(buffer.selected_text(), "brave ");
        buffer.select_word_at(17);
        assert_eq!(buffer.selected_text(), "world");
        buffer.select_line_at(3);
        assert_eq!(buffer.selected_text(), "hello brave world");
        let mut lines = TextBuffer::default();
        lines.insert("one two\nthree");
        lines.select_line_at(9);
        assert_eq!(lines.selected_text(), "three");
    }

    #[test]
    fn single_line_edits_preserve_suffixes_and_composition_offsets() {
        let mut buffer = TextBuffer::default();
        buffer.set_single_line(true);
        buffer.set_max_utf16(5);
        buffer.insert("a\nb\r\nc");
        assert_eq!(buffer.text(), "a b c");
        buffer.clear();
        buffer.insert("a\r\nbc");
        assert_eq!(buffer.text(), "a bc");
        buffer.replace(Some(1..3), "\r\nx");
        assert_eq!(buffer.text(), "a xc");
        buffer.clear();
        buffer.insert("ab");
        buffer.compose(None, "\n😀\r\nç", Some(7..9));
        assert_eq!(buffer.text(), "ab😀ç");
        assert_eq!(buffer.selected_text(), "ç");
        buffer.commit_composition();
        assert_eq!(buffer.marked(), None);
        assert_eq!(buffer.text().encode_utf16().count(), 5);
    }

    #[test]
    fn bounded_fields_count_utf16_and_keep_text_outside_the_replacement() {
        let mut buffer = TextBuffer::default();
        buffer.set_max_utf16(32);
        buffer.insert(&format!("{}😀z", "a".repeat(31)));
        assert_eq!(buffer.text(), "a".repeat(31));
        buffer.clear();
        buffer.insert(&"b".repeat(40));
        assert_eq!(buffer.text().len(), 32);
        buffer.replace(Some(0..1), "😀");
        assert_eq!(buffer.text(), "b".repeat(31));
        buffer.replace(Some(0..2), "😀z!");
        assert_eq!(buffer.text(), format!("😀z{}", "b".repeat(29)));
        buffer.undo();
        assert_eq!(buffer.text(), "");
        buffer.redo();
        assert_eq!(buffer.text().encode_utf16().count(), 32);
    }

    #[test]
    fn bounded_ime_can_exceed_the_limit_until_commit() {
        for replace_commit in [false, true] {
            let mut buffer = TextBuffer::default();
            buffer.set_max_utf16(32);
            buffer.insert(&"a".repeat(31));
            buffer.compose(None, "😀z", None);
            assert_eq!(buffer.text().encode_utf16().count(), 34);
            assert!(buffer.marked().is_some());
            if replace_commit {
                buffer.replace(None, "😀z");
            } else {
                buffer.commit_composition();
            }
            assert_eq!(buffer.text(), "a".repeat(31));
            assert_eq!(buffer.marked(), None);
        }
    }

    fn typed(text: &str) -> TextBuffer {
        let mut buffer = TextBuffer::default();
        for character in text.chars() {
            buffer.insert(&character.to_string());
        }
        buffer
    }

    #[test]
    fn typing_moves_the_caret() {
        let buffer = typed("çığ");
        assert_eq!(buffer.text(), "çığ");
        assert_eq!(buffer.caret(), "çığ".len());
    }

    #[test]
    fn backspace_removes_a_whole_character() {
        let mut buffer = typed("aç");
        buffer.backspace();
        assert_eq!(buffer.text(), "a");
        buffer.backspace();
        buffer.backspace();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn delete_works_forward_and_stops_at_the_end() {
        let mut buffer = typed("ab");
        buffer.set_caret(0);
        buffer.delete();
        assert_eq!(buffer.text(), "b");
        buffer.set_caret(1);
        buffer.delete();
        assert_eq!(buffer.text(), "b");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut buffer = typed("hello");
        buffer.select_all();
        buffer.insert("x");
        assert_eq!(buffer.text(), "x");
    }

    #[test]
    fn arrows_collapse_a_selection_to_its_edge() {
        let mut buffer = typed("hello");
        buffer.selection = 1..3;
        assert_eq!(buffer.left(), 1);
        assert_eq!(buffer.right(), 3);
    }

    #[test]
    fn shift_selection_can_cross_its_anchor() {
        let mut buffer = typed("hello");
        buffer.set_caret(2);
        buffer.select_to(4);
        assert_eq!(buffer.selection(), 2..4);
        buffer.select_to(1);
        assert_eq!(buffer.selection(), 1..2);
        assert!(buffer.is_reversed());
        assert_eq!(buffer.caret(), 1);
    }

    #[test]
    fn word_movement_skips_punctuation() {
        let mut buffer = typed("foo, bar baz");
        buffer.set_caret(buffer.text().len());
        assert_eq!(buffer.word_left(), 9);
        buffer.set_caret(9);
        assert_eq!(buffer.word_left(), 5);
        buffer.set_caret(0);
        assert_eq!(buffer.word_right(), 3);
        buffer.set_caret(3);
        assert_eq!(buffer.word_right(), 8);
    }

    #[test]
    fn line_home_and_end_stop_at_newlines() {
        let mut buffer = typed("ab\ncd\nef");
        buffer.set_caret(4);
        assert_eq!(buffer.line_start(), 3);
        assert_eq!(buffer.line_end(), 5);
    }

    #[test]
    fn undo_groups_a_word_and_redo_restores_it() {
        let mut buffer = typed("one two");
        buffer.undo();
        assert_eq!(buffer.text(), "one");
        buffer.undo();
        assert_eq!(buffer.text(), "");
        buffer.redo();
        assert_eq!(buffer.text(), "one");
    }

    #[test]
    fn a_new_edit_clears_redo() {
        let mut buffer = typed("a");
        buffer.undo();
        buffer.insert("b");
        buffer.redo();
        assert_eq!(buffer.text(), "b");
    }

    #[test]
    fn composition_replaces_itself_then_commits() {
        let mut buffer = typed("x");
        buffer.compose(None, "に", None);
        assert_eq!(buffer.marked(), Some(1.."xに".len()));
        buffer.compose(None, "にほ", None);
        assert_eq!(buffer.text(), "xにほ");
        buffer.insert("日本");
        assert_eq!(buffer.text(), "x日本");
        assert_eq!(buffer.marked(), None);
    }

    #[test]
    fn empty_composition_cancels() {
        let mut buffer = typed("x");
        buffer.compose(None, "あ", None);
        buffer.compose(None, "", None);
        assert_eq!(buffer.text(), "x");
        assert_eq!(buffer.marked(), None);
    }

    #[test]
    fn one_undo_removes_a_whole_composition() {
        let mut buffer = typed("a ");
        buffer.compose(None, "に", None);
        buffer.compose(None, "にほ", None);
        buffer.insert("日本");
        buffer.undo();
        assert_eq!(buffer.text(), "a ");
    }

    #[test]
    fn utf16_offsets_count_surrogate_pairs() {
        let buffer = typed("a😀b");
        assert_eq!(buffer.offset_to_utf16(buffer.text().len()), 4);
        assert_eq!(buffer.offset_from_utf16(3), "a😀".len());
        assert_eq!(buffer.range_to_utf16(&(1.."a😀".len())), 1..3);
        assert_eq!(buffer.range_from_utf16(&(1..3)), 1.."a😀".len());
    }

    #[test]
    fn utf16_inside_composed_text_maps_to_its_own_bytes() {
        assert_eq!(
            TextBuffer::utf16_range_in("日本", &(1..2)),
            "日".len().."日本".len()
        );
    }
}
