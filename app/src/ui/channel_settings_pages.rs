//! The other channel settings pages, measured in `design/dom/settings/channel-{permissions,invites,integrations}-*`:
//! Permissions (synced with the category, the Private Channel switch), Invites (the channel's invite table with
//! revoke and "create one") and Integrations (webhooks and followed channels).

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use crate::parity::Parity;
use crate::ui::motion::Transitions as _;
use dc_state::permissions::{self, Overwrite, VIEW_CHANNEL};
use dc_state::{Channel, Id, User};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// Module 218113 `aE`: how long Security Actions pause things, in hours.
const LOCKDOWN_HOURS: [(u64, &str); 6] = [
    (1, "1 hour"),
    (2, "2 hours"),
    (4, "4 hours"),
    (6, "6 hours"),
    (12, "12 hours"),
    (24, "24 hours"),
];

/// "Security Actions" (module 671576): the pause length, Pause Invites, Pause DMs; Save puts them.
pub(crate) struct SecurityActions {
    pub guild: Id,
    pub hours: u64,
    pub invites: bool,
    pub dms: bool,
    was: (u64, bool, bool),
    pub select_open: bool,
}

fn active(until: Option<&String>) -> bool {
    until
        .and_then(|until| chrono::DateTime::parse_from_rfc3339(until).ok())
        .is_some_and(|until| until.timestamp_millis() > chrono::Utc::now().timestamp_millis())
}

/// Save of module 671576: `(T || D) && !L && !z`, a pause was on when the dialog opened and none is now.
pub(super) fn lifts_lockdown(was: (u64, bool, bool), invites: bool, dms: bool) -> bool {
    (was.1 || was.2) && !invites && !dms
}

/// Module 859706 `c`: the body of `PUT /guilds/{id}/incident-actions`.
pub(super) fn incident_body(
    invites: bool,
    dms: bool,
    hours: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    let any = invites || dms;
    let until = (now + chrono::Duration::hours(hours as i64))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    serde_json::json!({
        "invites_disabled_until": if invites { serde_json::json!(until) } else { serde_json::Value::Null },
        "dms_disabled_until": if dms { serde_json::json!(until) } else { serde_json::Value::Null },
        "lockdown_duration_hours": if any { serde_json::json!(hours) } else { serde_json::Value::Null },
    })
}

/// `channelPermissionSettingsAdvancedModeOn` in local storage.
const ADVANCED_KEY: &str = "channelPermissionSettingsAdvancedModeOn";
/// The "+" over Roles/Members (`/assets/ef103ccb22234d89.svg`).
pub(super) const ADD_TARGET_ART: &str = "https://discordapp.com/assets/ef103ccb22234d89.svg";
/// "Need help with permissions?"
pub(super) const PERMISSIONS_HELP: &str = "https://support.discord.com/hc/en-us/articles/206029707";

/// Module 824953: a new webhook is one of these, at random.
pub(super) const WEBHOOK_NAMES: [&str; 2] = ["Spidey Bot", "Captain Hook"];

/// `dd:hh:mm:ss` until an invite runs out (days only when there are any); "∞" for one that never does.
pub(super) fn expires_in(created_ms: i64, max_age_s: i64, now_ms: i64) -> String {
    if max_age_s == 0 {
        return "\u{221e}".to_owned();
    }
    let left = ((created_ms + max_age_s * 1_000 - now_ms) / 1_000).max(0);
    let (days, hours, minutes, seconds) =
        (left / 86_400, left / 3_600 % 24, left / 60 % 60, left % 60);
    if days > 0 {
        format!("{days}:{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }
}

/// `Sz` of module 218113: any of Administrator, Manage Server, Ban, Kick and Timeout Members shows the incident items.
pub(super) const INCIDENT_ACTIONS: u64 = permissions::ADMINISTRATOR
    | permissions::MANAGE_GUILD
    | permissions::BAN_MEMBERS
    | permissions::KICK_MEMBERS
    | permissions::MODERATE_MEMBERS;

/// `_J` of module 903093: Pause Invites or Pause DMs still runs.
pub(super) fn under_lockdown(invites_until: Option<&String>, dms_until: Option<&String>) -> bool {
    active(invites_until) || active(dms_until)
}

impl Root {
    /// The `server-lockdown` item of module 664967 (the server header menu and the server context menu): shown in a
    /// Community server to a member holding any of `Sz` (`shouldShowIncidentActions` of module 585510), labelled Edit
    /// Security Actions while a pause runs (`isUnderLockdown`) and Security Actions otherwise. `Some(lockdown)` when
    /// shown.
    pub(super) fn security_actions_item(&self, guild: Id) -> Option<bool> {
        let known = self.stores.guild(guild)?;
        if !known.features.iter().any(|feature| feature == "COMMUNITY") {
            return None;
        }
        let me = self.stores.me()?;
        if permissions::guild_permissions(known, me) & INCIDENT_ACTIONS == 0 {
            return None;
        }
        Some(under_lockdown(
            known.invites_disabled_until.as_ref(),
            known.dms_disabled_until.as_ref(),
        ))
    }

    pub(super) fn open_security_actions(&mut self, guild: Id, cx: &mut Context<Self>) {
        let Some(known) = self.stores.guilds.iter().find(|known| known.id == guild) else {
            return;
        };
        let hours = known
            .lockdown_duration_hours
            .filter(|hours| LOCKDOWN_HOURS.iter().any(|(value, _)| value == hours))
            .unwrap_or(2);
        let invites = active(known.invites_disabled_until.as_ref())
            || known.features.iter().any(|f| f == "INVITES_DISABLED");
        let dms = active(known.dms_disabled_until.as_ref());
        self.security_actions = Some(SecurityActions {
            guild,
            hours,
            invites,
            dms,
            was: (hours, invites, dms),
            select_open: false,
        });
        cx.notify();
    }

    pub(super) fn security_actions_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.security_actions.as_ref()?;
        let changed = (open.hours, open.invites, open.dms) != open.was;
        let hours_label = LOCKDOWN_HOURS
            .iter()
            .find(|(value, _)| *value == open.hours)
            .map_or("2 hours", |(_, label)| label);
        let toggle = |id: &'static str,
                      title: &'static str,
                      detail: &'static str,
                      on: bool,
                      invites: bool| {
            div()
                .p(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(color("--border-subtle"))
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child(detail),
                        ),
                )
                .child(
                    div()
                        .id(id)
                        .flex_none()
                        .w(px(53.))
                        .flex()
                        .justify_end()
                        .cursor_pointer()
                        .child(super::create_channel::switch(on))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(open) = this.security_actions.as_mut() {
                                if invites {
                                    open.invites = !open.invites;
                                } else {
                                    open.dms = !open.dms;
                                }
                            }
                            cx.notify();
                        })),
                )
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .id("security-actions-time")
                    .relative()
                    .child(self.anchor_probe("security-actions-time"))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if open.select_open { color("--input-border-active") } else { color("--input-border-default") })
