//! Create Channel and Create Category (the server menu's entries), measured in
//! `design/dom/modals/create-channel-darker-cozy.json`: Channel Type radios (Text, Voice, Forum) with their
//! descriptions, Channel Name with the type's icon, and the Private Channel switch. Create posts
//! `POST /guilds/{guild}/channels {name, type, parent_id?}`; text and voice names are lower-cased with spaces as
//! dashes, as the official field normalizes them.

use super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::json;

/// GUILD_TEXT, GUILD_VOICE, GUILD_CATEGORY and GUILD_FORUM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Text = 0,
    Voice = 2,
    Category = 4,
    Forum = 15,
}

pub(crate) struct CreateChannel {
    pub kind: Kind,
    pub name: Entity<Editor>,
    _events: Subscription,
    pub private: bool,
    pub parent: Option<Id>,
}

/// The official name rule for text-like channels: lower case, whitespace runs become one dash.
pub(crate) fn channel_slug(name: &str) -> String {
    name.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// `container_9AraPt`: a 48x24 switch, the white 24px thumb at the side it is set to.
pub(super) fn switch(on: bool) -> gpui::Div {
    div()
        .relative()
        .flex_none()
        .w(px(48.))
        .h(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(if on {
            color("--background-brand")
        } else {
            gpui::rgba(0x0000001f)
        })
        .child(
            div()
                .absolute()
                .top(px(-1.))
                .left(px(if on { 23. } else { -1. }))
                .size(px(24.))
                .rounded_full()
                .bg(gpui::rgb(0xffffff)),
        )
}

/// `radioIndicator_oJuRbG`: brand with a white 8px dot when picked, a faint ring otherwise.
pub(super) fn radio(picked: bool) -> gpui::Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(if picked {
            color("--checkbox-border-selected-default")
        } else {
            color("--checkbox-border-default")
        })
        .bg(if picked {
            color("--checkbox-background-selected-default")
        } else {
            color("--checkbox-background-default")
        })
        .flex()
        .items_center()
        .justify_center()
        .when(picked, |this| {
            this.child(div().size(px(8.)).rounded_full().bg(gpui::rgb(0xffffff)))
        })
}

impl Root {
    pub(super) fn open_create_channel(
        &mut self,
        category: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(
                if category {
                    "New Category"
                } else {
                    "new-channel"
                }
                .to_owned(),
            );
            editor
        });
        let events = cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.submit_create_channel(cx);
                }
                EditorEvent::Cancel => this.create_channel = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(name.read(cx), cx), cx);
        self.create_channel = Some(CreateChannel {
            kind: if category { Kind::Category } else { Kind::Text },
            name,
            _events: events,
            private: false,
            parent: None,
        });
        cx.notify();
    }

    fn submit_create_channel(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Some(guild)) = (self.create_channel.as_ref(), self.guild) else {
            return;
        };
        let typed = form.name.read(cx).text().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let name = match form.kind {
            Kind::Text | Kind::Forum => channel_slug(&typed),
            Kind::Voice | Kind::Category => typed,
        };
        let mut body = json!({"type": form.kind as u64, "name": name, "permission_overwrites": []});
        if let Some(parent) = form.parent {
            body["parent_id"] = json!(parent.to_string());
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/channels"),
            body: Some(body),
        });
        self.create_channel = None;
        cx.notify();
    }

    pub(super) fn create_channel_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.create_channel.as_ref()?;
        let category = form.kind == Kind::Category;
        let typed = !form.name.read(cx).text().trim().is_empty();
        let label = |text: &'static str| {
            div()
                .mb(px(8.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let option = |kind: Kind, name: &'static str, title: &'static str, detail: &'static str| {
            let picked = form.kind == kind;
            div()
                .id(SharedString::from(format!("channel-type-{kind:?}")))
                .relative()
                .h(px(42.))
                .pl(px(56.))
                .rounded(px(8.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = this.create_channel.as_mut() {
                        form.kind = kind;
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .child(radio(picked)),
                )
                .child(div().absolute().left(px(32.)).top(px(0.)).child(icon(
                    name,
                    20.,
                    "--text-strong",
                )))
                .child(
                    div()
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .ml(px(-24.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(detail),
                )
        };
        let name_icon = match form.kind {
            Kind::Voice => "channel_type_voice",
            Kind::Forum => "channel_type_forum",
            Kind::Category => "",
            Kind::Text => "channel_type_text",
        };
        let private = form.private;
        let body = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .when(!category, |this| {
                this.child(
                    div()
                        .child(label("Channel Type"))
                        .child(
                            div()
                                .pt(px(8.))
                                .pb(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .child(option(
                                    Kind::Text,
                                    "channel_type_text",
                                    "Text",
                                    "Send messages, images, GIFs, emoji, opinions, and puns",
                                ))
                                .child(option(
                                    Kind::Voice,
                                    "channel_type_voice",
                                    "Voice",
                                    "Hang out together with voice, video, and screen share",
                                ))
                                .child(option(
                                    Kind::Forum,
                                    "channel_type_forum",
                                    "Forum",
                                    "Create a space for organized discussions",
                                )),
                        ),
                )
            })
            .child(
                div()
                    .child(label(if category { "Category Name" } else { "Channel Name" }))
                    .child(
                        div()
                            .h(px(40.))
                            .px(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-active"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .when(!category, |this| {
                                this.child(
                                    div()
                                        .size(px(30.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(name_icon, 16., "--icon-strong")),
                                )
                            })
                            .when(category, |this| this.pl(px(12.)))
                            .child(div().flex_1().min_w(px(0.)).child(form.name.clone()))
                            // The emoji button, grey until hovered (`spriteGreyscale_SjG8Ug`).
                            .children(self.images.get("emoji_button").cloned().map(|image| {
                                div()
                                    .w(px(32.))
                                    .h(px(30.))
                                    .flex()
                                    .items_center()
                                    .child(gpui::img(image).size(px(18.)).grayscale(true))
                            })),
                    ),
            )
            .child(
                div()
                    .id("create-channel-private")
                    .cursor_pointer()
                    .flex()
                    .gap(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.create_channel.as_mut() {
                            form.private = !form.private;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(div().pr(px(4.)).child(icon("channel_private", 16., "--interactive-icon-default")))
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(if category { "Private Category" } else { "Private Channel" }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(if category {
                                        "By making a category private, only selected members and roles will be able to view this category. Synced channels in this category will automatically match to this setting."
                                    } else {
                                        "Only selected members and roles will be able to view this channel."
                                    }),
                            ),
                    )
                    .child(switch(private)),
            )
            .into_any_element();
        let create_label = if category {
            "Create Category"
        } else {
            "Create Channel"
        };
        Some(modal::render(
            modal::Spec {
                id: "create-channel-modal",
                title: create_label.into(),
                subtitle: None,
                body: Some(body),
                buttons: [
                    modal::button(
                        "create-channel-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.create_channel = None,
                    ),
                    modal::button(
                        "create-channel-submit",
                        create_label,
                        if typed {
                            modal::Tone::Primary
                        } else {
                            modal::Tone::PrimaryDisabled
                        },
                        |this, _, cx| this.submit_create_channel(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.create_channel = None),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::channel_slug;

    #[test]
    fn names_take_the_channel_form() {
        assert_eq!(channel_slug("My New  Channel"), "my-new-channel");
    }
}
