//! The widgets column of the editor (`container_y4ZEM5`): the Board / Activity / Wishlist tab bar and the Board's
//! empty state with its grid of widget tiles to add (`empty_jFcvcH`, `widgetGrid_jFcvcH`). The tiles show the
//! skeleton of a widget with its game name; the hero art of each tile is a remote image the capture does not name.
//! Measured in `design/dom/settings/profile-editor-darker-cozy.json`.

use super::{Mode, Tab, text};
use crate::parity::Parity;
use crate::ui::{Root, color};
use gpui::{AnyElement, Context, Div, IntoElement, div, prelude::*, px};

/// The tiles of the Board's empty state, in grid order (`widgetGrid_jFcvcH`).
const TILES: [&str; 10] = [
    "Marvel Rivals",
    "Wolvesville",
    "Wuthering Waves",
    "Heartopia",
    "Arknights Endfield",
    "Neverness to Everness",
    "Favorite Game",
    "Games I Like",
    "Games in Rotation",
    "Want to Play",
];

fn skeleton(width: f32, height: f32, radius: f32) -> Div {
    div()
        .w(px(width))
        .h(px(height))
        .rounded(px(radius))
        .bg(color("--background-mod-subtle"))
}

fn widget_tile(name: &'static str, glyph: &'static str) -> Div {
    div()
        .relative()
        .w(px(198.5))
        .h(px(99.))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(color("--background-mod-subtle"))
        .child(
            div()
                .absolute()
                .left(px(16.))
                .top(px(16.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(skeleton(42., 12., 8.))
                .child(
                    div()
                        .w(px(85.))
                        .h(px(1.))
                        .bg(color("--background-mod-subtle")),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .w(px(85.))
                        .children((0..6).map(|_| skeleton(23., 15., 12.))),
                ),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(crate::ui::icon(glyph, 24., "--text-default"))
                .child(text(name, 16., 20., 500., "--text-strong")),
        )
}

impl Root {
    pub(super) fn profile_editor_widgets(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.profile_editor.as_ref().expect("editor is open");
        let (tab, nitro) = (open.tab, open.mode == Mode::Nitro);
        let tabs: &[(Tab, &'static str)] = if nitro {
            &[(Tab::Activity, "Activity"), (Tab::Wishlist, "Wishlist")]
        } else {
            &[
                (Tab::Board, "Board"),
                (Tab::Activity, "Activity"),
                (Tab::Wishlist, "Wishlist"),
            ]
        };
        let bar = div()
            .flex()
            .border_b_1()
            .border_color(color("--border-subtle"))
            .children(tabs.iter().enumerate().map(|(index, (kind, name))| {
                let selected = *kind == tab;
                let kind = *kind;
                div()
                    .id(*name)
                    .h(px(30.))
                    .when(index > 0, |this| this.ml(px(24.)))
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(if selected {
                        color("--text-strong")
                    } else {
                        gpui::rgba(0)
                    })
                    .child(text(
                        *name,
                        14.,
                        18.,
                        if selected { 600. } else { 500. },
                        if selected {
                            "--text-strong"
                        } else {
                            "--text-muted"
                        },
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.tab = kind;
                        }
                        cx.notify();
                    }))
            }));
        let content: AnyElement = match tab {
            Tab::Board => div()
                .flex()
                .flex_col()
                .gap(px(24.))
                .child(
                    div()
                        .w(px(320.))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .child(text("Customize your profile with Widgets", 16., 20., 500., "--text-strong"))
                        .child(
                            div()
                                .text_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-default"))
                                .child("Choose from our library of Widgets to share more about yourself and your interests"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.))
                        .children(TILES.iter().enumerate().map(|(index, name)| {
                            widget_tile(name, if index < 6 { "discover_link" } else { "plus_small" })
                        })),
                )
                .into_any_element(),
            Tab::Activity => self.profile_editor_activity_tab().into_any_element(),
            Tab::Wishlist => self
                .profile_editor_wishlist_tab(window, cx)
                .into_any_element(),
        };
        div()
            .parity("profile-editor-widgets")
            .w(px(469.))
            .h_full()
            .flex_none()
            .pl(px(36.))
            .pt(px(17.))
            .ml(px(-4.))
            .flex()
            .flex_col()
            .child(div().w(px(413.)).pl(px(0.)).child(bar))
            .child(
                div()
                    .id("profile-editor-widgets-scroll")
                    .w(px(413.))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(px(19.))
                    .child(content),
            )
            .into_any_element()
    }
}
