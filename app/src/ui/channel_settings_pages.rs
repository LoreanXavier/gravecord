//! The other channel settings pages, measured in `design/dom/settings/channel-{permissions,invites,integrations}-*`:
//! Permissions (synced with the category, the Private Channel switch), Invites (the channel's invite table with
//! revoke and "create one") and Integrations (webhooks and followed channels).

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_state::permissions::{Overwrite, VIEW_CHANNEL};
use dc_state::{Channel, Id, User};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// Module 824953: a new webhook is one of these, at random.
const WEBHOOK_NAMES: [&str; 2] = ["Spidey Bot", "Captain Hook"];

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

impl Root {
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
                    .into_any_element(),
            ))
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
        let rows = invites.into_iter().map(|invite| {
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
                .child(column(162.).child(mono(code.clone())))
                .child(column(68.).child(mono(if max > 0 {
                    format!("{uses}/{max}")
                } else {
                    uses.to_string()
                })))
                .child(column(115.).child(mono(left)))
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
        });
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
            .child(div().mb(px(20.)).h(px(1.)).bg(color("--border-subtle")))
            .child(
                div()
                    .flex()
                    .child(head("Inviter", 203.))
                    .child(head("Invite Code", 163.))
                    .child(head("Uses", 71.))
                    .child(head("Expires", 121.))
                    .child(head("Roles", 102.)),
            )
            .children(rows)
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

#[cfg(test)]
mod tests {
    use super::expires_in;

    #[test]
    fn invite_countdowns_read_like_the_table() {
        let created = 0;
        assert_eq!(expires_in(created, 86_400, 3_600_000 + 1_000), "22:59:59");
        assert_eq!(expires_in(created, 30 * 86_400, 0), "30:00:00:00");
        assert_eq!(expires_in(created, 0, 0), "\u{221e}");
    }
}
