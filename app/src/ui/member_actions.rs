//! The moderation dialogs of a member's menu: Kick (module 547166), Ban (333179) and Timeout (797472), with the requests of module 778858 (`kickUser`, `banUser`, `setCommunicationDisabledUntil`,
//! `setMemberFlags`, `setServerMute`, `setServerDeaf`, `setChannel`). The strings come from the en-US table of the pin.

use super::icon;
use super::{Root, color, editor_style, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Subscription,
    Window, div, prelude::*, px,
};
use serde_json::{Value, json};

const KEY: &str = "member-action:";
/// Invite to Server: the answer to the invite that was just made for a person.
const INVITE_KEY: &str = "ctx-invite:";
/// Capture only: the two roles the harness makes in the fixture server (`fixture-role:coloured|plain`).
const FIXTURE_ROLE_KEY: &str = "fixture-role:";
/// `maxLength` of the reason fields (modules 547166, 333179, 797472).
const REASON_LIMIT: usize = 512;

/// Module 797472 `R`: the durations of Timeout, in seconds.
pub(super) const TIMEOUTS: [(i64, &str); 6] = [
    (60, "60 secs"),
    (300, "5 mins"),
    (600, "10 mins"),
    (3600, "1 hour"),
    (86400, "1 day"),
    (604800, "1 week"),
];

/// Module 333179 `j()`: how much of a banned person's history goes, in seconds.
pub(super) const DELETE_HISTORY: [(i64, &str); 7] = [
    (0, "Don't Delete Any"),
    (3600, "Previous Hour"),
    (6 * 3600, "Previous 6 Hours"),
    (12 * 3600, "Previous 12 Hours"),
    (86400, "Previous 24 Hours"),
    (3 * 86400, "Previous 3 Days"),
    (7 * 86400, "Previous 7 Days"),
];

/// Module 333179 `q`: the canned ban reasons; the last one asks for text.
pub(super) const BAN_REASONS: [&str; 4] = [
    "Suspicious or spam account",
    "Compromised or hacked account",
    "Breaking server rules",
    "Other",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Kick,
    Ban,
    Timeout,
    /// Module 21754: lifts a timeout after a confirmation.
    RemoveTimeout,
    /// Module 752118: another member's server nickname.
    Nickname,
}

pub(super) struct Dialog {
    kind: Kind,
    guild: Id,
    user: Id,
    /// The name the titles use (`getName`: the display name).
    name: String,
    input: Entity<Editor>,
    _events: Subscription,
    /// Ban: the chosen reason; Timeout: the chosen duration.
    option: Option<usize>,
    /// Ban: the history to delete (default Previous Hour, module 333179 `y`).
    delete: usize,
    /// Ban: the reason was missing when Ban was pressed (`IrYX1y`).
    missing_reason: bool,
    pending: bool,
    key: String,
    primary: FocusHandle,
    /// Nickname: the nickname the member has now (empty for none), which Save compares with.
    initial: String,
    /// RemoveTimeout: the time left, as the subtitle names it.
    remaining: String,
    /// Ban: the Delete Message History list is open.
    delete_open: bool,
}

/// `kickUser`: `DELETE /guilds/:g/members/:u` with the reason in the query.
pub(super) fn kick_request(guild: Id, user: Id, reason: &str, key: String) -> Request {
    let route = format!("/guilds/{guild}/members/{user}");
    Request::Rest {
        key: Some(key),
        method: reqwest::Method::DELETE,
        route: if reason.is_empty() {
            route
        } else {
            format!("{route}?reason={}", dc_http::percent_encode(reason))
        },
        body: None,
    }
}

/// `banUser`: `PUT /guilds/:g/bans/:u` with `delete_message_seconds`, the reason as the audit header.
pub(super) fn ban_request(guild: Id, user: Id, seconds: i64, reason: &str, key: String) -> Request {
    Request::Rest {
        key: Some(key),
        method: reqwest::Method::PUT,
        route: dc_http::with_audit_reason(&format!("/guilds/{guild}/bans/{user}"), reason),
        body: Some(json!({"delete_message_seconds": seconds})),
    }
}

