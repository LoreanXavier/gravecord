//! The Stage's requests to speak and its video seat notice.
//!
//! - The header's toggle (`eo`/`er`/`ea` of module 482877): for someone with Mute Members in the Stage while the
//!   sidebar is closed, the list-hand icon (`HandRequestSpeakListIcon`) with a red count badge of the raised hands
//!   (`raisedHandCount_0zxwb-`: 16 high, from the icon's middle, 3px under it, `body-xs/semibold`, "99+" past 99; the
//!   icon cut by a 10px circle at (20, 19) while there is a badge), tooltip Show Requests.
//! - The sidebar (module 374873, `container_s7QiTL`: 320 wide on `--background-base-low`): the header with the hand
//!   and "Requests to Speak" and a close button; for someone who may manage the channel or its roles, "Allow requests
//!   from @everyone" (the @everyone overwrite of Request to Speak, `b6` of module 849736); the list title "Requests to
//!   Speak — N"; a row per raised hand (40px avatar, the name and the person's kind of member of module 925931, Invite
//!   to Speak / Invite Sent and Dismiss, `lL` of module 849736); with no hand "No Requests" and "Requests to speak will
//!   show up here." under the Stage glyph between two sparkles (modules 345687 and 489673).
//! - "This Video Stage is full!" (module 366265, `boostUpsell_kDLNhI` under the header, 52 high): for a Stage moderator
//!   while the Stage shows video (`qT` of module 96566) and holds as many people as the guild's video Stage seats
//!   (module 533233, not for a guild without Community past 300 seats), until dismissed for the channel.

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_state::{Channel, Id, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::json;

/// The empty state's sparkles (module 489673: 856682 under the glyph's left, 346554 over its right), 12px.
const SPARKLE_BOTTOM: &str = "https://discord.com/assets/3a6a08a976f34e04.svg";
const SPARKLE_TOP: &str = "https://discord.com/assets/30d1720360dd2c40.svg";

/// The badge's text: the count, or "99+".
pub(super) fn hands_badge(count: usize) -> String {
    if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    }
}

/// `A` of module 533233: whether the full video Stage notice shows.
pub(super) fn video_stage_full(
    video: bool,
    dismissed: bool,
    moderator: bool,
    community: bool,
    seats: u64,
    people: u64,
) -> bool {
    video && !dismissed && moderator && !(!community && seats > 300) && people >= seats
}

/// `_` of module 366265: Boost can add seats (a Community guild below 10,000, another below Boost level 3 at no more
/// than 150): the boost title and Not Now / Boost, else the maxed-out title and Dismiss.
pub(super) fn video_stage_can_boost(community: bool, premium_tier: u64, seats: u64) -> bool {
    if community {
        seats < 10_000
    } else {
        premium_tier != 3 && seats <= 150
    }
}

/// `g` of module 925931: what kind of member someone with a raised hand is.
pub(super) fn member_kind(
    new_account: bool,
    joined_at: Option<i64>,
    has_roles: bool,
    now: i64,
) -> &'static str {
    if new_account {
        return "VaCdhQ";
    }
    match joined_at {
        None => "CQmzib",
        Some(_) if has_roles => "97/NdO",
        Some(joined) if now - joined < 86_400_000 => "IKE48n",
        Some(_) => "u0gUWt",
    }
}

