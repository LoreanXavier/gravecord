//! Server Settings > Server Template (module 989433 `pC`, `pS`, `p_`, `pv`, `pT`, `py`; requests module 292572).
//!
//! `GET /guilds/{id}/templates` tells whether one exists. Without one: the title (at least 2 characters, `pg`), the
//! description, and Generate Template (`POST` with name and description). With one: its `discord.new` link, Sync
//! Template while it is dirty (`PUT /guilds/{id}/templates/{code}`), Delete Template behind a confirmation (`DELETE`)
//! and Preview Template, which opens the template's page: the official preview dialog is not drawn.
//! Measured in `design/dom/settings/guild-template-darker-cozy.json`.

use super::{Root, color};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, div,
    prelude::*, px, svg,
};
use serde_json::Value;

pub(super) const TEMPLATES_PREFIX: &str = "guild-templates:";
const CHANGED_PREFIX: &str = "guild-templates:changed:";
/// `window.GLOBAL_ENV.GUILD_TEMPLATE_HOST` in the pinned build.
const TEMPLATE_HOST: &str = "discord.new";

pub(crate) struct TemplateState {
    pub(super) title: Entity<Editor>,
    pub(super) description: Entity<Editor>,
    /// Delete Template is asking.
    pub(super) confirm_delete: bool,
    _events: [Subscription; 2],
}

/// Module 989433 `pg`.
fn title_ok(title: &str) -> bool {
    title.trim().chars().count() >= 2
}

fn link_of(code: &str) -> String {
    format!("https://{TEMPLATE_HOST}/{code}")
}

