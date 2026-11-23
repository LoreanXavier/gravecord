//! Invite to Server / Stream / Activity / Stage / Event (modules 137811 & 520006), measured in
//! `design/dom/modals/invite-darker-cozy.json`: the title and "Recipients will land in #channel",
//! a friend search, rows with Invite, and the link footer. Opening it creates the invite as
//! `TRAFFIC/invite-open.json` records: `POST /channels/{id}/invites {validate, max_age:
//! 2592000, max_uses: 0, target_type: null|1|2, temporary: false, flags: 0}`. Invite on a row
//! sends the link in that person's DM; Copy puts it on the clipboard.

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

/// `IS_GUEST_INVITE` flag bit (module 821418 / 137811).
pub(super) const IS_GUEST_INVITE: u64 = 1;
/// `IS_APPLICATION_BYPASS` flag bit (module 821418 / 312556).
pub(super) const IS_APPLICATION_BYPASS: u64 = 8;

/// Target of an instant invite dialog (modules 137811 & 520006).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum InviteTarget {
    #[default]
    Server,
    /// `target_type: 1` (`STREAM`) + `target_user_id`.
    Stream {
        user_id: Id,
    },
    /// `target_type: 2` (`EMBEDDED_APPLICATION`) + `target_application_id`.
    Activity {
        application_id: Id,
        application_name: String,
    },
    Stage,
    Event {
        event_id: Id,
    },
}

pub(crate) struct Invite {
    pub channel: Id,
    pub target: InviteTarget,
    pub search: Entity<Editor>,
    _events: Subscription,
    pub query: String,
    pub role_search: Entity<Editor>,
    _role_events: Subscription,
    pub role_query: String,
    pub code: Option<String>,
    /// People this dialog already sent the link to.
    pub sent: HashSet<Id>,
    /// The invite could not be made (`networkError` of module 312556).
    pub error: Option<InviteError>,
    /// `currentPage` of module 137811's modal state.
    pub page: InvitePage,
    /// `maxAge`, `maxUses`, `temporary`, `flags` and `roleIds` of the modal state: what the link has, and on the
    /// settings page what Generate a New Link asks for.
    pub max_age: u64,
    pub max_uses: u64,
    pub temporary: bool,
    pub flags: u64,
    pub roles: Vec<Id>,
    /// The settings page's open select.
    pub open_select: Option<InviteSelect>,
    /// Force the no-suggestions layout (`137811` `HFbByJ`/`ueBhA9` + `QKJru/` checkbox + `Yx4IiC` gear) when true.
    #[cfg(feature = "capture")]
    pub force_no_suggestions: bool,
}

/// `eZ.QR` of module 172799.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InvitePage {
    Main,
    Settings,
}

/// The selects of the settings page (`eA` of module 137811).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InviteSelect {
    MaxAge,
    MaxUses,
    Roles,
}

/// `MAX_AGE_OPTIONS` of module 558213 as `getMaxAgeOptions` (module 735547) keeps them: 14, 30 and 60 days only when
/// the guild's default-expiration experiment names them. The recorded default is 30 days (`MAX_AGE`), so 30 days
/// stays and 14 and 60 go.
pub(super) const MAX_AGE_OPTIONS: [u64; 8] =
    [1800, 3600, 21_600, 43_200, 86_400, 604_800, MAX_AGE, 0];
/// `MAX_USES_OPTIONS` of module 558213.
pub(super) const MAX_USES_OPTIONS: [u64; 7] = [0, 1, 5, 10, 25, 50, 100];

/// An Expire After option's label (module 558213: `iXLF9W` minutes, `xCjYxK` hours, `k2UNz+` days, `PqEzn8` Never).
pub(super) fn max_age_label(seconds: u64) -> String {
    match seconds {
        0 => dc_intl::t("PqEzn8").to_owned(),
        s if s < 3600 => dc_intl::format("iXLF9W", &[("minutes", ((s / 60) as i64).into())]),
        s if s < 86_400 => dc_intl::format("xCjYxK", &[("hours", ((s / 3600) as i64).into())]),
        s => dc_intl::format("k2UNz+", &[("days", ((s / 86_400) as i64).into())]),
    }
}

/// A Max Number of Uses option's label (`r/IcuP`: "No limit", "1 use", "5 uses").
pub(super) fn max_uses_label(uses: u64) -> String {
    dc_intl::format("r/IcuP", &[("maxUses", (uses as i64).into())])
}

/// `Be` of module 735547: "Your invite link expires in 7 days, or after 5 uses." and its kin.
pub(super) fn expiry_line(max_age: u64, max_uses: u64) -> String {
    let uses: dc_intl::Arg = (max_uses as i64).into();
    let unlimited = max_uses == 0;
    match max_age {
        0 if unlimited => dc_intl::t("QrHBnC").to_owned(),
        0 => dc_intl::format("yJnTxI", &[("numUses", uses)]),
        s if s < 3600 && unlimited => dc_intl::t("/WbTXD").to_owned(),
        s if s < 3600 => dc_intl::format("eDRWJK", &[("numUses", uses)]),
        s if s < 86_400 => {
            let hours: dc_intl::Arg = ((s / 3600) as i64).into();
            if unlimited {
                dc_intl::format("ZVdJMy", &[("numHours", hours)])
            } else {
                dc_intl::format("NgZgAB", &[("numHours", hours), ("numUses", uses)])
            }
        }
        s => {
            let days: dc_intl::Arg = ((s / 86_400) as i64).into();
            if unlimited {
                dc_intl::format("T96qss", &[("numDays", days)])
            } else {
                dc_intl::format("TfuB9B", &[("numDays", days), ("numUses", uses)])
            }
        }
    }
}

