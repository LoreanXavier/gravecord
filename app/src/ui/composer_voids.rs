//! The looks of the composer's voids (`crate::editor_atoms`): user, role and channel mentions, `@everyone` and
//! `@here`, and custom emoji, drawn as module 787564 draws them in the Slate editor. Each one goes through the same
//! mention component as a message (module 332173, the `.wrapper_NDaLCQ` pill; the channel's icon of module 232042;
//! the role colour of `er`), so the composer reuses the message renderer's piece for it.

use super::{Root, hsla};
use crate::editor::{AtomLook, Editor, Pill};
use crate::editor_atoms::{AtomKind, RawKind, find_atoms, raw_mentions};
use dc_markdown::{Kind, Span};
use gpui::{Context, Entity};
use std::collections::HashMap;

/// `.emoji` (`--custom-emoji-size-emoji: 1.375em`) at the composer's 16px.
const EMOJI_SIZE: f32 = 22.;

thread_local! {
    /// The text each field's raw-mention pass last looked at (the composer's, then the edit box's), so an unchanged
    /// text is not resolved again each frame.
    static SCANNED: std::cell::RefCell<[String; 2]> = const {
        std::cell::RefCell::new([String::new(), String::new()])
    };
}

impl Root {
    /// Each frame: raw mentions become tokens, and the voids of the composer's text (and the edit box's, the same
    /// Slate editor in the official client) get their looks.
    pub(super) fn composer_voids_frame(&mut self, cx: &mut Context<Self>) {
        let composer = self.composer.clone();
        self.editor_voids(&composer, 0, cx);
        if let Some((_, editor)) = self.editing.clone() {
            self.editor_voids(&editor, 1, cx);
        }
    }

    fn editor_voids(&mut self, editor: &Entity<Editor>, slot: usize, cx: &mut Context<Self>) {
        let mut text = editor.read(cx).text().to_owned();
        // Module 2368's first pass: a finished `@name`, `#channel` or `:emoji:` the client can resolve becomes its
        // token (and so a void), whether it was typed or pasted. Only while the preview is on (`areStylesDisabled`).
        let fresh = SCANNED.with(|scanned| scanned.borrow()[slot] != text);
        if fresh && crate::markup_preview::enabled() {
            let edits = raw_mentions(&text, |kind| self.resolve_raw(kind));
            if editor.update(cx, |editor, cx| {
                let changed = editor.rewrite(edits);
                if changed {
                    cx.notify();
                }
                changed
            }) {
                text = editor.read(cx).text().to_owned();
            }
        }
        SCANNED.with(|scanned| scanned.borrow_mut()[slot] = text.clone());
        let mut looks: HashMap<AtomKind, AtomLook> = HashMap::new();
        for atom in find_atoms(&text) {
            if let std::collections::hash_map::Entry::Vacant(entry) = looks.entry(atom.kind) {
                let look = self.void_look(entry.key());
                entry.insert(look);
            }
        }
        editor.update(cx, |editor, _| editor.set_atom_looks(looks));
    }

    fn void_look(&mut self, kind: &AtomKind) -> AtomLook {
        let span_kind = match kind {
            AtomKind::User(id) => Kind::User(*id),
            AtomKind::Role(id) => Kind::Role(*id),
            AtomKind::Channel(id) => Kind::Channel(*id),
            AtomKind::Emoji { name, id, animated } => {
                let ext = if *animated { "gif" } else { "webp" };
                self.request_image(
                    format!("custom:{id}"),
                    format!("https://cdn.discordapp.com/emojis/{id}.{ext}?size=48"),
                );
                Kind::Emoji {
                    name: name.clone(),
                    id: *id,
                    animated: *animated,
                }
            }
            AtomKind::Text(_) => Kind::Everyone,
        };
        let text = match kind {
            AtomKind::Text(word) => word.clone(),
            _ => String::new(),
        };
        let span = Span {
            text,
            style: Default::default(),
            kind: span_kind,
        };
        let piece = self.piece(
            &span,
            hsla("--text-default"),
            crate::editor::FONT_SIZE,
            400.,
            EMOJI_SIZE,
        );
        let label = match kind {
            // 787564 `el`: someone the client does not know shows as the raw token inside the pill.
            AtomKind::User(id) if !self.knows_user(*id) => format!("<@{id}>"),
            _ => piece.text.clone(),
        };
        AtomLook {
            label,
            color: piece.color,
            weight: piece.font.weight.0,
            pill: piece.boxed.as_ref().map(|boxed| Pill {
                background: boxed.background,
                padding_x: boxed.padding_x,
                height: boxed.height,
                top: boxed.top,
                radius: boxed.radius,
            }),
            icon: piece.boxed.as_ref().and_then(|boxed| boxed.icon.clone()),
            image: piece.image.clone(),
            copy: self.void_copy(kind),
            prevent_selection: matches!(kind, AtomKind::User(_) | AtomKind::Role(_)),
        }
    }

