//! The Friends page: the tab bar (Online, All, Pending while requests wait, Add Friend), the people column with its
//! search, and Add Friend's form. Measured in `design/dom/friends/*-darker-cozy.json` and `home/friends-darker-cozy`.
//! Requests: `POST /users/@me/relationships {username, discriminator, note}` (module 49229), accept with
//! `PUT /users/@me/relationships/{id}`, ignore, cancel and remove with `DELETE` on the same route; a DM opens with
//! `POST /users/@me/channels {recipients}` when none exists.

use super::{Root, avatar_source, color, editor_style, icon, status_dot, status_label};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, SharedString, Subscription,
    Window, deferred, div, img, prelude::*, px,
};
use serde_json::json;

pub(super) const RELATIONSHIP_FRIEND: u64 = 1;
pub(super) const RELATIONSHIP_INCOMING: u64 = 3;
pub(super) const RELATIONSHIP_OUTGOING: u64 = 4;
const NOTE_LIMIT: usize = 120;
pub(super) const FRIEND_REQUEST_KEY: &str = "friend-request:";
pub(super) const OPEN_DM_KEY: &str = "open-dm";
const WUMPUS: &str = "https://discordapp.com/assets/25c27c1328c986f6.svg";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FriendsTab {
    #[default]
    Online,
    All,
    Pending,
    Add,
}

/// Add Friend's two fields and what the last request answered.
pub(crate) struct AddFriend {
    pub name: Entity<Editor>,
    pub note: Entity<Editor>,
    _events: [Subscription; 2],
    /// `Ok(username)` after a sent request, `Err(())` after a refused one.
    pub result: Option<Result<String, ()>>,
}

/// The people column's search field.
pub(crate) struct PeopleSearch {
    pub field: Entity<Editor>,
    _events: Subscription,
    pub query: String,
}

/// A friend row's open menu (Start Video Call, Start Voice Call, Remove Friend).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FriendMenu {
    pub user: Id,
    pub at: gpui::Point<gpui::Pixels>,
}