/// The body Generate a New Link posts (`eQ` of module 137811 through `createInvite` of module 574332, which drops an
/// empty `role_ids`; `target_user_id` and `target_application_id` are undefined for a server invite and never sent).
#[cfg(test)]
pub(super) fn generate_body(max_age: u64, max_uses: u64, temporary: bool, roles: &[Id]) -> Value {
    generate_body_with_flags(max_age, max_uses, temporary, 0, roles)
}

#[cfg(test)]
pub(super) fn generate_body_with_flags(
    max_age: u64,
    max_uses: u64,
    temporary: bool,
    flags: u64,
    roles: &[Id],
) -> Value {
    generate_body_for_target(
        max_age,
        max_uses,
        temporary,
        flags,
        roles,
        &InviteTarget::Server,
    )
}

pub(super) fn generate_body_for_target(
    max_age: u64,
    max_uses: u64,
    temporary: bool,
    flags: u64,
    roles: &[Id],
    target: &InviteTarget,
) -> Value {
    let mut body = json!({
        "max_age": max_age,
        "max_uses": max_uses,
        "target_type": null,
        "temporary": temporary,
        "flags": flags,
    });
    match target {
        InviteTarget::Stream { user_id } => {
            body["target_type"] = json!(1);
            body["target_user_id"] = json!(user_id.to_string());
        }
        InviteTarget::Activity { application_id, .. } => {
            body["target_type"] = json!(2);
            body["target_application_id"] = json!(application_id.to_string());
        }
        InviteTarget::Server | InviteTarget::Stage | InviteTarget::Event { .. } => {}
    }
    if !roles.is_empty() {
        body["role_ids"] = roles.iter().map(|role| role.to_string()).collect();
    }
    body
}

/// Modal title & link-section header copy for each `InviteTarget` (modules 137811 & 520006).
pub(super) fn invite_target_copy(
    target: &InviteTarget,
    guild_name: &str,
) -> (String, &'static str) {
    match target {
        InviteTarget::Server => (
            format!("Invite friends to {guild_name}"),
            dc_intl::t("MLkj7N"),
        ),
        InviteTarget::Stream { .. } => (dc_intl::t("6VQaqd").to_owned(), dc_intl::t("1b9neu")),
        InviteTarget::Activity {
            application_name, ..
        } => (
            dc_intl::format(
                "ZdK3dW",
                &[(
                    "applicationName",
                    dc_intl::Arg::Str(std::borrow::Cow::Borrowed(application_name)),
                )],
            ),
            dc_intl::t("iI1gMg"),
        ),
        InviteTarget::Stage => (dc_intl::t("zTrsH0").to_owned(), dc_intl::t("2frWa3")),
        InviteTarget::Event { .. } => (dc_intl::t("JKV4FL").to_owned(), dc_intl::t("KaWCyD")),
    }
}

/// Invite link URL formatted for the target (appends `?event={id}` for Event invites, module 137811).
pub(super) fn invite_url_for_target(code: &str, target: &InviteTarget) -> String {
    match target {
        InviteTarget::Event { event_id } => format!("https://discord.gg/{code}?event={event_id}"),
        _ => format!("https://discord.gg/{code}"),
    }
}

