//! The server's Members view (`/channels/{guild}/member-safety`, module 116778 with the table of module 920360): the
//! channel list's Members row replaces the chat with "Recent Members", the table of `POST /guilds/{id}/members-search`
//! with the search, Sort and Prune the settings page has (they share `members_state`), the join method of each row
//! and the row's two buttons. Measured in `design/dom/channel/member-safety-darker-cozy.json`.
//!
//! The row appears when the member can reach the page (`canAccessMemberSafetyPage`: Administrator, Manage Server, Ban,
//! Kick, Timeout, Manage Roles or Manage Nicknames) on a Community server, or on one whose Members row the settings
//! page switched on (`ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY`).
//!
//! Not drawn: the join-request tabs of a server with manual approval, the column filters' popouts, the row's
//! Add Roles, the Signals flags and the Mod View sidebar.

use super::server_members::{MEMBERS_PREFIX, from_now};
use super::{Root, avatar_source, color, icon};
use crate::parity::Parity;
use dc_state::{Id, User, permissions};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};
use serde_json::Value;

const MODERATION_FEATURE: &str = "ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY";
/// `canAccessMemberSafetyPage`'s permissions (module 402655); Manage Nicknames is bit 27.
const MANAGE_NICKNAMES: u64 = 1 << 27;
const REACH: [u64; 7] = [
    permissions::ADMINISTRATOR,
    permissions::MANAGE_GUILD,
    permissions::BAN_MEMBERS,
    permissions::KICK_MEMBERS,
    permissions::MODERATE_MEMBERS,
    permissions::MANAGE_ROLES,
    MANAGE_NICKNAMES,
];

/// The join-method column's words (`getJoinTypeLabel`, module 589935 with the strings of module 11541).
fn join_method(kind: u64, code: Option<&str>) -> (String, bool) {
    match kind {
        1 => ("Bot Invite".to_owned(), false),
        2 => ("Integration".to_owned(), false),
        3 => ("Server Discovery".to_owned(), false),
        4 => ("Student Hub".to_owned(), false),
        5 | 6 => match code {
            Some(code) => (code.to_owned(), true),
            None if kind == 6 => ("Vanity URL".to_owned(), false),
            None => ("Server Invite".to_owned(), false),
        },
        7 => match code {
            Some(code) => (format!("Manual Verification ({code})"), false),
            None => ("Manual Verification".to_owned(), false),
        },
        8 => ("Linked Channel".to_owned(), false),
        _ => ("Unknown".to_owned(), false),
    }
}

/// What a row sends: a code with no type is an invite (module 166233).
fn join_kind(entry: &Value) -> u64 {
    let kind = entry["join_source_type"].as_u64().unwrap_or(0);
    if kind == 0 && entry["source_invite_code"].is_string() {
        5
    } else {
        kind
    }
}

impl Root {
    /// `canAccessMemberSafetyPage` and the feature that lists the row.
    pub(super) fn member_safety_listed(&self, guild: Id) -> bool {
        let Some((known, me)) = self.stores.guild(guild).zip(self.stores.me()) else {
            return false;
        };
        let reach = permissions::guild_permissions(known, me);
        REACH.iter().any(|bit| reach & bit == *bit)
            && known
                .features
                .iter()
                .any(|feature| feature == "COMMUNITY" || feature == MODERATION_FEATURE)
    }