impl Root {
    pub(super) fn request_template_page(&mut self, guild: Id, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Fetch {
            key: format!("{TEMPLATES_PREFIX}{guild}"),
            route: format!("/guilds/{guild}/templates"),
        });
        let Some(open) = self.server_settings.as_mut() else {
            return;
        };
        if open.template.is_some() {
            return;
        }
        let field = |placeholder: &str, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_owned();
            cx.new(|cx| {
                let mut editor = Editor::new(super::editor_style(), cx);
                editor.set_placeholder(placeholder);
                editor
            })
        };
        let title = field(
            "Who is this server for? E.g. School Club, Artists' Community",
            cx,
        );
        let description = field("What can people do in this server?", cx);
        let notify = |this: &mut Root, event: &EditorEvent, cx: &mut Context<Root>| {
            if matches!(event, EditorEvent::Cancel) {
                this.server_settings = None;
            }
            cx.notify();
        };
        let title_events = cx.subscribe(&title, move |this, _, event: &EditorEvent, cx| {
            notify(this, event, cx)
        });
        let description_events = cx
            .subscribe(&description, move |this, _, event: &EditorEvent, cx| {
                notify(this, event, cx)
            });
        if let Some(open) = self.server_settings.as_mut() {
            open.template = Some(TemplateState {
                title,
                description,
                confirm_delete: false,
                _events: [title_events, description_events],
            });
        }
    }

    /// A change went through: ask again what the server has.
    pub(super) fn templates_fetched(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(guild) = key.strip_prefix(CHANGED_PREFIX) {
            if let Ok(guild) = guild.parse::<Id>() {
                self.request_template_page(guild, cx);
            }
        } else if let Some(guild) = key
            .strip_prefix(TEMPLATES_PREFIX)
            .and_then(|guild| guild.parse::<Id>().ok())
        {
            // `reset`: the fields take the template's values.
            self.reset_template(guild, cx);
        }
        cx.notify();
    }

    pub(super) fn reset_template(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(found) = self.template_of(guild) else {
            return;
        };
        let name = found["name"].as_str().unwrap_or_default().to_owned();
        let description = found["description"].as_str().unwrap_or_default().to_owned();
        if let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.template.as_ref())
        {
            state.title.update(cx, |editor, _| editor.set_text(&name));
            state
                .description
                .update(cx, |editor, _| editor.set_text(&description));
        }
        cx.notify();
    }

    /// `showNotice`: a template exists and the fields differ from it.
    pub(super) fn template_dirty(&self, guild: Id, cx: &Context<Self>) -> bool {
        let (Some(found), Some(state)) = (
            self.template_of(guild),
            self.server_settings
                .as_ref()
                .and_then(|open| open.template.as_ref()),
        ) else {
            return false;
        };
        state.title.read(cx).text().trim() != found["name"].as_str().unwrap_or_default()
            || state.description.read(cx).text().trim()
                != found["description"].as_str().unwrap_or_default()
    }

    /// `save`: `PATCH /guilds/{id}/templates/{code}`.
    pub(super) fn save_template(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(found) = self.template_of(guild) else {
            return;
        };
        let Some(code) = found["code"].as_str().map(str::to_owned) else {
            return;
        };
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.template.as_ref())
        else {
            return;
        };
        let name = state.title.read(cx).text().to_owned();
        let description = state.description.read(cx).text().to_owned();
        self.template_change(
            guild,
            reqwest::Method::PATCH,
            format!("/guilds/{guild}/templates/{code}"),
            Some(serde_json::json!({"name": name, "description": description})),
        );
    }

    fn template_of(&self, guild: Id) -> Option<Value> {
        self.fetched
            .get(&format!("{TEMPLATES_PREFIX}{guild}"))?
            .as_array()?
            .first()
            .cloned()
    }

    fn template_change(
        &mut self,
        guild: Id,
        method: reqwest::Method,
        route: String,
        body: Option<Value>,
    ) {
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{CHANGED_PREFIX}{guild}")),
            method,
            route,
            body,
        });
    }

    /// `descriptionRow_5tsZcq`.
    fn template_row(tick: bool, text: &'static str) -> gpui::Div {
        div()
            .mt(px(16.))
            .flex()
            .items_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .text_color(color("--text-default"))
            .child(
                // A white disc under the tinted shape (`secondaryColor` WHITE).
                div()
                    .relative()
                    .size(px(20.))
                    .mr(px(16.))
                    .flex_none()
                    .child(
                        div()
                            .absolute()
                            .left(px(2.))
                            .top(px(2.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(gpui::white()),
                    )
                    .child(
                        svg()
                            .absolute()
                            .size(px(20.))
                            .path(if tick {
                                "icons/template_yes.svg"
                            } else {
                                "icons/template_no.svg"
                            })
                            .text_color(color(if tick { "--green-360" } else { "--red-400" })),
                    ),
            )
            .child(text)
    }

    fn template_button(
        id: &'static str,
        label: String,
        primary: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(40.))
            .px(px(15.))
            .rounded(px(8.))
            .border_1()
            .border_color(if primary {
                color("--control-primary-border-default")
            } else {
                color("--control-secondary-border-default")
            })
            .bg(if primary {
                color("--control-primary-background-default")
            } else {
                color("--control-secondary-background-default")
            })
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(if primary {
                color("--white")
            } else {
                color("--control-secondary-text-default")
            })
            .child(label)
    }

    pub(super) fn server_template_page(&self, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self
            .server_settings
            .as_ref()
            .and_then(|open| open.template.as_ref())
        else {
            return div().into_any_element();
        };
        let template = self.template_of(guild);
        let label = |text: &'static str, required: bool| {
            div()
                .mb(px(8.))
                .flex()
                .gap(px(4.))
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
                .when(required, |this| {
                    this.child(
                        div()
                            .font_weight(FontWeight(400.))
                            .text_color(color("--text-feedback-critical"))
                            .child("*"),
                    )
                })
        };
        let input = |field: Entity<Editor>, height: f32, id: &'static str| {
            div()
                .id(id)
                .h(px(height))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--input-border-default"))
                .bg(color("--input-background-default"))
                .px(px(12.))
                .when(height <= 40., |this| this.flex().items_center())
                .when(height > 40., |this| this.pt(px(12.)))
                .child(div().w_full().child(field))
        };
        let actions: AnyElement = match &template {
            None => {
                let title = state.title.read(cx).text().to_owned();
                let ready = title_ok(&title);
                Self::template_button(
                    "server-template-generate",
                    "Generate Template".to_owned(),
                    true,
                )
                .when(!ready, |this| this.opacity(0.5))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let Some(state) = this
                        .server_settings
                        .as_ref()
                        .and_then(|open| open.template.as_ref())
                    else {
                        return;
                    };
                    let name = state.title.read(cx).text().to_owned();
                    if !title_ok(&name) {
                        return;
                    }
                    let description = state.description.read(cx).text().to_owned();
                    this.template_change(
                        guild,
                        reqwest::Method::POST,
                        format!("/guilds/{guild}/templates"),
                        Some(serde_json::json!({"name": name, "description": description})),
                    );
                }))
                .into_any_element()
            }
            Some(found) => {
                let code = found["code"].as_str().unwrap_or_default().to_owned();
                let dirty = found["is_dirty"].as_bool() == Some(true);
                let link = link_of(&code);
                let copy_link = link.clone();
                let preview = link.clone();
                let sync_code = code.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(label("Template Link", false))
                    .child(
                        div()
                            .w_full()
                            .h(px(40.))
                            .pl(px(12.))
                            .pr(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .child(div().flex_1().text_size(px(16.)).line_height(px(20.)).text_color(color("--text-default")).child(link))
                            .child(
                                div()
                                    .id("server-template-copy")
                                    .h(px(32.))
                                    .px(px(11.))
                                    .rounded(px(8.))
                                    .bg(color("--control-primary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .child("Copy")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_link.clone()));
                                    })),
                            ),
                    )
                    .when(dirty, |this| {
                        this.child(
                            div()
                                .mt(px(8.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-feedback-warning"))
                                .child("* You have unsynced changes. Update the channels, roles, and permissions in your template by syncing."),
                        )
                    })
                    .child(
                        div()
                            .mt(px(20.))
                            .flex()
                            .when(dirty, |this| {
                                this.child(Self::template_button("server-template-sync", "Sync Template".to_owned(), true).on_click(cx.listener(move |this, _, _, cx| {
                                    this.template_change(guild, reqwest::Method::PUT, format!("/guilds/{guild}/templates/{sync_code}"), None);
                                    cx.notify();
                                })))
                            })
                            .child(
                                div()
                                    .ml_auto()
                                    .flex()
                                    .gap(px(16.))
                                    .child(
                                        div()
                                            .id(SharedString::from("server-template-delete"))
                                            .h(px(40.))
                                            .px(px(15.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--control-critical-secondary-border-default"))
                                            .bg(color("--control-critical-secondary-background-default"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--control-critical-secondary-text-default"))
                                            .child("Delete Template")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if let Some(state) = this.server_settings.as_mut().and_then(|open| open.template.as_mut()) {
                                                    state.confirm_delete = true;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                    .child(Self::template_button("server-template-preview", "Preview Template".to_owned(), false).on_click(cx.listener(move |_, _, _, cx| cx.open_url(&preview)))),
                            ),
                    )
                    .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Server Template"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("A server template is an easy way to share your server setup and help anyone create a server instantly."),
            )
            .child(
                div()
                    .mt(px(14.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("When someone uses your server template link, they create a new server pre-filled with the same channels, roles, permissions, and settings as yours."),
            )
            .child(
                div()
                    .mt(px(20.))
                    .p(px(16.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .child(div().text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("Templates will copy:"))
                            .child(Self::template_row(true, "Channels and channel topics"))
                            .child(Self::template_row(true, "Roles and permissions"))
                            .child(Self::template_row(true, "Default server settings")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(div().text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight(500.)).text_color(color("--text-strong")).child("Templates will not copy:"))
                            .child(Self::template_row(false, "Messages or any content"))
                            .child(Self::template_row(false, "Members or bots"))
                            .child(Self::template_row(false, "Your server icon, Boosts, or other perks")),
                    ),
            )
            .child(div().my(px(20.)).h(px(1.)).bg(color("--border-subtle")))
            .child(label("Template Title", true))
            .child(input(state.title.clone(), 40., "server-template-title"))
            .child(div().mt(px(20.)).child(label("Template Description", false)))
            .child(input(state.description.clone(), 89., "server-template-description"))
            .child(div().mt(px(20.)).flex().child(actions))
            .into_any_element()
    }

    /// `pb`: "Delete Template", `DELETE /guilds/{id}/templates/{code}`.
    pub(super) fn template_delete_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.server_settings.as_ref()?;
        if !open.template.as_ref()?.confirm_delete {
            return None;
        }
        let guild = open.guild;
        let code = self.template_of(guild)?["code"].as_str()?.to_owned();
        let close = |this: &mut Root| {
            if let Some(state) = this
                .server_settings
                .as_mut()
                .and_then(|open| open.template.as_mut())
            {
                state.confirm_delete = false;
            }
        };
        Some(super::modal::render(
            super::modal::Spec {
                id: "server-template-delete-modal",
                title: "Delete Template".into(),
                subtitle: Some(
                    div()
                        .child("Are you sure you want to delete your template? This will prevent anyone with the link from using it.")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button("server-template-delete-cancel", "Cancel", super::modal::Tone::Secondary, move |this, _, _| close(this)),
                    super::modal::button("server-template-delete-confirm", "Delete Template", super::modal::Tone::Critical, move |this, _, cx| {
                        this.template_change(guild, reqwest::Method::DELETE, format!("/guilds/{guild}/templates/{code}"), None);
                        if let Some(list) = this.fetched.get_mut(&format!("{TEMPLATES_PREFIX}{guild}")).and_then(|body| body.as_array_mut()) {
                            list.clear();
                        }
                        if let Some(state) = this.server_settings.as_ref().and_then(|open| open.template.as_ref()) {
                            state.title.update(cx, |editor, _| editor.set_text(""));
                            state.description.update(cx, |editor, _| editor.set_text(""));
                        }
                        close(this);
                    }),
                ],
                close: Box::new(move |this, _, _| close(this)),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_need_two_characters() {
        assert!(!title_ok(" a "));
        assert!(title_ok("ab"));
        assert_eq!(link_of("x1"), "https://discord.new/x1");
    }
}
