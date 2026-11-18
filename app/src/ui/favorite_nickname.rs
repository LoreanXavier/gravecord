//! "Change Nickname" / "Rename Category" of the Favorites server (module 250737, the item of a favourite's menu, shown
//! only inside the Favorites server: `ai` of module 5180), which opens module 872243: the heading as the title and as
//! the submit button, one text field (`formTitle` "Nickname" or "Category Name", the channel's name as placeholder,
//! 100 characters at most, focused), then "This name will only appear in your favorites." (`description_bi57kT`: 8px
//! above it, `body-sm/normal` in `text-default`). Submit stores the nickname in the favourite (`w6` of module 146092:
//! the entry's `nickname`, empty when cleared) and closes.

use super::{Root, color, editor_style, modal};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_proto::settings::FAVORITE_CATEGORY;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Subscription,
    Window, div, prelude::*, px,
};

/// `maxLength` of the field.
const LIMIT: usize = 100;

pub(super) struct Dialog {
    pub input: Entity<Editor>,
    _events: Subscription,
    channel: Id,
    /// The favourite is a category the account made (`GUILD_CATEGORY` in the menu's channel): "Rename Category".
    category: bool,
    save_focus: FocusHandle,
}

/// The heading and the field's label (module 250737): `xXYKiP` / `OCAkGP` for a category, else `dilOF6` / `621LJD`.
pub(super) fn texts(category: bool) -> (&'static str, &'static str) {
    if category {
        (dc_intl::t("xXYKiP"), dc_intl::t("OCAkGP"))
    } else {
        (dc_intl::t("dilOF6"), dc_intl::t("621LJD"))
    }
}

/// The favourites with `channel`'s nickname set to `text` (`w6`: nothing happens for a channel that is not one).
pub(super) fn renamed(
    mut favorites: dc_proto::settings::Favorites,
    channel: Id,
    text: &str,
) -> Option<dc_proto::settings::Favorites> {
    let (_, known) = favorites
        .channels
        .iter_mut()
        .find(|(id, _)| *id == channel)?;
    known.nickname = text.to_owned();
    Some(favorites)
}

impl Root {
    /// The placeholder of the field (module 47167 `Ay`): the channel's own name, a direct message's recipient.
    fn favorite_placeholder(&self, channel: Id) -> String {
        if let Some(known) = self
            .stores
            .guilds
            .iter()
            .find_map(|guild| guild.channels.iter().find(|known| known.id == channel))
        {
            return known.name.clone();
        }
        self.stores
            .private_channels
            .iter()
            .find(|dm| dm.id == channel)
            .map(|dm| self.private_channel_name(dm))
            .unwrap_or_default()
    }

    /// "Change Nickname" of a favourite (module 250737 `action`).
    pub(super) fn open_favorite_nickname(
        &mut self,
        channel: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let favorites = self.stores.favorites();
        let Some((_, known)) = favorites.channels.iter().find(|(id, _)| *id == channel) else {
            return;
        };
        let category = known.kind == FAVORITE_CATEGORY;
        let initial = known.nickname.clone();
        let placeholder = self.favorite_placeholder(channel);
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            let mut input = Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_reports_all_edits(true);
            input.set_max_utf16(LIMIT);
            input.set_single_line(true);
            input.set_placeholder(placeholder);
            input.set_text(&initial);
            input
        });
        let events = cx.subscribe(&input, |this, editor, event: &EditorEvent, cx| {
            if !this
                .favorite_nickname
                .as_ref()
                .is_some_and(|dialog| dialog.input.entity_id() == editor.entity_id())
            {
                return;
            }
            match event {
                EditorEvent::Edited => {}
                EditorEvent::Submit(_) => this.save_favorite_nickname(cx),
                EditorEvent::Cancel => this.favorite_nickname = None,
            }
            cx.notify();
        });
        let field_focus = input.read(cx).focus_handle(cx);
        self.favorite_nickname = Some(Dialog {
            input,
            _events: events,
            channel,
            category,
            save_focus: cx.focus_handle(),
        });
        // `autoFocus` of the field, once the menu that opened the dialog has closed and given focus back.
        window.focus(&field_focus, cx);
        window.defer(cx, move |window, cx| window.focus(&field_focus, cx));
        cx.notify();
    }

    fn save_favorite_nickname(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.favorite_nickname.take() else {
            return;
        };
        let text = dialog.input.read(cx).text();
        if let Some(favorites) = renamed(self.stores.favorites(), dialog.channel, text) {
            let (heading, _) = texts(dialog.category);
            self.send_favorites(&favorites, heading);
        }
        cx.notify();
    }

    pub(super) fn favorite_nickname_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.favorite_nickname.as_ref()?;
        let (heading, label) = texts(dialog.category);
        let focused = dialog.input.read(cx).focus_handle(cx).is_focused(window);
        let input = dialog.input.clone();
        // The same text input (module 95477 `k`) as the friend nickname's, measured there.
        let field = div()
            .parity_with_border("favorite-nickname-input", if focused { 2. } else { 1. })
            .id("favorite-nickname-input")
            .h(px(40.))
            .w_full()
            .rounded(px(8.))
            .bg(color("--input-background-default"))
            .when(focused, |d| d.border_2())
            .when(!focused, |d| d.border_1())
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .rule(crate::ui::motion::Rule::TextField)
            .px(px(if focused { 11. } else { 12. }))
            .flex()
            .items_center()
            .on_click(cx.listener(move |_, _, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx);
                cx.notify();
            }))
            .child(div().flex_1().min_w(px(0.)).child(dialog.input.clone()));
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .parity("favorite-nickname-label")
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(label),
                    )
                    .child(field),
            )
            .child(
                div()
                    .parity("favorite-nickname-description")
                    .mt(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("2vyz+N")),
            );
        let mut save = modal::button(
            "favorite-nickname-save",
            heading,
            modal::Tone::Primary,
            |this, _, cx| this.save_favorite_nickname(cx),
        );
        save.focus = Some(dialog.save_focus.clone());
        Some(modal::render(
            modal::Spec {
                id: "favorite-nickname-modal",
                title: heading.into(),
                subtitle: None,
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "favorite-nickname-cancel",
                        dc_intl::t("ETE/oC"),
                        modal::Tone::Secondary,
                        |this, _, _| this.favorite_nickname = None,
                    ),
                    save,
                ],
                close: Box::new(|this, _, _| this.favorite_nickname = None),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_proto::settings::{FAVORITE_REFERENCE, FavoriteChannel, Favorites};

    #[test]
    fn a_channel_and_a_category_have_their_own_words() {
        assert_eq!(texts(false), ("Change Nickname", "Nickname"));
        assert_eq!(texts(true), ("Rename Category", "Category Name"));
        assert_eq!(
            dc_intl::t("2vyz+N"),
            "This name will only appear in your favorites."
        );
    }

    #[test]
    fn the_nickname_goes_into_the_favourite_and_only_a_favourite() {
        let favorites = Favorites {
            channels: vec![(
                7,
                FavoriteChannel {
                    kind: FAVORITE_REFERENCE,
                    position: 1,
                    ..FavoriteChannel::default()
                },
            )],
            ..Favorites::default()
        };
        let named = renamed(favorites.clone(), 7, "my place").unwrap();
        assert_eq!(named.channels[0].1.nickname, "my place");
        let cleared = renamed(named, 7, "").unwrap();
        assert_eq!(cleared.channels[0].1.nickname, "");
        assert!(renamed(favorites, 8, "x").is_none());
    }
}
