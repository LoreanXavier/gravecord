//! Channel-linked game lobby rows & unlink modals (modules 422542 `eo`, 106895, 428185, 514810).
//!
//! Renders linked-channel rows inside the Server Settings & Channel Settings Invites tables
//! (`Invite Source` header `l7VISQ`, app icon + name, `N/A` uses `jHpxwo`, `Never` expires `PqEzn8`),
//! the `Edit Channel Link` modal (`106895`: `OJknhi`, `0NJZAU`, `LLWaxQ`, `514810` `xPrJ+q`/`EyygeM`),
//! the `Remove Channel Link` confirmation (`428185`: `JmUENg`, `6l2osp`, `cY+Oob`, `6D5WVg`, sending
//! `DELETE /channels/{channel_id}/linked-lobby` per `435183`), and the missing-permission alert (`SrvsML`).

use super::{Root, color, icon};
use crate::net::Request;
use dc_intl::Arg;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, deferred, div, img, prelude::*, px,
    rgba,
};
use serde_json::Value;
use std::borrow::Cow;

pub(crate) const UNLINK_LOBBY_KEY: &str = "channel:unlink-lobby";
pub(crate) const LINKED_LOBBIES_PREFIX: &str = "linked-lobbies:";

#[derive(Clone, Debug)]
pub(crate) struct LinkedChannelRow {
    pub guild_id: Id,
    pub channel_id: Id,
    pub channel_name: String,
    pub application_id: String,
    pub application_name: String,
    pub application_icon_url: Option<String>,
    pub linked_by_name: Option<String>,
    pub linked_by_avatar_url: Option<String>,
    pub linked_at_medium: Option<String>,
    pub can_unlink: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum ChannelLinkModal {
    /// Module `106895`: `Edit Channel Link`.
    Edit(LinkedChannelRow),
    /// Module `428185`: `Remove Channel Link` confirm.
    ConfirmRemove {
        guild_id: Id,
        channel_id: Id,
        application_name: String,
        busy: bool,
        error: Option<String>,
    },
    /// Module `422542` `eo`: alert when viewer lacks `MANAGE_CHANNELS` / unlink permission.
    NoPermission,
}

/// Official `CHANNEL_LINKED_LOBBY` DELETE request (`435183` `removeLinkedLobby`, `652215`).
pub(crate) fn remove_linked_lobby_request(channel_id: Id) -> Request {
    Request::Rest {
        key: Some(UNLINK_LOBBY_KEY.to_owned()),
        method: reqwest::Method::DELETE,
        route: format!("/channels/{channel_id}/linked-lobby"),
        body: None,
    }
}

/// Formats `514810` (`N`): `Added by {username} on {linkedAtDate}` (`xPrJ+q`) or `Added on {linkedAtDate}` (`EyygeM`).
pub(crate) fn linked_metadata_line(
    linked_by_name: Option<&str>,
    linked_at_medium: Option<&str>,
) -> Option<String> {
    let date = linked_at_medium?;
    if let Some(username) = linked_by_name.filter(|s| !s.is_empty()) {
        Some(dc_intl::format(
            "xPrJ+q",
            &[
                ("username", Arg::Str(Cow::Borrowed(username))),
                (
                    "linkedAtDate",
                    Arg::When {
                        date: Cow::Borrowed(date),
                        time: Cow::Borrowed(""),
                    },
                ),
            ],
        ))
    } else {
        Some(dc_intl::format(
            "EyygeM",
            &[(
                "linkedAtDate",
                Arg::When {
                    date: Cow::Borrowed(date),
                    time: Cow::Borrowed(""),
                },
            )],
        ))
    }
}

impl Root {
    /// Linked channels for a guild (optionally filtered to a single channel), used by `422542` `ec`.
    pub(in crate::ui) fn linked_channels_for_guild(
        &self,
        guild: Id,
        only_channel: Option<Id>,
    ) -> Vec<LinkedChannelRow> {
        let Some(entries) = self
            .fetched
            .get(&format!("{LINKED_LOBBIES_PREFIX}{guild}"))
            .and_then(|v| {
                v.as_array()
                    .or_else(|| v.get("body").and_then(Value::as_array))
            })
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in entries {
            let Some(ch_id) = item["channel_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            if only_channel.is_some_and(|only| only != ch_id) {
                continue;
            }
            let channel_name = item["channel_name"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    self.stores
                        .guild(guild)
                        .and_then(|g| g.channels.iter().find(|c| c.id == ch_id))
                        .map(|c| c.name.clone())
                })
                .unwrap_or_else(|| "general".to_owned());
            let app_id = item["application_id"].as_str().unwrap_or("0").to_owned();
            let app_name = item["application_name"]
                .as_str()
                .unwrap_or("Linked App")
                .to_owned();
            let app_icon_url = item["application_icon"].as_str().map(|hash| {
                format!("https://cdn.discordapp.com/app-icons/{app_id}/{hash}.webp?size=64")
            });
            let linked_by_name = item["linked_by_name"].as_str().map(str::to_owned);
            let linked_by_avatar_url = item["linked_by_avatar_url"].as_str().map(str::to_owned);
            let linked_at_medium = item["linked_at_medium"].as_str().map(str::to_owned);
            let can_unlink = item["can_unlink"].as_bool().unwrap_or(true);
            out.push(LinkedChannelRow {
                guild_id: guild,
                channel_id: ch_id,
                channel_name,
                application_id: app_id,
                application_name: app_name,
                application_icon_url: app_icon_url,
                linked_by_name,
                linked_by_avatar_url,
                linked_at_medium,
                can_unlink,
            });
        }
        out
    }

