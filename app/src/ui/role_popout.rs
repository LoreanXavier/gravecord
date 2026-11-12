//! The popout a role mention opens: module 593284 (`O` for the popout, `P` for a row), the header of module 282006, the
//! styles `rolePopout_ku1OHH`, `roleHeader_ku1OHH` and `roleScroller_ku1OHH` of stylesheet 44141 (240px wide, a 1px
//! `--border-subtle` border, 8px radius, `--shadow-high`, `max-height: calc(100vh - 20px)`; a 40px header with 12px above
//! and 16px / 8px beside it; the scroller 4px below the header and 8px above the bottom edge).
//!
//! The members are the guild member store's, filtered by the role (everyone for `@everyone`) and sorted by nick or
//! name. Opening preloads (module 545868): `GET /guilds/{id}/roles/{role}/member-ids`, whose ids are then asked of the
//! gateway (`requestMembersById`), and `GET /guilds/{id}/roles/member-counts` for the number in the header.

use super::motion::{PopKind, PopSide, Popout};
use super::sticker_popout::text;
use super::{Root, avatar_source, color};
use crate::net::Request;
use dc_state::members::Member;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, Pixels, SharedString, anchored, deferred,
    div, point, prelude::*, px,
};

/// `GUILD_ROLE_MEMBER_IDS` answers.
pub(super) const IDS_PREFIX: &str = "rolepop-ids:";
/// `GUILD_ROLE_MEMBER_COUNTS` answers.
pub(super) const COUNTS_PREFIX: &str = "rolepop-counts:";
/// `rolePopout`'s width.
const WIDTH: f32 = 240.;
/// The gap `Popout` leaves between the target and the popout.
const GAP: f32 = 8.;
/// `max-height: calc(100vh - 20px)`.
const VIEWPORT_MARGIN: f32 = 20.;
/// The rows of the member list (`member_Iy0c7A` inner height 40px plus its 1px above and below).
const ROW: f32 = 42.;

pub(super) struct RolePopout {
    guild: Id,
    /// The role; the guild's own id for `@everyone`.
    role: Id,
    anchor: Bounds<Pixels>,
}

/// `n === x ? all : members.filter(m => m.roles.includes(n))` sorted by `nick ?? name` (lodash `sortBy` compares
/// strings by code unit).
pub(super) fn with_role(members: &[Member], guild: Id, role: Id) -> Vec<&Member> {
    let mut list: Vec<&Member> = members
        .iter()
        .filter(|member| role == guild || member.roles.contains(&role))
        .collect();
    list.sort_by(|a, b| sort_name(a).cmp(sort_name(b)));
    list
}

fn sort_name(member: &Member) -> &str {
    member
        .nick
        .as_deref()
        .unwrap_or_else(|| member.user.display_name())
}