/// The roles an invite may hand out (`ek` of module 137811): none without Manage Roles; never @everyone, a managed
/// role or a linked-roles one; the owner may give any other, anyone else only roles under their highest
/// (`wO` of module 488926), highest first (`getSortedRoles`).
pub(super) fn assignable_roles(guild: &dc_state::Guild, me: Id, manage_roles: bool) -> Vec<Id> {
    if !manage_roles {
        return Vec::new();
    }
    let owner = guild.owner_id == Some(me);
    let position = |role: &Id| guild.role_info.get(role).map_or(0, |info| info.position);
    let highest = guild.my_roles.iter().map(position).max();
    let mut roles: Vec<Id> = guild
        .roles
        .keys()
        .copied()
        .filter(|role| *role != guild.id)
        .filter(|role| {
            guild
                .role_info
                .get(role)
                .is_none_or(|info| !info.managed && !info.guild_connection)
        })
        .filter(|role| owner || highest.is_some_and(|top| top > position(role)))
        .collect();
    roles.sort_by_key(|role| (std::cmp::Reverse(position(role)), *role));
    roles
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
        self.open_invite_for(channel, InviteTarget::Server, cx);
    }

    /// Opens the instant invite modal for a specific channel & target (module 520006 / 137811).
    pub(super) fn open_invite_for(
        &mut self,
        channel: Id,
        target: InviteTarget,
        cx: &mut Context<Self>,
    ) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(dc_intl::t("CmSHYx").to_owned());
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
        let role_search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(dc_intl::t("/djIh7").to_owned());
            editor
        });
        let role_events = cx.subscribe(&role_search, |this, editor, event: &EditorEvent, cx| {
            if let Some(invite) = this.invite.as_mut() {
                match event {
                    EditorEvent::Edited => invite.role_query = editor.read(cx).text().to_owned(),
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text))
                    }
                    EditorEvent::Cancel => invite.open_select = None,
                }
            }
            cx.notify();
        });
        let previous = (target == InviteTarget::Server)
            .then(|| self.invite_codes.get(&channel).cloned())
            .flatten();
        let mut body = generate_body_for_target(MAX_AGE, 0, false, 0, &[], &target);
        body["validate"] = json!(previous);
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/invites"),
            body: Some(body),
        });
        self.invite = Some(Invite {
            channel,
            target,
            search,
            _events: events,
            query: String::new(),
            role_search,
            _role_events: role_events,
            role_query: String::new(),
            code: previous,
            sent: HashSet::new(),
            error: None,
            page: InvitePage::Main,
            max_age: MAX_AGE,
            max_uses: 0,
            temporary: false,
            flags: 0,
            roles: Vec::new(),
            open_select: None,
            #[cfg(feature = "capture")]
            force_no_suggestions: false,
        });
        cx.notify();
    }

    /// Generate a New Link (`eQ` of module 137811), then back to the main page (`handleDone`). With no limit, no expiry
    /// and no temporary membership, someone who cannot create invites is shown the server's custom link instead.
    fn generate_invite(&mut self, cx: &mut Context<Self>) {
        let vanity = self.invite_shows_vanity();
        let can_create = self.invite_can_create();
        let temporary = self.invite_temporary();
        let Some(invite) = self.invite.as_mut() else {
            return;
        };
        invite.page = InvitePage::Main;
        invite.open_select = None;
        if invite.max_uses == 0 && invite.max_age == 0 && !temporary && vanity {
            invite.error = None;
            cx.notify();
            return;
        }
        if !can_create {
            cx.notify();
            return;
        }
        invite.error = None;
        let body = generate_body_for_target(
            invite.max_age,
            invite.max_uses,
            temporary,
            invite.flags,
            &invite.roles,
            &invite.target,
        );
        let _ = self.requests.send(Request::Rest {
            key: Some(CREATE_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: format!("/channels/{}/invites", invite.channel),
            body: Some(body),
        });
        cx.notify();
    }

    fn invite_guild(&self) -> Option<&dc_state::Guild> {
        let channel = self.invite.as_ref()?.channel;
        let guild = self.guild_of_channel(channel)?;
        self.stores.guild(guild)
    }

    /// `ea` of module 137811: Create Invite in the invite's channel.
    fn invite_can_create(&self) -> bool {
        let Some(invite) = self.invite.as_ref() else {
            return false;
        };
        let Some(guild) = self.invite_guild() else {
            return false;
        };
        let Some(channel) = guild
            .channels
            .iter()
            .find(|known| known.id == invite.channel)
        else {
            return false;
        };
        self.my_permission(
            guild.id,
            Some(channel),
            dc_state::permissions::CREATE_INSTANT_INVITE,
        )
    }

    /// `ef` of module 137811: the custom link stands in when the person cannot make invites and the server has one.
    fn invite_shows_vanity(&self) -> bool {
        !self.invite_can_create()
            && self
                .invite_guild()
                .and_then(|guild| guild.vanity_url_code.as_ref())
                .is_some_and(|code| !code.is_empty())
    }

    /// `eG` of module 137811: a manual-approval server hides Grant temporary membership (the guest voice invite
    /// experiment, the other cause, defaults off).
    fn invite_hides_temporary(&self) -> bool {
        self.invite_guild().is_some_and(|guild| {
            let has = |feature: &str| guild.features.iter().any(|known| known == feature);
            has("MEMBER_VERIFICATION_MANUAL_APPROVAL") && has("MEMBER_VERIFICATION_GATE_ENABLED")
        })
    }

    /// `eL`: the temporary switch counts only while it is shown.
    fn invite_temporary(&self) -> bool {
        !self.invite_hides_temporary()
            && self.invite.as_ref().is_some_and(|invite| invite.temporary)
    }

    fn invite_roles(&self) -> Vec<Id> {
        let Some(guild) = self.invite_guild() else {
            return Vec::new();
        };
        let Some(me) = self.stores.me() else {
            return Vec::new();
        };
        let manage = self.my_permission(guild.id, None, dc_state::permissions::MANAGE_ROLES);
        assignable_roles(guild, me.id, manage)
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
            // The modal state starts from the store's invite (`ev`, `ex`, `eI` of module 137811).
            let body = &report["body"];
            if invite.page == InvitePage::Main {
                if let Some(age) = body["max_age"].as_u64() {
                    invite.max_age = age;
                }
                if let Some(uses) = body["max_uses"].as_u64() {
                    invite.max_uses = uses;
                }
                if let Some(temporary) = body["temporary"].as_bool() {
                    invite.temporary = temporary;
                }
            }
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

    /// A select of the settings page (`890497`, `selectField`): a 40px field on `--input-background-default`, the
    /// choice 12px in at 16/20 (or the placeholder), the 20px chevron in a 36px button. Measured on the live dialog.
    fn invite_select_field(
        &self,
        which: InviteSelect,
        content: AnyElement,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = match which {
            InviteSelect::MaxAge => "invite-select-age",
            InviteSelect::MaxUses => "invite-select-uses",
            InviteSelect::Roles => "invite-select-roles",
        };
        let open = self
            .invite
            .as_ref()
            .is_some_and(|invite| invite.open_select == Some(which));
        div()
            .id(key)
            .relative()
            .child(self.anchor_probe(key))
            .min_h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if open {
                color("--input-border-active")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .when(disabled, |this| this.opacity(0.5).cursor_not_allowed())
            .when(!disabled, |this| this.cursor_pointer())
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(content))
            .child(
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
            .on_click(cx.listener(move |this, _, _, cx| {
                if !disabled && let Some(invite) = this.invite.as_mut() {
                    invite.open_select = if invite.open_select == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// A FormItem (module 452027): the label (body-md/medium, `--text-strong`) 8px over its control.
    fn invite_labelled(label: &str, control: AnyElement) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(label.to_owned()),
            )
            .child(control)
            .into_any_element()
    }

    /// The role dot of a role option and tag (module 545442), centred in 20px.
    fn invite_role_dot(&self, role: Id) -> AnyElement {
        let colour = self
            .invite_guild()
            .and_then(|guild| guild.role_info.get(&role))
            .map_or(0, |info| info.color);
        div()
            .size(px(20.))
            .flex()
            .items_center()
            .justify_center()
            .child(super::role_dot::dot(if colour == 0 {
                0x99aab5
            } else {
                colour
            }))
            .into_any_element()
    }

    fn invite_role_name(&self, role: Id) -> String {
        self.invite_guild()
            .and_then(|guild| guild.roles.get(&role).cloned())
            .unwrap_or_default()
    }

    /// Server invite link settings (`em` and `eA` of module 137811 on the dialog of module 189213): Expire After, Max
    /// Number of Uses, Roles (Optional) when the person may hand some out, and Grant temporary membership; Cancel and
    /// Generate a New Link both return to the main page.
    fn invite_settings_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let invite = self.invite.as_ref()?;
        let single = |text: String| {
            div()
                .pl(px(12.))
                .text_size(px(16.))
                .line_height(px(20.))
                .text_color(color("--text-strong"))
                .child(text)
                .into_any_element()
        };
        let age = self.invite_select_field(
            InviteSelect::MaxAge,
            single(max_age_label(invite.max_age)),
            false,
            cx,
        );
        let uses = self.invite_select_field(
            InviteSelect::MaxUses,
            single(max_uses_label(invite.max_uses)),
            false,
            cx,
        );
        let temporary = self.invite_temporary();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(Self::invite_labelled(dc_intl::t("60qw2x"), age))
            .child(Self::invite_labelled(dc_intl::t("jDqWHW"), uses));
        if !self.invite_roles().is_empty() {
            // The multiple select: a tag per chosen role (`tag_` 30px on the secondary control colours, 4px 4px 4px
            // 8px, the dot, body-sm and a remove cross), else the "Select Roles" placeholder.
            let tags: Vec<AnyElement> = invite
                .roles
                .iter()
                .map(|role| {
                    let role = *role;
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .h(px(30.))
                        .pl(px(8.))
                        .pr(px(4.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .child(self.invite_role_dot(role))
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--control-secondary-text-default"))
                                .child(self.invite_role_name(role)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("invite-tag-{role}")))
                                .size(px(18.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    "clips_x_small",
                                    16.,
                                    "--control-secondary-text-default",
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.invite_temporary()
                                        && let Some(invite) = this.invite.as_mut()
                                    {
                                        invite.roles.retain(|known| *known != role);
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                })
                .collect();
            let content = if tags.is_empty() {
                div()
                    .pl(px(12.))
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--input-placeholder-text-default"))
                    .child(dc_intl::t("/djIh7"))
                    .into_any_element()
            } else {
                div()
                    .py(px(3.))
                    .pl(px(4.))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(tags)
                    .into_any_element()
            };
            let roles = self.invite_select_field(InviteSelect::Roles, content, temporary, cx);
            body = body.child(Self::invite_labelled(dc_intl::t("rPYJxL"), roles));
        }
        if !self.invite_hides_temporary() {
            let locked = !invite.roles.is_empty();
            body = body.child(
                div()
                    .id("invite-temporary")
                    .flex()
                    .items_start()
                    .gap(px(24.))
                    .when(!locked, |this| this.cursor_pointer())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !locked && let Some(invite) = this.invite.as_mut() {
                            invite.temporary = !invite.temporary;
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
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("wE+9dr")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(dc_intl::t("UN5IRX").replace('\n', " ")),
                            ),
                    )
                    .child(if locked {
                        super::create_channel::switch_dimmed(
                            invite.temporary,
                            "--background-surface-high",
                        )
                    } else {
                        super::create_channel::switch(invite.temporary)
                    }),
            );
        }
        let is_voice_channel = self.invite_guild().is_some_and(|guild| {
            guild
                .channels
                .iter()
                .find(|ch| ch.id == invite.channel)
                .is_some_and(|ch| {
                    matches!(
                        ch.kind,
                        dc_state::ChannelKind::Voice | dc_state::ChannelKind::Other(13)
                    )
                })
        });
        if is_voice_channel {
            let guest_on = (invite.flags & IS_GUEST_INVITE) != 0;
            body = body.child(
                div()
                    .id("invite-settings-guest")
                    .cursor_pointer()
                    .flex()
                    .items_start()
                    .gap(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(invite) = this.invite.as_mut() {
                            invite.flags ^= IS_GUEST_INVITE;
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
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child(dc_intl::t("siexRS")),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(dc_intl::t("/FeTK6")),
                            ),
                    )
                    .child(super::create_channel::switch(guest_on)),
            );
        }
        let back = |this: &mut Root| {
            if let Some(invite) = this.invite.as_mut() {
                invite.page = InvitePage::Main;
                invite.open_select = None;
            }
        };
        Some(modal::render(
            modal::Spec {
                id: "invite-settings-modal",
                title: dc_intl::t("Atdlyu").into(),
                subtitle: None,
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "invite-settings-cancel",
                        dc_intl::t("ETE/oC"),
                        modal::Tone::Secondary,
                        move |this, _, _| back(this),
                    ),
                    modal::button(
                        "invite-settings-generate",
                        dc_intl::t("pz1lRG"),
                        modal::Tone::Primary,
                        |this, _, cx| this.generate_invite(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.invite = None),
            },
            cx,
        ))
    }

    /// The open list of a settings select (`maxOptionsVisible` 5: 40px rows in a 4px padded list, at most 208 high),
    /// 4px under the field; single choices tick the chosen row, the roles list checks a box per role.
    pub(super) fn invite_select_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let invite = self.invite.as_ref()?;
        if invite.page != InvitePage::Settings {
            return None;
        }
        let which = invite.open_select?;
        let key = match which {
            InviteSelect::MaxAge => "invite-select-age",
            InviteSelect::MaxUses => "invite-select-uses",
            InviteSelect::Roles => "invite-select-roles",
        };
        let field = self.tip_anchors.borrow().get(key).copied()?.dilate(px(1.));
        let rows: Vec<AnyElement> = match which {
            InviteSelect::MaxAge | InviteSelect::MaxUses => {
                let (options, current): (Vec<u64>, u64) = if which == InviteSelect::MaxAge {
                    (MAX_AGE_OPTIONS.to_vec(), invite.max_age)
                } else {
                    (MAX_USES_OPTIONS.to_vec(), invite.max_uses)
                };
                options
                    .into_iter()
                    .map(|value| {
                        let label = if which == InviteSelect::MaxAge {
                            max_age_label(value)
                        } else {
                            max_uses_label(value)
                        };
                        div()
                            .id(SharedString::from(format!("{key}-{value}")))
                            .flex_none()
                            .h(px(40.))
                            .pl(px(8.))
                            .pr(px(4.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|style| style.bg(color("--background-mod-normal")))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-strong"))
                                    .child(label),
                            )
                            .when(value == current, |this| {
                                this.child(div().px(px(4.)).child(icon(
                                    "select_check",
                                    16.,
                                    "--text-brand",
                                )))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(invite) = this.invite.as_mut() {
                                    if which == InviteSelect::MaxAge {
                                        invite.max_age = value;
                                    } else {
                                        invite.max_uses = value;
                                    }
                                    invite.open_select = None;
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
            InviteSelect::Roles => {
                let role_q = invite.role_query.trim().to_lowercase();
                let filtered: Vec<Id> = self
                    .invite_roles()
                    .into_iter()
                    .filter(|&role| {
                        role_q.is_empty()
                            || self.invite_role_name(role).to_lowercase().contains(&role_q)
                    })
                    .collect();
                let mut list: Vec<AnyElement> = vec![
                    div()
                        .flex_none()
                        .h(px(36.))
                        .px(px(8.))
                        .mb(px(4.))
                        .rounded(px(6.))
                        .border_1()
                        .border_color(color("--input-border-default"))
                        .bg(color("--input-background-default"))
                        .flex()
                        .items_center()
                        .child(invite.role_search.clone())
                        .into_any_element(),
                ];
                if filtered.is_empty() {
                    list.push(
                        div()
                            .h(px(36.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("ojoWgX"))
                            .into_any_element(),
                    );
                } else {
                    for role in filtered {
                        let chosen = invite.roles.contains(&role);
                        list.push(
                            div()
                                .id(SharedString::from(format!("{key}-{role}")))
                                .flex_none()
                                .h(px(40.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .hover(|style| style.bg(color("--background-mod-normal")))
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(32.))
                                        .mr(px(4.))
                                        .flex()
                                        .justify_center()
                                        .child(self.invite_role_dot(role)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .text_color(color("--text-strong"))
                                        .child(self.invite_role_name(role)),
                                )
                                .child(
                                    div()
                                        .px(px(4.))
                                        .child(super::server_onboarding::checkbox(chosen, false)),
                                )
                                // `closeOnSelect: false`: the list stays open.
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(invite) = this.invite.as_mut() {
                                        if invite.roles.contains(&role) {
                                            invite.roles.retain(|known| *known != role);
                                        } else {
                                            invite.roles.push(role);
                                        }
                                    }
                                    cx.notify();
                                }))
                                .into_any_element(),
                        );
                    }
                }
                list
            }
        };
        let height = (rows.len().min(5) as f32) * 40. + 8.;
        let menu_h = height + 2.;
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let open_up = space_below < menu_h && space_above > space_below;
        let top = if open_up {
            (field.top() - px(4.) - px(menu_h)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            gpui::deferred(
                div()
                    .id("invite-select-layer")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .h(px(menu_h))
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
                                && let Some(invite) = this.invite.as_mut()
                            {
                                invite.open_select = None;
                            }
                            cx.notify();
                        },
                    ))
                    .child(div().flex().flex_col().children(rows)),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(super) fn invite_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let invite = self.invite.as_ref()?;
        if invite.page == InvitePage::Settings {
            return self.invite_settings_modal(cx);
        }
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
        // Module 137811 / 520006: omit users already connected to the target voice/Stage channel.
        if matches!(
            invite.target,
            InviteTarget::Stream { .. } | InviteTarget::Activity { .. } | InviteTarget::Stage
        ) && let Some(gid) = self.guild_of_channel(invite.channel)
        {
            let in_channel: HashSet<Id> = self
                .stores
                .voice_members(gid, invite.channel)
                .into_iter()
                .map(|vs| vs.user_id)
                .collect();
            people.retain(|user| !in_channel.contains(&user.id));
        }
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
            .map(|code| invite_url_for_target(code, &invite.target))
            .unwrap_or_default();
        let (modal_title, link_header) = invite_target_copy(&invite.target, &guild_name);
        let copy_link = link.clone();
        let shown_link = link;
        #[cfg(feature = "capture")]
        let force_empty = invite.force_no_suggestions;
        #[cfg(not(feature = "capture"))]
        let force_empty = false;
        let has_suggestions = !force_empty && (!people.is_empty() || !query.is_empty());

        let copy_bar = div()
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
                    .child(if self.streamer_hides_invites() && !shown_link.is_empty() {
                        dc_intl::t("6HzNgZ").to_owned()
                    } else {
                        shown_link
                    }),
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
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_link.clone()));
                            this.show_app_toast("Copied!", cx);
                        }
                    }))
                    .child("Copy"),
            );

        let body = if !has_suggestions {
            // No-suggestions variant of module 137811 (`HFbByJ`/`ueBhA9`, `QKJru/` never-expire checkbox, `Yx4IiC` gear).
            let is_owner = self
                .current_guild()
                .zip(self.stores.current_user.as_ref())
                .is_some_and(|(g, me)| g.owner_id == Some(me.id));
            let intro = if is_owner {
                dc_intl::t("HFbByJ")
            } else {
                dc_intl::t("ueBhA9")
            };
            let never_expires = invite.max_age == 0;
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .mb(px(12.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(intro),
                )
                .child(copy_bar)
                .child(
                    div()
                        .mt(px(14.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("invite-never-expire")
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(invite) = this.invite.as_mut() {
                                        invite.max_age =
                                            if invite.max_age == 0 { MAX_AGE } else { 0 };
                                    }
                                    this.generate_invite(cx);
                                }))
                                .child(super::server_onboarding::checkbox(never_expires, false))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("QKJru/")),
                                ),
                        )
                        .child(
                            div()
                                .id("invite-settings-gear")
                                .size(px(32.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .hover(|s| s.bg(color("--background-mod-subtle")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon(
                                    "guild_menu_settings",
                                    18.,
                                    "--interactive-text-default",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(invite) = this.invite.as_mut() {
                                        invite.page = InvitePage::Settings;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .children(
                    invite
                        .error
                        .clone()
                        .map(|error| self.invite_error_line(error, cx)),
                )
                .into_any_element()
        } else {
            div()
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
                                .child(link_header),
                        )
                        .child(copy_bar)
                        .children({
                            // Module 312556 (`isApplicationBypassToggleEnabled`): Bypass Join Applications switch.
                            let bypass_enabled = self.current_guild().is_some_and(|g| {
                                g.features
                                    .iter()
                                    .any(|f| f == "MEMBER_VERIFICATION_GATE_ENABLED")
                                    && g.features
                                        .iter()
                                        .any(|f| f == "MEMBER_VERIFICATION_MANUAL_APPROVAL")
                            });
                            let bypass_on = (invite.flags & IS_APPLICATION_BYPASS) != 0;
                            bypass_enabled.then(|| {
                                div()
                                    .id("invite-bypass-applications")
                                    .mt(px(14.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(inv) = this.invite.as_mut() {
                                            inv.flags ^= IS_APPLICATION_BYPASS;
                                            this.generate_invite(cx);
                                        }
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .child(dc_intl::t("1i1bUl")),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-muted"))
                                                    .child(dc_intl::t("jvd/LF")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .w(px(36.))
                                            .h(px(20.))
                                            .rounded_full()
                                            .p(px(2.))
                                            .bg(if bypass_on {
                                                color("--control-primary-background-default")
                                            } else {
                                                color("--background-base-lowest")
                                            })
                                            .flex()
                                            .items_center()
                                            .when(bypass_on, |d| d.justify_end())
                                            .child(
                                                div()
                                                    .size(px(16.))
                                                    .rounded_full()
                                                    .bg(color("--white")),
                                            ),
                                    )
                            })
                        })
                        .child(if self.invite_shows_vanity() {
                            div()
                                .mt(px(16.))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-muted"))
                                .child(dc_intl::t("0M2U95"))
                                .into_any_element()
                        } else {
                            div()
                                .mt(px(16.))
                                .flex()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .child(div().text_color(color("--text-muted")).child(format!(
                                    "{}\u{a0}",
                                    expiry_line(invite.max_age, invite.max_uses)
                                )))
                                .child(
                                    div()
                                        .id("invite-edit-link")
                                        .cursor_pointer()
                                        .hover(|style| style.underline())
                                        .text_color(color("--text-link"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(invite) = this.invite.as_mut() {
                                                invite.page = InvitePage::Settings;
                                            }
                                            cx.notify();
                                        }))
                                        .child(dc_intl::t("VNe8P/")),
                                )
                                .into_any_element()
                        })
                        .children(
                            invite
                                .error
                                .clone()
                                .map(|error| self.invite_error_line(error, cx)),
                        ),
                )
                .into_any_element()
        };
        let subtitle: AnyElement = match &invite.target {
            InviteTarget::Stream { .. } => div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(icon("voice_channel", 16., "--text-subtle"))
                        .child(channel_name),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(dc_intl::t("CXpS1I")),
                )
                .into_any_element(),
            InviteTarget::Activity { .. } => div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon("voice_channel", 16., "--text-subtle"))
                .child(channel_name)
                .into_any_element(),
            InviteTarget::Stage => div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(icon("stage_channel", 16., "--text-subtle"))
                .child(channel_name)
                .into_any_element(),
            InviteTarget::Event { .. } => div()
                .text_color(color("--text-subtle"))
                .child(dc_intl::t("ueCrHB"))
                .into_any_element(),
            InviteTarget::Server => div()
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
        };
        Some(modal::render(
            modal::Spec {
                id: "invite-modal",
                title: modal_title.into(),
                subtitle: Some(subtitle),
                body: Some(body),
                buttons: Vec::new(),
                close: Box::new(|this, _, _| this.invite = None),
            },
            cx,
        ))
    }

    /// Module 520006 `A`: Invite to Stream / Invite to Activity / Invite to Stage action button.
    #[allow(dead_code)]
    pub(super) fn invite_target_button(
        &self,
        id: &'static str,
        channel: Id,
        target: InviteTarget,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let label = match &target {
            InviteTarget::Stream { .. } => dc_intl::t("6VQaqd"),
            InviteTarget::Activity { .. } => dc_intl::t("OzOM/q"),
            InviteTarget::Stage => dc_intl::t("6F9ivu"),
            _ => dc_intl::tr("Invite"),
        };
        let target_click = target.clone();
        Some(
            div()
                .id(id)
                .h(px(32.))
                .px(px(12.))
                .rounded(px(6.))
                .bg(color("--control-secondary-background-default"))
                .hover(|s| s.bg(color("--control-secondary-background-hover")))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon(
                    "voice_view_invite",
                    16.,
                    "--control-secondary-text-default",
                ))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--control-secondary-text-default"))
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_invite_for(channel, target_click.clone(), cx);
                }))
                .into_any_element(),
        )
    }

    /// Capture helper: `invite-target [stream|activity|stage|event]` opens the targeted invite modal
    /// (modules 520006 / 137811).
    #[cfg(feature = "capture")]
    pub(crate) fn script_invite_target(&mut self, which: &str, cx: &mut Context<Self>) -> String {
        let channel = self
            .channel
            .or_else(|| {
                self.stores
                    .guilds
                    .first()
                    .and_then(|g| g.channels.first().map(|c| c.id))
            })
            .unwrap_or(1);
        let target = match which.trim() {
            "stream" => InviteTarget::Stream {
                user_id: self.stores.current_user.as_ref().map(|u| u.id).unwrap_or(1),
            },
            "activity" => InviteTarget::Activity {
                application_id: 880218394199220334,
                application_name: "Watch Together".to_owned(),
            },
            "stage" => InviteTarget::Stage,
            "event" => InviteTarget::Event {
                event_id: 1234567890,
            },
            _ => InviteTarget::Server,
        };
        self.open_invite_for(channel, target, cx);
        if let Some(invite) = self.invite.as_mut()
            && invite.code.is_none()
        {
            invite.code = Some("gravecord".to_owned());
        }
        cx.notify();
        format!("invite-target: {which}")
    }

    /// Capture helper: `invite-no-suggestions` opens the invite modal in its empty-friends layout (module 137811).
    #[cfg(feature = "capture")]
    pub(crate) fn script_invite_no_suggestions(&mut self, cx: &mut Context<Self>) -> String {
        self.open_invite(cx);
        if let Some(invite) = self.invite.as_mut() {
            invite.force_no_suggestions = true;
            if invite.code.is_none() {
                invite.code = Some("gravecord".to_owned());
            }
        }
        cx.notify();
        "invite-no-suggestions: opened".to_owned()
    }
}

pub(super) const DM_KEY: &str = "invite-dm:";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_settings_offer_the_official_options_and_words() {
        let ages: Vec<String> = MAX_AGE_OPTIONS
            .iter()
            .map(|age| max_age_label(*age))
            .collect();
        assert_eq!(
            ages,
            [
                "30 minutes",
                "1 hour",
                "6 hours",
                "12 hours",
                "1 day",
                "7 days",
                "30 days",
                "Never"
            ]
        );
        let uses: Vec<String> = MAX_USES_OPTIONS
            .iter()
            .map(|uses| max_uses_label(*uses))
            .collect();
        assert_eq!(
            uses,
            [
                "No limit", "1 use", "5 uses", "10 uses", "25 uses", "50 uses", "100 uses"
            ]
        );
        assert_eq!(
            expiry_line(MAX_AGE, 0),
            "Your invite link expires in 30 days."
        );
        assert_eq!(
            expiry_line(86_400, 1),
            "Your invite link expires in 1 day, or after 1 use."
        );
        assert_eq!(
            expiry_line(21_600, 0),
            "Your invite link expires in 6 hours."
        );
        assert_eq!(
            expiry_line(3600, 5),
            "Your invite link expires in 1 hour, or after 5 uses."
        );
        assert_eq!(
            expiry_line(1800, 0),
            "Your invite link expires in 30 minutes."
        );
        assert_eq!(
            expiry_line(1800, 10),
            "Your invite link expires in 30 minutes, or after 10 uses."
        );
        assert_eq!(expiry_line(0, 0), "Your invite link will never expire.");
        assert_eq!(
            expiry_line(0, 25),
            "Your invite link expires after 25 uses."
        );
    }

    #[test]
    fn generate_posts_the_chosen_settings_and_drops_empty_roles() {
        assert_eq!(
            generate_body(604_800, 5, true, &[]),
            json!({"max_age": 604800, "max_uses": 5, "target_type": null, "temporary": true, "flags": 0})
        );
        assert_eq!(
            generate_body(0, 0, false, &[7, 9])["role_ids"],
            json!(["7", "9"])
        );
    }

    #[test]
    fn only_roles_under_ones_highest_can_be_handed_out() {
        let mut guild = dc_state::Guild {
            id: 1,
            owner_id: Some(50),
            my_roles: vec![3],
            ..Default::default()
        };
        for (role, position, managed) in [
            (1, 0, false),
            (2, 1, false),
            (3, 2, false),
            (4, 3, false),
            (5, 1, true),
        ] {
            guild.roles.insert(role, format!("r{role}"));
            guild.role_info.insert(
                role,
                dc_state::RoleInfo {
                    position,
                    managed,
                    ..Default::default()
                },
            );
        }
        assert_eq!(assignable_roles(&guild, 60, true), vec![2]);
        assert_eq!(assignable_roles(&guild, 60, false), Vec::<Id>::new());
        assert_eq!(assignable_roles(&guild, 50, true), vec![4, 3, 2]);
    }

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

    #[test]
    fn guest_invite_flags_and_no_suggestions_strings_resolve() {
        let body = generate_body_with_flags(3600, 5, false, IS_GUEST_INVITE, &[]);
        assert_eq!(body["flags"], json!(IS_GUEST_INVITE));
        for key in [
            "siexRS", "/FeTK6", "HFbByJ", "ueBhA9", "QKJru/", "Yx4IiC", "ojoWgX", "CmSHYx",
            "MLkj7N",
        ] {
            assert!(
                dc_intl::has_key(key),
                "missing en-US key {key} in invite 137811"
            );
        }
    }

    #[test]
    fn targeted_invite_bodies_and_520006_strings_resolve() {
        let stream_body = generate_body_for_target(
            MAX_AGE,
            0,
            false,
            0,
            &[],
            &InviteTarget::Stream { user_id: 77 },
        );
        assert_eq!(stream_body["target_type"], json!(1));
        assert_eq!(stream_body["target_user_id"], json!("77"));

        let act_body = generate_body_for_target(
            MAX_AGE,
            0,
            false,
            0,
            &[],
            &InviteTarget::Activity {
                application_id: 880218394199220334,
                application_name: "Watch Together".to_owned(),
            },
        );
        assert_eq!(act_body["target_type"], json!(2));
        assert_eq!(
            act_body["target_application_id"],
            json!("880218394199220334")
        );

        assert_eq!(
            invite_url_for_target("abc", &InviteTarget::Event { event_id: 999 }),
            "https://discord.gg/abc?event=999"
        );
        for key in [
            "6VQaqd", "ZdK3dW", "JKV4FL", "zTrsH0", "CXpS1I", "ueCrHB", "1b9neu", "iI1gMg",
            "KaWCyD", "2frWa3", "OzOM/q", "6F9ivu",
        ] {
            assert!(dc_intl::has_key(key), "missing key {key} in 520006/137811");
        }
    }
}
