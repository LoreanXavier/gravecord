//! What the reply preview line says about the message it answers (module 448368 `V`): a placeholder in place of the
//! text for a message of a blocked or ignored author, a pin notice, or one with no text (its poll's question, a
//! sticker, a command, a voice message, a components message, or anything else: an attachment), and an icon after it
//! for a sticker, a command, a voice message, or attached files and embeds. A forward (module 427930) leads with the
//! forward arrow, and is read through its snapshot (module 387408).

use super::*;

/// `CHANNEL_PINNED_MESSAGE`.
const PINNED_TYPE: u64 = 6;
/// `POLL_RESULT`: its line is drawn elsewhere and takes no image icon.
const POLL_RESULT_TYPE: u64 = 46;

/// The text of the line when it is not the message's own text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Placeholder {
    /// `XAkOo2` "Blocked message".
    Blocked,
    /// `G7p6v/` "Ignored message".
    Ignored,
    /// `sCfDDl` "Message pinned."
    Pinned,
    /// A poll with no text: its question, in the content style (not a placeholder).
    PollQuestion,
    /// `kHdYCW` "Click to see sticker".
    Sticker,
    /// `E+6SSY` "Click to see command".
    Command,
    /// `XC3A52` "Click to see voice message".
    Voice,
    /// `Xxat6S` "Click to see message".
    Components,
    /// A poll result: `Vn97Ka` "{username}'s poll **{title}** has closed" (module 969632 `Il`).
    PollResult,
    /// `6hGo0c` "Click to see attachment".
    Attachment,
}

/// The icon after the line (`repliedTextContentTrailingIcon_2i8Bfm`, 20px).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trailing {
    /// `StickerSmallIcon`.
    Sticker,
    /// `SlashBoxIcon`.
    Command,
    /// `MicrophoneIcon`, 19/24 as wide as high.
    Voice,
    /// `ImageIcon`.
    Image,
}

/// The line's parts, as `V` returns them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplyLine {
    pub placeholder: Option<Placeholder>,
    pub trailing: Option<Trailing>,
    /// The forward arrow ahead of the line (`ArrowAngleRightUpIcon`, `repliedTextContentLeadingIcon_2i8Bfm`).
    pub forwarded: bool,
}

/// What `V` reads off the message: whether its own text is empty (the rendered content of the referenced message,
/// before the forward is read through), and the rest from the message merged with its snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ReplyFacts {
    pub empty: bool,
    pub forwarded: bool,
    pub blocked: bool,
    pub ignored: bool,
    pub kind: u64,
    pub poll: bool,
    pub sticker: bool,
    pub command: bool,
    pub voice: bool,
    pub components_v2: bool,
    pub files: bool,
}

/// Module 448368 `V`.
pub(super) fn reply_line(facts: ReplyFacts) -> ReplyLine {
    let result = facts.kind == POLL_RESULT_TYPE;
    let mut placeholder = None;
    let mut trailing = None;
    if facts.blocked {
        placeholder = Some(Placeholder::Blocked);
    } else if facts.ignored {
        placeholder = Some(Placeholder::Ignored);
    } else if facts.kind == PINNED_TYPE {
        placeholder = Some(Placeholder::Pinned);
    } else if facts.empty {
        placeholder = if facts.poll {
            Some(Placeholder::PollQuestion)
        } else if result {
            Some(Placeholder::PollResult)
        } else if facts.sticker {
            Some(Placeholder::Sticker)
        } else if facts.command {
            Some(Placeholder::Command)
        } else if facts.voice {
            Some(Placeholder::Voice)
        } else if facts.components_v2 {
            Some(Placeholder::Components)
        } else {
            trailing = Some(Trailing::Image);
            Some(Placeholder::Attachment)
        };
    }
    if facts.sticker {
        trailing = Some(Trailing::Sticker);
    } else if facts.command {
        trailing = Some(Trailing::Command);
    } else if facts.voice {
        trailing = Some(Trailing::Voice);
    } else if facts.files && !result {
        trailing = Some(Trailing::Image);
    }
    ReplyLine {
        placeholder,
        trailing,
        forwarded: facts.forwarded,
    }
}

/// `poll_question_text` of a poll result, cut as module 23339 `EJ(text, 50)` cuts it: past 50 characters, the
/// first 49 without trailing spaces and dots, then an ellipsis.
pub(super) fn cut_to(text: &str, limit: usize) -> String {
    if text.encode_utf16().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit - 1).collect();
    format!(
        "{}\u{2026}",
        kept.trim_end_matches(|c: char| c.is_whitespace() || c == '.')
    )
}

fn poll_result_title(message: &Message) -> String {
    let question = message
        .embeds
        .first()
        .and_then(|embed| {
            embed
                .fields
                .iter()
                .find(|field| field.name == "poll_question_text")
        })
        .map(|field| field.value.as_str())
        .unwrap_or("");
    cut_to(question, 50)
}

