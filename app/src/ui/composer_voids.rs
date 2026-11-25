//! The looks of the composer's voids (`crate::editor_atoms`): user, role and channel mentions, `@everyone` and
//! `@here`, and custom emoji, drawn as module 787564 draws them in the Slate editor. Each one goes through the same
//! mention component as a message (module 332173, the `.wrapper_NDaLCQ` pill; the channel's icon of module 232042;
//! the role colour of `er`), so the composer reuses the message renderer's piece for it.

use super::{Root, hsla};
use crate::editor::{AtomLook, Pill};
use crate::editor_atoms::{AtomKind, find_atoms};
use dc_markdown::{Kind, Span};
use gpui::Context;
use std::collections::HashMap;

/// `.emoji` (`--custom-emoji-size-emoji: 1.375em`) at the composer's 16px.
const EMOJI_SIZE: f32 = 22.;

impl Root {
    /// Each frame: the looks of the voids in the composer's text.
    pub(super) fn composer_voids_frame(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().to_owned();
        let mut looks: HashMap<AtomKind, AtomLook> = HashMap::new();
        for atom in find_atoms(&text) {
            if let std::collections::hash_map::Entry::Vacant(slot) = looks.entry(atom.kind) {
                let look = self.void_look(slot.key());
                slot.insert(look);
            }
        }
        self.composer
            .update(cx, |editor, _| editor.set_atom_looks(looks));
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