.rule(crate::ui::motion::Rule::TextField)
                    .bg(color("--input-background-default"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .child(div().flex_1().pl(px(12.)).text_size(px(16.)).text_color(color("--text-default")).child(hours_label))
                    .child(div().w(px(36.)).pr(px(4.)).flex().justify_center().child(icon("combo_chevron", 20., "--text-strong")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.security_actions.as_mut() {
                            open.select_open = !open.select_open;
                        }
                        cx.notify();
                    })),
            )
            .child(toggle(
                "security-actions-invites",
                "Pause Invites",
                "Temporarily stop new members from joining this server via invite or vanity links.",
                open.invites,
                true,
            ))
            .child(toggle(
                "security-actions-dms",
                "Pause DMs",
                "Temporarily stop new direct messages from being sent between members in your server. Friends can still DM each other, moderators can still DM members, and your Apps can still DM members.",
                open.dms,
                false,
            ))
            .into_any_element();
        Some(super::modal::render(
            super::modal::Spec {
                id: "security-actions-modal",
                title: "Security Actions".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    super::modal::button(
                        "security-actions-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| {
                            this.security_actions = None;
                        },
                    ),
                    super::modal::button(
                        "security-actions-save",
                        "Save",
                        if changed {
                            super::modal::Tone::Primary
                        } else {
                            super::modal::Tone::PrimaryDisabled
                        },
                        |this, _, cx| {
                            let Some(open) = this.security_actions.take() else {
                                return;
                            };
                            // Module 671576: a save that lifts every pause that was on asks why the server was
                            // locked (module 336496).
                            if lifts_lockdown(open.was, open.invites, open.dms) {
                                this.open_lockdown_feedback(cx);
                            }
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::PUT,
                                route: format!("/guilds/{}/incident-actions", open.guild),
                                body: Some(incident_body(
                                    open.invites,
                                    open.dms,
                                    open.hours,
                                    chrono::Utc::now(),
                                )),
                            });
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.security_actions = None),
            },
            cx,
        ))
    }

    /// The pause length's list under its field.
    pub(super) fn security_actions_select(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.security_actions.as_ref()?;
        if !open.select_open {
            return None;
        }
        let field = self
            .tip_anchors
            .borrow()
            .get("security-actions-time")
            .copied()?;
        let current = open.hours;
        let rows = LOCKDOWN_HOURS.iter().map(|(hours, label)| {
            let hours = *hours;
            div()
                .id(SharedString::from(format!(
                    "security-actions-hours-{hours}"
                )))
                .flex_none()
                .h(px(40.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .pl(px(8.))
                        .text_size(px(16.))
                        .text_color(color("--text-strong"))
                        .child(*label),
                )
                .when(hours == current, |this| {
                    this.child(
                        div()
                            .px(px(4.))
                            .child(icon("select_check", 20., "--text-strong")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(open) = this.security_actions.as_mut() {
                        open.hours = hours;
                        open.select_open = false;
                    }
                    cx.notify();
                }))
        });
        let space_below = (self.viewport.1 - f32::from(field.bottom()) - 12.).max(0.);
        let space_above = (f32::from(field.top()) - 12.).max(0.);
        let top = if space_below < 220. && space_above > space_below {
            (field.top() - px(4.) - px(220.)).max(px(8.))
        } else {
            field.bottom() + px(4.)
        };
        Some(
            gpui::deferred(
                div()
                    .id("security-actions-list")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(top)
                    .w(field.size.width)
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![super::shadow_high()])
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(
                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                            if !field.contains(&event.position)
                                && let Some(open) = this.security_actions.as_mut()
                            {
                                open.select_open = false;
                                cx.notify();
                            }
                        },
                    ))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    fn page_title(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(20.))
            .line_height(px(24.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"))
            .child(text)
    }

    /// Permissions: the sync notice and Private Channel (`PUT /channels/{id}/permissions/{@everyone}` denying View
    /// Channel, or allowing it back).
    pub(super) fn channel_permissions_page(
        &self,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel.id));
        let everyone = guild.map(|guild| guild.id);
        let parent = channel.parent_id.and_then(|parent| {
            guild.and_then(|guild| guild.channels.iter().find(|known| known.id == parent))
        });
        let synced = parent.is_some_and(|parent| {
            let mut ours = channel.overwrites.clone();
            let mut theirs = parent.overwrites.clone();
            ours.sort_by_key(|entry| entry.id);
            theirs.sort_by_key(|entry| entry.id);
            ours == theirs
        });
        let private = channel
            .overwrites
            .iter()
            .any(|entry| Some(entry.id) == everyone && entry.deny & VIEW_CHANNEL != 0);
        let id = channel.id;
        let current: Option<Overwrite> = channel
            .overwrites
            .iter()
            .find(|entry| Some(entry.id) == everyone)
            .cloned();
        let card = |child: AnyElement| {
            div()
                .p(px(16.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-mod-subtle"))
                .child(child)
        };
        div()
            .flex()
            .flex_col()
            .child(Self::page_title("Channel Permissions"))
            .child(
                div()
                    .mb(px(16.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Use permissions to customize who can do what in this channel."),
            )
            .when_some(parent.filter(|_| !synced), |this, parent| {
                // `OIhm0M` with Sync Now (module 336541): the category's overwrites after a confirmation.
                this.child(
                    card(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(icon("permissions_unsynced", 20., "--status-warning"))
                            .child("Permissions not synced with category:")
                            .child(div().flex_1().font_weight(FontWeight(600.)).child(parent.name.clone()))
                            .child(
                                div()
                                    .id("channel-permissions-sync")
                                    .h(px(32.))
                                    .px(px(11.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--control-secondary-border-default"))
                                    .bg(color("--control-secondary-background-default"))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--control-secondary-text-default"))
                                    .child("Sync Now")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(settings) = this.channel_settings.as_mut() {
                                            settings.confirm_sync = true;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .into_any_element(),
                    )
                    .mb(px(16.)),
                )
            })
            .when_some(parent.filter(|_| synced), |this, parent| {
                this.child(
                    card(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(icon("permissions_synced", 20., "--status-warning"))
                            .child("Permissions synced with category:")
                            .child(div().font_weight(FontWeight(600.)).child(parent.name.clone()))
                            .into_any_element(),
                    )
                    .mb(px(16.)),
                )
            })
            .child(card(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("channel_lock", 20., "--text-strong"))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child("Private Channel"),
                            )
                            .child(
                                div()
                                    .id("channel-settings-private")
                                    .cursor_pointer()
                                    .child(super::create_channel::switch(private))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let Some(everyone) = everyone else {
                                            return;
                                        };
                                        let (allow, deny) = current
                                            .as_ref()
                                            .map_or((0, 0), |entry| (entry.allow, entry.deny));
                                        let deny = if private { deny & !VIEW_CHANNEL } else { deny | VIEW_CHANNEL };
                                        let _ = this.requests.send(Request::Rest {
                                            key: None,
                                            method: reqwest::Method::PUT,
                                            route: format!("/channels/{id}/permissions/{everyone}"),
                                            body: Some(serde_json::json!({
                                                "id": everyone.to_string(),
                                                "type": 0,
                                                "allow": allow.to_string(),
                                                "deny": deny.to_string(),
                                            })),
                                        });
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child("By making a channel private, only select members and roles will be able to view this channel."),
                    )
                    .when(private, |box_div| {
                        let guild_id = self.guild.unwrap_or(1);
                        box_div.child(
                            div()
                                .id("channel-settings-add-members-roles")
                                .mt(px(4.))
                                .self_start()
                                .cursor_pointer()
                                .px(px(14.))
                                .h(px(32.))
                                .rounded(px(6.))
                                .bg(color("--control-primary-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--white"))
                                .child(dc_intl::t("dMJ3Y6"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_grant_channel_access(guild_id, id, false, cx);
                                })),
                        )
                    })
                    .into_any_element(),
            ))
            .child(self.advanced_permissions(channel, cx))
            .into_any_element()
    }

    /// Asks for what a page lists.
    pub(super) fn request_channel_settings_page(&mut self, channel: Id, invites: bool) {
        let (key, route) = if invites {
            (
                format!("channel-invites:{channel}"),
                format!("/channels/{channel}/invites"),
            )
        } else {
            (
                format!("channel-webhooks:{channel}"),
                format!("/channels/{channel}/webhooks"),
            )
        };
        let _ = self.requests.send(Request::Fetch { key, route });
    }

    /// Invites: who made each, its code, uses, time left; the X revokes (`DELETE /invites/{code}`).
    pub(super) fn channel_invites_page(&self, channel: Id, cx: &mut Context<Self>) -> AnyElement {
        let invites: Vec<serde_json::Value> = self
            .fetched
            .get(&format!("channel-invites:{channel}"))
            .and_then(|body| body.as_array().cloned())
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp_millis();
        let guild = self.guild_of_channel(channel);
        let column = |width: f32| div().w(px(width)).flex_none();
        let head = |text: &'static str, width: f32| {
            column(width)
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let mono = |text: String| {
            div()
                .font_family(super::MONO)
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let rows = invites
            .into_iter()
            .map(|invite| {
                let code = invite["code"].as_str().unwrap_or_default().to_owned();
                let inviter = User::from_value(&invite["inviter"]);
                let picture = inviter
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let uses = invite["uses"].as_u64().unwrap_or(0);
                let max = invite["max_uses"].as_u64().unwrap_or(0);
                let created = invite["created_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map_or(now, |at| at.timestamp_millis());
                let left = expires_in(created, invite["max_age"].as_i64().unwrap_or(0), now);
                let group = SharedString::from(format!("invite-row-{code}"));
                let revoke = code.clone();
                div()
                    .group(group.clone())
                    .relative()
                    .h(px(62.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .child(
                        column(210.)
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(div().size(px(24.)).rounded_full().children(
                                picture.map(|picture| img(picture).size(px(24.)).rounded_full()),
                            ))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(
                                        inviter
                                            .map(|user| user.display_name().to_owned())
                                            .unwrap_or_default(),
                                    ),
                            ),
                    )
                    .child(column(162.).child(self.invite_source_cell(&code, &invite, cx)))
                    .child(column(68.).child(mono(if max > 0 {
                        format!("{uses}/{max}")
                    } else {
                        uses.to_string()
                    })))
                    .child(column(115.).child(mono(left)))
                    .child(column(102.).children(self.invite_roles_cell(
                        &code,
                        &super::server_settings_pages::invite_roles(
                            guild.and_then(|guild| self.stores.guild(guild)),
                            &invite,
                        ),
                        cx,
                    )))
                    .child(
                        div()
                            .id(SharedString::from(format!("invite-revoke-{code}")))
                            .absolute()
                            .right(px(-31.))
                            .top(px(-12.))
                            .size(px(16.))
                            .cursor_pointer()
                            .opacity(0.)
                            .group_hover(group, |style| style.opacity(1.))
                            .child(icon("modal_close", 16., "--icon-subtle"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let _ = this.requests.send(Request::Rest {
                                    key: None,
                                    method: reqwest::Method::DELETE,
                                    route: format!("/invites/{revoke}"),
                                    body: None,
                                });
                                let key = format!("channel-invites:{channel}");
                                if let Some(list) = this
                                    .fetched
                                    .get_mut(&key)
                                    .and_then(|body| body.as_array_mut())
                                {
                                    list.retain(|invite| {
                                        invite["code"].as_str() != Some(revoke.as_str())
                                    });
                                }
                                cx.notify();
                            })),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .child(div().pb(px(40.)).child(Self::page_title("Invites")))
            .child(
                div()
                    .mb(px(20.))
                    .flex()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child("Here\u{2019}s a list of all active invite links. You can revoke any one or\u{a0}")
                    .child(
                        div()
                            .id("channel-invites-create")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .hover(|style| style.underline())
                            .child("create one")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.channel_settings = None;
                                if let Some(guild) = this.guild {
                                    this.select(guild, Some(channel));
                                }
                                this.open_invite(cx);
                            })),
                    )
                    .child("."),
            )
            .child({
                // `inviteDisabledContainer_k0kUi1` (a row 8px apart): Security Actions for the whole server, "Enable
                // Invites" (`primary`) while invites are paused, with `inviteDisabledTip_k0kUi1` (4px apart: the 20px
                // circle-error icon in `YELLOW_300`, "Invites to this server are currently paused.", `body-sm/normal`),
                // else "Pause Invites" (`critical-primary`); without Manage Server it is disabled (module 422542 `ec`).
                let (paused, allowed) =
                    guild.map_or((false, false), |guild| self.invites_pause_state(guild));
                let (border, background) = if paused {
                    (
                        "--control-primary-border-default",
                        "--control-primary-background-default",
                    )
                } else {
                    (
                        "--control-critical-primary-border-default",
                        "--control-critical-primary-background-default",
                    )
                };
                div()
                    .mb(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("channel-invites-pause")
                            .h(px(32.))
                            .px(px(11.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(border))
                            .bg(color(background))
                            .when(allowed, |button| button.cursor_pointer())
                            .when(!allowed, |button| button.opacity(0.5))
                            .flex()
                            .items_center()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(if paused {
                                dc_intl::t("/dbw3H")
                            } else {
                                dc_intl::t("Uwsjn6")
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(guild) = guild.filter(|_| allowed) {
                                    this.open_security_actions(guild, cx);
                                }
                            })),
                    )
                    .when(paused, |row| {
                        row.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(icon("shop_circle_error", 20., "--yellow-300"))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .line_height(px(18.))
                                        .text_color(color("--text-default"))
                                        .child(dc_intl::t("2LLbj9")),
                                ),
                        )
                    })
            })
            .child(div().mb(px(20.)).h(px(1.)).bg(color("--border-subtle")))
            .child({
                let linked = guild
                    .map(|g| self.linked_channels_for_guild(g, Some(channel)))
                    .unwrap_or_default();
                let has_linked = !linked.is_empty();
                let linked_rows: Vec<AnyElement> = linked
                    .into_iter()
                    .map(|row| self.linked_channel_invite_row(row, false, cx))
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .child(head(dc_intl::t("EgHyKy"), 203.))
                            .child(head(
                                if has_linked {
                                    dc_intl::t("l7VISQ")
                                } else {
                                    dc_intl::t("LBlFEN")
                                },
                                163.,
                            ))
                            .child(head(dc_intl::t("erWrmA"), 71.))
                            .child(head(dc_intl::t("1aM27d"), 121.))
                            .child(head(dc_intl::t("LPJmL/"), 102.)),
                    )
                    .children(rows)
                    .children(linked_rows)
            })
            .into_any_element()
    }

    /// Integrations: the channel's webhooks (Create Webhook posts one with module 824953's random name) and followed
    /// channels.
    pub(super) fn channel_integrations_page(
        &self,
        channel: Id,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let webhooks = self
            .fetched
            .get(&format!("channel-webhooks:{channel}"))
            .and_then(|body| body.as_array().map(Vec::len))
            .unwrap_or(0);
        let card = |glyph: &'static str, title: &'static str, count: String, button: AnyElement| {
            div()
                .h(px(88.))
                .px(px(20.))
                .rounded(px(8.))
                .bg(color("--background-mod-subtle"))
                .flex()
                .items_center()
                .gap(px(20.))
                .child(
                    div()
                        .size(px(40.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, 24., "--icon-subtle")),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(color("--text-default"))
                                .child(count),
                        ),
                )
                .child(button)
        };
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(32.))
                .px(px(11.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--control-primary-border-default"))
                .bg(color("--control-primary-background-default"))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color("--white"))
                .child(label)
        };
        let count = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        div()
            .flex()
            .flex_col()
            .child(div().pb(px(20.)).child(Self::page_title("Integrations")))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Customize your server with integrations. Manage webhooks and followed channels that post into this channel."),
            )
            .child(
                div()
                    .id("channel-integrations-learn")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-link"))
                    .child("Learn more about managing integrations.")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.follow_link(
                            // `MVz.INTEGRATIONS`.
                            "https://support.discord.com/hc/en-us/articles/360045093012",
                            "Learn more about managing integrations.",
                            cx,
                        );
                    })),
            )
            .child(div().my(px(32.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                card(
                    "integrations_webhook",
                    "Webhooks",
                    count(webhooks, "webhook", "webhooks"),
                    button("channel-create-webhook", "Create Webhook")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let name = WEBHOOK_NAMES[(chrono::Utc::now().timestamp_subsec_nanos() % 2) as usize];
                            let _ = this.requests.send(Request::Rest {
                                key: Some(format!("channel-webhook-created:{channel}")),
                                method: reqwest::Method::POST,
                                route: format!("/channels/{channel}/webhooks"),
                                body: Some(serde_json::json!({"name": name})),
                            });
                            cx.notify();
                        }))
                        .into_any_element(),
                )
                .mb(px(16.)),
            )
            .child(card(
                "integrations_followed",
                "Channels Followed",
                count(0, "channel", "channels"),
                button("channel-followed-learn", "Learn More")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.follow_link(
                            // `MVz.CHANNEL_FOLLOWING`.
                            "https://support.discord.com/hc/en-us/articles/360028384531",
                            "Learn More",
                            cx,
                        );
                    }))
                    .into_any_element(),
            ))
            .into_any_element()
    }
}