/// `Intl.NumberFormat(locale).format(count)` for English: thousands grouped by commas.
pub(super) fn group_digits(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The header's second span: `\u{a0}\u{2014} {count}` (module 282006), nothing without a count.
pub(super) fn count_label(count: Option<u64>) -> Option<String> {
    count.map(|count| format!("\u{a0}\u{2014} {}", group_digits(count)))
}

/// The line under the rows when the role has more members than the store knows (`9oMmZC`: "+{count} more").
pub(super) fn more_label(count: Option<u64>, listed: usize) -> Option<String> {
    let count = count?;
    (count > listed as u64).then(|| format!("+{} more", count - listed as u64))
}

/// `--shadow-high`, as the profile popout paints it.
fn shadow_high() -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 1., 0.08),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        },
        gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.24),
            offset: point(px(0.), px(12.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

impl Root {
    /// A role mention (or `@everyone`) was clicked: the popout opens beside it and the preloads go out.
    pub(super) fn open_role_popout(
        &mut self,
        role: Option<Id>,
        anchor: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(guild) = self.current_guild().map(|guild| guild.id) else {
            return;
        };
        let role = role.unwrap_or(guild);
        // `null == p`: a role the guild no longer has draws nothing.
        let known = role == guild
            || self
                .current_guild()
                .is_some_and(|guild| guild.roles.contains_key(&role));
        if !known {
            return;
        }
        self.tip = None;
        self.tip_pending = None;
        if role != guild {
            let ids = format!("{IDS_PREFIX}{guild}:{role}");
            if self.requested.insert(ids.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key: ids,
                    route: format!("/guilds/{guild}/roles/{role}/member-ids"),
                });
            }
            let counts = format!("{COUNTS_PREFIX}{guild}");
            if self.requested.insert(counts.clone()) {
                let _ = self.requests.send(Request::Fetch {
                    key: counts,
                    route: format!("/guilds/{guild}/roles/member-counts"),
                });
            }
        }
        self.role_popout = Some(RolePopout {
            guild,
            role,
            anchor,
        });
        self.request_role_popout_images();
        cx.notify();
    }

    pub(super) fn close_role_popout(&mut self, cx: &mut Context<Self>) {
        if self.role_popout.take().is_some() {
            cx.notify();
        }
    }

    /// The member ids arrived: the ones the store lacks are asked of the gateway, as `requestMembersById` does.
    pub(super) fn role_popout_fetched(
        &mut self,
        key: &str,
        body: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(rest) = key.strip_prefix(IDS_PREFIX)
            && let Some((guild, _)) = rest.split_once(':')
            && let Ok(guild) = guild.parse::<Id>()
        {
            let wanted: Vec<Id> = body
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| id.as_str().and_then(|id| id.parse().ok()))
                .filter(|id| self.stores.members.get(guild, *id).is_none())
                .collect();
            if !wanted.is_empty() {
                let _ = self.requests.send(Request::GuildMembers {
                    guilds: vec![guild],
                    query: String::new(),
                    limit: 0,
                    user_ids: wanted,
                });
            }
        }
        self.fetched.insert(key.to_owned(), body);
        cx.notify();
    }

    fn role_member_count(&self, guild: Id, role: Id) -> Option<u64> {
        if role == guild {
            return None;
        }
        self.fetched
            .get(&format!("{COUNTS_PREFIX}{guild}"))?
            .get(role.to_string())?
            .as_u64()
    }

    fn role_row(&self, member: &Member, guild: Id, cx: &mut Context<Self>) -> AnyElement {
        let user: &User = &member.user;
        let status = self
            .stores
            .presence
            .get(user.id, Some(guild))
            .map_or("offline", |presence| presence.status.as_str());
        let name = member
            .nick
            .clone()
            .unwrap_or_else(|| user.display_name().to_owned());
        let tint = self
            .stores
            .guild(guild)
            .and_then(|guild| guild.role_color(&member.roles))
            .map_or_else(|| color("--channels-default"), gpui::rgb);
        let item = div()
            .h(px(ROW - 2.))
            .w(px(WIDTH - 2. - 16.))
            .pl(px(8.))
            .pr(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .child(self.member_avatar(user, status, false, false, cx))
            .child(
                div()
                    .ml(px(12.))
                    .min_w(px(0.))
                    .flex_1()
                    .text_ellipsis()
                    .font_weight(FontWeight(500.))
                    .line_height(px(20.))
                    .text_color(tint)
                    .child(SharedString::from(name)),
            );
        div()
            .flex_none()
            .h(px(ROW))
            .pt(px(1.))
            .ml(px(8.))
            .child(self.profile_trigger(format!("rolepop-{}", user.id), user.id, item, cx))
            .into_any_element()
    }

    /// Avatars of the members the popout lists.
    pub(super) fn request_role_popout_images(&mut self) {
        let Some(open) = self.role_popout.as_ref() else {
            return;
        };
        let wanted: Vec<User> =
            with_role(self.stores.members.of(open.guild), open.guild, open.role)
                .into_iter()
                .map(|member| member.user.clone())
                .collect();
        for user in wanted {
            let (key, url) = avatar_source(&user);
            self.request_image(key, url);
        }
    }

    pub(super) fn role_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.role_popout.as_ref()?;
        let guild = self.stores.guild(open.guild)?;
        let title = if open.role == open.guild {
            "@everyone".to_owned()
        } else {
            guild.roles.get(&open.role)?.clone()
        };
        let members = with_role(self.stores.members.of(open.guild), open.guild, open.role);
        let count = self.role_member_count(open.guild, open.role);
        let rows: Vec<AnyElement> = members
            .iter()
            .map(|member| self.role_row(member, open.guild, cx))
            .collect();
        let more = more_label(count, rows.len());
        let (row_count, has_more) = (rows.len(), more.is_some());
        let header = div()
            .flex_none()
            .h(px(40.))
            .pt(px(12.))
            .pl(px(16.))
            .pr(px(8.))
            .flex()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--channels-default"))
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(SharedString::from(title)),
            )
            .children(
                count_label(count).map(|label| div().flex_none().child(SharedString::from(label))),
            );
        let limit = (self.viewport.1 - VIEWPORT_MARGIN).max(80.);
        let popout = div()
            .id("role-popout")
            .w(px(WIDTH))
            .max_h(px(limit))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-higher"))
            .shadow(shadow_high())
            .occlude()
            .child(
                div()
                    .id("role-popout-scroller")
                    .mb(px(8.))
                    .mt(px(4.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .child(header)
                    .children(rows)
                    .children(more.map(|label| {
                        // `N.A` with `className: roleHeader`: the same 40px line as the header.
                        text(14., 18., 500., "--channels-default")
                            .flex_none()
                            .h(px(40.))
                            .pl(px(16.))
                            .pr(px(8.))
                            .pt(px(12.))
                            .child(SharedString::from(label))
                    })),
            );
        // `position: "top"` with `autoInvert` (module 787564 through `Popout`, module 265872): above the mention,
        // or below it when that leaves more room (`calculateState`, module 623646). The default `TRANSLATE`
        // animation slides it in from the side it opened on.
        let height =
            (2. + 4. + 40. + row_count as f32 * ROW + if has_more { 40. } else { 0. } + 8.)
                .min(limit);
        let below = opens_below(
            f32::from(open.anchor.top()),
            f32::from(open.anchor.bottom()),
            height,
            self.viewport.1,
        );
        let (corner, at, side) = if below {
            (
                gpui::Anchor::TopLeft,
                point(open.anchor.left(), open.anchor.bottom() + px(GAP)),
                PopSide::Bottom,
            )
        } else {
            (
                gpui::Anchor::BottomLeft,
                point(open.anchor.left(), open.anchor.top() - px(GAP)),
                PopSide::Top,
            )
        };
        Some(
            deferred(
                div()
                    .id("role-popout-layer")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.close_role_popout(cx)),
                    )
                    .child(
                        anchored()
                            .anchor(corner)
                            .position(at)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .popout("role-popout", PopKind::Translate, side)
                                    .child(popout),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// Whether a popout asked for above its target opens below it instead (`autoInvert`, `calculateState` in module
/// 623646): the room left above is `top - GAP - height`, below it `viewport - (bottom + GAP + height)`; it inverts
/// when the room above is negative and the room below is larger.
fn opens_below(top: f32, bottom: f32, height: f32, viewport: f32) -> bool {
    let above = top - GAP - height;
    let below = viewport - (bottom + GAP + height);
    above < 0. && below > above
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popout_opens_above_the_mention_unless_it_does_not_fit() {
        assert!(!opens_below(400., 418., 200., 720.));
        // 100 - 8 - 200 < 0 and there is more room below.
        assert!(opens_below(100., 118., 200., 720.));
        // Neither fits: it stays where the room is larger.
        assert!(!opens_below(300., 318., 600., 400.));
    }

    fn member(id: Id, name: &str, nick: Option<&str>, roles: &[Id]) -> Member {
        Member {
            user: User {
                id,
                username: name.to_owned(),
                ..User::default()
            },
            nick: nick.map(str::to_owned),
            avatar: None,
            avatar_decoration: None,
            roles: roles.to_vec(),
            joined_at: None,
            pending: false,
            display_name_styles: None,
            timeout_until: None,
            flags: 0,
        }
    }

    #[test]
    fn only_members_with_the_role_are_listed_sorted_by_nick_then_name() {
        let members = vec![
            member(1, "zed", None, &[10]),
            member(2, "ann", Some("Zoe"), &[10]),
            member(3, "bob", None, &[11]),
            member(4, "cy", None, &[10, 11]),
        ];
        let listed: Vec<Id> = with_role(&members, 99, 10)
            .iter()
            .map(|m| m.user.id)
            .collect();
        // Code-unit order: the nick `Zoe` (capital Z) sorts before `cy` and `zed`.
        assert_eq!(listed, vec![2, 4, 1]);
    }

    #[test]
    fn everyone_lists_every_member() {
        let members = vec![member(1, "a", None, &[]), member(2, "b", None, &[5])];
        assert_eq!(with_role(&members, 99, 99).len(), 2);
    }

    #[test]
    fn the_header_count_is_grouped_and_follows_a_non_breaking_dash() {
        assert_eq!(
            count_label(Some(1234)).as_deref(),
            Some("\u{a0}\u{2014} 1,234")
        );
        assert_eq!(count_label(Some(7)).as_deref(), Some("\u{a0}\u{2014} 7"));
        assert_eq!(count_label(None), None);
        assert_eq!(group_digits(1_000_000), "1,000,000");
    }

    #[test]
    fn the_more_line_counts_the_members_the_store_does_not_know() {
        assert_eq!(more_label(Some(12), 5).as_deref(), Some("+7 more"));
        assert_eq!(more_label(Some(5), 5), None);
        assert_eq!(more_label(None, 5), None);
    }
}
