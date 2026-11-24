//! The server's Members view (`/channels/{guild}/member-safety`, module 116778 with the table of module 920360): the
//! channel list's Members row replaces the chat with "Recent Members", the table of `POST /guilds/{id}/members-search`
//! with the search, Sort and Prune the settings page has (they share `members_state`), the join method of each row
//! and the row's two buttons. Measured in `design/dom/channel/member-safety-darker-cozy.json`.
//!
//! The row appears when the member can reach the page (`canAccessMemberSafetyPage`: Administrator, Manage Server, Ban,
//! Kick, Timeout, Manage Roles or Manage Nicknames) on a Community server, or on one whose Members row the settings
//! page switched on (`ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY`).
//!
//! The column headers open the settings table's filter menus (`member_filters.rs`), and an empty or still-indexing
//! search shows module 920360's `R`.
//! Not drawn: the join-request tabs of a server with manual approval, the row's Add Roles and the Mod View sidebar.

use super::member_filters::FilterMenu;
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
            .children(
                self.member_view
                    .as_ref()
                    .and_then(|v| self.member_safety_bulk_bar(v.guild, cx)),
            )
            .children(
                self.member_view
                    .as_ref()
                    .and_then(|v| self.member_safety_bulk_ban_modal(v.guild, cx)),
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
        let box_mark = |ticked: bool, disabled: bool| {
            div()
                .size(px(20.))
                .rounded(px(4.))
                .border_1()
                .when(ticked, |this| {
                    this.border_color(color("--control-primary-background-default"))
                        .bg(color("--control-primary-background-default"))
                })
                .when(!ticked, |this| {
                    this.border_color(gpui::rgba(0x97979fa3))
                        .bg(gpui::rgba(0x00000014))
                })
                .when(disabled, |this| this.opacity(0.45))
                .flex()
                .items_center()
                .justify_center()
                .when(ticked, |this| {
                    this.child(icon("checkbox_check", 16., "--white"))
                })
        };
        let page_size = state.page_size.max(1) as usize;
        let skip = (state.current_page.saturating_sub(1) as usize).saturating_mul(page_size);
        let visible_actionable: Vec<Id> = members
            .iter()
            .skip(skip)
            .take(page_size)
            .filter_map(|entry| entry["member"]["user"]["id"].as_str()?.parse::<Id>().ok())
            .filter(|uid| Some(*uid) != owner && Some(*uid) != me)
            .collect();
        let all_visible_selected = !visible_actionable.is_empty()
            && visible_actionable
                .iter()
                .all(|uid| state.selected_user_ids.contains(uid));
        let rows: Vec<AnyElement> = members
            .iter()
            .enumerate()
            .skip(skip)
            .take(page_size)
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
                let checked = user_id.is_some_and(|uid| state.selected_user_ids.contains(&uid));
                div()
                    .id(SharedString::from(format!("member-row-{index}")))
                    .h(px(51.))
                    .rounded(px(4.))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .flex()
                    .items_center()
                    .child(
                        column(36.).child(
                            div()
                                .id(SharedString::from(format!("member-row-chk-{index}")))
                                .when(actionable, |d| d.cursor_pointer())
                                .child(box_mark(checked, !actionable))
                                .when(actionable, |d| {
                                    d.on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(uid) = user_id
                                            && let Some(view) = this.member_view.as_mut()
                                        {
                                            if !view.state.selected_user_ids.remove(&uid) {
                                                view.state.selected_user_ids.insert(uid);
                                            }
                                            cx.notify();
                                        }
                                    }))
                                }),
                        ),
                    )
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
                            .child(
                                div()
                                    .id(SharedString::from(format!("member-more-{index}")))
                                    .size(px(28.))
                                    .flex_none()
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|style| style.bg(color("--background-mod-subtle")))
                                    .child(icon("more_dots", 18., "--icon-subtle"))
                                    .on_click(cx.listener(
                                        move |this, event: &gpui::ClickEvent, _, cx| {
                                            // Module 772063 (`member-application-context-menu`): Open in Mod View / Ban User / Copy User ID.
                                            if let Some(user) = user_id {
                                                this.open_context_menu(
                                                super::context_menu::Target::MemberApplication {
                                                    guild,
                                                    user,
                                                },
                                                event.position(),
                                                cx,
                                            );
                                            }
                                            cx.notify();
                                        },
                                    )),
                            ),
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
                            .child(
                                column(36.).child(
                                    div()
                                        .id("member-safety-select-all")
                                        .when(!visible_actionable.is_empty(), |d| {
                                            d.cursor_pointer()
                                        })
                                        .child(box_mark(
                                            all_visible_selected,
                                            visible_actionable.is_empty(),
                                        ))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let Some(view) = this.member_view.as_mut() else {
                                                return;
                                            };
                                            if all_visible_selected {
                                                for uid in &visible_actionable {
                                                    view.state.selected_user_ids.remove(uid);
                                                }
                                            } else {
                                                for uid in &visible_actionable {
                                                    view.state.selected_user_ids.insert(*uid);
                                                }
                                            }
                                            cx.notify();
                                        })),
                                ),
                            )
                            .child(
                                div()
                                    .w(px(177.4))
                                    .flex_none()
                                    .px(px(8.))
                                    .child(head("Name", false)),
                            )
                            .child(column(93.7).child(self.members_filter_header(
                                guild,
                                FilterMenu::Joined,
                                "xcKP1P",
                                cx,
                            )))
                            .child(column(92.9).child(self.members_filter_header(
                                guild,
                                FilterMenu::Account,
                                "sPph4O",
                                cx,
                            )))
                            .child(column(94.3).child(self.members_join_header(guild, cx)))
                            .child(column(169.5).child(self.members_filter_header(
                                guild,
                                FilterMenu::Roles,
                                "2SZsWX",
                                cx,
                            )))
                            .child(column(92.4).child(self.members_filter_header(
                                guild,
                                FilterMenu::Signals,
                                "7V3759",
                                cx,
                            ))),
                    )
                    .children(super::server_members::no_results_of(
                        report,
                        members.is_empty(),
                    ))
                    .children(rows),
            )
            .when(!indexing, |this| {
                this.child(self.member_safety_pagination_footer(guild, total, cx))
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

/// Module 123393 (`useGuildJoinRequestActions`): maximum rejection reason length (`867276`).
pub(crate) const JOIN_REQUEST_REJECTION_MAX_LEN: usize = 160;

/// Module 123393 (`624458.A.updateGuildJoinRequest`): `PATCH /guilds/{guildId}/requests/{requestId}`
/// with `{ "action": "APPROVED" }`.
#[allow(dead_code)]
pub(crate) fn approve_join_request_request(guild_id: Id, request_id: Id) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("join-request:approve:{guild_id}:{request_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}/requests/{request_id}"),
        body: Some(serde_json::json!({ "action": "APPROVED" })),
    }
}