impl Root {
    /// "Advanced permissions" (`channelPermissionSettingsAdvancedModeOn`): the roles and members with an overwrite,
    /// and for the chosen one every permission of module 558393's spec as deny / pass through / allow.
    fn advanced_permissions(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let open = crate::storage::get(ADVANCED_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let header = div()
            .id("channel-permissions-advanced")
            .mt(px(40.))
            .pt(px(32.))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child("Advanced permissions"),
            )
            .child(
                gpui::svg()
                    .path("icons/chevron_right_20.svg")
                    .size(px(20.))
                    .text_color(color("--text-strong"))
                    // The icon points down (open); closed, it is turned to point right.
                    .when(!open, |this| {
                        this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                            3. * std::f32::consts::FRAC_PI_2,
                        )))
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                crate::storage::set(ADVANCED_KEY, serde_json::json!(!open));
                if !open {
                    this.request_image(ADD_TARGET_ART.to_owned(), ADD_TARGET_ART.to_owned());
                }
                cx.notify();
            }));
        if !open {
            return header.into_any_element();
        }
        let Some(guild) = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel.id))
        else {
            return header.into_any_element();
        };
        let settings = self.channel_settings.as_ref();
        let edits = settings
            .map(|settings| settings.perm_edits.clone())
            .unwrap_or_default();
        // @everyone first, then roles by name, then members.
        let mut targets: Vec<(Id, u64, String)> = vec![(guild.id, 0, "@everyone".to_owned())];
        for entry in &channel.overwrites {
            if entry.id == guild.id {
                continue;
            }
            let name = if entry.kind == 0 {
                guild.roles.get(&entry.id).cloned().unwrap_or_default()
            } else {
                self.stores
                    .users
                    .get(&entry.id)
                    .map(|user| user.display_name().to_owned())
                    .unwrap_or_default()
            };
            targets.push((entry.id, entry.kind, name));
        }
        for (id, (_, _, kind)) in &edits {
            if !targets.iter().any(|(known, _, _)| known == id) {
                let name = if *kind == 0 {
                    guild.roles.get(id).cloned().unwrap_or_default()
                } else {
                    self.stores
                        .users
                        .get(id)
                        .map(|user| user.display_name().to_owned())
                        .unwrap_or_default()
                };
                targets.push((*id, *kind, name));
            }
        }
        let selected = settings
            .and_then(|settings| settings.perm_target)
            .unwrap_or(guild.id);
        let kind_of = targets
            .iter()
            .find(|(id, _, _)| *id == selected)
            .map_or(0, |(_, kind, _)| *kind);
        let (allow, deny) = edits.get(&selected).map_or_else(
            || {
                channel
                    .overwrites
                    .iter()
                    .find(|entry| entry.id == selected)
                    .map_or((0, 0), |entry| (entry.allow, entry.deny))
            },
            |(allow, deny, _)| (*allow, *deny),
        );
        // `sidebarScrollable_EDKX0U`: 15px of padding and the 8px scrollbar gutter on the right.
        let list = div()
            .w(px(232.))
            .flex_none()
            .pt(px(24.))
            .pr(px(23.))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .px(px(10.))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(div().flex_1().child("Roles/Members"))
                    .child(
                        div()
                            .id("permissions-add-target")
                            .relative()
                            .child(self.anchor_probe("permissions-add-target"))
                            .cursor_pointer()
                            .children(
                                self.images
                                    .get(ADD_TARGET_ART)
                                    .cloned()
                                    .map(|art| img(art).size(px(16.))),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_add_target(window, cx)
                            })),
                    ),
            )
            .children(targets.iter().map(|(id, kind, name)| {
                let (id, kind) = (*id, *kind);
                let (channel_id, everyone) = (channel.id, guild.id);
                let chosen = id == selected;
                div()
                    .id(SharedString::from(format!("permissions-target-{id}")))
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(if chosen {
                        color("--text-strong")
                    } else {
                        color("--text-subtle")
                    })
                    .when(chosen, |this| this.bg(color("--background-mod-strong")))
                    .when(!chosen, |this| {
                        this.hover(|style| style.bg(color("--background-mod-subtle")))
                    })
                    .child(name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(settings) = this.channel_settings.as_mut() {
                            settings.perm_target = Some(id);
                        }
                        cx.notify();
                    }))
                    // `onContextMenu` of module 336541: the row's menu (module 495603).
                    .on_mouse_down(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.open_context_menu(
                                super::context_menu::Target::PermissionRow {
                                    channel: channel_id,
                                    target: id,
                                    role: kind == 0,
                                    everyone: id == everyone,
                                },
                                event.position,
                                cx,
                            );
                        }),
                    )
            }))
            .child(
                div()
                    .mx(px(10.))
                    .mt(px(20.))
                    .mb(px(14.))
                    .h(px(1.))
                    .bg(color("--border-subtle")),
            )
            .child(
                div()
                    .id("permissions-help")
                    .cursor_pointer()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-link"))
                    .hover(|style| style.underline())
                    .child("Need help with permissions?")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.follow_link(PERMISSIONS_HELP, "Need help with permissions?", cx);
                    })),
            );
        let sections: &[super::permission_spec::Section] = match channel.kind {
            dc_state::ChannelKind::Voice => super::permission_spec::VOICE,
            dc_state::ChannelKind::Category => super::permission_spec::CATEGORY,
            _ => super::permission_spec::TEXT,
        };
        let everyone = selected == guild.id;
        let channel_id = channel.id;
        let state_button = |flag: u64, state: u8, current: u8| {
            let (glyph, tint, background) = match state {
                0 => (
                    "perm_deny",
                    "--text-feedback-critical",
                    "--control-critical-primary-background-default",
                ),
                1 => (
                    "perm_passthrough",
                    "--text-strong",
                    "--background-mod-strong",
                ),
                _ => (
                    "perm_allow",
                    "--text-feedback-positive",
                    "--control-connected-background-default",
                ),
            };
            let on = state == current;
            div()
                .id(SharedString::from(format!("perm-{flag}-{state}")))
                .w(px(31.))
                .h(px(26.))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .bg(if on && state != 1 {
                    color(background)
                } else if on {
                    color("--background-mod-strong")
                } else {
                    color("--background-base-low")
                })
                .child(icon(
                    glyph,
                    16.,
                    if on && state != 1 { "--white" } else { tint },
                ))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let Some(settings) = this.channel_settings.as_mut() else {
                        return;
                    };
                    let (mut allow, mut deny) = (allow, deny);
                    allow &= !flag;
                    deny &= !flag;
                    match state {
                        0 => deny |= flag,
                        2 => allow |= flag,
                        _ => {}
                    }
                    settings.perm_edits.insert(selected, (allow, deny, kind_of));
                    let _ = channel_id;
                    cx.notify();
                }))
        };
        let form =
            div()
                .flex_1()
                .min_w(px(0.))
                .pt(px(24.))
                .pr(px(15.))
                .flex()
                .flex_col()
                .children(sections.iter().map(|section| {
                    let count = section.permissions.len();
                    div()
                        .mb(px(40.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(12.))
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(section.title),
                        )
                        .children(section.description.map(|description| {
                            div()
                                .mb(px(12.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-subtle"))
                                .child(description)
                        }))
                        .children(section.permissions.iter().enumerate().map(
                            |(index, permission)| {
                                let current = if deny & permission.flag != 0 {
                                    0
                                } else if allow & permission.flag != 0 {
                                    2
                                } else {
                                    1
                                };
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .gap(px(24.))
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
                                                            .child(permission.title),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(14.))
                                                            .line_height(px(18.))
                                                            .text_color(color("--text-subtle"))
                                                            .child(if everyone {
                                                                permission.everyone_description
                                                            } else {
                                                                permission.description
                                                            }),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .h(px(28.))
                                                    .rounded(px(8.))
                                                    .border_1()
                                                    .border_color(color("--border-muted"))
                                                    .overflow_hidden()
                                                    .flex()
                                                    .child(state_button(
                                                        permission.flag,
                                                        0,
                                                        current,
                                                    ))
                                                    .child(state_button(
                                                        permission.flag,
                                                        1,
                                                        current,
                                                    ))
                                                    .child(state_button(
                                                        permission.flag,
                                                        2,
                                                        current,
                                                    )),
                                            ),
                                    )
                                    .when(index + 1 < count, |this| {
                                        this.child(
                                            div()
                                                .my(px(24.))
                                                .h(px(1.))
                                                .bg(color("--border-subtle")),
                                        )
                                    })
                            },
                        ))
                }));
        div()
            .flex()
            .flex_col()
            .child(header)
            .child(div().flex().child(list).child(form))
            .into_any_element()
    }
}

