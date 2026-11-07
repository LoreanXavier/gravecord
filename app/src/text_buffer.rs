//! The editor's text model, free of any UI: selection, IME marked text, undo, and the UTF-16 offsets operating systems use.
//! Offsets are bytes into UTF-8 and always sit on a character boundary.

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
        self.marked = None;
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
        *self = Self::default();
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