/// Module 856768 / `778858.A.startBulkBan`: `POST /guilds/:g/bulk-ban/v2` with `user_ids`,
/// `delete_message_seconds`, and `X-Audit-Log-Reason` header.
pub(super) fn bulk_ban_request(
    guild: Id,
    user_ids: &[Id],
    seconds: i64,
    reason: &str,
    key: String,
) -> Request {
    let ids: Vec<String> = user_ids.iter().map(Id::to_string).collect();
    Request::Rest {
        key: Some(key),
        method: reqwest::Method::POST,
        route: dc_http::with_audit_reason(&format!("/guilds/{guild}/bulk-ban/v2"), reason),
        body: Some(json!({
            "user_ids": ids,
            "delete_message_seconds": seconds,
        })),
    }
}

/// `setCommunicationDisabledUntil`: `PATCH /guilds/:g/members/:u`; `until` is an ISO time, or `None` to lift it.
pub(super) fn timeout_request(
    guild: Id,
    user: Id,
    until: Option<String>,
    reason: &str,
    key: String,
) -> Request {
    Request::Rest {
        key: Some(key),
        method: reqwest::Method::PATCH,
        route: dc_http::with_audit_reason(&format!("/guilds/{guild}/members/{user}"), reason),
        body: Some(json!({"communication_disabled_until": until})),
    }
}

/// `setServerMute` / `setServerDeaf` / `setChannel` / `setMemberFlags`: one field of `PATCH /guilds/:g/members/:u`.
/// A time left as the countdown of module 21754 shows it: the two largest units.
pub(super) fn time_left_label(seconds: i64) -> String {
    let parts = [
        (seconds / 86400, "d"),
        (seconds % 86400 / 3600, "h"),
        (seconds % 3600 / 60, "m"),
        (seconds % 60, "s"),
    ];
    let shown: Vec<String> = parts
        .iter()
        .filter(|(count, _)| *count > 0)
        .take(2)
        .map(|(count, unit)| format!("{count}{unit}"))
        .collect();
    if shown.is_empty() {
        "0s".to_owned()
    } else {
        shown.join(" ")
    }
}

/// The nickname field's length (module 752118 `maxLength: 32`).
const NICK_LIMIT: usize = 32;

/// `PATCH /guilds/:g/members/:u` with one field, answered under `key`.
pub(super) fn member_patch_keyed(
    guild: Id,
    user: Id,
    field: &str,
    value: Value,
    key: String,
) -> Request {
    match member_patch(guild, user, field, value) {
        Request::Rest {
            method,
            route,
            body,
            ..
        } => Request::Rest {
            key: Some(key),
            method,
            route,
            body,
        },
        other => other,
    }
}

pub(super) fn member_patch(guild: Id, user: Id, field: &str, value: Value) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild}/members/{user}"),
        body: Some(json!({ field: value })),
    }
}

