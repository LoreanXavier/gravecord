//! A code block's copy button: module 862316 `ec` (and `code_block` in 647660) draws a `CopyIcon` in
//! `codeActions_ku1OHH` beside the block, which stylesheet 44141 hides until the pointer is over the block
//! (`.markup .codeContainer:hover .codeActions { display: block }`) and places 4px from the right edge and 8px from the
//! top (`.markup .codeActions { inset-inline-end: 4px; position: absolute; top: 8px }`, `.codeContainer { max-width:
//! 50vw; position: relative }`). A click writes the block's text to the clipboard and the icon becomes the
//! `CheckmarkLargeIcon` for as long as the block is on screen. Inline code has no button.

use super::{Root, color, icon};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px,
};
use std::collections::HashSet;

/// The block a button belongs to: a message id and the block's index in it.
pub(super) type BlockKey = (u64, u32);

/// What the button shows and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Copy,
    Copied,
}

pub(super) fn glyph(copied: &HashSet<BlockKey>, key: BlockKey) -> Glyph {
    if copied.contains(&key) {
        Glyph::Copied
    } else {
        Glyph::Copy
    }
}

/// `xs` icons are 16px (module 27989's size table).
const ICON: f32 = 16.;

impl Root {
    /// `Block::Code`: the framed block with the hover button at its top right.
    pub(super) fn code_block(
        &self,
        key: BlockKey,
        gap: f32,
        language: Option<&str>,
        text: &str,
    ) -> AnyElement {
        let group = SharedString::from(format!("code-block-{}-{}", key.0, key.1));
        let weak = self.weak.clone();
        let copied = glyph(&self.copied_code, key) == Glyph::Copied;
        let body = text.to_owned();
        let button = div()
            .id(SharedString::from(format!("code-copy-{}-{}", key.0, key.1)))
            .absolute()
            .top(px(8.))
            .right(px(4.))
            .size(px(ICON))
            .cursor_pointer()
            .invisible()
            .group_hover(group.clone(), |style| style.visible())
            .child(if copied {
                icon("checkmark_large", ICON, "--text-code")
            } else {
                icon("menu_copy_text", ICON, "--text-code")
            })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let text = body.clone();
                let _ = weak.update(cx, |this, cx| this.copy_code(key, text, cx));
            });
        div()
            .group(group)
            .relative()
            .mt(px(gap))
            .w(px(640.))
            .p(px(7.))
            .rounded(px(4.))
            .border_1()
            .border_color(color("--background-mod-subtle"))
            .bg(color("--background-code"))
            .font_family(super::MONO)
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(400.))
            .text_color(color("--text-code"))
            .child(self.selectable_code(language, text, key))
            .child(button)
            .into_any_element()
    }

    fn copy_code(&mut self, key: BlockKey, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied_code.insert(key);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_shows_the_copy_icon_until_it_is_copied() {
        let mut copied = HashSet::new();
        assert_eq!(glyph(&copied, (7, 1)), Glyph::Copy);
        copied.insert((7, 1));
        assert_eq!(glyph(&copied, (7, 1)), Glyph::Copied);
        // Another block of the same message keeps its own state.
        assert_eq!(glyph(&copied, (7, 2)), Glyph::Copy);
    }
}
