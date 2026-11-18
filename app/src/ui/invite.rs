//! Invite to Server (server menu and the invite icons), measured in `design/dom/modals/invite-darker-cozy.json`:
//! the title and "Recipients will land in #channel", a friend search, rows with Invite, and the link footer. Opening
//! it creates the invite as `TRAFFIC/invite-open.json` records: `POST /channels/{id}/invites {validate, max_age:
//! 2592000, max_uses: 0, target_type: null, temporary: false, flags: 0}`. Invite on a row sends the link in that
//! person's DM; Copy puts it on the clipboard.

use super::{Root, avatar_source, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::{Id, User};
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, FontWeight, IntoElement, SharedString,
    Subscription, div, img, prelude::*, px,
};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) const CREATE_KEY: &str = "invite-create";
/// Thirty days, the default the recording sends.
const MAX_AGE: u64 = 2_592_000;
/// `TOO_MANY_INVITES` (module 652215).
const TOO_MANY_INVITES: u64 = 30016;
/// `INVITES_HELP` (module 652215).
const INVITES_HELP: &str = "https://support.discord.com/hc/en-us/articles/208866998-Invites-101";

pub(crate) struct Invite {
    pub channel: Id,
    pub search: Entity<Editor>,
    _events: Subscription,
    pub query: String,
    pub code: Option<String>,
    /// People this dialog already sent the link to.
    pub sent: HashSet<Id>,
    /// The invite could not be made (`networkError` of module 312556).
    pub error: Option<InviteError>,
}

// GAP: Bypass Join Applications and the custom invite link line of the invite settings (module 312556) are not drawn.
/// Why the invite could not be made, as `v` of module 312556 words it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum InviteError {
    /// `TOO_MANY_INVITES` (30016).
    TooMany,
    Message(String),
}