/// `handleDeletePermission` of module 336541 behind module 495603's Remove: "Delete Permission Settings", the
/// `xERCnZ` sentence with the bold name, Cancel and Okay (module 579872 `show`).
pub(super) fn remove_permission_alert(
    channel: Id,
    target: Id,
    name: &str,
) -> super::chat_notices::Alert {
    use super::chat_notices::{Alert, Then};
    let mut alert = Alert::show(
        "delete-permission-settings",
        dc_intl::t("GuPYQB"),
        None,
        Some(dc_intl::t("ETE/oC").to_owned()),
        None,
        Then::ClearOverwrite { channel, target },
    );
    alert.subtitle = Some(
        dc_intl::format_runs("xERCnZ", &[("name", name.into())])
            .into_iter()
            .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
            .collect(),
    );
    alert
}

impl Root {
    /// Remove User / Remove Role of module 495603: the confirm named after the row.
    pub(super) fn confirm_remove_permission(
        &mut self,
        channel: Id,
        target: Id,
        cx: &mut Context<Self>,
    ) {
        let Some(guild) = self.guild_of_channel(channel) else {
            return;
        };
        let name = self
            .stores
            .guild(guild)
            .and_then(|known| known.roles.get(&target).cloned())
            .or_else(|| {
                self.stores
                    .users
                    .get(&target)
                    .map(|user| user.display_name().to_owned())
            })
            .unwrap_or_default();
        self.chat_extra.alert = Some(remove_permission_alert(channel, target, &name));
        cx.notify();
    }