    /// The channel list's Members row, between Events and Server Boosts.
    pub(super) fn member_safety_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild = self.guild?;
        if !self.member_safety_listed(guild) {
            return None;
        }
        let selected = self
            .member_view
            .as_ref()
            .is_some_and(|view| view.guild == guild);
        let tint = if selected {
            "--text-strong"
        } else {
            "--channels-default"
        };
        Some(
            div()
                .flex_none()
                .h(px(34.))
                .pt(px(1.))
                .pl(px(8.))
                .child(
                    div()
                        .id("channel-members")
                        .h(px(32.))
                        .w(px(self.sidebar_row_width()))
                        .pl(px(8.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(selected, |this| this.bg(color("--background-mod-strong")))
                        .when(!selected, |this| {
                            this.hover(|style| style.bg(color("--background-mod-subtle")))
                        })
                        .child(icon("members_nav", 20., tint))
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .font_weight(FontWeight(500.))
                                .text_color(color(if selected {
                                    "--text-strong"
                                } else {
                                    "--channels-default"
                                }))
                                .child("Members"),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.open_member_view(guild, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The header and the card, in the place of the chat.
    pub(super) fn member_safety_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .parity("chat")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .pt(px(1.))
            .overflow_hidden()
            .child(
                div()
                    .flex_none()
                    .h(px(49.))
                    .border_b_1()
                    .border_color(color("--background-mod-subtle"))
                    .pl(px(26.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .bg(color("--background-base-lower"))
                    .child(icon("members_nav", 20., "--channels-default"))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child("Members"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(color("--background-base-lower"))
                    .pt(px(16.))
                    .pl(px(16.))
                    .pr(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(
                        self.member_view.as_ref().and_then(|v| {
                            self.guild_raid_alert_banner(v.guild, "member-safety", cx)
                        }),
                    )
                    .child(self.member_safety_card(cx)),
            )
            .into_any_element()
    }

    fn member_safety_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.member_view.as_ref() else {
            return div().into_any_element();
        };
        let guild = view.guild;
        let state = &view.state;
        let report = self.fetched.get(&format!("{MEMBERS_PREFIX}{guild}"));
        let indexing = report.is_none_or(|report| report["status"] == 202);
        let members: Vec<Value> = report
            .and_then(|report| report["body"]["members"].as_array().cloned())
            .unwrap_or_default();
        let total = report
            .and_then(|report| report["body"]["total_result_count"].as_u64())
            .unwrap_or(members.len() as u64);
        let now = chrono::Utc::now().timestamp();
        let owner = self
            .stores
            .guild(guild)
            .map(|known| known.owner_id)
            .unwrap_or_default();
        let me = self.stores.me().map(|me| me.id);
        let column = |width: f32| div().w(px(width)).flex_none().px(px(8.));
        let head = |text: &'static str, filter: bool| {
            let cell = div().flex().items_center().gap(px(4.)).child(
                div()
                    .min_w_0()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(text),
            );
            if filter {
                cell.child(
                    div()
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("member_filter", 16., "--text-muted")),
                )
            } else {
                cell
            }
        };
        let box_mark = |disabled: bool| {
            div()
                .size(px(20.))
                .rounded(px(4.))
                .border_1()
                .border_color(gpui::rgba(0x97979fa3))
                .bg(gpui::rgba(0x00000014))
                .when(disabled, |this| this.opacity(0.5))
        };
        let rows: Vec<AnyElement> = members
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let member = &entry["member"];
                let user = User::from_value(&member["user"]);
                let id = member["user"]["id"].as_str().unwrap_or_default();
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let joined_server = member["joined_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map_or(0, |at| at.timestamp());
                let created = id
                    .parse::<u64>()
                    .map_or(0, |id| ((id >> 22) as i64 + 1_420_070_400_000) / 1000);
                let name = user
                    .as_ref()
                    .map(|user| user.display_name().to_owned())
                    .unwrap_or_default();
                let username = user
                    .as_ref()
                    .map(|user| user.username.clone())
                    .unwrap_or_default();
                // The server's owner and this account cannot be actioned.
                let actionable = id
                    .parse::<Id>()
                    .is_ok_and(|id| Some(id) != owner && Some(id) != me);
                let (method, pill) =
                    join_method(join_kind(entry), entry["source_invite_code"].as_str());
                let unknown = join_kind(entry) == 0;
                let cell = |text: String| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--text-default"))
                        .child(text)
                };
                let user_id = id.parse::<Id>().ok();
                div()
                    .id(SharedString::from(format!("member-row-{index}")))
                    .h(px(51.))
                    .rounded(px(4.))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .child(column(36.).child(box_mark(!actionable)))
                    .child(
                        div()
                            .w(px(177.4))
                            .flex_none()
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(32.)).flex_none().rounded_full().children(
                                picture.map(|picture| img(picture).size(px(32.)).rounded_full()),
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .line_height(px(18.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .line_height(px(16.))
                                            .text_color(color("--text-default"))
                                            .child(username),
                                    ),
                            ),
                    )
                    .child(column(93.7).child(cell(from_now(now - joined_server))))
                    .child(column(92.9).child(cell(from_now(now - created))))
                    .child(column(94.3).child(if pill {
                        div()
                            .h(px(18.))
                            .px(px(4.))
                            .rounded(px(4.))
                            .bg(gpui::rgba(0x97979f1f))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .self_start()
                            .child(icon("member_invite_link", 12., "--text-default"))
                            .child(cell(method))
                            .into_any_element()
                    } else {
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(if unknown {
                                color("--text-muted")
                            } else {
                                color("--text-default")
                            })
                            .child(method)
                            .into_any_element()
                    }))
                    .child(column(169.5))
                    .child(column(92.4))
                    .child(
                        div()
                            .flex_1()
                            .pl(px(12.8))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(self.member_action(
                                SharedString::from(format!("member-mod-{index}")),
                                "member_mod_view",
                                18.,
                                cx,
                                move |this, window, cx| {
                                    if let Some(user) = user_id {
                                        this.open_full_profile(user, window, cx);
                                    }
                                },
                            ))
                            .child(self.member_action(
                                SharedString::from(format!("member-more-{index}")),
                                "more_dots",
                                18.,
                                cx,
                                move |this, window, cx| {
                                    if let Some(user) = user_id {
                                        this.open_full_profile(user, window, cx);
                                    }
                                },
                            )),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .w_full()
            .rounded(px(12.))
            .border_1()
            .border_color(gpui::rgba(0x97979f1f))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-strong"))
                            .child("Recent Members"),
                    )
                    .child(
                        div()
                            .w(px(250.))
                            .h(px(32.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-default"))
                            .bg(color("--input-background-default"))
                            .pl(px(8.))
                            .pr(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(icon("picker_search", 14., "--input-text-default"))
                            .child(div().flex_1().min_w(px(0.)).child(state.search.clone())),
                    )
                    .child(self.members_sort_button(cx))
                    .child(self.members_prune_button(guild, cx)),
            )
            .child(
                div()
                    .pt(px(1.))
                    .pb(px(8.))
                    .px(px(8.))
                    .child(
                        div()
                            .h(px(60.))
                            .pb(px(8.))
                            .flex()
                            .items_center()
                            .child(column(36.).child(box_mark(false)))
                            .child(div().w(px(177.4)).flex_none().px(px(8.)).child(head("Name", false)))
                            .child(column(93.7).child(head("Member Since", true)))
                            .child(column(92.9).child(head("Joined Discord", true)))
                            .child(column(94.3).child(head("Join Method", true)))
                            .child(column(169.5).child(head("Roles", true)))
                            .child(column(92.4).child(head("Signals", true))),
                    )
                    .when(indexing, |this| {
                        this.child(
                            div()
                                .h(px(240.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .text_color(color("--text-muted"))
                                .child("Before searching, we need to index this server. Give us a bit."),
                        )
                    })
                    .children(rows),
            )
            .when(!indexing, |this| {
                this.child(
                    div()
                        .h(px(45.))
                        .mb(px(9.))
                        .px(px(16.))
                        .border_t_1()
                        .border_color(gpui::rgba(0x97979f1f))
                        .flex()
                        .items_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child("Showing\u{a0}")
                        .child(
                            div()
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(total.to_string()),
                        )
                        .child(if total == 1 {
                            "\u{a0}member"
                        } else {
                            "\u{a0}members"
                        }),
                )
            })
            .into_any_element()
    }

    /// One of a row's 28px buttons.
    fn member_action(
        &self,
        id: SharedString,
        name: &'static str,
        size: f32,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut gpui::Window, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .size(px(28.))
            .flex_none()
            .rounded(px(4.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .child(icon(name, size, "--icon-subtle"))
            .on_click(cx.listener(move |this, _, window, cx| {
                run(this, window, cx);
                cx.notify();
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_join_method_reads_like_the_table() {
        assert_eq!(
            join_method(5, Some("uBEeuET")),
            ("uBEeuET".to_owned(), true)
        );
        assert_eq!(join_method(0, None), ("Unknown".to_owned(), false));
        assert_eq!(join_method(3, None).0, "Server Discovery");
        assert_eq!(join_method(7, Some("ab")).0, "Manual Verification (ab)");
        assert_eq!(join_method(6, None).0, "Vanity URL");
    }

    #[test]
    fn a_code_without_a_type_is_an_invite() {
        assert_eq!(
            join_kind(&json!({"join_source_type": 0, "source_invite_code": "x"})),
            5
        );
        assert_eq!(
            join_kind(&json!({"join_source_type": 0, "source_invite_code": null})),
            0
        );
        assert_eq!(join_kind(&json!({"join_source_type": 3})), 3);
    }

    #[test]
    fn the_page_is_reached_by_any_of_seven_permissions() {
        assert!(REACH.contains(&permissions::BAN_MEMBERS));
        assert!(REACH.contains(&MANAGE_NICKNAMES));
        assert_eq!(REACH.len(), 7);
    }
}
