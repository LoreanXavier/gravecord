//! The servers this account belongs to that are not available in its region (GeoRestrictedGuildStore, module
//! 543897) and the "Server Unavailable" dialog (module 537560).
//!
//! The rail draws them after the server tree (`d` of `iX` in module 396655, before Add a Server): `tJ`, the server's
//! icon (or acronym) in the rail's blob with a lower badge holding the exclamation mark of module 791606
//! (`geoRestrictedBadge_1czW-w`: `--status-warning-background` behind `--status-warning-text`, the 16px round icon
//! badge of module 812993 `fk` with its 2px padding), the server's name as its tooltip. A click opens module 537560 on
//! the dialog of module 189213: "Server Unavailable", "{serverName} is not available in your region." with the name
//! bold, Leave Server (`secondary`, `leaveGuild`) and OK.

use super::chat_notices::{Alert, AlertButton, Then};
use super::modal::Tone;
use super::tooltip::{Body, Tip};
use super::{Root, color, icon};
use dc_state::{GeoRestrictedGuild, Id};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

/// Module 537560: the dialog for one geo-restricted server.
pub(super) fn unavailable_alert(guild: Id, name: &str) -> Alert {
    Alert {
        id: "server-unavailable",
        title: dc_intl::t("aCAiGl").to_owned(),
        subtitle: Some(
            dc_intl::format_runs("4cJV9S", &[("serverName", name.into())])
                .into_iter()
                .map(|(text, tag)| (text, tag.as_deref() == Some("$b")))
                .collect(),
        ),
        content: None,
        buttons: vec![
            AlertButton {
                id: "server-unavailable-leave",
                label: dc_intl::t("J2TBi3").to_owned(),
                tone: Tone::Secondary,
                then: Then::LeaveGuild { guild },
            },
            AlertButton {
                id: "server-unavailable-ok",
                label: dc_intl::t("TyCVIq").to_owned(),
                tone: Tone::Primary,
                then: Then::Close,
            },
        ],
    }
}

fn image_key(guild: Id) -> String {
    format!("guild:{guild}")
}

impl Root {
    /// Asks for the icons of the geo-restricted servers (`getGuildIconURL` with `size: 42`, never animated).
    pub(super) fn request_geo_restricted_icons(&mut self) {
        let wanted: Vec<(Id, String)> = self
            .stores
            .geo_restricted_guilds
            .iter()
            .filter_map(|guild| Some((guild.id, guild.icon.clone()?)))
            .collect();
        for (id, hash) in wanted {
            self.request_image(
                image_key(id),
                format!("https://cdn.discordapp.com/icons/{id}/{hash}.webp?size=42"),
            );
        }
    }

