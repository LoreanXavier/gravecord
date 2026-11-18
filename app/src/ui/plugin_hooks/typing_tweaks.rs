//! TypingTweaks (Vencord `src/plugins/typingTweaks`, GPL-3.0-or-later): in the "is typing" line each bold name becomes
//! the person: a 16px avatar before the name (`showAvatars`), the name in their role colour (`showRoleColors`), and a
//! click opens their profile. With more than three typing, `alternativeFormatting` lists them as "A, B, and N others are
//! typing..."; `amITyping` keeps you in the list. Blocked and ignored people are left out. Not part of the pinned
//! client.

use super::super::{Root, avatar_source, color};
use crate::plugins::State;
use dc_state::Id;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, img, px,
};
use serde_json::Value;
use std::time::Instant;

const ID: &str = "TypingTweaks";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Settings {
    avatars: bool,
    role_colors: bool,
    alternative: bool,
    me: bool,
}

fn read(state: &State) -> Option<Settings> {
    let on = |value: Value, default: bool| match value {
        Value::Bool(value) => value,
        _ => default,
    };
    state.is_on(ID).then(|| Settings {
        avatars: on(state.value(ID, "showAvatars"), true),
        role_colors: on(state.value(ID, "showRoleColors"), true),
        alternative: on(state.value(ID, "alternativeFormatting"), true),
        me: on(state.value(ID, "amITyping"), false),
    })
}

/// One piece of the line: plain text, or the n-th typing person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Piece {
    Text(String),
    User(usize),
}

/// The line for `count` people: the official sentence with each bold run taken by the next person in order
/// (`renderTypingUsers`), or with `alternative` and more than three, `buildSeveralUsers` (its first `count - 2`
/// people, then "and `count - 2` others are typing...", as the original counts).
pub(super) fn pieces(
    count: usize,
    alternative: bool,
    runs: Vec<(String, Option<Box<str>>)>,
) -> Vec<Piece> {
    if count > 3 && alternative {
        let shown = count - 2;
        let mut out = Vec::new();
        for index in 0..shown {
            out.push(Piece::User(index));
            out.push(Piece::Text(", ".to_owned()));
        }
        out.push(Piece::Text(format!("and {shown} others are typing...")));
        return out;
    }
    let mut next = 0;
    runs.into_iter()
        .map(|(text, tag)| {
            if tag.as_deref() == Some("$b") && next < count {
                next += 1;
                Piece::User(next - 1)
            } else {
                Piece::Text(text)
            }
        })
        .collect()
}

impl Root {
    /// The typing people while the plugin is on (`useTypingUsers`): blocked and ignored left out, you kept with
    /// `amITyping`. `None` while it is off.
    pub(in crate::ui) fn typing_tweaks_users(&self, channel: Id) -> Option<Vec<Id>> {
        if crate::plugins::bypassed() {
            return None;
        }
        let settings = crate::plugins::with(read)?;
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let now = Instant::now();
        Some(
            self.stores
                .typing
                .get(&channel)
                .into_iter()
                .flatten()
                .filter(|(user, ends)| *ends > now && (Some(*user) != me || settings.me))
                .map(|(user, _)| *user)
                .filter(|user| {
                    self.stores.relationships.get(user) != Some(&2)
                        && !self.stores.is_ignored(*user)
                })
                .filter(|user| self.stores.users.contains_key(user))
                .collect(),
        )
    }

    /// The text of the line for `users` (from [`Self::typing_tweaks_users`]); `runs` is the official sentence.
    pub(in crate::ui) fn typing_tweaks_line(
        &self,
        users: &[Id],
        runs: Vec<(String, Option<Box<str>>)>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let settings = crate::plugins::with(read).unwrap_or(Settings {
            avatars: true,
            role_colors: true,
            alternative: true,
            me: false,
        });
        pieces(users.len(), settings.alternative, runs)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(text) => div().child(text).into_any_element(),
                Piece::User(index) => self.typing_user(users[index], settings, cx),
            })
            .collect()
    }

    /// `TypingUser`: a bold, clickable name with its avatar and role colour.
    fn typing_user(&self, id: Id, settings: Settings, cx: &mut Context<Self>) -> AnyElement {
        let Some(user) = self.stores.users.get(&id) else {
            return div().into_any_element();
        };
        let name = self.guild_display_name(user);
        let tint = settings
            .role_colors
            .then(|| self.role_color_of(id))
            .flatten()
            .map(gpui::rgb)
            .unwrap_or_else(|| color("--text-default"));
        let picture = settings
            .avatars
            .then(|| self.images.get(&avatar_source(user).0).cloned())
            .flatten();
        div()
            .id(SharedString::from(format!("typing-user-{id}")))
            .flex()
            .items_center()
            .cursor_pointer()
            .font_weight(FontWeight(600.))
            .text_color(tint)
            .when(settings.avatars, |this| {
                // `.vc-typing-tweaks-user [class*=wrapper]`: inline, `margin-right: .25em`.
                this.child(
                    div()
                        .size(px(16.))
                        .mr(px(3.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(color("--background-mod-normal"))
                        .children(picture.map(|picture| img(picture).size_full().rounded_full())),
                )
            })
            .child(name)
            // `openUserProfile`: the full profile.
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_full_profile(id, window, cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runs(names: usize) -> Vec<(String, Option<Box<str>>)> {
        let mut out = Vec::new();
        for index in 0..names {
            out.push((format!("N{index}"), Some("$b".into())));
            out.push((" , ".to_owned(), None));
        }
        out.push(("are typing...".to_owned(), None));
        out
    }

    #[test]
    fn bold_runs_become_people_and_many_use_the_alternative_line() {
        assert_eq!(
            pieces(2, true, runs(2)),
            [
                Piece::User(0),
                Piece::Text(" , ".to_owned()),
                Piece::User(1),
                Piece::Text(" , ".to_owned()),
                Piece::Text("are typing...".to_owned()),
            ]
        );
        // "**Several people** are typing..." without the alternative line: its bold run is the first person.
        let several = vec![
            ("Several people".to_owned(), Some("$b".into())),
            (" are typing...".to_owned(), None),
        ];
        assert_eq!(
            pieces(5, false, several),
            [Piece::User(0), Piece::Text(" are typing...".to_owned())]
        );
        let alternative = pieces(5, true, Vec::new());
        assert_eq!(alternative.len(), 7);
        assert_eq!(
            alternative[6],
            Piece::Text("and 3 others are typing...".to_owned())
        );
        let mut state = State::default();
        assert_eq!(read(&state), None);
        state.set_enabled(ID, true);
        assert_eq!(
            read(&state),
            Some(Settings {
                avatars: true,
                role_colors: true,
                alternative: true,
                me: false
            })
        );
        state.set_value(ID, "amITyping", &json!(true));
        state.set_value(ID, "showAvatars", &json!(false));
        assert!(read(&state).is_some_and(|settings| settings.me && !settings.avatars));
    }
}