/// Module 123393 + 867276: `PATCH /guilds/{guildId}/requests/{requestId}` with
/// `{ "action": "REJECTED", "rejection_reason"?: reason }` (clamped to 160 chars).
pub(crate) fn reject_join_request_request(
    guild_id: Id,
    request_id: Id,
    reason: Option<&str>,
) -> crate::net::Request {
    let mut body = serde_json::json!({ "action": "REJECTED" });
    if let Some(text) = reason.map(str::trim).filter(|s| !s.is_empty()) {
        let clamped: String = text.chars().take(JOIN_REQUEST_REJECTION_MAX_LEN).collect();
        body["rejection_reason"] = serde_json::Value::String(clamped);
    }
    crate::net::Request::Rest {
        key: Some(format!("join-request:reject:{guild_id}:{request_id}")),
        method: reqwest::Method::PATCH,
        route: format!("/guilds/{guild_id}/requests/{request_id}"),
        body: Some(body),
    }
}

/// Module 123393 (`624458.A.createOrEnterJoinRequestInterview`): `POST /join-requests/{requestId}/interview`.
#[allow(dead_code)]
pub(crate) fn create_join_request_interview_request(request_id: Id) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("join-request:interview:{request_id}")),
        method: reqwest::Method::POST,
        route: format!("/join-requests/{request_id}/interview"),
        body: None,
    }
}

/// Module 123393 + 867276: open Join Request rejection confirmation modal state.
#[derive(Clone, Debug)]
pub(crate) struct JoinRequestRejectModalState {
    pub guild_id: Id,
    pub request_id: Id,
    pub applicant_name: String,
    pub reason: String,
    pub submitting: bool,
}