    /// "Sync permissions?" (module 544169): the channel takes its category's overwrites
    /// (`PATCH /channels/{id}` with `permission_overwrites`).
    pub(super) fn sync_permissions_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.channel_settings.as_ref()?;
        if !open.confirm_sync {
            return None;
        }
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == open.channel))?;
        let channel = guild
            .channels
            .iter()
            .find(|known| known.id == open.channel)?
            .clone();
        let parent = guild
            .channels
            .iter()
            .find(|known| Some(known.id) == channel.parent_id)?
            .clone();
        let overwrites: Vec<serde_json::Value> = parent
            .overwrites
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "id": entry.id.to_string(),
                    "type": entry.kind,
                    "allow": entry.allow.to_string(),
                    "deny": entry.deny.to_string(),
                })
            })
            .collect();
        let id = channel.id;
        Some(super::modal::render_small_stacked(
            super::modal::Spec {
                id: "sync-permissions-modal",
                title: "Sync permissions?".into(),
                subtitle: Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .child("Would you like to sync\u{a0}")
                        .child(
                            div()
                                .font_weight(FontWeight(600.))
                                .child(format!("#{}", channel.name)),
                        )
                        .child("\u{a0}permissions with\u{a0}")
                        .child(
                            div()
                                .font_weight(FontWeight(600.))
                                .child(parent.name.clone()),
                        )
                        .child("?")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "sync-permissions-keep",
                        "Keep Current Permissions",
                        super::modal::Tone::Secondary,
                        |this, _, _| {
                            if let Some(settings) = this.channel_settings.as_mut() {
                                settings.confirm_sync = false;
                            }
                        },
                    ),
                    super::modal::button(
                        "sync-permissions-confirm",
                        "Sync Permissions",
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::PATCH,
                                route: format!("/channels/{id}"),
                                body: Some(
                                    serde_json::json!({"permission_overwrites": overwrites}),
                                ),
                            });
                            if let Some(settings) = this.channel_settings.as_mut() {
                                settings.confirm_sync = false;
                                settings.perm_edits.clear();
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| {
                    if let Some(settings) = this.channel_settings.as_mut() {
                        settings.confirm_sync = false;
                    }
                }),
            },
            cx,
        ))
    }
}