    /// `tJ` of module 396655 for each geo-restricted server.
    pub(super) fn geo_restricted_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.stores
            .geo_restricted_guilds
            .iter()
            .map(|guild| self.geo_restricted_tile(guild, cx))
            .collect()
    }

    fn geo_restricted_tile(
        &self,
        guild: &GeoRestrictedGuild,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = guild.id;
        let name = guild.name.clone();
        let row_key = format!("geo-guild-{id}");
        let selected = false;
        let tile = div()
            .id(SharedString::from(row_key.clone()))
            .size(px(40.))
            .rounded(px(12.))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.chat_extra.alert = Some(unavailable_alert(id, &name));
                cx.notify();
            }));
        let tile = match self.images.get(&image_key(id)).cloned() {
            Some(image) => tile.child(img(image).size(px(40.)).rounded(px(12.))),
            None => tile
                .bg(super::motion::rail::tile_background(
                    &row_key,
                    selected,
                    "--background-mod-subtle",
                ))
                .text_size(px(18.))
                .line_height(px(21.6))
                .font_weight(FontWeight(500.))
                .text_color(super::motion::rail::tile_foreground(&row_key, selected))
                .child(dc_state::Guild::acronym_of(&guild.name)),
        };
        div()
            .relative()
            .w(px(72.))
            .h(px(40.))
            .flex_none()
            .flex()
            .justify_center()
            .child(super::motion::rail::pill(&row_key, false, false))
            .child(super::motion::hover_probe(super::motion::rail::hover_key(
                &row_key,
            )))
            .child(self.with_tip(
                row_key.clone(),
                Tip::rail(Body::Heading(guild.name.clone().into())),
                tile,
                cx,
            ))
            // The lower badge in its cutout, where the rail puts the mention count.
            .child(
                div()
                    .absolute()
                    .right(px(14.))
                    .bottom(px(-2.))
                    .size(px(20.))
                    .rounded_full()
                    .bg(color("--background-base-lowest"))
                    .child(
                        div()
                            .absolute()
                            .left(px(2.))
                            .top(px(2.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(color("--status-warning-background"))
                            .p(px(2.))
                            .child(icon("geo_restricted", 12., "--status-warning-text")),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(feature = "capture")]
impl Root {
    /// Capture only: `standin <name> [args]` opens what the fixture account cannot reach on its own.
    /// - `follow-success`: module 505859 as a follow's success opens it.
    /// - `geo <id> <name>`: a geo-restricted server in the rail (a local `GUILD_DELETE` with `geo_restricted`).
    /// - `geo-unavailable`: module 537560 for the first geo-restricted server.
    pub(crate) fn script_standin(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        let line = match args {
            ["follow-success"] => {
                self.open_follow_success();
                "standin: follow success".to_owned()
            }
            ["geo", id, name @ ..] => {
                let data = serde_json::json!({
                    "id": id, "unavailable": true, "geo_restricted": true, "name": name.join(" "),
                });
                self.stores.dispatch("GUILD_DELETE", &data);
                format!(
                    "standin: {} geo-restricted, {} tiles",
                    self.stores.geo_restricted_guilds.len(),
                    self.geo_restricted_tiles(cx).len()
                )
            }
            ["geo-unavailable"] => match self.stores.geo_restricted_guilds.first() {
                Some(guild) => {
                    self.chat_extra.alert = Some(unavailable_alert(guild.id, &guild.name));
                    "standin: server unavailable".to_owned()
                }
                None => "standin: no geo-restricted server".to_owned(),
            },
            _ => self.script_standin_more(args, cx),
        };
        cx.notify();
        line
    }

    fn script_standin_more(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        match args {
            // A server the account only lurks (module 30492 `isLurking`): the open one is marked lurked here, locally.
            ["lurk"] => match self.guild {
                Some(guild) => {
                    self.stores.lurking_guilds.insert(guild);
                    "standin: lurking the open server".to_owned()
                }
                None => "standin: no server open".to_owned(),
            },
            // Module 688796 as the cropper's GIF avatar without Nitro opens it (`avatar` or `banner`).
            ["animated-upsell", which] => {
                self.animated_upload_upsell = Some(*which == "banner");
                "standin: animated upload upsell".to_owned()
            }
            // Module 716480 as Add a Server opens it at the limit (the fixture account is far from 100 servers).
            ["guild-cap"] => {
                self.open_guild_cap_upsell(cx);
                format!(
                    "standin: guild cap upsell, {} servers",
                    self.stores.guilds.len()
                )
            }
            // Module 633605 as the new server welcome's Add your first app opens it (the fixture server is older
            // than 30 days, so its welcome has no cards).
            ["app-directory-promo"] => match self.guild {
                Some(guild) => {
                    self.open_app_directory_promo(guild, cx);
                    "standin: app directory promo".to_owned()
                }
                None => "standin: no server in view".to_owned(),
            },
            // Module 455796 as a `GUILD_MEMBER_UPDATE` with `COMPLETED_HOME_ACTIONS` opens it (the fixture server has
            // no guide).
            ["guide-celebration", count] => {
                let count = count.parse().unwrap_or(3);
                self.open_guide_celebration(count, cx);
                "standin: guide celebration".to_owned()
            }
            // The fixture server as a Community server under a pause, locally (Security Actions items, module 664967).
            ["community", rest @ ..] => {
                let Some(guild) = self.guild else {
                    return "standin: no server in view".to_owned();
                };
                let lockdown = rest.first() == Some(&"lockdown");
                if let Some(known) = self
                    .stores
                    .guilds
                    .iter_mut()
                    .find(|known| known.id == guild)
                {
                    if !known.features.iter().any(|feature| feature == "COMMUNITY") {
                        known.features.push("COMMUNITY".to_owned());
                    }
                    if lockdown {
                        known.invites_disabled_until =
                            Some((chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339());
                    }
                }
                cx.notify();
                format!("standin: community, lockdown {lockdown}")
            }
            // A role of the server added to the open channel's advanced permissions without saving (as the "+" popout
            // adds one), so its row can be right-pressed (module 495603).
            ["perm-role"] => {
                let role = self.guild.and_then(|guild| {
                    self.stores
                        .guild(guild)
                        .and_then(|known| known.roles.keys().copied().find(|role| *role != guild))
                });
                match (role, self.channel_settings.as_mut()) {
                    (Some(role), Some(settings)) => {
                        settings.perm_edits.insert(role, (0, 0, 0));
                        format!("standin: role {role} added")
                    }
                    _ => "standin: no role or no channel settings".to_owned(),
                }
            }
            // The channel list's boost progress bar switched on locally (module 709843's opener).
            ["progress-bar"] => {
                let Some(guild) = self.guild else {
                    return "standin: no server in view".to_owned();
                };
                if let Some(known) = self
                    .stores
                    .guilds
                    .iter_mut()
                    .find(|known| known.id == guild)
                {
                    known.premium_progress_bar_enabled = true;
                }
                "standin: progress bar on".to_owned()
            }
            // "Do you want to also end the event?" (module 521549) as Disconnect opens it; the fixture has no call.
            ["end-event"] => {
                self.voice_alert = Some(super::voice_alerts::VoiceAlert::EventEndOnLeave {
                    guild: self.guild.unwrap_or_default(),
                    event: 1,
                });
                "standin: end event".to_owned()
            }
            _ => format!("standin: unknown {args:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unavailable_dialog_names_the_server_in_bold() {
        let alert = unavailable_alert(9, "Far Away");
        assert_eq!(alert.title, "Server Unavailable");
        let subtitle = alert.subtitle.unwrap();
        assert!(
            subtitle
                .iter()
                .any(|(text, bold)| *bold && text == "Far Away")
        );
        assert!(
            subtitle
                .iter()
                .any(|(text, _)| text.contains("not available in your region"))
        );
        let labels: Vec<&str> = alert.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Leave Server", "OK"]);
        assert!(matches!(
            alert.buttons[0].then,
            Then::LeaveGuild { guild: 9 }
        ));
    }
}