/// The words of a placeholder for `message` as plain text: the poll's question, the poll result's line, or the
/// placeholder's own string.
pub(super) fn placeholder_text(placeholder: Placeholder, message: &Message) -> String {
    match placeholder {
        Placeholder::PollQuestion => message
            .extra
            .poll
            .as_ref()
            .map(|poll| poll.question.clone())
            .unwrap_or_default(),
        Placeholder::PollResult => dc_intl::format(
            "Vn97Ka",
            &[
                (
                    "username",
                    dc_intl::Arg::from(message.author.display_name()),
                ),
                (
                    "title",
                    dc_intl::Arg::from(poll_result_title(message).as_str()),
                ),
            ],
        ),
        other => other.text().to_owned(),
    }
}

impl Placeholder {
    fn text(self) -> &'static str {
        match self {
            Self::Blocked => dc_intl::t!("XAkOo2"),
            Self::Ignored => dc_intl::t!("G7p6v/"),
            Self::Pinned => dc_intl::t!("sCfDDl"),
            Self::PollQuestion | Self::PollResult => "",
            Self::Sticker => dc_intl::t!("kHdYCW"),
            Self::Command => dc_intl::t!("E+6SSY"),
            Self::Voice => dc_intl::t!("XC3A52"),
            Self::Components => dc_intl::t!("Xxat6S"),
            Self::Attachment => dc_intl::t!("6hGo0c"),
        }
    }
}

impl Root {
    /// The facts of `V` for a referenced message, with the relationship store's `isBlockedForMessage` and
    /// `isIgnoredForMessage` (module 994500: the author, or the user who ran its command).
    pub(super) fn reply_facts(&self, message: &Message) -> ReplyFacts {
        let (blocked, ignored) = self.hidden_for_message(message);
        let snapshot = message.forwarded.as_deref();
        let flags = snapshot.map_or(message.flags, |forward| forward.flags);
        let files = snapshot.map_or(
            !message.attachments.is_empty() || !message.embeds.is_empty(),
            |forward| !forward.attachments.is_empty() || !forward.embeds.is_empty(),
        );
        ReplyFacts {
            empty: message.content.is_empty(),
            forwarded: snapshot.is_some(),
            blocked,
            ignored,
            kind: message.kind,
            poll: message.extra.poll.is_some(),
            // The merge of module 387408 takes the snapshot's stickers, which a forward does not carry here.
            sticker: snapshot.is_none() && !message.sticker_items.is_empty(),
            command: message.extra.interaction.is_some(),
            voice: flags & dc_state::VOICE_MESSAGE_FLAG != 0,
            components_v2: flags & dc_state::components::FLAG_IS_COMPONENTS_V2 != 0,
            files,
        }
    }

    /// The text cell of the reply preview: the forward arrow, the text or its placeholder (`repliedTextPlaceholder`:
    /// italic, 2px at the end), and the trailing icon (4px apart, 20px).
    pub(super) fn reply_content_row(
        &self,
        message: &Message,
        line: ReplyLine,
        strong: bool,
    ) -> AnyElement {
        let tone = if strong {
            "--text-strong"
        } else {
            "--text-default"
        };
        let text: AnyElement = match line.placeholder {
            Some(Placeholder::PollResult) => {
                let runs = dc_intl::format_runs(
                    "Vn97Ka",
                    &[
                        (
                            "username",
                            dc_intl::Arg::from(self.guild_display_name(&message.author).as_str()),
                        ),
                        (
                            "title",
                            dc_intl::Arg::from(poll_result_title(message).as_str()),
                        ),
                    ],
                );
                div()
                    .flex()
                    .pr(px(2.))
                    .italic()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .children(runs.into_iter().map(|(text, tag)| {
                        div()
                            .flex_none()
                            .when(tag.as_deref() == Some("$b"), |run| {
                                run.font_weight(FontWeight(700.))
                            })
                            .child(text)
                    }))
                    .into_any_element()
            }
            None => self
                .preview_text(&first_spans(&message.content), strong)
                .into_any_element(),
            Some(Placeholder::PollQuestion) => div()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(
                    message
                        .extra
                        .poll
                        .as_ref()
                        .map(|poll| poll.question.clone())
                        .unwrap_or_default(),
                )
                .into_any_element(),
            Some(placeholder) => div()
                .pr(px(2.))
                .italic()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(placeholder.text())
                .into_any_element(),
        };
        let trailing = line.trailing.map(|kind| {
            // `currentColor` of `repliedMessage_2i8Bfm` (`--text-default`): the icon sits beside the preview whose
            // hover turns `--text-strong`, not inside it.
            let quiet = "--text-default";
            let glyph = match kind {
                Trailing::Sticker => icon("sticker_small", 20., quiet),
                Trailing::Command => icon("slash_box", 20., quiet),
                Trailing::Voice => icon("cp_mic", 20., quiet).w(px(20. * 19. / 24.)).h(px(20.)),
                Trailing::Image => icon("search_has_image", 20., quiet),
            };
            div().ml(px(4.)).flex_none().child(glyph)
        });
        div()
            .flex()
            .items_center()
            .h(px(18.))
            .min_w(px(0.))
            .text_color(color(tone))
            .when(line.forwarded, |row| {
                row.child(
                    div()
                        .mr(px(4.))
                        .flex_none()
                        // No colour is passed: the icon's own `INTERACTIVE_ICON_DEFAULT`.
                        .child(icon("forwarded_icon", 20., "--interactive-icon-default")),
                )
            })
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w(px(0.))
                    .h(px(18.))
                    .overflow_hidden()
                    .child(text),
            )
            .children(trailing)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> ReplyFacts {
        ReplyFacts {
            kind: 0,
            ..ReplyFacts::default()
        }
    }