/// `v` of module 312556: the API's `TOO_MANY_INVITES`, its message, or `eAn6z2`.
pub(super) fn invite_error(report: &Value) -> Option<InviteError> {
    // An inert run's refusal never reached Discord: nothing to say about it.
    if report["ok"] == true || report["refused"] == true {
        return None;
    }
    let body = &report["body"];
    if body["code"].as_u64() == Some(TOO_MANY_INVITES) {
        return Some(InviteError::TooMany);
    }
    Some(InviteError::Message(
        body["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| dc_intl::t("eAn6z2").to_owned()),
    ))
}

impl Root {
    pub(super) fn open_invite(&mut self, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.or_else(|| {
            self.current_guild()
                .and_then(|guild| {
                    guild
                        .channels
                        .iter()
                        .find(|channel| channel.kind == dc_state::ChannelKind::Text)
                })
                .map(|channel| channel.id)
        }) else {
            return;
        };
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search for friends".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            if let Some(invite) = this.invite.as_mut() {
                match event {
                    EditorEvent::Edited => invite.query = editor.read(cx).text().to_owned(),
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text))
                    }
                    EditorEvent::Cancel => this.invite = None,
                }
            }
            cx.notify();
        });
        let previous = self.invite_codes.get(&channel).cloned();
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/invites"),
            body: Some(json!({
                "validate": previous,
                "max_age": MAX_AGE,
                "max_uses": 0,
                "target_type": null,
                "temporary": false,
                "flags": 0,
            })),
        });
        self.invite = Some(Invite {
            channel,
            search,
            _events: events,
            query: String::new(),
            code: previous,
            sent: HashSet::new(),
            error: None,
        });
        cx.notify();
    }

    pub(super) fn invite_created(&mut self, report: &Value, cx: &mut Context<Self>) {
        if let Some(error) = invite_error(report)
            && let Some(invite) = self.invite.as_mut()
        {
            invite.error = Some(error);
            cx.notify();
            return;
        }
        let Some(code) = report["body"]["code"].as_str().map(str::to_owned) else {
            return;
        };
        if let Some(invite) = self.invite.as_mut() {
            self.invite_codes.insert(invite.channel, code.clone());
            invite.code = Some(code);
        } else if let Some(channel) = self.group_dm.as_ref().and_then(|picker| picker.channel) {
            // "Or, send an invite link" of the group picker.
            self.invite_codes.insert(channel, code);
        }
        cx.notify();
    }

    /// Invite on a row: open (or create) the DM and send the link.
    fn send_invite(&mut self, user: Id, cx: &mut Context<Self>) {
        let Some(invite) = self.invite.as_mut() else {
            return;
        };
        let Some(code) = invite.code.clone() else {
            return;
        };
        invite.sent.insert(user);
        self.send_link_to_dm(user, format!("https://discord.gg/{code}"));
        cx.notify();
    }

    /// A link to someone: in their DM, opened first when there is none.
    pub(super) fn send_link_to_dm(&mut self, user: Id, link: String) {
        match self
            .stores
            .private_channels
            .iter()
            .find(|channel| channel.recipients == [user])
            .map(|channel| channel.id)
        {
            Some(channel) => {
                self.start_local_text(channel, link);
            }
            None => {
                self.pending_invite_link = Some(link);
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{}{user}", super::invite::DM_KEY)),
                    method: reqwest::Method::POST,
                    route: "/users/@me/channels".to_owned(),
                    body: Some(json!({"recipients": [user.to_string()]})),
                });
            }
        }
    }

    pub(super) fn invite_dm_opened(&mut self, report: &Value) {
        let Some(channel) = report["body"]["id"]
            .as_str()
            .and_then(|id| id.parse::<Id>().ok())
        else {
            return;
        };
        self.stores.add_private_channel(&report["body"]);
        if let Some(content) = self.pending_invite_link.take() {
            self.start_local_text(channel, content);
        }
        if std::mem::take(&mut self.open_dm_after_send) {
            self.select_home(Some(channel));
        }
    }

    fn invite_row(&self, user: &User, cx: &mut Context<Self>) -> AnyElement {
        let id = user.id;
        let sent = self
            .invite
            .as_ref()
            .is_some_and(|invite| invite.sent.contains(&id));
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        div()
            .ml(px(-8.))
            .h(px(48.))
            .pl(px(5.))
            .pr(px(8.))
            .py(px(7.))
            .rounded(px(3.))
            .flex()
            .items_center()
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .child(
                div()
                    .ml(px(3.))
                    .size(px(32.))
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full())),
            )
            .child(
                div()
                    .ml(px(10.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("invite-row-{id}")))
                    .h(px(32.))
                    .px(px(11.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--control-secondary-border-default"))
                    .bg(color("--control-secondary-background-default"))
                    .when(!sent, |this| this.cursor_pointer())
                    .when(sent, |this| this.opacity(0.5))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--control-secondary-text-default"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !sent {
                            this.send_invite(id, cx);
                        }
                    }))
                    .child(if sent { "Sent" } else { "Invite" }),
            )
            .into_any_element()
    }

    /// `v` of module 312556 under the link (`errorMessage_COIWNl`, 8px down, body-xs in `--text-feedback-critical`):
    /// too many invites names the server's invite list and the help article to someone who may manage the server, and
    /// only the cause to anyone else; another failure shows its message.
    fn invite_error_line(&self, error: InviteError, cx: &mut Context<Self>) -> AnyElement {
        let line = div()
            .mt(px(8.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(color("--text-feedback-critical"));
        let manage = self.guild.is_some_and(|guild| {
            self.my_permission(guild, None, dc_state::permissions::MANAGE_GUILD)
        });
        match error {
            InviteError::Message(message) => line.child(message).into_any_element(),
            InviteError::TooMany if !manage => line.child(dc_intl::t("/FxH6G")).into_any_element(),
            InviteError::TooMany => {
                // `H/RUY1` with its two hooks.
                let lead = "The maximum number of server invites have been generated. ";
                let list = "View invites for this server.";
                let help = "Learn more";
                let text =
                    format!("{lead}{list} {help} about how server invitations work on Discord!");
                let list_at = lead.len()..lead.len() + list.len();
                let help_at = list_at.end + 1..list_at.end + 1 + help.len();
                let link = gpui::HighlightStyle {
                    color: Some(color("--text-link").into()),
                    ..Default::default()
                };
                let root = cx.entity().downgrade();
                let guild = self.guild;
                line.child(
                    gpui::InteractiveText::new(
                        "invite-error",
                        gpui::StyledText::new(text).with_highlights(vec![
                            (list_at.clone(), link),
                            (help_at.clone(), link),
                        ]),
                    )
                    .on_click(vec![list_at, help_at], move |at, _, cx| {
                        if at == 1 {
                            cx.open_url(INVITES_HELP);
                            return;
                        }
                        let _ = root.update(cx, |this, cx| {
                            this.invite = None;
                            if let Some(guild) = guild {
                                this.open_server_settings_at(
                                    guild,
                                    super::server_settings::Page::Invites,
                                    cx,
                                );
                            }
                            cx.notify();
                        });
                    }),
                )
                .into_any_element()
            }
        }
    }

    pub(super) fn invite_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let invite = self.invite.as_ref()?;
        let guild_name = self
            .current_guild()
            .map(|guild| guild.name.clone())
            .unwrap_or_default();
        let channel_name = self
            .current_guild()
            .and_then(|guild| {
                guild
                    .channels
                    .iter()
                    .find(|channel| channel.id == invite.channel)
            })
            .map(|channel| channel.name.clone())
            .unwrap_or_default();
        let query = invite.query.trim().to_lowercase();
        // Friends, then the people of open DMs, as the official list offers them.
        let mut people: Vec<User> = self.stores.friends().into_iter().cloned().collect();
        for channel in &self.stores.private_channels {
            if let Some(user) = self.stores.dm_recipient(channel)
                && !people.iter().any(|known| known.id == user.id)
            {
                people.push(user.clone());
            }
        }
        people.sort_by_key(|user| user.display_name().to_lowercase());
        people.retain(|user| {
            query.is_empty()
                || user.display_name().to_lowercase().contains(&query)
                || user.username.to_lowercase().contains(&query)
        });
        let rows: Vec<AnyElement> = people
            .iter()
            .map(|user| self.invite_row(user, cx))
            .collect();
        let link = invite
            .code
            .as_ref()
            .map(|code| format!("https://discord.gg/{code}"))
            .unwrap_or_default();
        let copy_link = link.clone();
        let shown_link = link;
        let body = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .pl(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("gif_search", 16., "--icon-default")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pr(px(12.))
                            .child(invite.search.clone()),
                    ),
            )
            .child(
                div()
                    .id("invite-list")
                    .h(px(325.))
                    .mx(px(-24.))
                    .pl(px(24.))
                    .pr(px(8.))
                    .py(px(12.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(
                div()
                    .mx(px(-24.))
                    .h(px(9.))
                    .border_t_1()
                    .border_color(color("--border-subtle")),
            )
            .child(
                div()
                    .pt(px(16.))
                    .child(
                        div()
                            .mb(px(8.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Or, send a server invite link to a friend"),
                    )
                    .child(
                        div()
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .pl(px(10.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(
                                        if self.streamer_hides_invites() && !shown_link.is_empty() {
                                            crate::ui::settings::HIDDEN_INVITE_LINK.to_owned()
                                        } else {
                                            shown_link
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .id("invite-copy")
                                    .mr(px(4.))
                                    .h(px(30.))
                                    .px(px(20.))
                                    .rounded(px(4.))
                                    .bg(color("--background-brand"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--white"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !copy_link.is_empty() {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy_link.clone(),
                                            ));
                                            this.show_app_toast("Copied!", cx);
                                        }
                                    }))
                                    .child("Copy"),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .child(
                                div()
                                    .text_color(color("--text-muted"))
                                    .child("Your invite link expires in 30 days.\u{a0}"),
                            )
                            .child(
                                div()
                                    .text_color(color("--text-link"))
                                    .child("Edit invite link."),
                            ),
                    )
                    .children(
                        invite
                            .error
                            .clone()
                            .map(|error| self.invite_error_line(error, cx)),
                    ),
            )
            .into_any_element();
        Some(modal::render(
            modal::Spec {
                id: "invite-modal",
                title: format!("Invite friends to {guild_name}").into(),
                subtitle: Some(
                    div()
                        .flex()
                        .items_center()
                        .child("Recipients will land in\u{a0}")
                        .child(div().ml(px(2.)).mr(px(2.)).child(icon(
                            "text_channel",
                            16.,
                            "--text-subtle",
                        )))
                        .child(channel_name)
                        .into_any_element(),
                ),
                body: Some(body),
                buttons: Vec::new(),
                close: Box::new(|this, _, _| this.invite = None),
            },
            cx,
        ))
    }
}

pub(super) const DM_KEY: &str = "invite-dm:";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_invite_says_why_as_the_modal_does() {
        assert_eq!(
            invite_error(&json!({"ok": true, "body": {"code": "abc"}})),
            None
        );
        assert_eq!(
            invite_error(&json!({"ok": false, "status": 0, "body": null, "refused": true})),
            None
        );
        assert_eq!(
            invite_error(
                &json!({"ok": false, "status": 400, "body": {"code": 30016, "message": "Maximum"}})
            ),
            Some(InviteError::TooMany)
        );
        assert_eq!(
            invite_error(
                &json!({"ok": false, "status": 403, "body": {"code": 50013, "message": "Missing Permissions"}})
            ),
            Some(InviteError::Message("Missing Permissions".into()))
        );
        assert_eq!(
            invite_error(&json!({"ok": false, "status": 0, "body": null})),
            Some(InviteError::Message(
                "Seems like something's gone wrong. Sorry about that.".into()
            ))
        );
    }
}
