//! Translate+ (Equicord `src/equicordplugins/translatePlus`, GPL-3.0-or-later): "Translate" in a message's menu
//! (after Copy Text) and in its hover bar, for a message with text. The translation (`plugins::translate_plus`, asked
//! of `net.rs`) is drawn under the message, after its other accessories (`renderMessageAccessory`): the plugin's icon,
//! the text read as markdown, then " (translated from <language> - Dismiss)", in muted italic 8px below the rest
//! (`.eq-trans-accessory`). Dismiss takes it away; leaving the channel does too, as the original's accessory state
//! goes with the message's row. Not part of the pinned client.
//!
//! GAP: a code block inside the translation is drawn as plain text in the line, where the original's markdown parser
//! draws a code block.

use super::super::Root;
use dc_state::{Id, Message};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

const ID: &str = "Translate+";
const KEY: &str = "plugin-translate-plus:";

thread_local! {
    /// The translations shown, by message: the language found and the text.
    static SHOWN: RefCell<HashMap<Id, (String, String)>> = RefCell::new(HashMap::new());
    /// The channel they belong to.
    static CHANNEL: RefCell<Option<Id>> = const { RefCell::new(None) };
}

/// Whether "Translate" is offered for a message with this text.
pub(in crate::ui) fn translate_plus(content: &str) -> bool {
    !content.is_empty() && !crate::plugins::bypassed() && crate::plugin_on!("Translate+")
}

impl Root {
    /// `handleTranslate`: the message's text goes out to be translated.
    pub(in crate::ui) fn translate_plus_start(&mut self, message: Id, content: String) {
        if !translate_plus(&content) {
            return;
        }
        let ask = crate::plugins::with(|state| {
            let flag = |name: &str| state.value(ID, name) == Value::Bool(true);
            let target = state.value(ID, "target");
            crate::net::Request::PluginTranslate {
                key: format!("{KEY}{message}"),
                text: content,
                target: target.as_str().unwrap_or("en").to_owned(),
                toki: flag("toki"),
                sitelen: flag("sitelen"),
                shavian: flag("shavian"),
            }
        });
        let _ = self.requests.send(ask);
    }

    /// A translation arrived. True when the key was ours.
    pub(in crate::ui) fn translate_plus_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(message) = key.strip_prefix(KEY) else {
            return false;
        };
        // A failed Toki Pona call leaves things as they were (the original's promise rejects).
        if let (Some(message), Some(src), Some(text)) = (
            message.parse::<Id>().ok(),
            body["src"].as_str(),
            body["text"].as_str(),
        ) {
            SHOWN.with(|shown| {
                shown
                    .borrow_mut()
                    .insert(message, (src.to_owned(), text.to_owned()))
            });
            cx.notify();
        }
        true
    }

    /// The translation under `message`, while the plugin is on.
    pub(in crate::ui) fn translate_plus_accessory(
        &self,
        message: &Message,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use crate::inline::{BoxStyle, Piece};
        use gpui::{IntoElement, ParentElement, Styled, div, px};
        let on = !crate::plugins::bypassed() && crate::plugin_on!("Translate+");
        let moved = CHANNEL.with(|channel| channel.replace(self.channel) != self.channel);
        if !on || moved {
            SHOWN.with(|shown| shown.borrow_mut().clear());
        }
        if !on {
            return None;
        }
        let (src, text) = SHOWN.with(|shown| shown.borrow().get(&message.id).cloned())?;
        let size = self.chat_font_size();
        let line = (size * 1.375).round();
        let emoji = (22. * size / 16.).round();
        let muted: gpui::Hsla = super::super::color("--text-muted").into();
        let plain = |text: String| dc_markdown::Span {
            text,
            style: dc_markdown::Style {
                italic: true,
                ..dc_markdown::Style::default()
            },
            kind: dc_markdown::Kind::Text,
        };
        let mut spans: Vec<dc_markdown::Span> = Vec::new();
        for (index, block) in dc_markdown::parse(&text).iter().enumerate() {
            if index > 0 {
                spans.push(plain("\n".to_owned()));
            }
            match block {
                dc_markdown::Block::Code { text, .. } => spans.push(plain(text.clone())),
                other => spans.extend(super::super::block_spans(other).iter().cloned().map(
                    |mut span| {
                        span.style.italic = true;
                        span
                    },
                )),
            }
        }
        let links = super::super::link_runs(&spans);
        let mut pieces: Vec<Piece> = Vec::with_capacity(spans.len() + 4);
        // `<Icon height={16} width={16} />` with `margin-right: .25em`, on the text's baseline.
        let mut icon = self.piece(&plain(String::new()), muted, size, 400., emoji);
        icon.boxed = Some(BoxStyle {
            picture: None,
            icon: Some((
                "icons/plugin_translate.svg".into(),
                px(16.),
                px(16. + size / 4.),
            )),
            padding_x: px(0.),
            height: px(16.),
            top: px((line - 16.) / 2. - 1.),
            background: gpui::Hsla::transparent_black(),
            border: gpui::Hsla::transparent_black(),
            radius: px(0.),
            text_line_height: px(line),
            text_top: px(0.),
        });
        pieces.push(icon);
        pieces.extend(
            spans
                .iter()
                .map(|span| self.piece(span, muted, size, 400., emoji)),
        );
        let name = crate::plugins::translate_plus::language_name(&src);
        pieces.push(self.piece(
            &plain(format!(" (translated from {name} - ")),
            muted,
            size,
            400.,
            emoji,
        ));
        let dismiss_at = pieces.len();
        // `.eq-trans-dismiss`: the link colour, underlined on hover and focus.
        let mut dismiss = self.piece(
            &plain("Dismiss".to_owned()),
            super::super::color("--text-link").into(),
            size,
            400.,
            emoji,
        );
        dismiss.hover_underline = true;
        pieces.push(dismiss);
        pieces.push(self.piece(&plain(")".to_owned()), muted, size, 400., emoji));
        let id = message.id;
        let root = cx.weak_entity();
        Some(
            div()
                .mt(px(size / 2.))
                .child(crate::inline::inline(pieces, px(line)).on_piece_click(
                    move |piece, _, cx| {
                        if piece == dismiss_at {
                            SHOWN.with(|shown| shown.borrow_mut().remove(&id));
                            let _ = root.update(cx, |_, cx| cx.notify());
                        } else if let Some(Some((url, label))) =
                            piece.checked_sub(1).and_then(|at| links.get(at)).cloned()
                        {
                            let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                        }
                    },
                ))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_offered_while_off() {
        // Off (the unit tests' state): no menu row, no button, no request.
        assert!(!translate_plus("hola"));
        assert!(!translate_plus(""));
    }
}