impl Root {
    /// The header's toggle, or `None` without Mute Members or while the sidebar is open.
    pub(super) fn stage_requests_toggle(
        &self,
        channel: &Channel,
        hands: usize,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let guild = self.guild?;
        if !self.my_permission(guild, Some(channel), permissions::MUTE_MEMBERS) {
            return None;
        }
        let tip_label = if self.stage_requests_open {
            dc_intl::t("gKGz7A")
        } else {
            dc_intl::t("ImQ4dW")
        };
        let badge = (!self.stage_requests_open && hands > 0).then(|| {
            div()
                .absolute()
                .left(px(16.))
                .bottom(px(-3.))
                .h(px(16.))
                .min_w(px(16.))
                .px(px(4.))
                .rounded_full()
                .bg(color("--background-feedback-notification"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(600.))
                .text_color(color("--white"))
                .child(hands_badge(hands))
        });
        let button = div()
            .id("stage-requests-toggle")
            .relative()
            .ml(px(-4.))
            .min_w(px(32.))
            .h(px(24.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_color(color("--interactive-text-default"))
            .hover(|style| style.text_color(color("--white")))
            .child(icon(
                "stage_hand_request_list",
                24.,
                "--interactive-text-default",
            ))
            .children(badge)
            .on_click(cx.listener(|this, _, _, cx| {
                this.stage_requests_open = !this.stage_requests_open;
                if this.stage_requests_open {
                    this.request_stage_requests_art();
                }
                cx.notify();
            }));
        Some(
            self.with_tip(
                "stage-requests-toggle-tip",
                super::tooltip::Tip::text(tip_label, super::tooltip::Side::Bottom),
                button,
                cx,
            )
            .into_any_element(),
        )
    }

    /// `b6` of module 849736: Request to Speak allowed or denied on the Stage's @everyone overwrite.
    fn set_everyone_may_request(&mut self, guild: Id, channel: &Channel, allow: bool) {
        let current = channel
            .overwrites
            .iter()
            .find(|entry| entry.kind == 0 && entry.id == guild);
        let (mut allowed, mut denied) = current.map_or((0, 0), |entry| (entry.allow, entry.deny));
        if allow {
            allowed |= permissions::REQUEST_TO_SPEAK;
            denied &= !permissions::REQUEST_TO_SPEAK;
        } else {
            allowed &= !permissions::REQUEST_TO_SPEAK;
            denied |= permissions::REQUEST_TO_SPEAK;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PUT,
            route: format!("/channels/{}/permissions/{guild}", channel.id),
            body: Some(json!({
                "id": guild.to_string(),
                "type": 0,
                "allow": allowed.to_string(),
                "deny": denied.to_string(),
            })),
        });
    }

    /// `lL` of module 849736: Invite to Speak (`suppress: false`) or Dismiss (`suppress: true`) for a raised hand.
    fn answer_hand(&mut self, guild: Id, channel: Id, user: Id, dismiss: bool) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: format!("/guilds/{guild}/voice-states/{user}"),
            body: Some(json!({"suppress": dismiss, "channel_id": channel.to_string()})),
        });
    }

    /// Module 374873, while open.
    pub(super) fn stage_requests_sidebar(
        &self,
        channel: &Channel,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.stage_requests_open {
            return None;
        }
        let guild = self.guild?;
        let known = self.stores.guild(guild)?;
        let manager = self.my_permission(guild, Some(channel), permissions::MANAGE_CHANNELS)
            || self.my_permission(guild, Some(channel), permissions::MANAGE_ROLES);
        let hands: Vec<(Id, bool)> = self
            .stores
            .voice_members(guild, channel.id)
            .iter()
            .filter(|state| state.request_to_speak_timestamp.is_some())
            .map(|state| (state.user_id, !state.suppress))
            .collect();
        let header = div()
            .flex_none()
            .h(px(48.))
            .px(px(16.))
            .flex()
            .items_center()
            .rounded_tl(px(8.))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.16),
                offset: gpui::point(px(0.), px(1.)),
                blur_radius: px(0.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(icon("system_hand", 24., "--interactive-icon-default"))
            .child(
                div()
                    .ml(px(12.))
                    .flex_1()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(dc_intl::t("TYZgzW")),
            )
            .child(
                self.with_tip(
                    "stage-requests-close-tip",
                    super::tooltip::Tip::text(dc_intl::t("cpT0Cq"), super::tooltip::Side::Bottom),
                    div()
                        .id("stage-requests-close")
                        .cursor_pointer()
                        .child(icon("x_small", 24., "--interactive-text-default"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.stage_requests_open = false;
                            cx.notify();
                        })),
                    cx,
                ),
            );
        let everyone = permissions::everyone_can(known, channel, permissions::REQUEST_TO_SPEAK);
        let target = channel.clone();
        let toggle = manager.then(|| {
            div()
                .id("stage-requests-everyone")
                .h(px(66.))
                .pt(px(20.))
                .flex()
                .items_start()
                .gap(px(8.))
                .cursor_pointer()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("GYCh0W")),
                )
                .child(super::create_channel::switch(everyone))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_everyone_may_request(guild, &target, !everyone);
                    cx.notify();
                }))
        });
        let title = div()
            .pt(px(24.))
            .h(px(40.))
            .text_size(px(14.))
            .line_height(px(16.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-default"))
            .child(if hands.is_empty() {
                dc_intl::t("TYZgzW").to_owned()
            } else {
                dc_intl::format("5z7q5a", &[("numHands", (hands.len() as i64).into())])
            });
        let now = chrono::Utc::now().timestamp_millis();
        let rows: Vec<AnyElement> = hands
            .iter()
            .map(|(user, awaiting)| {
                let member = self.stores.members.get(guild, *user);
                let known_user = self
                    .stores
                    .users
                    .get(user)
                    .or_else(|| member.map(|member| &member.user));
                let picture =
                    known_user.and_then(|known| self.images.get(&avatar_source(known).0).cloned());
                let name = member
                    .and_then(|member| member.nick.clone())
                    .or_else(|| known_user.map(|known| known.display_name().to_owned()))
                    .unwrap_or_default();
                let joined = member
                    .and_then(|member| member.joined_at.as_deref())
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.timestamp_millis());
                // GAP: `isNewUser` (an account made in the last week) is not tracked; it counts as an older one.
                let kind = member_kind(
                    false,
                    joined,
                    member.is_some_and(|member| !member.roles.is_empty()),
                    now,
                );
                let (person, channel_id, awaiting) = (*user, channel.id, *awaiting);
                let small_button = |id: String, glyph: &'static str| {
                    div()
                        .id(SharedString::from(id))
                        .size(px(32.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--control-secondary-border-default"))
                        .bg(color("--control-secondary-background-default"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, 16., "--control-secondary-text-default"))
                };
                let invite_label = if awaiting { "h9rsTd" } else { "f0T7hI" };
                div()
                    .h(px(56.))
                    .pt(px(16.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(40.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(color("--background-mod-strong"))
                                    .children(
                                        picture.map(|picture| {
                                            img(picture).size(px(40.)).rounded_full()
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .ml(px(16.))
                                    .mr(px(8.))
                                    .min_w(px(1.))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t(kind)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                self.with_tip(
                                    format!("stage-hand-invite-tip-{person}"),
                                    super::tooltip::Tip::text(
                                        dc_intl::t(invite_label),
                                        super::tooltip::Side::Top,
                                    ),
                                    small_button(
                                        format!("stage-hand-invite-{person}"),
                                        "system_mic_arrow",
                                    )
                                    .when(awaiting, |this| this.opacity(0.5))
                                    .when(!awaiting, |this| {
                                        this.cursor_pointer().on_click(cx.listener(
                                            move |this, _, _, cx| {
                                                this.answer_hand(guild, channel_id, person, false);
                                                cx.notify();
                                            },
                                        ))
                                    }),
                                    cx,
                                ),
                            )
                            .child(
                                self.with_tip(
                                    format!("stage-hand-dismiss-tip-{person}"),
                                    super::tooltip::Tip::text(
                                        dc_intl::t("moABMy"),
                                        super::tooltip::Side::Top,
                                    ),
                                    small_button(format!("stage-hand-dismiss-{person}"), "x_small")
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.answer_hand(guild, channel_id, person, true);
                                            cx.notify();
                                        })),
                                    cx,
                                ),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = hands.is_empty().then(|| {
            let sparkle = |url: &'static str| {
                div().size(px(12.)).children(
                    self.images
                        .get(url)
                        .cloned()
                        .map(|art| img(art).size(px(12.))),
                )
            };
            div()
                .pt(px(32.))
                .pb(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .text_center()
                .child(
                    div()
                        .flex()
                        .child(div().self_end().child(sparkle(SPARKLE_BOTTOM)))
                        .child(
                            div()
                                .mx(px(4.))
                                .p(px(12.))
                                .rounded_full()
                                .bg(color("--background-base-lowest"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("stage_channel", 32., "--interactive-text-default")),
                        )
                        .child(div().self_start().child(sparkle(SPARKLE_TOP))),
                )
                .child(
                    div()
                        .mt(px(16.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child(dc_intl::t("7R24mX")),
                )
                .child(
                    div()
                        .mt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::t("Rpr2s0")),
                )
        });
        Some(
            div()
                .id("stage-requests-sidebar")
                .flex_none()
                .w(px(320.))
                .h_full()
                .rounded_l(px(8.))
                .bg(color("--background-base-low"))
                .flex()
                .flex_col()
                .child(header)
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .px(px(16.))
                        .flex()
                        .flex_col()
                        .children(toggle)
                        .child(title)
                        .children(rows)
                        .children(empty),
                )
                .into_any_element(),
        )
    }

    /// The pictures the sidebar's empty state draws.
    pub(super) fn request_stage_requests_art(&mut self) {
        for url in [SPARKLE_BOTTOM, SPARKLE_TOP] {
            self.request_image(url.to_owned(), url.to_owned());
        }
    }

    /// Module 366265 under the Stage header, or `None`.
    pub(super) fn video_stage_full_notice(
        &self,
        channel: &Channel,
        people: u64,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let guild = self.guild?;
        let known = self.stores.guild(guild)?;
        let me = self.stores.current_user.as_ref()?.id;
        let community = known.features.iter().any(|feature| feature == "COMMUNITY");
        let seats = known.max_stage_video_channel_users;
        // `qT` of module 96566: someone's camera is on in the Stage, or a speaker streams.
        let video = self
            .stores
            .voice_members(guild, channel.id)
            .iter()
            .any(|state| state.self_video || (state.self_stream && !state.suppress));
        if !video_stage_full(
            video,
            self.stage_video_upsell_dismissed.contains(&channel.id),
            self.is_stage_moderator(guild, channel.id, me),
            community,
            seats,
            people,
        ) {
            return None;
        }
        let boost = video_stage_can_boost(community, known.premium_tier, seats);
        let id = channel.id;
        let button = |key: &'static str, label: &'static str, tone: &'static str| {
            let (background, text) = match tone {
                "secondary" => (
                    "--control-secondary-background-default",
                    "--control-secondary-text-default",
                ),
                _ => ("--control-primary-background-default", "--white"),
            };
            div()
                .id(key)
                .h(px(32.))
                .px(px(12.))
                .min_w(px(60.))
                .rounded(px(8.))
                .bg(color(background))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(color(text))
                .child(dc_intl::t(label))
        };
        let dismiss = move |this: &mut Self, cx: &mut Context<Self>| {
            this.stage_video_upsell_dismissed.insert(id);
            cx.notify();
        };
        let buttons = if boost {
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    button("stage-video-full-not-now", "L5eIZ2", "secondary")
                        .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx))),
                )
                // `expressive` in the module; the boost dialog of `K4` (module 233824).
                .child(
                    button("stage-video-full-boost", "Uj0md3", "primary").on_click(
                        cx.listener(move |this, _, _, cx| this.boost_this_server(guild, cx)),
                    ),
                )
        } else {
            div().flex().items_center().child(
                button("stage-video-full-dismiss", "WAI6xu", "primary")
                    .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx))),
            )
        };
        Some(
            div()
                .h(px(52.))
                .px(px(16.))
                .py(px(8.))
                .rounded(px(5.))
                .bg(color("--background-base-lowest"))
                .flex()
                .items_center()
                .child(img("icons/stage_boost_badge.svg").size(px(36.)))
                .child(
                    div()
                        .ml(px(10.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t(if boost { "T+zF9M" } else { "IZ+SVv" })),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child(dc_intl::t(if boost { "Izgpmv" } else { "7FHbPG" })),
                        ),
                )
                .child(div().flex_1())
                .child(buttons)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_caps_at_ninety_nine() {
        assert_eq!(hands_badge(3), "3");
        assert_eq!(hands_badge(99), "99");
        assert_eq!(hands_badge(100), "99+");
    }

    #[test]
    fn the_full_video_stage_notice_follows_module_533233() {
        // A moderator in a full Stage with video.
        assert!(video_stage_full(true, false, true, false, 50, 50));
        // Without video, dismissed, or for someone who is no moderator: nothing.
        assert!(!video_stage_full(false, false, true, false, 50, 50));
        assert!(!video_stage_full(true, true, true, false, 50, 50));
        assert!(!video_stage_full(true, false, false, false, 50, 50));
        // Not full yet.
        assert!(!video_stage_full(true, false, true, false, 50, 49));
        // Past 300 seats only a Community guild sees it.
        assert!(!video_stage_full(true, false, true, false, 400, 400));
        assert!(video_stage_full(true, false, true, true, 400, 400));
        assert!(video_stage_can_boost(false, 1, 150));
        assert!(!video_stage_can_boost(false, 3, 50));
        assert!(video_stage_can_boost(true, 3, 300));
        assert!(!video_stage_can_boost(true, 3, 10_000));
    }

    #[test]
    fn a_raised_hand_names_the_kind_of_member() {
        let day = 86_400_000;
        assert_eq!(member_kind(true, Some(0), true, day), "VaCdhQ");
        assert_eq!(member_kind(false, None, false, day), "CQmzib");
        assert_eq!(member_kind(false, Some(0), true, 2 * day), "97/NdO");
        assert_eq!(member_kind(false, Some(day), false, day + 10), "IKE48n");
        assert_eq!(member_kind(false, Some(0), false, 2 * day), "u0gUWt");
    }
}