    /// Renders a linked-channel row in the Invites table (`eo` of module 422542).
    pub(in crate::ui) fn linked_channel_invite_row(
        &self,
        row: LinkedChannelRow,
        show_channel: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let group = SharedString::from(format!(
            "linked-channel-row-{}-{}",
            row.channel_id, row.application_id
        ));
        let inviter_name = row
            .linked_by_name
            .clone()
            .unwrap_or_else(|| dc_intl::t("jHpxwo").to_owned());
        let inviter_pic = row
            .linked_by_avatar_url
            .as_deref()
            .and_then(|url| self.images.get(url).cloned());
        let app_pic = row
            .application_icon_url
            .as_deref()
            .and_then(|url| self.images.get(url).cloned());
        let app_name = row.application_name.clone();
        let ch_name = format!("#{}", row.channel_name);
        let row_click = row.clone();
        let row_id = SharedString::from(format!("linked-channel-unlink-{}", row.channel_id));

        let mono = |text: &str| {
            div()
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color("--text-muted"))
                .child(text.to_owned())
        };

        div()
            .group(group.clone())
            .relative()
            .h(px(62.))
            .border_b_1()
            .border_color(color("--border-subtle"))
            .flex()
            .gap(px(20.))
            .items_center()
            .child(
                div()
                    .w(px(190.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .size(px(24.))
                            .flex_none()
                            .rounded_full()
                            .bg(color("--background-mod-strong"))
                            .children(
                                inviter_pic
                                    .map(|picture| img(picture).size(px(24.)).rounded_full()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .text_color(color("--text-default"))
                                    .child(inviter_name),
                            )
                            .when(show_channel, |col| {
                                col.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(ch_name),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .w(px(142.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .rounded(px(4.))
                            .bg(color("--control-primary-background-default"))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(app_pic, |b, pic| b.child(img(pic).size(px(20.))))
                            .when(row.application_icon_url.is_none(), |b| {
                                b.child(icon("bot_game_controller", 12., "--white"))
                            }),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-default"))
                            .child(app_name),
                    ),
            )
            .child(
                div()
                    .w(px(48.))
                    .flex_none()
                    .child(mono(dc_intl::t("jHpxwo"))),
            )
            .child(
                div()
                    .w(px(95.))
                    .flex_none()
                    .child(mono(dc_intl::t("PqEzn8"))),
            )
            .child(div().w(px(95.)).flex_none())
            .child(
                div()
                    .id(row_id)
                    .absolute()
                    .right(px(-31.))
                    .top(px(23.))
                    .size(px(16.))
                    .cursor_pointer()
                    .opacity(0.6)
                    .group_hover(group, |style| style.opacity(1.))
                    .child(icon("modal_close", 16., "--icon-subtle"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_channel_link_modal(row_click.clone(), cx);
                    })),
            )
            .into_any_element()
    }

    pub(in crate::ui) fn open_channel_link_modal(
        &mut self,
        row: LinkedChannelRow,
        cx: &mut Context<Self>,
    ) {
        if row.can_unlink {
            self.channel_link_modal = Some(ChannelLinkModal::Edit(row));
        } else {
            self.channel_link_modal = Some(ChannelLinkModal::NoPermission);
        }
        cx.notify();
    }

    pub(in crate::ui) fn close_channel_link_modal(&mut self) -> bool {
        self.channel_link_modal.take().is_some()
    }

    pub(in crate::ui) fn channel_link_fetched(&mut self, body: &Value) {
        let Some(ChannelLinkModal::ConfirmRemove {
            guild_id,
            channel_id,
            busy,
            error,
            ..
        }) = self.channel_link_modal.as_mut()
        else {
            return;
        };
        *busy = false;
        if body["ok"] == true {
            let gid = *guild_id;
            let cid = channel_id.to_string();
            if let Some(v) = self
                .fetched
                .get_mut(&format!("{LINKED_LOBBIES_PREFIX}{gid}"))
            {
                let list = if v.is_array() {
                    v.as_array_mut()
                } else {
                    v.get_mut("body").and_then(Value::as_array_mut)
                };
                if let Some(list) = list {
                    list.retain(|entry| entry["channel_id"].as_str() != Some(cid.as_str()));
                }
            }
            self.channel_link_modal = None;
        } else {
            *error = Some(dc_intl::t("6D5WVg").to_owned());
        }
    }

    /// Renders `106895` (`Edit Channel Link`), `428185` (`Remove Channel Link` confirm), or `422542` no-permission alert.
    pub(in crate::ui) fn channel_link_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.channel_link_modal.clone()?;
        let (title, body, footer): (String, AnyElement, AnyElement) = match modal {
            ChannelLinkModal::Edit(row) => {
                let title = dc_intl::t("OJknhi").to_owned();
                let ch_label = format!("#{}", row.channel_name);
                let linked_to = dc_intl::format(
                    "0NJZAU",
                    &[("channelName", Arg::Str(Cow::Borrowed(&ch_label)))],
                );
                let meta = linked_metadata_line(
                    row.linked_by_name.as_deref(),
                    row.linked_at_medium.as_deref(),
                );
                let app_pic = row
                    .application_icon_url
                    .as_deref()
                    .and_then(|url| self.images.get(url).cloned());
                let gid = row.guild_id;
                let cid = row.channel_id;
                let app_name = row.application_name.clone();
                let body = div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(56.))
                            .flex_none()
                            .rounded(px(12.))
                            .bg(color("--control-primary-background-default"))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(app_pic, |b, pic| b.child(img(pic).size(px(56.))))
                            .when(row.application_icon_url.is_none(), |b| {
                                b.child(icon("bot_game_controller", 28., "--white"))
                            }),
                    )
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
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(row.application_name),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-muted"))
                                    .child(linked_to),
                            )
                            .when_some(meta, |col, line| {
                                col.child(
                                    div()
                                        .my(px(8.))
                                        .h(px(1.))
                                        .w_full()
                                        .bg(color("--border-subtle")),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-muted"))
                                        .child(line),
                                )
                            }),
                    )
                    .into_any_element();
                let footer = div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("channel-link-cancel")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("ETE/oC"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_channel_link_modal();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("channel-link-remove")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-critical-primary-background-default"))
                            .hover(|s| s.bg(color("--control-critical-primary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("LLWaxQ"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.channel_link_modal = Some(ChannelLinkModal::ConfirmRemove {
                                    guild_id: gid,
                                    channel_id: cid,
                                    application_name: app_name.clone(),
                                    busy: false,
                                    error: None,
                                });
                                cx.notify();
                            })),
                    )
                    .into_any_element();
                (title, body, footer)
            }
            ChannelLinkModal::ConfirmRemove {
                channel_id,
                application_name,
                busy,
                error,
                ..
            } => {
                let title = dc_intl::t("JmUENg").to_owned();
                let subtitle = dc_intl::format(
                    "6l2osp",
                    &[(
                        "applicationName",
                        Arg::Str(Cow::Borrowed(&application_name)),
                    )],
                );
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(subtitle),
                    )
                    .when_some(error, |col, msg| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(rgba(0xf23f421f))
                                .border_1()
                                .border_color(color("--text-feedback-critical"))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(color("--text-feedback-critical"))
                                .child(msg),
                        )
                    })
                    .into_any_element();
                let footer = div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("channel-unlink-cancel")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .hover(|s| s.bg(color("--control-secondary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("ETE/oC"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_channel_link_modal();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("channel-unlink-confirm")
                            .h(px(38.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .when(busy, |b| b.opacity(0.5))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("cY+Oob"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(ChannelLinkModal::ConfirmRemove {
                                    busy, error, ..
                                }) = this.channel_link_modal.as_mut()
                                {
                                    if *busy {
                                        return;
                                    }
                                    *busy = true;
                                    *error = None;
                                }
                                let _ = this.requests.send(remove_linked_lobby_request(channel_id));
                                cx.notify();
                            })),
                    )
                    .into_any_element();
                (title, body, footer)
            }
            ChannelLinkModal::NoPermission => {
                let title = dc_intl::t("JmUENg").to_owned();
                let body = div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(dc_intl::t("SrvsML"))
                    .into_any_element();
                let footer = div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        div()
                            .id("channel-unlink-noperm-ok")
                            .h(px(38.))
                            .px(px(20.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .child(dc_intl::t("BddRzS"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_channel_link_modal();
                                cx.notify();
                            })),
                    )
                    .into_any_element();
                (title, body, footer)
            }
        };

        let dialog = div()
            .id("channel-link-modal")
            .occlude()
            .w(px(440.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.28),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(SharedString::from(title)),
                    )
                    .child(
                        div()
                            .id("channel-link-x")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 18., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_channel_link_modal();
                                cx.notify();
                            })),
                    ),
            )
            .child(div().px(px(24.)).pb(px(20.)).child(body))
            .child(
                div()
                    .px(px(24.))
                    .py(px(16.))
                    .bg(color("--background-base-lower"))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .rounded_b(px(12.))
                    .child(footer),
            );

        Some(
            deferred(
                div()
                    .id("channel-link-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_channel_link_modal();
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
    #[cfg(feature = "capture")]
    pub(crate) fn script_channel_link(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let guild_id = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|g| g.id))
            .unwrap_or(1568692564637524109);
        let channel_id = self.channel.unwrap_or(1568692565136375900);
        let row = LinkedChannelRow {
            guild_id,
            channel_id,
            channel_name: "general".to_owned(),
            application_id: "99001122".to_owned(),
            application_name: "Supervive Lobby".to_owned(),
            application_icon_url: None,
            linked_by_name: Some("onugeccen".to_owned()),
            linked_by_avatar_url: None,
            linked_at_medium: Some("Nov 20, 2026".to_owned()),
            can_unlink: arg != "noperm",
        };
        match arg {
            "confirm" => {
                self.channel_link_modal = Some(ChannelLinkModal::ConfirmRemove {
                    guild_id,
                    channel_id,
                    application_name: row.application_name,
                    busy: false,
                    error: None,
                });
            }
            "noperm" => {
                self.channel_link_modal = Some(ChannelLinkModal::NoPermission);
            }
            _ => {
                self.channel_link_modal = Some(ChannelLinkModal::Edit(row));
            }
        }
        cx.notify();
        format!("channel-link: {arg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlink_lobby_request_and_514810_metadata_match_spec() {
        let Request::Rest {
            key,
            method,
            route,
            body,
        } = remove_linked_lobby_request(555)
        else {
            panic!("expected rest");
        };
        assert_eq!(key.as_deref(), Some(UNLINK_LOBBY_KEY));
        assert_eq!(method, reqwest::Method::DELETE);
        assert_eq!(route, "/channels/555/linked-lobby");
        assert!(body.is_none());

        let with_user = linked_metadata_line(Some("alice"), Some("Nov 20, 2026")).unwrap();
        assert!(with_user.contains("alice"));
        assert!(with_user.contains("Nov 20, 2026"));

        let anon = linked_metadata_line(None, Some("Nov 20, 2026")).unwrap();
        assert!(anon.contains("Nov 20, 2026"));
        assert!(!anon.contains("alice"));
    }
}
