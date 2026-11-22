//! App Title Bar Friends List Popout (module 824042, `container_SzQzM4`).
//!
//! Renders the title bar Friends icon button (`TdEu5X` tooltip, 6px red badge dot when incoming
//! pending friend requests > 0) and the `350px × 80vh` floating Friends List popout with Online /
//! All / Pending tabs, DM open, Voice Call, Accept / Ignore pending requests, context menus, and
//! `SEND_GAME_INVITE_MESSAGE` (`UVBA9g` `"Want to play {gameName}?"` / `DwAcMz` `"Want to play?"`).

use super::{Root, avatar_source, color, icon};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, SharedString, deferred, div, img,
    prelude::*, px, rgba,
};

/// Active tab inside the Title Bar Friends List Popout (module 824042).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FriendsPopoutTab {
    #[default]
    Online,
    All,
    Pending,
}

/// State of the Title Bar Friends List Popout (module 824042).
#[derive(Clone, Debug, Default)]
pub(super) struct FriendsPopoutState {
    pub open: bool,
    pub tab: FriendsPopoutTab,
}

/// Module 824042 (`SEND_GAME_INVITE_MESSAGE`): formats `"Want to play {gameName}?"` (`UVBA9g`) when a
/// game name is present, else `"Want to play?"` (`DwAcMz`).
pub(super) fn game_invite_message_content(game_name: Option<&str>) -> String {
    match game_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => dc_intl::format("UVBA9g", &[("gameName", name.into())]),
        None => dc_intl::t("DwAcMz").to_owned(),
    }
}

/// Counts incoming pending friend requests (`relationship type == 3`) for the titlebar red badge dot.
pub(super) fn pending_incoming_friend_count(
    relationships: &std::collections::HashMap<Id, u64>,
) -> usize {
    relationships.values().filter(|&&kind| kind == 3).count()
}

impl Root {
    /// Toggles the App Title Bar Friends List Popout (module 824042).
    pub(super) fn toggle_friends_popout(&mut self, cx: &mut Context<Self>) {
        self.friends_popout.open = !self.friends_popout.open;
        if self.friends_popout.open {
            self.inbox = None;
        }
        cx.notify();
    }

