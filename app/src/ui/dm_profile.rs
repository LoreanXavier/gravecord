//! The profile beside a DM: `aside_HbUF2J` holding a `user-profile-sidebar-redesign` card. Measured in
//! `design/dom/dm/loreanxavier-darker-cozy.json`, whose user has a Nitro profile theme (both colors black, overlay
//! `#00000099`, text at 96% white). Themes with a light primary color are not modelled yet and draw unthemed.

use super::{Root, avatar_source, color, icon, status_dot_large};
use crate::parity::Parity;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, Hsla, IntoElement, Rgba, div, hsla, img, linear_color_stop,
    linear_gradient, prelude::*, px, rgba,
};

/// Colors of the card: background stops, overlay, and the text it carries.
pub(super) struct Palette {
    pub top: Rgba,
    pub bottom: Rgba,
    pub overlay: Hsla,
    pub text: Hsla,
    pub subtle: Hsla,
    pub themed: bool,
}

fn luminance(value: u32) -> f32 {
    let channel = |shift: u32| ((value >> shift) & 0xff) as f32 / 255.;
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// An entry of `mutual_friends`: a user object, or one wrapped as `{user}`.
pub(super) fn mutual_user(entry: &serde_json::Value) -> Option<dc_state::User> {
    let value = if entry["user"].is_object() {
        &entry["user"]
    } else {
        entry
    };
    dc_state::User::from_value(value)
}

/// Discord's snowflake epoch, for the account's creation date.
const EPOCH_MS: i64 = 1_420_070_400_000;

pub(super) fn member_since(user: Id) -> String {
    let millis = (user >> 22) as i64 + EPOCH_MS;
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%b %-d, %Y")
                .to_string()
        })
        .unwrap_or_default()
}

impl Root {
    /// The bio as markdown in `body-sm/normal`: 14px on 18px lines, links in `--text-link` and clickable.
    pub(super) fn bio(&self, bio: &str, text: Hsla, cx: &mut Context<Self>) -> impl IntoElement {
        let blocks = dc_markdown::parse(bio);
        div().flex().flex_col().children(blocks.iter().map(|block| {
            let spans = super::block_spans(block).to_vec();
            let pieces = spans
                .iter()
                .map(|span| self.piece(span, text, 14., 400., 18.))
                .collect();
            let links = super::link_runs(&spans);
            let root = cx.weak_entity();
            crate::inline::inline(pieces, px(18.)).on_piece_click(move |piece, _, cx| {
                if let Some(Some((url, label))) = links.get(piece).cloned() {
                    let _ = root.update(cx, |this, cx| this.follow_link(&url, &label, cx));
                }
            })
        }))
    }

    /// `avatars_NbMTpy` at size 16: up to three mutual friends, each overlapping the next by 3px (module 791556).
    pub(super) fn mutual_avatars(&self, body: Option<&serde_json::Value>) -> impl IntoElement {
        let users: Vec<dc_state::User> = body
            .and_then(|body| body["mutual_friends"].as_array())
            .into_iter()
            .flatten()
            .filter_map(mutual_user)
            .take(3)
            .collect();
        let last = users.len().saturating_sub(1);
        div()
            .flex()
            .items_center()
            .children(users.into_iter().enumerate().map(|(index, user)| {
                let picture = self.images.get(&avatar_source(&user).0).cloned();
                div()
                    .size(px(16.))
                    .rounded_full()
                    .when(index != last, |this| this.mr(px(-3.)))
                    .children(picture.map(|picture| img(picture).size(px(16.)).rounded_full()))
            }))
    }