    #[test]
    fn text_alone_has_no_placeholder_or_icon() {
        assert_eq!(reply_line(facts()), ReplyLine::default());
    }

    #[test]
    fn text_with_files_ends_with_the_image_icon() {
        let line = reply_line(ReplyFacts {
            files: true,
            ..facts()
        });
        assert_eq!(line.placeholder, None);
        assert_eq!(line.trailing, Some(Trailing::Image));
    }

    #[test]
    fn empty_message_reads_click_to_see_attachment_with_the_image_icon() {
        let line = reply_line(ReplyFacts {
            empty: true,
            ..facts()
        });
        assert_eq!(line.placeholder, Some(Placeholder::Attachment));
        assert_eq!(line.trailing, Some(Trailing::Image));
    }

    #[test]
    fn blocked_and_ignored_win_over_everything_but_keep_the_icon() {
        let line = reply_line(ReplyFacts {
            blocked: true,
            ignored: true,
            empty: true,
            sticker: true,
            ..facts()
        });
        assert_eq!(line.placeholder, Some(Placeholder::Blocked));
        assert_eq!(line.trailing, Some(Trailing::Sticker));
        let line = reply_line(ReplyFacts {
            ignored: true,
            files: true,
            ..facts()
        });
        assert_eq!(line.placeholder, Some(Placeholder::Ignored));
        assert_eq!(line.trailing, Some(Trailing::Image));
    }

    #[test]
    fn empty_kinds_in_order() {
        let of = |f: ReplyFacts| reply_line(ReplyFacts { empty: true, ..f });
        assert_eq!(
            of(ReplyFacts {
                poll: true,
                sticker: true,
                ..facts()
            })
            .placeholder,
            Some(Placeholder::PollQuestion)
        );
        assert_eq!(
            of(ReplyFacts {
                sticker: true,
                command: true,
                ..facts()
            }),
            ReplyLine {
                placeholder: Some(Placeholder::Sticker),
                trailing: Some(Trailing::Sticker),
                forwarded: false,
            }
        );
        assert_eq!(
            of(ReplyFacts {
                command: true,
                voice: true,
                ..facts()
            }),
            ReplyLine {
                placeholder: Some(Placeholder::Command),
                trailing: Some(Trailing::Command),
                forwarded: false,
            }
        );
        assert_eq!(
            of(ReplyFacts {
                voice: true,
                files: true,
                ..facts()
            }),
            ReplyLine {
                placeholder: Some(Placeholder::Voice),
                trailing: Some(Trailing::Voice),
                forwarded: false,
            }
        );
        assert_eq!(
            of(ReplyFacts {
                components_v2: true,
                ..facts()
            }),
            ReplyLine {
                placeholder: Some(Placeholder::Components),
                trailing: None,
                forwarded: false,
            }
        );
    }

    #[test]
    fn pin_notice_and_poll_result() {
        let line = reply_line(ReplyFacts {
            kind: PINNED_TYPE,
            empty: true,
            ..facts()
        });
        assert_eq!(line.placeholder, Some(Placeholder::Pinned));
        assert_eq!(line.trailing, None);
        let line = reply_line(ReplyFacts {
            kind: POLL_RESULT_TYPE,
            empty: true,
            files: true,
            ..facts()
        });
        assert_eq!(line.placeholder, Some(Placeholder::PollResult));
        assert_eq!(line.trailing, None);
    }

    #[test]
    fn titles_are_cut_at_fifty() {
        assert_eq!(cut_to("short", 50), "short");
        let long = format!("{}. more", "a".repeat(47));
        assert_eq!(cut_to(&long, 50), format!("{}\u{2026}", "a".repeat(47)));
    }

    #[test]
    fn a_forward_leads_with_the_arrow_and_reads_as_empty() {
        let line = reply_line(ReplyFacts {
            forwarded: true,
            empty: true,
            ..facts()
        });
        assert!(line.forwarded);
        assert_eq!(line.placeholder, Some(Placeholder::Attachment));
    }
}