impl Root {
    /// Renders the `123393`/`867276` Join Request rejection confirmation modal when open.
    pub(super) fn join_request_reject_modal_layer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.join_request_reject_modal.as_ref()?;
        let guild_id = state.guild_id;
        let request_id = state.request_id;
        let reason = state.reason.clone();
        let submitting = state.submitting;
        let title = if dc_intl::has_key("mFP/qw") {
            dc_intl::t("mFP/qw").to_owned()
        } else {
            "Reject Application".to_owned()
        };
        let reject_cta = if dc_intl::has_key("hDtbsz") {
            dc_intl::t("hDtbsz").to_owned()
        } else {
            "Reject".to_owned()
        };
        Some(
            gpui::deferred(
                div()
                    .id("join-request-reject-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !submitting {
                                this.join_request_reject_modal = None;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        div()
                            .id("join-request-reject-modal")
                            .w(px(440.))
                            .rounded(px(12.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-high"))
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(
                                div()
                                    .px(px(20.))
                                    .pt(px(20.))
                                    .pb(px(12.))
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(18.))
                                                    .font_weight(FontWeight(700.))
                                                    .text_color(color("--text-strong"))
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .text_color(color("--text-muted"))
                                                    .child(format!(
                                                        "Applicant: {}",
                                                        state.applicant_name
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("join-request-reject-close")
                                            .size(px(28.))
                                            .rounded(px(6.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.join_request_reject_modal = None;
                                                cx.notify();
                                            }))
                                            .child(icon("modal_close", 16., "--icon-subtle")),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(20.))
                                    .pb(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .p(px(10.))
                                            .min_h(px(72.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-normal"))
                                            .bg(color("--input-background-default"))
                                            .text_size(px(13.))
                                            .text_color(color(if state.reason.is_empty() {
                                                "--text-muted"
                                            } else {
                                                "--text-default"
                                            }))
                                            .child(if state.reason.is_empty() {
                                                "Optional rejection reason (max 160 characters)"
                                                    .to_owned()
                                            } else {
                                                state.reason.clone()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .justify_end()
                                            .text_size(px(11.))
                                            .text_color(color("--text-muted"))
                                            .child(format!(
                                                "{}/{}",
                                                state.reason.chars().count(),
                                                JOIN_REQUEST_REJECTION_MAX_LEN
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .px(px(20.))
                                    .py(px(14.))
                                    .bg(color("--background-base-lower"))
                                    .border_t_1()
                                    .border_color(color("--border-subtle"))
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .id("join-request-reject-cancel")
                                            .h(px(36.))
                                            .px(px(14.))
                                            .rounded(px(8.))
                                            .cursor_pointer()
                                            .hover(|s| s.bg(color("--background-mod-subtle")))
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .text_color(color("--text-default"))
                                            .child(dc_intl::t("ETE/oC"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.join_request_reject_modal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("join-request-reject-confirm")
                                            .h(px(36.))
                                            .px(px(16.))
                                            .rounded(px(8.))
                                            .bg(color("--status-danger"))
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--white"))
                                            .child(reject_cta)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let _ = this.requests.send(
                                                    reject_join_request_request(
                                                        guild_id,
                                                        request_id,
                                                        Some(&reason),
                                                    ),
                                                );
                                                this.join_request_reject_modal = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_join_request_reject(
        &mut self,
        arg: &str,
        cx: &mut Context<Self>,
    ) -> String {
        if arg.trim() == "close" {
            self.join_request_reject_modal = None;
            cx.notify();
            return "join-request-reject: closed".to_owned();
        }
        self.join_request_reject_modal = Some(JoinRequestRejectModalState {
            guild_id: self.guild.unwrap_or(1),
            request_id: 9001,
            applicant_name: "new_applicant".to_owned(),
            reason: "Please fill out all membership questions before reapplying.".to_owned(),
            submitting: false,
        });
        cx.notify();
        "join-request-reject: opened".to_owned()
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

    #[test]
    fn join_request_moderation_123393_requests_and_intl() {
        assert_eq!(dc_intl::t("R0RpRX"), "Oops, something went wrong...");
        assert!(matches!(
            approve_join_request_request(10, 20),
            crate::net::Request::Rest { route, body: Some(ref b), .. }
                if route == "/guilds/10/requests/20" && b["action"] == "APPROVED"
        ));
        let long_reason = "x".repeat(200);
        let req = reject_join_request_request(10, 20, Some(&long_reason));
        let crate::net::Request::Rest {
            route,
            body: Some(b),
            ..
        } = req
        else {
            panic!("expected Rest");
        };
        assert_eq!(route, "/guilds/10/requests/20");
        assert_eq!(b["action"], "REJECTED");
        assert_eq!(
            b["rejection_reason"].as_str().unwrap().len(),
            JOIN_REQUEST_REJECTION_MAX_LEN
        );
        assert!(matches!(
            create_join_request_interview_request(20),
            crate::net::Request::Rest { route, .. } if route == "/join-requests/20/interview"
        ));
    }
}