    pub(super) fn dm_palette(&self, user: Id) -> Palette {
        let body = self.fetched.get(&format!("profile:{user}"));
        let colors: Option<(u32, u32)> = body.and_then(|body| {
            let list = body["user_profile"]["theme_colors"].as_array()?;
            Some((
                list.first()?.as_u64()? as u32,
                list.get(1)?.as_u64()? as u32,
            ))
        });
        match colors {
            Some((primary, secondary)) if luminance(primary) < 0.5 => Palette {
                top: rgba((primary << 8) | 0xff),
                bottom: rgba((secondary << 8) | 0xff),
                // `--profile-gradient-overlay-color: #00000099`.
                overlay: hsla(0., 0., 0., 0.6),
                text: hsla(0., 0., 1., 0.96),
                // Mutuals in the capture: oklab(0.7886 …) on the dark overlay.
                subtle: hsla(0., 0., 0.71, 1.),
                themed: true,
            },
            _ => Palette {
                top: color("--chat-background-default"),
                bottom: color("--chat-background-default"),
                overlay: hsla(0., 0., 0., 0.),
                text: color("--text-default").into(),
                subtle: color("--text-subtle").into(),
                themed: false,
            },
        }
    }

    pub(super) fn dm_profile(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.show_dm_profile {
            return None;
        }
        let channel = self.current_dm()?;
        let user = self.stores.dm_recipient(channel)?.clone();
        let palette = self.dm_palette(user.id);
        let user_id = user.id;
        let body = self.fetched.get(&format!("profile:{}", user.id));
        let avatar = self.images.get(&avatar_source(&user).0).cloned();
        let banner: Rgba = if palette.themed {
            palette.top
        } else {
            color("--background-mod-strong")
        };
        let pronouns = body
            .and_then(|body| body["user_profile"]["pronouns"].as_str())
            .filter(|pronouns| !pronouns.is_empty())
            .map(str::to_owned);
        let bio = body
            .and_then(|body| body["user_profile"]["bio"].as_str())
            .filter(|bio| !bio.is_empty())
            .map(str::to_owned);
        let mutual_friends = body
            .and_then(|body| body["mutual_friends"].as_array().map(Vec::len))
            .unwrap_or(0);
        let mutual_servers = body
            .and_then(|body| body["mutual_guilds"].as_array().map(Vec::len))
            .unwrap_or(0);
        let badges = self.badges(user.id);
        let presence = self
            .stores
            .presences
            .get(&user.id)
            .cloned()
            .unwrap_or_else(|| "offline".to_owned());
        let text = palette.text;
        let subtle = palette.subtle;
        let dot = |tint: Hsla| div().size(px(4.)).rounded_full().bg(tint);
        let plural = |count: usize, one: &str, many: &str| {
            if count == 1 {
                format!("1 {one}")
            } else {
                format!("{count} {many}")
            }
        };
        let header =
            div()
                .relative()
                .h(px(144.))
                .mb(px(8.))
                .child(div().h(px(105.)).rounded_t(px(6.)).bg(banner))
                .child(
                    // `avatar_WRLIGC clickable`: "View Full Profile".
                    div()
                        .id("dm-profile-avatar")
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_full_profile(user_id, window, cx)
                        }))
                        .absolute()
                        .left(px(8.))
                        .top(px(49.))
                        .size(px(92.))
                        .rounded_full()
                        .bg(palette.top)
                        .child(
                            div()
                                .size(px(92.))
                                .rounded_full()
                                .bg(palette.overlay)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .relative()
                                        .size(px(80.))
                                        .rounded_full()
                                        .children(avatar.map(|picture| {
                                            img(picture).size(px(80.)).rounded_full()
                                        }))
                                        .child(status_dot_large(&presence, "--black")),
                                ),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(8.))
                        .flex()
                        .gap(px(8.))
                        .child(self.profile_friend_button(user.id, cx))
                        .child(self.profile_more_button(user.id, cx)),
                );
        let names = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(text)
                    .child(user.display_name().to_owned()),
            )
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(text)
                    .child(user.username.clone())
                    .when_some(pronouns, |this, pronouns| {
                        this.child(dot(text)).child(pronouns)
                    })
                    .when(!badges.is_empty(), |this| {
                        this.child(
                            div()
                                .h(px(22.))
                                .px(px(3.))
                                .py(px(1.))
                                .rounded(px(8.))
                                .bg(hsla(240. / 360., 0.03, 0.61, 0.12))
                                .flex()
                                .items_center()
                                .gap(px(2.))
                                .children(badges),
                        )
                    }),
            );
        let mutuals = div()
            .h(px(24.))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(subtle)
            .when(mutual_friends > 0, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(self.mutual_avatars(body))
                        .child(plural(mutual_friends, "Mutual Friend", "Mutual Friends")),
                )
            })
            .when(mutual_friends > 0 && mutual_servers > 0, |this| {
                this.child(dot(subtle))
            })
            .when(mutual_servers > 0, |this| {
                this.child(plural(mutual_servers, "Mutual Server", "Mutual Servers"))
            });
        let sections = div()
            .pl(px(16.))
            .w(px(284.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(names)
                    .child(mutuals),
            )
            .when_some(bio, |this, bio| this.child(self.bio(&bio, text, cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .font_weight(FontWeight(600.))
                            .text_color(text)
                            .child("Member Since"),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(text)
                            .child(member_since(user.id)),
                    ),
            );
        let footer = div().p(px(16.)).child(
            div()
                .id("dm-profile-full")
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_full_profile(user_id, window, cx)
                }))
                .h(px(40.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-muted"))
                .bg(hsla(0., 0., 1., 0.08))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--control-secondary-text-default"))
                .child("View Full Profile"),
        );
        let card = div()
            .w(px(304.))
            .flex_1()
            .min_h(px(0.))
            .p(px(2.))
            .rounded(px(8.))
            // `box-shadow: rgba(255, 255, 255, 0.08) 0 0 0 1px`: a ring outside the card, not a border.
            .shadow(vec![gpui::BoxShadow {
                color: hsla(0., 0., 1., 0.08),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
            .bg(linear_gradient(
                180.,
                linear_color_stop(palette.top, 0.),
                linear_color_stop(palette.bottom, 1.),
            ))
            .child(
                div()
                    .size_full()
                    .rounded(px(6.))
                    .bg(palette.overlay)
                    .flex()
                    .flex_col()
                    .child(div().flex_1().child(header).child(sections))
                    .child(footer),
            );
        // Blocked, or ignored while the speedbump is on: "Show Profile?" first.
        let ignored = !self.is_blocked(user.id) && self.stores.is_ignored(user.id);
        let gated = (self.is_blocked(user.id)
            || (ignored && !self.stores.ignore_speedbump_disabled))
            && !self.shown_blocked_profiles.contains(&user.id);
        Some(
            div()
                .parity("dm-profile")
                .flex_none()
                .w(px(320.))
                .h_full()
                .p(px(8.))
                .bg(color("--background-base-lower"))
                .flex()
                .flex_col()
                .when(!gated, |this| this.child(card))
                .when(gated, |this| {
                    this.child(
                        div()
                            .size_full()
                            .rounded(px(8.))
                            .bg(color("--background-base-lowest"))
                            .child(self.blocked_profile_gate(&user, ignored, cx)),
                    )
                })
                .into_any_element(),
        )
    }
}

/// `bannerButton_C4gRrZ`: 32px round on the banner, a 16px white icon.
pub(super) fn round_button(name: &'static str) -> gpui::Div {
    div()
        .size(px(32.))
        .rounded_full()
        .border_1()
        .border_color(hsla(0., 0., 1., 0.08))
        .bg(hsla(0., 0., 0., 0.56))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name, 16., "--white"))
}

#[cfg(test)]
mod tests {
    use super::member_since;

    #[test]
    fn member_since_reads_the_snowflake() {
        // loreanxavier's account, created on 29 October 2021 in the capture.
        assert!(member_since(903_350_000_000_000_000).ends_with("2021"));
    }
}