impl Root {
    fn toggle_add_target(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let Some(settings) = self.channel_settings.as_mut() else {
            return;
        };
        if settings.add_target.take().is_some() {
            cx.notify();
            return;
        }
        let field = cx.new(|cx| {
            let mut editor = crate::editor::Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Role/Member".to_owned());
            editor
        });
        let events = cx.subscribe(&field, |this, _, event: &crate::editor::EditorEvent, cx| {
            if matches!(event, crate::editor::EditorEvent::Cancel)
                && let Some(settings) = this.channel_settings.as_mut()
            {
                settings.add_target = None;
            }
            cx.notify();
        });
        let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
        window.focus(&handle, cx);
        if let Some(settings) = self.channel_settings.as_mut() {
            settings.add_target = Some((field, events));
        }
        cx.notify();
    }

    /// Roles without an overwrite here, then members this client knows in the server, matching the search.
    fn add_target_candidates(&self, query: &str) -> Vec<(Id, u64, String, Option<User>)> {
        let Some(settings) = self.channel_settings.as_ref() else {
            return Vec::new();
        };
        let Some(guild) = self.stores.guilds.iter().find(|guild| {
            guild
                .channels
                .iter()
                .any(|known| known.id == settings.channel)
        }) else {
            return Vec::new();
        };
        let Some(channel) = guild
            .channels
            .iter()
            .find(|known| known.id == settings.channel)
        else {
            return Vec::new();
        };
        let taken = |id: Id| {
            channel.overwrites.iter().any(|entry| entry.id == id)
                || settings.perm_edits.contains_key(&id)
        };
        let query = query.trim().to_lowercase();
        let mut out: Vec<(Id, u64, String, Option<User>)> = guild
            .roles
            .iter()
            .filter(|(id, _)| **id != guild.id && !taken(**id))
            .filter(|(_, name)| name.to_lowercase().contains(&query))
            .map(|(id, name)| (*id, 0, name.clone(), None))
            .collect();
        out.sort_by(|a, b| a.2.cmp(&b.2));
        let mut members: Vec<User> = self.stores.current_user.iter().cloned().collect();
        if let Some(list) = self.stores.member_lists.get(&guild.id) {
            for row in &list.rows {
                if let dc_state::member_list::Row::Member { user, .. } = row
                    && !members.iter().any(|known| known.id == user.id)
                {
                    members.push(User::clone(user));
                }
            }
        }
        for user in members {
            if taken(user.id) {
                continue;
            }
            if user.display_name().to_lowercase().contains(&query)
                || user.username.to_lowercase().contains(&query)
            {
                out.push((user.id, 1, user.display_name().to_owned(), Some(user)));
            }
        }
        out
    }