    /// Capture-harness opener for module 824042 (`friends-popout [open|pending|close]`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_friends_popout(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.friends_popout.open = false;
            cx.notify();
            return "friends-popout: closed".to_owned();
        }
        self.inbox = None;
        self.friends_popout.open = true;
        self.friends_popout.tab = match mode {
            "all" => FriendsPopoutTab::All,
            "pending" => FriendsPopoutTab::Pending,
            _ => FriendsPopoutTab::Online,
        };
        cx.notify();
        format!("friends-popout: {mode}")
    }

    /// Sends a `"Want to play ...?"` game invite message (`UVBA9g` / `DwAcMz`, module 824042) to an
    /// existing DM channel with `user_id` (or opens the DM if none exists yet).
    fn send_game_invite_to_friend(
        &mut self,
        user_id: Id,
        game_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let content = game_invite_message_content(game_name.as_deref());
        if let Some(dm_id) = self
            .stores
            .private_channels
            .iter()
            .find(|dm| dm.kind == 1 && dm.recipients.contains(&user_id))
            .map(|dm| dm.id)
        {
            let _ = self.requests.send(Request::Send {
                channel: dm_id,
                content,
                files: Vec::new(),
                reply: None,
            });
            self.show_success_toast(dc_intl::t("DwAcMz").to_owned(), cx);
        } else {
            self.open_dm_with(user_id, cx);
        }
    }

    /// Renders the `350px × 80vh` floating Friends List Popout (`container_SzQzM4`, module 824042).
    pub(super) fn friends_popout_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.friends_popout.open {
            return None;
        }
        let tab = self.friends_popout.tab;
        let pending_count = pending_incoming_friend_count(&self.stores.relationships);
        let popout_h = (self.viewport.1 * 0.80).clamp(260., 560.);

        let mut rows_data: Vec<_> = self
            .stores
            .relationships
            .iter()
            .filter_map(|(&uid, &kind)| {
                let status = self
                    .stores
                    .presences
                    .get(&uid)
                    .map(String::as_str)
                    .unwrap_or("offline");
                let keep = match tab {
                    FriendsPopoutTab::Online => kind == 1 && status != "offline",
                    FriendsPopoutTab::All => kind == 1,
                    FriendsPopoutTab::Pending => kind == 3 || kind == 4,
                };
                if !keep {
                    return None;
                }
                let user = self.stores.users.get(&uid);
                let name = user
                    .map(|u| u.display_name().to_owned())
                    .unwrap_or_else(|| format!("User {uid}"));
                let avatar = user.and_then(|u| {
                    let (key, _) = avatar_source(u);
                    self.images.get(&key).cloned()
                });
                let game_name = self.stores.presence.get(uid, None).and_then(|p| {
                    p.visible_activities()
                        .iter()
                        .find(|a| a["type"].as_u64() == Some(0))
                        .and_then(|a| a["name"].as_str())
                        .map(str::to_owned)
                });
                Some((uid, name, status.to_owned(), game_name, kind, avatar))
            })
            .collect();
        rows_data.sort_by_key(|a| a.1.to_lowercase());

        let tab_pill = |target: FriendsPopoutTab, id: &'static str, label: String| {
            let active = tab == target;
            div()
                .id(id)
                .px(px(10.))
                .h(px(26.))
                .rounded(px(6.))
                .bg(if active {
                    color("--background-mod-strong")
                } else {
                    rgba(0x00000000)
                })
                .hover(|s| s.bg(color("--background-mod-subtle")))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(13.))
                .font_weight(FontWeight(if active { 600. } else { 500. }))
                .text_color(color(if active {
                    "--text-strong"
                } else {
                    "--text-muted"
                }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.friends_popout.tab = target;
                    cx.notify();
                }))
        };

        let body: AnyElement = if rows_data.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .p(px(24.))
                .child(icon("friends_header", 36., "--text-muted"))
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .text_center()
                        .child(match tab {
                            FriendsPopoutTab::Online => "No friends online right now.",
                            FriendsPopoutTab::All => "No friends added yet.",
                            FriendsPopoutTab::Pending => "No pending friend requests.",
                        }),
                )
                .into_any_element()
        } else {
            div()
                .id("friends-popout-list")
                .flex_1()
                .overflow_y_scroll()
                .px(px(8.))
                .py(px(6.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(rows_data.into_iter().map(
                    |(uid, name, status, game_name, kind, avatar)| {
                        let initial = name
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string();
                        let sub = game_name.clone().unwrap_or_else(|| match kind {
                            3 => "Incoming Friend Request".to_owned(),
                            4 => "Outgoing Friend Request".to_owned(),
                            _ => status.clone(),
                        });
                        let status_dot = match status.as_str() {
                            "online" => color("--status-positive"),
                            "idle" => color("--status-warning"),
                            "dnd" => color("--status-danger"),
                            _ => color("--text-muted"),
                        };
                        let invite_game = game_name.clone();
                        div()
                            .id(SharedString::from(format!("friends-popout-row-{uid}")))
                            .w_full()
                            .px(px(8.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .on_mouse_down(
                                gpui::MouseButton::Right,
                                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.open_context_menu(
                                        super::context_menu::Target::User {
                                            user: uid,
                                            guild: None,
                                        },
                                        ev.position,
                                        cx,
                                    );
                                }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.friends_popout.open = false;
                                this.open_dm_with(uid, cx);
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        div()
                                            .relative()
                                            .size(px(32.))
                                            .flex_none()
                                            .child(
                                                div()
                                                    .size(px(32.))
                                                    .rounded_full()
                                                    .overflow_hidden()
                                                    .bg(color("--background-brand"))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(match avatar {
                                                        Some(image) => img(image)
                                                            .size_full()
                                                            .object_fit(ObjectFit::Cover)
                                                            .into_any_element(),
                                                        None => div()
                                                            .text_size(px(13.))
                                                            .font_weight(FontWeight(700.))
                                                            .text_color(color("--white"))
                                                            .child(initial)
                                                            .into_any_element(),
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .right(px(-1.))
                                                    .bottom(px(-1.))
                                                    .size(px(10.))
                                                    .rounded_full()
                                                    .border_2()
                                                    .border_color(color(
                                                        "--background-surface-high",
                                                    ))
                                                    .bg(status_dot),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .text_size(px(14.))
                                                    .line_height(px(18.))
                                                    .font_weight(FontWeight(600.))
                                                    .text_color(color("--text-strong"))
                                                    .truncate()
                                                    .child(name),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .line_height(px(16.))
                                                    .text_color(color("--text-muted"))
                                                    .truncate()
                                                    .child(sub),
                                            ),
                                    ),
                            )
                            .child(if kind == 3 {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("fp-accept-{uid}")))
                                            .size(px(28.))
                                            .rounded_full()
                                            .bg(color("--background-base-lower"))
                                            .hover(|s| s.bg(color("--status-positive")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child(icon("friend_accept", 16., "--text-strong"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                let _ = this.requests.send(Request::Rest {
                                                    key: Some(format!("friend-accept:{uid}")),
                                                    method: reqwest::Method::PUT,
                                                    route: format!(
                                                        "/users/@me/relationships/{uid}"
                                                    ),
                                                    body: Some(serde_json::json!({})),
                                                });
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("fp-ignore-{uid}")))
                                            .size(px(28.))
                                            .rounded_full()
                                            .bg(color("--background-base-lower"))
                                            .hover(|s| s.bg(color("--status-danger")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child(icon("friend_ignore", 16., "--text-strong"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                let _ = this.requests.send(Request::Rest {
                                                    key: Some(format!("friend-ignore:{uid}")),
                                                    method: reqwest::Method::DELETE,
                                                    route: format!(
                                                        "/users/@me/relationships/{uid}"
                                                    ),
                                                    body: None,
                                                });
                                            })),
                                    )
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("fp-invite-{uid}")))
                                            .size(px(28.))
                                            .rounded_full()
                                            .bg(color("--background-base-lower"))
                                            .hover(|s| s.bg(color("--background-mod-strong")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child(icon("clips_play", 14., "--text-strong"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.send_game_invite_to_friend(
                                                    uid,
                                                    invite_game.clone(),
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("fp-dm-{uid}")))
                                            .size(px(28.))
                                            .rounded_full()
                                            .bg(color("--background-base-lower"))
                                            .hover(|s| s.bg(color("--background-mod-strong")))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child(icon("channel_type_text", 14., "--text-strong"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.friends_popout.open = false;
                                                this.open_dm_with(uid, cx);
                                            })),
                                    )
                            })
                    },
                ))
                .into_any_element()
        };

        let card = div()
            .id("friends-popout-card")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .top(px(36.))
            .right(px(56.))
            .w(px(350.))
            .h(px(popout_h))
            .rounded(px(8.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(14.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(icon("friends_header", 18., "--text-strong"))
                                    .child(
                                        div()
                                            .text_size(px(16.))
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(700.))
                                            .text_color(color("--text-strong"))
                                            .child(dc_intl::t("TdEu5X")),
                                    ),
                            )
                            .child(
                                div()
                                    .id("friends-popout-close")
                                    .size(px(26.))
                                    .rounded(px(6.))
                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("modal_close", 14., "--interactive-icon-default"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.friends_popout.open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(tab_pill(
                                FriendsPopoutTab::Online,
                                "fp-tab-online",
                                "Online".to_owned(),
                            ))
                            .child(tab_pill(
                                FriendsPopoutTab::All,
                                "fp-tab-all",
                                "All".to_owned(),
                            ))
                            .child(tab_pill(
                                FriendsPopoutTab::Pending,
                                "fp-tab-pending",
                                if pending_count > 0 {
                                    format!("Pending ({pending_count})")
                                } else {
                                    "Pending".to_owned()
                                },
                            )),
                    ),
            )
            .child(body);

        Some(
            deferred(
                div()
                    .id("friends-popout-backdrop")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.friends_popout.open = false;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friends_popout_824042_keys_and_game_invite_formatting() {
        assert_eq!(dc_intl::t("TdEu5X"), "Friends");
        assert_eq!(game_invite_message_content(None), dc_intl::t("DwAcMz"));
        let named = game_invite_message_content(Some("Valorant"));
        assert!(named.contains("Valorant"), "got {named}");
        let mut rels = std::collections::HashMap::new();
        rels.insert(1, 1);
        rels.insert(2, 3);
        rels.insert(3, 3);
        rels.insert(4, 4);
        assert_eq!(pending_incoming_friend_count(&rels), 2);
    }
}