impl Root {
    /// Harness, the fixture server only: makes one coloured and one plain role, then gives the coloured one to this
    /// account itself (never to anyone else), so the member menu shows its Roles row.
    #[cfg(feature = "capture")]
    pub(crate) fn script_fixture_roles(&mut self) -> String {
        const FIXTURE: Id = 1_568_692_564_637_524_109;
        let names = [
            ("coloured", "gravecord red", 0x00e9_1e63_u32),
            ("plain", "gravecord plain", 0),
        ];
        for (slot, name, colour) in names {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{FIXTURE_ROLE_KEY}{slot}")),
                method: reqwest::Method::POST,
                route: format!("/guilds/{FIXTURE}/roles"),
                body: Some(json!({ "name": name, "color": colour, "permissions": "0" })),
            });
        }
        "fixture roles: requested".to_owned()
    }

    /// Harness: opens a moderation dialog for the first other known person in the open server, and stops there (the
    /// confirm button is never pressed by a script).
    #[cfg(feature = "capture")]
    pub(crate) fn script_member_action(
        &mut self,
        kind: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        // The fixture server when none is open (the only server a script may act in).
        let guild = self.guild.unwrap_or(1_568_692_564_637_524_109);
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let Some(user) = self
            .stores
            .users
            .keys()
            .copied()
            .filter(|id| Some(*id) != me)
            .min()
        else {
            return "no other person known".to_owned();
        };
        match kind {
            "nickname" => self.open_nickname_dialog(guild, user, window, cx),
            "remove-timeout" => self.open_remove_timeout(guild, user, window, cx),
            "kick" => self.open_member_action(Kind::Kick, guild, user, window, cx),
            "ban" => self.open_member_action(Kind::Ban, guild, user, window, cx),
            "ban-delete-open" => {
                if let Some(d) = self.member_action.as_mut() {
                    d.delete_open = true;
                    cx.notify();
                } else {
                    self.open_member_action(Kind::Ban, guild, user, window, cx);
                    if let Some(d) = self.member_action.as_mut() {
                        d.delete_open = true;
                    }
                    cx.notify();
                }
            }
            "ban-delete-7d" => {
                if let Some(d) = self.member_action.as_mut() {
                    d.delete = DELETE_HISTORY.len() - 1;
                    d.delete_open = false;
                    cx.notify();
                }
            }
            _ => self.open_member_action(Kind::Timeout, guild, user, window, cx),
        }
        format!("dialog open for {user}")
    }

    pub(super) fn open_member_action(
        &mut self,
        kind: Kind,
        guild: Id,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
        else {
            return;
        };
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            let mut input = Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_reports_all_edits(true);
            input.set_max_utf16(REASON_LIMIT);
            if kind != Kind::Nickname {
                input.set_enter_inserts_newline(true);
            }
            if kind == Kind::Timeout {
                input.set_placeholder(
                    "Enter a reason. This will only be visible in the Audit Log and will not be shown to the member.".to_owned(),
                );
            }
            input
        });
        let events = cx.subscribe(&input, |this, editor, event: &EditorEvent, cx| {
            if !this
                .member_action
                .as_ref()
                .is_some_and(|dialog| dialog.input.entity_id() == editor.entity_id())
            {
                return;
            }
            match event {
                EditorEvent::Edited => {
                    if let Some(dialog) = this.member_action.as_mut() {
                        dialog.missing_reason = false;
                    }
                }
                EditorEvent::Submit(_) => {}
                EditorEvent::Cancel => this.member_action = None,
            }
            cx.notify();
        });
        let primary = cx.focus_handle();
        let key = format!("{KEY}{}", input.entity_id());
        self.member_action = Some(Dialog {
            kind,
            guild,
            user,
            name,
            input,
            _events: events,
            option: (kind == Kind::Timeout).then_some(0),
            delete: 1,
            missing_reason: false,
            pending: false,
            key,
            primary: primary.clone(),
            initial: String::new(),
            remaining: String::new(),
            delete_open: false,
        });
        window.focus(&primary, cx);
        cx.notify();
    }

    /// Invite to Server: an invite to the server's first text channel is made, and its link goes to the DM.
    pub(super) fn send_server_invite(&mut self, guild: Id, user: Id) {
        let Some((_, _, channel)) = self
            .invite_servers(Some(user))
            .into_iter()
            .find(|(id, _, _)| *id == guild)
        else {
            return;
        };
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{INVITE_KEY}{user}")),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/invites"),
            body: Some(json!({
                "validate": null,
                "max_age": 2_592_000,
                "max_uses": 0,
                "target_type": null,
                "temporary": false,
                "flags": 0,
            })),
        });
    }

    /// "Change Nickname" of another member (module 346247 then 752118): the field starts with their nickname.
    pub(super) fn open_nickname_dialog(
        &mut self,
        guild: Id,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_member_action(Kind::Nickname, guild, user, window, cx);
        let nick = self
            .stores
            .members
            .get(guild, user)
            .and_then(|member| member.nick.clone())
            .unwrap_or_default();
        let placeholder = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        if let Some(dialog) = self.member_action.as_mut() {
            dialog.initial = nick.clone();
            dialog.input.update(cx, |input, _| {
                input.set_max_utf16(NICK_LIMIT);
                input.set_text(&nick);
                input.set_placeholder(placeholder);
            });
        }
    }

    /// "Remove Timeout From <name>" (module 21754): the confirmation names the time left.
    pub(super) fn open_remove_timeout(
        &mut self,
        guild: Id,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_member_action(Kind::RemoveTimeout, guild, user, window, cx);
        let until = self
            .stores
            .members
            .get(guild, user)
            .and_then(|member| member.timeout_until.clone());
        let left = until
            .and_then(|until| chrono::DateTime::parse_from_rfc3339(&until).ok())
            .map(|until| (until.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds())
            .unwrap_or(0)
            .max(0);
        if let Some(dialog) = self.member_action.as_mut() {
            dialog.remaining = time_left_label(left);
        }
    }

    /// The press of the dialog's primary button.
    fn run_member_action(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.member_action.as_mut().filter(|dialog| !dialog.pending) else {
            return;
        };
        let reason = dialog.input.read(cx).text();
        let request = match dialog.kind {
            Kind::Kick => kick_request(dialog.guild, dialog.user, reason, dialog.key.clone()),
            Kind::Ban => {
                let canned = dialog.option.filter(|index| *index + 1 < BAN_REASONS.len());
                let text = match (dialog.option, canned) {
                    (_, Some(index)) => BAN_REASONS[index].to_owned(),
                    _ => reason.to_string(),
                };
                // `IrYX1y`: a ban needs a reason, one of the canned ones or the text under Other.
                if dialog.option.is_none() || text.trim().is_empty() {
                    dialog.missing_reason = true;
                    cx.notify();
                    return;
                }
                ban_request(
                    dialog.guild,
                    dialog.user,
                    DELETE_HISTORY[dialog.delete].0,
                    &text,
                    dialog.key.clone(),
                )
            }
            Kind::RemoveTimeout => {
                timeout_request(dialog.guild, dialog.user, None, "", dialog.key.clone())
            }
            Kind::Nickname => {
                let nick = reason.to_string();
                if nick == dialog.initial {
                    // Module 752118: nothing changed, so the dialog just closes.
                    self.member_action = None;
                    cx.notify();
                    return;
                }
                member_patch_keyed(
                    dialog.guild,
                    dialog.user,
                    "nick",
                    json!(nick),
                    dialog.key.clone(),
                )
            }
            Kind::Timeout => {
                let seconds = TIMEOUTS[dialog.option.unwrap_or(0)].0;
                let until = (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                timeout_request(
                    dialog.guild,
                    dialog.user,
                    Some(until),
                    reason,
                    dialog.key.clone(),
                )
            }
        };
        if self.requests.send(request).is_ok() {
            dialog.pending = true;
        }
        cx.notify();
    }

    /// The answer of the dialog's request: a toast, and the dialog closes (`O9C3Nt`, `/Mmbfv`, `epyCuh`).
    pub(super) fn member_action_fetched(
        &mut self,
        key: &str,
        body: &Value,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(user) = key
            .strip_prefix(INVITE_KEY)
            .and_then(|user| user.parse::<Id>().ok())
        {
            // Module 777933: the made invite goes to the person's DM as a link.
            if let Some(code) = body["body"]["code"].as_str() {
                self.send_link_to_dm(user, format!("https://discord.gg/{code}"));
            } else {
                self.show_app_toast(
                    "Something went wrong. Please try again later or contact support.",
                    cx,
                );
            }
            return true;
        }
        if key.starts_with(FIXTURE_ROLE_KEY) {
            let role = body["body"]["id"]
                .as_str()
                .and_then(|id| id.parse::<Id>().ok());
            let me = self.stores.current_user.as_ref().map(|me| me.id);
            if let (true, Some(role), Some(me)) = (key.ends_with("coloured"), role, me) {
                let guild: Id = 1_568_692_564_637_524_109;
                let _ = self.requests.send(Request::Rest {
                    key: Some(format!("{FIXTURE_ROLE_KEY}given")),
                    method: reqwest::Method::PUT,
                    route: format!("/guilds/{guild}/members/{me}/roles/{role}"),
                    body: None,
                });
            }
            tracing::info!(target: "harness", "fixture role: {key} answered {}", body["ok"]);
            return true;
        }
        if !key.starts_with(KEY) {
            return false;
        }
        let Some(dialog) = self
            .member_action
            .as_mut()
            .filter(|dialog| dialog.key == key)
        else {
            return true;
        };
        if body["ok"] == true {
            let kind = dialog.kind;
            let name = dialog.name.clone();
            self.member_action = None;
            match kind {
                Kind::Timeout => self.show_success_toast(format!("{name} Timed Out"), cx),
                Kind::Kick => {
                    self.show_success_toast("User was kicked from the server.".into(), cx)
                }
                Kind::Ban => self.show_success_toast("User was banned from the server.".into(), cx),
                Kind::RemoveTimeout => self.show_success_toast("Timeout Removed".into(), cx),
                Kind::Nickname => {}
            }
        } else {
            dialog.pending = false;
            self.show_app_toast(
                "Something went wrong. Please try again later or contact support.",
                cx,
            );
        }
        cx.notify();
        true
    }

    /// The reason field of the open dialog, for the harness to find the focused field.
    #[cfg(feature = "capture")]
    pub(super) fn member_action_input(&self) -> Option<Entity<Editor>> {
        self.member_action.as_ref().map(|open| open.input.clone())
    }

    pub(super) fn member_action_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.member_action.as_ref()?;
        let name = &dialog.name;
        let (title, subtitle, confirm, tone) = match dialog.kind {
            Kind::Kick => (
                // Module 547166: the title carries the username, the question the display name.
                format!(
                    "Kick {} from Server",
                    self.stores
                        .users
                        .get(&dialog.user)
                        .map_or(name.as_str(), |known| known.username.as_str())
                ),
                format!(
                    "Are you sure you want to kick @{name} from the server? They will be able to rejoin again with a new invite."
                ),
                "Kick",
                modal::Tone::Critical,
            ),
            Kind::Ban => (
                format!("Ban @{name}?"),
                String::new(),
                "Ban",
                modal::Tone::Critical,
            ),
            Kind::RemoveTimeout => (
                "Remove Timeout".to_owned(),
                format!("{name} has {} remaining in timeout.", dialog.remaining),
                "Remove Timeout",
                modal::Tone::Critical,
            ),
            Kind::Nickname => (
                "Change Nickname".to_owned(),
                String::new(),
                "Save",
                modal::Tone::Primary,
            ),
            Kind::Timeout => (
                format!("Timeout {name}"),
                "Members who are in timeout are temporarily not allowed to chat or react in text channels. They are also not allowed to connect to voice or Stage channels. ".to_owned(),
                "Timeout",
                modal::Tone::Primary,
            ),
        };
        let label = |text: &'static str| {
            div()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let heading = |text: &'static str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let focused = dialog.input.read(cx).focus_handle(cx).is_focused(window);
        let input = dialog.input.clone();
        let line = dialog.kind == Kind::Nickname;
        let height = match dialog.kind {
            Kind::Nickname => 40.,
            Kind::Ban => 131.,
            // `rows: 2` of module 547166 (frame member-menu/dialog-kick: a 68px box).
            Kind::Kick => 68.,
            _ => 110.,
        };
        let field = div()
            .parity_with_border("member-action-reason", if focused { 2. } else { 1. })
            .id("member-action-reason")
            .h(px(height))
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
            .when(line, |d| {
                d.px(px(if focused { 10. } else { 11. }))
                    .flex()
                    .items_center()
            })
            .when(!line, |d| {
                d.pt(px(if focused { 10. } else { 11. }))
                    .px(px(if focused { 8. } else { 9. }))
            })
            .on_click(cx.listener(move |_, _, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx);
                cx.notify();
            }))
            .child(dialog.input.clone());
        // The segmented duration control: contiguous 32px buttons, the picked one filled brand.
        let segments = |labels: Vec<&'static str>, selected: Option<usize>| {
            let last = labels.len() - 1;
            div()
                .flex()
                .children(labels.into_iter().enumerate().map(|(index, text)| {
                    let on = selected == Some(index);
                    div()
                        .id(gpui::SharedString::from(format!(
                            "timeout-duration-{index}"
                        )))
                        .h(px(32.))
                        .flex_none()
                        .pl(px(if index == 0 { 15. } else { 14. }))
                        .pr(px(if index == 0 { 16. } else { 15. }))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .when(index == 0, |d| d.rounded_tl(px(8.)).rounded_bl(px(8.)))
                        .when(index == last, |d| d.rounded_tr(px(8.)).rounded_br(px(8.)))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .when(on, |d| d.bg(color("--brand-500")))
                        .text_color(color(if on { "--white" } else { "--text-default" }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(dialog) = this.member_action.as_mut() {
                                dialog.option = Some(index);
                            }
                            cx.notify();
                        }))
                        .child(text)
                }))
        };
        // The ban reasons are radio rows: a 20px ring at the left, the label 32px in.
        let radios = |labels: Vec<&'static str>, selected: Option<usize>| {
            div().flex().flex_col().gap(px(16.)).pt(px(8.)).children(
                labels.into_iter().enumerate().map(|(index, text)| {
                    div()
                        .id(gpui::SharedString::from(format!("ban-reason-{index}")))
                        .relative()
                        .h(px(20.))
                        .pl(px(32.))
                        .cursor_pointer()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(
                            div()
                                .absolute()
                                .left(px(0.))
                                .top(px(0.))
                                .child(super::create_channel::radio(selected == Some(index))),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(dialog) = this.member_action.as_mut() {
                                dialog.option = Some(index);
                                dialog.missing_reason = false;
                            }
                            cx.notify();
                        }))
                        .child(text)
                }),
            )
        };
        // The Delete Message History select: a 40px box with the chevron at the right; its popout list is rendered
        // in `member_action_delete_select_layer` (`deferred` priority 3, viewport-flipping) so `modal::render`'s
        // `.overflow_y_scroll()` body never clips it and the footer never covers it.
        let history = {
            let open = dialog.delete_open;
            div()
                .id("ban-delete")
                .relative()
                .child(self.anchor_probe("ban-delete"))
                .h(px(40.))
                .w_full()
                .rounded(px(8.))
                .border_1()
                .border_color(if open {
                    color("--input-border-active")
                } else {
                    color("--input-border-default")
                })
                .bg(color("--input-background-default"))
                .flex()
                .items_center()
                .cursor_pointer()
                .pl(px(12.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-strong"))
                .child(div().flex_1().child(DELETE_HISTORY[dialog.delete].1))
                .child(
                    // `.chevronButton_vWC4bk`: 36px wide with `padding-inline: 0 var(--space-4)`.
                    div().w(px(36.)).pr(px(4.)).flex().justify_center().child(
                        gpui::svg()
                            .path("icons/combo_chevron.svg")
                            .size(px(20.))
                            .text_color(color("--text-strong"))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                if open { 0.5 } else { 0. },
                            ))),
                    ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(dialog) = this.member_action.as_mut() {
                        dialog.delete_open = !dialog.delete_open;
                    }
                    cx.notify();
                }))
        };
        let body: Option<AnyElement> = match dialog.kind {
            Kind::RemoveTimeout => Some(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--interactive-text-default"))
                    .child("Remove it now to let them post and react to messages, and join voice and stage channels. Learn More")
                    .into_any_element(),
            ),
            Kind::Nickname => {
                let has_nick = !dialog.initial.is_empty();
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        // `dq3At2`: a warning above the field for anyone else's nickname.
                        .child(
                            div()
                                .rounded(px(8.))
                                .p(px(10.))
                                .mb(px(12.))
                                .bg(color("--background-feedback-mod-warning"))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-default"))
                                .child("Nicknames are visible to everyone on this server. Do not change them unless you are enforcing a naming system or clearing a bad nickname."),
                        )
                        .child(label("Nickname"))
                        .child(field)
                        .when(has_nick, |d| {
                            let input = dialog.input.clone();
                            d.child(
                                div()
                                    .id("member-action-reset-nick")
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-brand"))
                                    .cursor_pointer()
                                    .child("Reset Nickname")
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        input.update(cx, |input, _| input.set_text(""));
                                        window.focus(&input.read(cx).focus_handle(cx), cx);
                                        cx.notify();
                                    })),
                            )
                        })
                        .into_any_element(),
                )
            }
            Kind::Kick => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    // The wrapper `T.Y` of module 547166 adds 16px under the field (body 112 high in the frame).
                    .pb(px(16.))
                    .child(label("Reason for Kick"))
                    .child(field)
                    .into_any_element(),
            ),
            Kind::Ban => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(label("Reason for Ban"))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .text_color(color("--text-feedback-critical"))
                                            .child("*"),
                                    ),
                            )
                            .when(dialog.missing_reason, |d| {
                                d.child(
                                    div()
                                        .text_size(px(14.))
                                        .text_color(color("--text-feedback-critical"))
                                        .child("Ban reason must be provided"),
                                )
                            })
                            .child(radios(BAN_REASONS.to_vec(), dialog.option))
                            .map(|d| {
                                if dialog.option == Some(BAN_REASONS.len() - 1) {
                                    d.child(field)
                                } else {
                                    d.pb(px(12.))
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(label("Delete Message History"))
                            .child(history),
                    )
                    .into_any_element(),
            ),
            Kind::Timeout => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(heading("Duration"))
                            .child(segments(
                                TIMEOUTS.iter().map(|entry| entry.1).collect(),
                                dialog.option,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(heading("Reason"))
                            .child(field),
                    )
                    .into_any_element(),
            ),
        };
        let mut primary = modal::button(
            "member-action-confirm",
            confirm,
            if dialog.pending {
                modal::Tone::PrimaryDisabled
            } else {
                tone
            },
            |this, _, cx| this.run_member_action(cx),
        );
        primary.focus = Some(dialog.primary.clone());
        Some(modal::render(
            modal::Spec {
                id: "member-action-modal",
                title: title.into(),
                subtitle: (!subtitle.is_empty()).then(|| {
                    if dialog.kind == Kind::Timeout {
                        let start = subtitle.len();
                        let text = format!("{subtitle}Learn more.");
                        let range = start..text.len();
                        gpui::InteractiveText::new(
                            "member-action-learn-more",
                            gpui::StyledText::new(text).with_highlights(vec![(
                                range.clone(),
                                gpui::HighlightStyle {
                                    color: Some(color("--text-link").into()),
                                    ..Default::default()
                                },
                            )]),
                        )
                        .on_click(vec![range], |_, _, cx| {
                            cx.open_url(
                                "https://support.discord.com/hc/en-us/articles/4413305239191",
                            )
                        })
                        .into_any_element()
                    } else {
                        div().child(subtitle).into_any_element()
                    }
                }),
                body,
                buttons: vec![
                    modal::button(
                        "member-action-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.member_action = None,
                    ),
                    primary,
                ],
                close: Box::new(|this, _, _| this.member_action = None),
            },
            cx,
        ))
    }

    /// Closes only the Ban modal's open Delete Message History select dropdown (for Escape handling).
    pub(super) fn close_member_action_delete_select(&mut self) -> bool {
        if let Some(dialog) = self.member_action.as_mut()
            && dialog.delete_open
        {
            dialog.delete_open = false;
            return true;
        }
        false
    }

    /// Top-level deferred popout for the Ban modal's "Delete Message History" select (module 333179):
    /// anchored to `ban-delete`, viewport-flipping upward when near the bottom edge so every option
    /// is visible and clickable above the modal scrim/footer.
    pub(super) fn member_action_delete_select_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.member_action.as_ref()?;
        if dialog.kind != Kind::Ban || !dialog.delete_open {
            return None;
        }
        let field = self
            .tip_anchors
            .borrow()
            .get("ban-delete")
            .copied()?
            .dilate(px(1.));
        let selected = dialog.delete;
        let ideal_height = (DELETE_HISTORY.len() as f32) * 40. + 10.;
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let open_up = space_below < ideal_height && space_above > space_below;
        let max_avail = if open_up { space_above } else { space_below }.max(128.);
        let menu_height = ideal_height.min(max_avail);
        let top = if open_up {
            (field.top() - px(4.) - px(menu_height)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        let rows = DELETE_HISTORY.iter().enumerate().map(|(index, entry)| {
            let active = index == selected;
            div()
                .id(gpui::SharedString::from(format!("ban-delete-{index}")))
                .flex_none()
                .h(px(40.))
                .pl(px(8.))
                .pr(px(4.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .when(active, |row| row.bg(color("--background-mod-subtle")))
                .hover(|style| style.bg(color("--background-mod-normal")))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-strong"))
                .child(div().flex_1().child(entry.1))
                .when(active, |row| {
                    row.child(
                        div()
                            .px(px(4.))
                            .child(icon("select_check", 16., "--text-brand")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(dialog) = this.member_action.as_mut() {
                        dialog.delete = index;
                        dialog.delete_open = false;
                    }
                    cx.notify();
                }))
        });
        Some(
            gpui::deferred(
                div()
                    .id("ban-delete-list")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .h(px(menu_height))
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .overflow_y_scroll()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(dialog) = this.member_action.as_mut()
                            {
                                dialog.delete_open = false;
                                cx.notify();
                            }
                        },
                    ))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest(request: Request) -> (reqwest::Method, String, Option<Value>) {
        match request {
            Request::Rest {
                method,
                route,
                body,
                ..
            } => (method, route, body),
            _ => panic!("not a REST request"),
        }
    }

    #[test]
    fn kick_is_a_delete_with_the_reason_in_the_query() {
        let (method, route, body) = rest(kick_request(9, 7, "rule 3 & more", "k".into()));
        assert_eq!(method, reqwest::Method::DELETE);
        assert_eq!(route, "/guilds/9/members/7?reason=rule%203%20%26%20more");
        assert_eq!(body, None);
        assert_eq!(
            rest(kick_request(9, 7, "", "k".into())).1,
            "/guilds/9/members/7"
        );
    }

    #[test]
    fn ban_is_a_put_with_the_history_and_the_audit_reason() {
        let (method, route, body) =
            rest(ban_request(9, 7, 3600, "Breaking server rules", "k".into()));
        assert_eq!(method, reqwest::Method::PUT);
        assert_eq!(
            route,
            "/guilds/9/bans/7?__audit_reason=Breaking%20server%20rules"
        );
        assert_eq!(body, Some(json!({"delete_message_seconds": 3600})));
    }

    #[test]
    fn timeout_patches_the_until_time_and_null_lifts_it() {
        let (method, route, body) = rest(timeout_request(
            9,
            7,
            Some("2026-11-11T12:00:00.000Z".into()),
            "",
            "k".into(),
        ));
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/guilds/9/members/7");
        assert_eq!(
            body,
            Some(json!({"communication_disabled_until": "2026-11-11T12:00:00.000Z"}))
        );
        let (_, _, lifted) = rest(timeout_request(9, 7, None, "", "k".into()));
        assert_eq!(lifted, Some(json!({"communication_disabled_until": null})));
    }

    #[test]
    fn member_fields_go_one_at_a_time() {
        let (method, route, body) = rest(member_patch(9, 7, "mute", json!(true)));
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/guilds/9/members/7");
        assert_eq!(body, Some(json!({"mute": true})));
        let (_, _, body) = rest(member_patch(9, 7, "channel_id", Value::Null));
        assert_eq!(body, Some(json!({"channel_id": null})));
    }

    #[test]
    fn the_durations_and_history_match_the_modules() {
        assert_eq!(
            TIMEOUTS.map(|entry| entry.0),
            [60, 300, 600, 3600, 86400, 604800]
        );
        assert_eq!(DELETE_HISTORY[1].0, 3600);
        assert_eq!(DELETE_HISTORY[6].0, 604800);
    }

    #[test]
    fn nickname_and_roles_are_member_patches() {
        let (method, route, body) =
            rest(member_patch_keyed(9, 7, "nick", json!("Ada"), "k".into()));
        assert_eq!(method, reqwest::Method::PATCH);
        assert_eq!(route, "/guilds/9/members/7");
        assert_eq!(body, Some(json!({"nick": "Ada"})));
        let (_, _, body) = rest(member_patch(9, 7, "roles", json!(["1", "2"])));
        assert_eq!(body, Some(json!({"roles": ["1", "2"]})));
        let (_, _, body) = rest(member_patch(9, 7, "channel_id", Value::Null));
        assert_eq!(body, Some(json!({"channel_id": null})));
    }

    #[test]
    fn remove_timeout_sends_null_and_the_time_left_has_two_units() {
        let (_, _, body) = rest(timeout_request(9, 7, None, "", "k".into()));
        assert_eq!(body, Some(json!({"communication_disabled_until": null})));
        assert_eq!(time_left_label(0), "0s");
        assert_eq!(time_left_label(59), "59s");
        assert_eq!(time_left_label(3725), "1h 2m");
        assert_eq!(time_left_label(90_061), "1d 1h");
    }
}