    pub(super) fn add_target_popout(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let settings = self.channel_settings.as_ref()?;
        let (field, _) = settings.add_target.as_ref()?;
        let anchor = self
            .tip_anchors
            .borrow()
            .get("permissions-add-target")
            .copied()?;
        let query = field.read(cx).text().to_owned();
        let rows = self
            .add_target_candidates(&query)
            .into_iter()
            .map(|(id, kind, name, user)| {
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                div()
                    .id(SharedString::from(format!("permissions-add-{id}")))
                    .flex_none()
                    .h(px(40.))
                    .rounded(px(4.))
                    .pl(px(8.))
                    .cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(user.is_some(), |this| {
                        this.child(div().size(px(32.)).rounded_full().children(
                            picture.map(|picture| img(picture).size(px(32.)).rounded_full()),
                        ))
                        .child(div().w(px(16.)))
                    })
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(name),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(settings) = this.channel_settings.as_mut() {
                            settings.perm_edits.insert(id, (0, 0, kind));
                            settings.perm_target = Some(id);
                            settings.add_target = None;
                        }
                        cx.notify();
                    }))
            });
        // The popout's left is the list's, 8px under the header row.
        let left = anchor.left() - px(183.);
        Some(
            gpui::deferred(
                div()
                    .parity("permissions-add")
                    .id("permissions-add-popout")
                    .occlude()
                    .absolute()
                    .left(left)
                    .top(anchor.bottom() + px(14.))
                    .w(px(250.))
                    .rounded(px(8.))
                    .bg(color("--background-base-lower"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.24),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down_out(cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                        let on_plus = this
                            .tip_anchors
                            .borrow()
                            .get("permissions-add-target")
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !on_plus && let Some(settings) = this.channel_settings.as_mut() {
                            settings.add_target = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .pt(px(12.))
                            .px(px(12.))
                            .pb(px(8.))
                            .child(
                                div()
                                    .mb(px(8.))
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-strong"))
                                    .child("Add:"),
                            )
                            .child(
                                div()
                                    .h(px(40.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-active"))
                                    .bg(color("--input-background-default"))
                                    .pl(px(12.))
                                    .pr(px(8.))
                                    .flex()
                                    .items_center()
                                    .child(div().flex_1().min_w(px(0.)).child(field.clone())),
                            ),
                    )
                    .child(div().p(px(4.)).flex().flex_col().children(rows)),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{expires_in, incident_body, lifts_lockdown};

    /// Module 671576: the feedback of module 336496 follows only a save that lifts every pause that was on.
    #[test]
    fn removing_a_permission_row_asks_with_its_bold_name() {
        use super::remove_permission_alert;
        let alert = remove_permission_alert(1, 2, "Mods");
        assert_eq!(alert.title, "Delete Permission Settings");
        let subtitle = alert.subtitle.unwrap();
        assert!(subtitle.iter().any(|(text, bold)| *bold && text == "Mods"));
        let labels: Vec<&str> = alert
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        assert_eq!(labels, ["Cancel", "Okay"]);
        assert!(
            !super::super::context_menu::permission_target_items(true, true, false)
                .iter()
                .any(|_| true)
        );
    }

    #[test]
    fn under_lockdown_means_a_pause_still_runs() {
        use super::under_lockdown;
        let later = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        let earlier = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        assert!(under_lockdown(Some(&later), None));
        assert!(under_lockdown(None, Some(&later)));
        assert!(!under_lockdown(Some(&earlier), None));
        assert!(!under_lockdown(None, None));
    }

    #[test]
    fn lifting_every_pause_asks_why_the_server_was_locked() {
        assert!(lifts_lockdown((2, true, false), false, false));
        assert!(lifts_lockdown((2, true, true), false, false));
        assert!(!lifts_lockdown((2, true, true), false, true));
        assert!(!lifts_lockdown((2, false, false), false, false));
        assert!(!lifts_lockdown((2, false, false), true, false));
    }

    #[test]
    fn security_actions_put_both_times_or_none() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-08T05:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let body = incident_body(true, false, 2, now);
        assert_eq!(body["invites_disabled_until"], "2026-11-08T07:00:00.000Z");
        assert!(body["dms_disabled_until"].is_null());
        assert_eq!(body["lockdown_duration_hours"], 2);
        let off = incident_body(false, false, 2, now);
        assert!(off["lockdown_duration_hours"].is_null());
    }

    #[test]
    fn invite_countdowns_read_like_the_table() {
        let created = 0;
        assert_eq!(expires_in(created, 86_400, 3_600_000 + 1_000), "22:59:59");
        assert_eq!(expires_in(created, 30 * 86_400, 0), "30:00:00:00");
        assert_eq!(expires_in(created, 0, 0), "\u{221e}");
    }
}