    /// Module 224868 `p`: what a raw mention names, as its token.
    fn resolve_raw(&self, kind: &RawKind) -> Option<String> {
        match kind {
            RawKind::At {
                name,
                discriminator,
            } => {
                // A role of the server by its exact name, when no discriminator was typed.
                if discriminator.is_none()
                    && let Some(guild) = self.current_guild()
                    && let Some((id, _)) = guild.roles.iter().find(|(_, role)| *role == name)
                {
                    return Some(format!("<@&{id}>"));
                }
                // `N()`: the one member (or DM recipient) whose username starts with the name, with the typed
                // discriminator ("0" for a unique username), and it must match exactly.
                let wanted = discriminator.as_deref().unwrap_or("0");
                let users: Vec<_> = self
                    .mention_candidates()
                    .into_iter()
                    .map(|(user, _)| user)
                    .filter(|user| {
                        let own = if user.discriminator.is_empty() {
                            "0"
                        } else {
                            user.discriminator.as_str()
                        };
                        user.username.starts_with(name.as_str()) && own == wanted
                    })
                    .collect();
                match users.as_slice() {
                    [user] if user.username == *name => Some(format!("<@{}>", user.id)),
                    _ => None,
                }
            }
            // A custom emoji the user can use here by its name (`getDisambiguatedEmojiContext`).
            RawKind::Emoji(name) => {
                let premium = dc_state::premium::PremiumType::from_wire(self.premium_wire())
                    .has(dc_state::premium::Feature::EmojisEverywhere);
                self.stores
                    .guilds
                    .iter()
                    .filter(|guild| Some(guild.id) == self.guild || premium)
                    .flat_map(|guild| guild.emojis.iter())
                    .find(|emoji| emoji.available && emoji.name == *name)
                    .map(|emoji| {
                        dc_state::autocomplete::custom_emoji_markup(
                            &emoji.name,
                            emoji.id,
                            emoji.animated,
                        )
                    })
            }
            // A channel of the server by its name, text channels first, then the other kinds, then its threads.
            RawKind::Channel(name) => {
                let guild = self.current_guild()?;
                guild
                    .channels
                    .iter()
                    .filter(|channel| channel.name == *name)
                    .min_by_key(|channel| !matches!(channel.kind, dc_state::ChannelKind::Text))
                    .map(|channel| channel.id)
                    .or_else(|| {
                        guild
                            .threads
                            .iter()
                            .find(|thread| thread.name == *name)
                            .map(|thread| thread.id)
                    })
                    .map(|id| format!("<#{id}>"))
            }
        }
    }

    fn knows_user(&self, id: dc_state::Id) -> bool {
        self.stores.users.contains_key(&id)
            || self
                .stores
                .current_user
                .as_ref()
                .is_some_and(|user| user.id == id)
    }

    /// Copy's `plain` text of a void (module 323350): `@` and the user tag, `@` and the role name, the channel's name
    /// with its `#`, the emoji's name, or the raw token when the client does not know it.
    fn void_copy(&self, kind: &AtomKind) -> String {
        match kind {
            AtomKind::User(id) => {
                let user = self.stores.users.get(id).or(self
                    .stores
                    .current_user
                    .as_ref()
                    .filter(|user| user.id == *id));
                match user {
                    Some(user) if user.discriminator.is_empty() || user.discriminator == "0" => {
                        format!("@{}", user.username)
                    }
                    Some(user) => format!("@{}#{}", user.username, user.discriminator),
                    None => format!("<@{id}>"),
                }
            }
            AtomKind::Role(id) => self
                .current_guild()
                .and_then(|guild| guild.roles.get(id))
                .map_or_else(|| format!("<@&{id}>"), |name| format!("@{name}")),
            AtomKind::Channel(id) => self
                .current_guild()
                .and_then(|guild| guild.channels.iter().find(|channel| channel.id == *id))
                .map_or_else(
                    || format!("<#{id}>"),
                    |channel| format!("#{}", channel.name),
                ),
            AtomKind::Emoji { name, .. } => format!(":{name}:"),
            AtomKind::Text(word) => word.clone(),
        }
    }
}