impl Root {
    pub(super) fn set_friends_tab(
        &mut self,
        tab: FriendsTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.friends_tab = tab;
        if tab == FriendsTab::Add {
            self.request_image("friends-wumpus".to_owned(), WUMPUS.to_owned());
        }
        if tab == FriendsTab::Add && self.add_friend.is_none() {
            let name = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder("Enter a username".to_owned());
                editor
            });
            let note = cx.new(|cx| {
                let mut editor = Editor::new(editor_style(), cx);
                editor.set_placeholder("Personalize your request (Optional)".to_owned());
                editor
            });
            // Enter submits the form; the editor empties itself on Enter, so the text goes back first.
            let name_events = cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
                match event {
                    EditorEvent::Submit(text) => {
                        editor.update(cx, |editor, _| editor.set_text(text));
                        this.send_friend_request(cx);
                    }
                    EditorEvent::Edited => {
                        if let Some(form) = this.add_friend.as_mut() {
                            form.result = None;
                        }
                    }
                    EditorEvent::Cancel => {}
                }
                cx.notify();
            });
            let note_events = cx.subscribe(&note, |this, editor, event: &EditorEvent, cx| {
                if let EditorEvent::Submit(text) = event {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.send_friend_request(cx);
                }
                cx.notify();
            });
            self.add_friend = Some(AddFriend {
                name,
                note,
                _events: [name_events, note_events],
                result: None,
            });
        }
        if tab == FriendsTab::Add
            && let Some(form) = self.add_friend.as_ref()
        {
            window.focus(&form.name.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn people_search_field(&mut self, cx: &mut Context<Self>) -> Entity<Editor> {
        if let Some(search) = self.people_search.as_ref() {
            return search.field.clone();
        }
        let field = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        let events = cx.subscribe(&field, |this, editor, event: &EditorEvent, cx| {
            if let (EditorEvent::Edited, Some(search)) = (event, this.people_search.as_mut()) {
                search.query = editor.read(cx).text().to_owned();
            }
            cx.notify();
        });
        self.people_search = Some(PeopleSearch {
            field: field.clone(),
            _events: events,
            query: String::new(),
        });
        field
    }

    pub(super) fn send_friend_request(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.add_friend.as_ref() else {
            return;
        };
        let username = form.name.read(cx).text().trim().to_owned();
        if username.is_empty() {
            return;
        }
        let note: String = form
            .note
            .read(cx)
            .text()
            .trim()
            .chars()
            .take(NOTE_LIMIT)
            .collect();
        let (name, discriminator) = match username.split_once('#') {
            Some((name, tag)) => (name.to_owned(), tag.parse::<u64>().ok()),
            None => (username.clone(), None),
        };
        let mut body = json!({"username": name, "discriminator": discriminator});
        if !note.is_empty() {
            body["note"] = json!(note);
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{FRIEND_REQUEST_KEY}{username}")),
            method: reqwest::Method::POST,
            route: "/users/@me/relationships".to_owned(),
            body: Some(body),
        });
        cx.notify();
    }

    /// Answers to this page's REST calls; true when `key` was one of them.
    pub(super) fn friends_fetched(
        &mut self,
        key: &str,
        report: &serde_json::Value,
        cx: &mut Context<Self>,
    ) -> bool {
        let ok = report["ok"].as_bool().unwrap_or(false);
        if let Some(username) = key.strip_prefix(FRIEND_REQUEST_KEY) {
            tracing::debug!(
                "friend request answered {}: {}",
                report["status"],
                report["body"]["code"]
            );
            self.friend_request_answered(username.to_owned(), ok, cx);
            return true;
        }
        if key == OPEN_DM_KEY {
            if ok {
                self.stores.add_private_channel(&report["body"]);
                if let Some(channel) = report["body"]["id"]
                    .as_str()
                    .and_then(|id| id.parse::<Id>().ok())
                {
                    self.select_home(Some(channel));
                }
            }
            cx.notify();
            return true;
        }
        false
    }

    /// The answer to a friend request from the network side.
    pub(super) fn friend_request_answered(
        &mut self,
        username: String,
        sent: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = self.add_friend.as_ref()
            && sent
        {
            // Cleared first: clearing reports an edit, which drops the last result.
            form.name.update(cx, |editor, _| editor.set_text(""));
            form.note.update(cx, |editor, _| editor.set_text(""));
        }
        if let Some(form) = self.add_friend.as_mut() {
            form.result = Some(if sent { Ok(username) } else { Err(()) });
        }
        cx.notify();
    }

    /// The DM with `user`, opened; created first when there is none.
    pub(super) fn open_dm_with(&mut self, user: Id, cx: &mut Context<Self>) {
        let existing = self
            .stores
            .private_channels
            .iter()
            .find(|channel| channel.recipients == [user])
            .map(|channel| channel.id);
        match existing {
            Some(channel) => self.select_home(Some(channel)),
            None => {
                let _ = self.requests.send(Request::Rest {
                    key: Some(OPEN_DM_KEY.to_owned()),
                    method: reqwest::Method::POST,
                    route: "/users/@me/channels".to_owned(),
                    body: Some(json!({"recipients": [user.to_string()]})),
                });
            }
        }
        cx.notify();
    }

    /// `addRelationship` / `blockUser` / `removeRelationship` of module 49229: `PUT /users/@me/relationships/{id}`
    /// with `{}` (a friend request, or accepting one) or `{type: 2}` (block), `DELETE` for `None`. The store
    /// changes at once; the gateway's RELATIONSHIP events confirm it.
    pub(super) fn set_relationship(&mut self, user: Id, kind: Option<u64>, cx: &mut Context<Self>) {
        let body = match kind {
            Some(2) => Some(json!({"type": 2})),
            Some(_) => Some(json!({})),
            None => None,
        };
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: if kind.is_some() {
                reqwest::Method::PUT
            } else {
                reqwest::Method::DELETE
            },
            route: format!("/users/@me/relationships/{user}"),
            body,
        });
        match kind {
            Some(2) => {
                self.stores.relationships.insert(user, 2);
            }
            // Accepting makes a friend; otherwise the request waits.
            Some(_) => {
                let next = if self.stores.relationships.get(&user) == Some(&RELATIONSHIP_INCOMING) {
                    RELATIONSHIP_FRIEND
                } else {
                    RELATIONSHIP_OUTGOING
                };
                self.stores.relationships.insert(user, next);
            }
            None => {
                self.stores.relationships.remove(&user);
            }
        }
        self.stores.clear_relationship_request_flags(user);
        self.rebuild_rows();
        cx.notify();
    }

    /// Accept is `PUT {}` on the relationship; ignore, cancel and remove are `DELETE`.
    fn relationship_action(&mut self, user: Id, accept: bool) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: if accept {
                reqwest::Method::PUT
            } else {
                reqwest::Method::DELETE
            },
            route: format!("/users/@me/relationships/{user}"),
            body: accept.then(|| json!({})),
        });
        if accept {
            self.stores.relationships.insert(user, RELATIONSHIP_FRIEND);
        } else {
            self.stores.relationships.remove(&user);
        }
        self.stores.clear_relationship_request_flags(user);
    }

    fn people_rows(&self) -> (String, Vec<(User, u64)>) {
        let query = self
            .people_search
            .as_ref()
            .map(|search| search.query.trim().to_lowercase())
            .unwrap_or_default();
        let matches = |user: &User| {
            query.is_empty()
                || user.display_name().to_lowercase().contains(&query)
                || user.username.to_lowercase().contains(&query)
        };
        match self.friends_tab {
            FriendsTab::Online | FriendsTab::Add => {
                let list: Vec<(User, u64)> = self
                    .stores
                    .online_friends()
                    .into_iter()
                    .filter(|user| matches(user))
                    .map(|user| (user.clone(), RELATIONSHIP_FRIEND))
                    .collect();
                (format!("Online \u{2014} {}", list.len()), list)
            }
            FriendsTab::All => {
                let list: Vec<(User, u64)> = self
                    .stores
                    .friends()
                    .into_iter()
                    .filter(|user| matches(user))
                    .map(|user| (user.clone(), RELATIONSHIP_FRIEND))
                    .collect();
                (format!("All friends \u{2014} {}", list.len()), list)
            }
            FriendsTab::Pending => {
                let mut list: Vec<(User, u64)> = self
                    .stores
                    .relationships
                    .iter()
                    .filter(|(_, kind)| {
                        matches!(**kind, RELATIONSHIP_INCOMING | RELATIONSHIP_OUTGOING)
                    })
                    .filter_map(|(user, kind)| Some((self.stores.users.get(user)?.clone(), *kind)))
                    .filter(|(user, _)| matches(user))
                    .collect();
                list.sort_by_key(|(user, _)| user.display_name().to_lowercase());
                (format!("Pending \u{2014} {}", list.len()), list)
            }
        }
    }

    fn has_pending(&self) -> bool {
        #[cfg(feature = "capture")]
        if self.capture_requests.is_some_and(|count| count > 0) {
            return true;
        }
        self.stores
            .relationships
            .values()
            .any(|kind| matches!(*kind, RELATIONSHIP_INCOMING | RELATIONSHIP_OUTGOING))
    }

    fn person_row(&self, user: &User, kind: u64, cx: &mut Context<Self>) -> AnyElement {
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let status = self.stores.status(user.id).to_owned();
        let id = user.id;
        let group = SharedString::from(format!("friend-{}", user.id));
        let menu_open = self.friend_menu.is_some_and(|menu| menu.user == id);
        // `actionButton_5GqXgh`: surface-high, the lowest base while the row is hovered or its menu is open.
        let action = |name: &'static str, key: String, _tint: &'static str| {
            div()
                .id(SharedString::from(key))
                .size(px(36.))
                .rounded_full()
                .bg(if menu_open {
                    color("--background-base-lowest")
                } else {
                    color("--background-surface-high")
                })
                .group_hover(group.clone(), |style| {
                    style.bg(color("--background-base-lowest"))
                })
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(name, 20., "--text-subtle"))
        };
        let subtitle = match kind {
            RELATIONSHIP_INCOMING => "Incoming Friend Request".to_owned(),
            RELATIONSHIP_OUTGOING => "Outgoing Friend Request".to_owned(),
            _ => status_label(&status).to_owned(),
        };
        let actions: Vec<AnyElement> = match kind {
            RELATIONSHIP_INCOMING => vec![
                action(
                    "friend_accept",
                    format!("accept-{id}"),
                    "--interactive-text-default",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.relationship_action(id, true);
                    cx.notify();
                }))
                .into_any_element(),
                action(
                    "friend_ignore",
                    format!("ignore-{id}"),
                    "--interactive-text-default",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.relationship_action(id, false);
                    cx.notify();
                }))
                .into_any_element(),
            ],
            RELATIONSHIP_OUTGOING => vec![
                action(
                    "friend_ignore",
                    format!("cancel-{id}"),
                    "--interactive-text-default",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.relationship_action(id, false);
                    cx.notify();
                }))
                .into_any_element(),
            ],
            _ => vec![
                action(
                    "chat_bubble",
                    format!("message-{id}"),
                    "--interactive-text-default",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_dm_with(id, cx);
                }))
                .into_any_element(),
                action(
                    "more_dots",
                    format!("more-{id}"),
                    "--interactive-text-default",
                )
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.friend_menu = Some(FriendMenu {
                        user: id,
                        at: event.position(),
                    });
                    cx.notify();
                }))
                .into_any_element(),
            ],
        };
        div()
            .relative()
            .pl(px(14.))
            .pr(px(26.))
            .child(
                div()
                    .absolute()
                    .left(px(30.))
                    .top(px(0.))
                    .w(px(479.))
                    .h(px(1.))
                    .bg(color("--background-mod-subtle")),
            )
            .child(
                div()
                    .id(SharedString::from(format!("person-{id}")))
                    .group(group.clone())
                    .h(px(61.))
                    .px(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .when(menu_open, |this| this.bg(color("--background-mod-normal")))
                    .hover(|style| style.bg(color("--background-mod-subtle")))
                    .when(kind == RELATIONSHIP_FRIEND, |this| {
                        this.on_click(cx.listener(move |this, _, _, cx| this.open_dm_with(id, cx)))
                    })
                    // A pending request opens the person's profile.
                    .when(kind != RELATIONSHIP_FRIEND, |this| {
                        this.on_click(cx.listener(move |this, _, window, cx| {
                            this.open_full_profile(id, window, cx)
                        }))
                    })
                    .child(
                        div()
                            .relative()
                            .size(px(32.))
                            .children(avatar.map(|image| img(image).size(px(32.)).rounded_full()))
                            .when(kind == RELATIONSHIP_FRIEND, |this| {
                                this.child(status_dot(&status, "--background-base-lower"))
                            }),
                    )
                    .child(
                        div()
                            .ml(px(12.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(600.))
                                            .text_color(color("--text-strong"))
                                            .child(user.display_name().to_owned()),
                                    )
                                    .child(
                                        div()
                                            .ml(px(5.))
                                            .opacity(0.)
                                            .group_hover(group, |style| style.opacity(1.))
                                            .text_size(px(14.))
                                            .line_height(px(16.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-default"))
                                            .child(user.username.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(17.5))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--text-muted"))
                                    .child(if kind == RELATIONSHIP_FRIEND {
                                        self.presence_line(user, None, false).unwrap_or_else(|| {
                                            div().child(subtitle.clone()).into_any_element()
                                        })
                                    } else {
                                        div().child(subtitle).into_any_element()
                                    }),
                            ),
                    )
                    .child(div().flex_1())
                    .child(div().flex().gap(px(10.)).children(actions)),
            )
            .into_any_element()
    }

    pub(super) fn friends_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.friends_tab;
        let requests = self.friend_request_count();
        let tab = |id: &'static str, label: &'static str, which: FriendsTab| {
            let active = current == which;
            div()
                .id(id)
                .mx(px(8.))
                .h(px(32.))
                .px(px(12.))
                .py(px(4.))
                .rounded(px(8.))
                .cursor_pointer()
                .flex()
                .items_center()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .when(active, |this| {
                    this.bg(color("--background-mod-strong"))
                        .text_color(color("--text-strong"))
                })
                .when(!active, |this| {
                    this.text_color(color("--interactive-text-default"))
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_friends_tab(which, window, cx)),
                )
                .child(label)
                .when(which == FriendsTab::Pending && requests > 0, |tab| {
                    tab.child(
                        super::unread_badges::number(
                            requests,
                            "--badge-notification-background",
                            Some("pending-request-badge"),
                        )
                        .ml(px(8.)),
                    )
                })
        };
        let adding = current == FriendsTab::Add;
        div()
            .parity_below("chat-header", 1.)
            .flex_none()
            .h(px(49.))
            .border_b_1()
            .border_color(color("--background-mod-subtle"))
            .pl(px(22.))
            .pr(px(8.))
            .flex()
            .items_center()
            .bg(color("--background-base-lower"))
            .child(icon("friends_header", 20., "--channels-default"))
            .child(
                div()
                    .ml(px(6.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Friends"),
            )
            .child(
                div()
                    .ml(px(12.))
                    .size(px(4.))
                    .rounded_full()
                    .bg(color("--background-mod-strong")),
            )
            .child(div().flex_none().w(px(4.)))
            .when(!self.stores.online_friends().is_empty(), |header| {
                header.child(tab("friends-online", "Online", FriendsTab::Online))
            })
            .when(!self.stores.friends().is_empty(), |header| {
                header.child(tab("friends-all", "All", FriendsTab::All))
            })
            .when(self.has_pending(), |this| {
                this.child(tab("friends-pending", "Pending", FriendsTab::Pending))
            })
            .child(
                // `addFriend_zrUwGA`: brand fill, or while open a 16% brand tint with brand text.
                div()
                    .id("friends-add")
                    .mx(px(8.))
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .when(adding, |this| {
                        this.bg(gpui::rgba(0x5865f229))
                            .text_color(color("--text-brand"))
                    })
                    .when(!adding, |this| {
                        this.bg(color("--background-brand"))
                            .text_color(color("--white"))
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_friends_tab(FriendsTab::Add, window, cx)
                    }))
                    .child("Add Friend"),
            )
            .child(div().flex_1())
            // `toolbar_ybUK7Z` (8px in) holding `inviteToolbar_zrUwGA` (8px out): New Group DM, a 32px button.
            .child(
                div().pl(px(8.)).pr(px(8.)).child(
                    div()
                        .id("new-group-dm")
                        .size(px(32.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_group_dm_picker(window, cx);
                        }))
                        .child(icon("new_group", 20., "--text-muted")),
                ),
            )
    }

    /// `addFriendInputWrapper`, measured in `friends/tab-add-friend`: header 30px in, 20px down; the form 8px
    /// under the title; the wrapper 16px under the subtitle with the username row (56) over the note (91).
    fn add_friend_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.add_friend.as_ref() else {
            return div().into_any_element();
        };
        let typed = !form.name.read(cx).text().trim().is_empty();
        let note_length = form.note.read(cx).text().chars().count();
        let failed = matches!(form.result, Some(Err(())));
        let wumpus = self.images.get("friends-wumpus").cloned();
        let line = |tint: &'static str| {
            div()
                .mt(px(8.))
                .text_size(px(14.))
                .line_height(px(18.))
                .text_color(color(tint))
        };
        let helper: AnyElement = match &form.result {
            // `Rtl1Ep`: "Success! Your friend request to **{discordTag}** was sent."
            Some(Ok(username)) => line("--text-feedback-positive")
                .flex()
                .child("Success! Your friend request to\u{a0}")
                .child(div().font_weight(FontWeight(600.)).child(username.clone()))
                .child("\u{a0}was sent.")
                .into_any_element(),
            Some(Err(())) => line("--text-feedback-critical")
                .child("Hm, didn\u{2019}t work. Double check that the username is correct.")
                .into_any_element(),
            None => line("--text-subtle")
                .child("What you write here will also show up in your DM if you become friends.")
                .into_any_element(),
        };
        let heading = |text: &'static str| {
            div()
                .h(px(24.))
                .text_size(px(20.))
                .line_height(px(24.))
                .font_weight(FontWeight(600.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let body_text = |text: &'static str| {
            div()
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-default"))
                .child(text)
        };
        let send = div()
            .parity_with_border("send-friend-request", 1.)
            .id("send-friend-request")
            .ml(px(16.))
            .flex_none()
            .w(px(149.02))
            .h(px(32.))
            .rounded(px(8.))
            .border_1()
            .border_color(color("--control-primary-border-default"))
            .bg(color("--control-primary-background-default"))
            .when(!typed, |this| this.opacity(0.5))
            .when(typed, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--control-primary-background-hover")))
            })
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .line_height(px(18.))
            .font_weight(FontWeight(500.))
            .text_color(color("--white"))
            .on_click(cx.listener(|this, _, _, cx| this.send_friend_request(cx)))
            .child("Send Friend Request");
        let wrapper = div()
            .parity_with_border("add-friend-wrapper", 1.)
            .mt(px(16.))
            .px(px(12.))
            .rounded(px(16.))
            .border_1()
            .border_color(if failed {
                color("--border-feedback-critical")
            } else {
                color("--border-subtle")
            })
            .bg(if failed {
                color("--input-background-error-default")
            } else {
                color("--input-background-default")
            })
            .child(
                div()
                    .h(px(56.))
                    .py(px(2.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .h(px(52.))
                            .py(px(4.))
                            .flex()
                            .items_center()
                            .child(div().w_full().child(form.name.clone())),
                    )
                    .child(send),
            )
            .child(
                div()
                    .h(px(91.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .pt(px(12.))
                    .child(div().h(px(41.)).child(form.note.clone()))
                    .child(
                        div()
                            .mt(px(3.))
                            .h(px(22.))
                            .pt(px(4.))
                            .flex()
                            .justify_end()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(NOTE_LIMIT.saturating_sub(note_length).to_string()),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(20.))
                    .pb(px(20.))
                    .px(px(30.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .child(heading("Add Friend"))
                    .child(
                        div()
                            .mt(px(8.))
                            .relative()
                            .child(body_text("You can add friends with their Discord username."))
                            .child(wrapper)
                            .child(helper)
                            // "Wumpus Waving", 84x72, 385px in and 36px above the subtitle.
                            .children(wumpus.map(|image| {
                                div()
                                    .absolute()
                                    .left(px(385.))
                                    .top(px(-36.))
                                    .child(img(image).w(px(84.)).h(px(72.)))
                            })),
                    ),
            )
            .child(
                div()
                    .pt(px(20.))
                    .pb(px(20.))
                    .px(px(30.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(heading("Other Places to Make Friends"))
                    .child(body_text(
                        "Don't have a username on hand? Check out our list of public servers that includes everything from gaming to cooking, music, anime and more.",
                    )),
            )
            .child(
                div().pl(px(20.)).child(
                    // `container_OlkxSE`: the Discover button, which opens server discovery.
                    div()
                        .parity_with_border("explore-servers", 1.)
                        .id("explore-servers")
                        .w(px(236.5))
                        .h(px(64.))
                        .py(px(1.))
                        .pl(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .cursor_pointer()
                        .hover(|style| style.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .on_click(|_, _, cx| cx.open_url("https://discord.com/discovery"))
                        .child(
                            div()
                                .ml(px(8.))
                                .size(px(36.))
                                .flex_none()
                                .rounded(px(8.))
                                .bg(gpui::rgb(0x3d9e60))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("discover", 24., "--white")),
                        )
                        .child(
                            div()
                                .ml(px(8.))
                                .w(px(140.47))
                                .flex_none()
                                .text_center()
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-default"))
                                .child("Explore Discoverable Servers"),
                        )
                        .child(
                            div()
                                .w(px(14.03))
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .child(icon("explore_arrow", 14., "--interactive-text-default")),
                        ),
                ),
            )
            .into_any_element()
    }

    pub(super) fn friends_people(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.friends_tab == FriendsTab::Add {
            return self.add_friend_form(cx);
        }
        let field = self.people_search_field(cx);
        let (label, people) = self.people_rows();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .parity_with_border("people-search", 1.)
                    .mt(px(12.))
                    .ml(px(24.))
                    .w(px(505.))
                    .h(px(40.))
                    .rounded(px(8.))
                    .bg(color("--input-background-default"))
                    .border_1()
                    .border_color(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .pl(px(11.))
                    .child(icon("search_people", 16., "--interactive-text-default"))
                    .child(
                        div()
                            .ml(px(11.))
                            .flex_1()
                            .min_w(px(0.))
                            .pr(px(8.))
                            .child(field),
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .ml(px(24.))
                    .h(px(34.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(label),
            )
            .child(
                div()
                    .id("people-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(
                        people
                            .iter()
                            .map(|(user, kind)| self.person_row(user, *kind, cx)),
                    ),
            )
            .into_any_element()
    }

    /// The friend row's menu: calls and Remove Friend (danger), which asks first.
    pub(super) fn friend_menu_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.friend_menu?;
        let user = menu.user;
        let row = |id: &'static str, label: &'static str, danger: bool| {
            div()
                .id(id)
                .h(px(34.))
                .px(px(8.))
                .rounded(px(2.))
                .cursor_pointer()
                .flex()
                .items_center()
                .text_size(px(14.))
                .line_height(px(18.))
                .font_weight(FontWeight(500.))
                .text_color(if danger {
                    color("--text-feedback-critical")
                } else {
                    color("--text-strong")
                })
                .hover(move |style| {
                    style
                        .bg(if danger {
                            color("--background-feedback-mod-critical")
                        } else {
                            color("--interactive-background-hover")
                        })
                        .rounded(px(4.))
                })
                .child(label)
        };
        Some(
            deferred(
                gpui::anchored()
                    .position(menu.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .parity_with_border("friend-menu", 1.)
                            .id("friend-menu")
                            .occlude()
                            // `menu_M3qk4j`, 144 wide at the pointer, 8px inside; items 34 tall.
                            .w(px(144.))
                            .p(px(8.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-subtle"))
                            .bg(color("--background-surface-higher"))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.24),
                                offset: gpui::point(px(0.), px(12.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.friend_menu = None;
                                cx.notify();
                            }))
                            .child(row("friend-video", "Start Video Call", false).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.friend_menu = None;
                                    this.start_dm_call(user, cx);
                                }),
                            ))
                            .child(row("friend-voice", "Start Voice Call", false).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.friend_menu = None;
                                    this.start_dm_call(user, cx);
                                }),
                            ))
                            .child(row("friend-remove", "Remove Friend", true).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.friend_menu = None;
                                    this.confirm_remove_friend = Some(user);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// A call from the friends list: the DM opens and the call starts there.
    pub(super) fn start_dm_call(&mut self, user: Id, cx: &mut Context<Self>) {
        self.open_dm_with(user, cx);
    }

    /// "Remove 'name'" (`fPLvZd`, `l5FFq6`).
    pub(super) fn remove_friend_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let user = self.confirm_remove_friend?;
        let name = self
            .stores
            .users
            .get(&user)
            .map(|known| known.display_name().to_owned())
            .unwrap_or_default();
        Some(super::modal::render(
            super::modal::Spec {
                id: "remove-friend-modal",
                title: format!("Remove '{name}'").into(),
                subtitle: Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .child("Are you sure you want to remove\u{a0}")
                        .child(div().font_weight(FontWeight(600.)).child(name.clone()))
                        .child("\u{a0}from your friends?")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "remove-friend-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| this.confirm_remove_friend = None,
                    ),
                    super::modal::button(
                        "remove-friend-confirm",
                        "Remove Friend",
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            this.confirm_remove_friend = None;
                            this.relationship_action(user, false);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.confirm_remove_friend = None),
            },
            cx,
        ))
    }
}
