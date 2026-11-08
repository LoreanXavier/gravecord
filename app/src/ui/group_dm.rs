//! New Message (`jD1qzM`): the group DM picker that the Friends page's New Group DM button and the Direct Messages
//! "+" open. Ported from module 753190 (the modal) and module 584777 (`PrivateChannelRecipientsInviteStore`):
//! with no query the rows are friends plus affinity users who are not friends, blocked or ignored, sorted by
//! affinity (`communicationProbability`) and then by name; typing filters by name. Rows toggle into tags in the
//! search field (Backspace on an empty query drops the last); Up/Down move the highlighted row and Enter toggles it.
//! Create sends `POST /users/@me/channels {recipients}`: one person opens the DM, more make a group.
//! Measured in `design/dom/friends/group-dm-darker-cozy.json` and `group-dm-selected-darker-cozy.json`.

use super::{Root, avatar_source, color, editor_style, icon, modal, status_dot};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, User};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, SharedString, Subscription,
    Window, div, img, prelude::*, px,
};
use serde_json::{Value, json};

/// `K.A({useNitroCapExperiment})` without the experiment: a group DM holds ten people, you included.
pub(super) const GROUP_LIMIT: usize = 10;
const AFFINITIES_KEY: &str = "affinities";
const RELATIONSHIP_BLOCKED: u64 = 2;
const RELATIONSHIP_FRIEND: u64 = 1;
/// The highlighted row's fill, `rgba(79, 80, 90, 0.3)` in the capture; no token carries it.
const ROW_SELECTED: u32 = 0x4f505a4d;

pub(crate) struct GroupDm {
    pub search: Entity<Editor>,
    _events: Subscription,
    pub query: String,
    /// In the order they were picked; the tags show in this order.
    pub selected: Vec<Id>,
    /// The keyboard-highlighted row.
    pub row: usize,
}

/// The users of `USER_AFFINITIES_V2` (`/users/@me/affinities/v2/users`, module 404885) with their communication
/// probability. The route answers
/// `{user_affinities: [{other_user_id, communication_probability}]}`; older answers list bare ids.
pub(super) fn affinities(body: &Value) -> Vec<(Id, f64)> {
    body["user_affinities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            if let Some(id) = entry.as_str() {
                return Some((id.parse().ok()?, 0.));
            }
            let id = entry["other_user_id"]
                .as_str()
                .or_else(|| entry["user_id"].as_str())?
                .parse()
                .ok()?;
            Some((
                id,
                entry["communication_probability"].as_f64().unwrap_or(0.),
            ))
        })
        .collect()
}

/// Store `L()` with an empty query, then the name filter: friends and non-friend affinity users, without blocked
/// people, bots and yourself, sorted by affinity and then by lowercased name.
pub(super) fn candidates(
    friends: &[User],
    affinity: &[(Id, f64)],
    relationships: &std::collections::HashMap<Id, u64>,
    users: &std::collections::HashMap<Id, User>,
    me: Option<Id>,
    query: &str,
) -> Vec<User> {
    let probability = |user: Id| {
        affinity
            .iter()
            .find(|(id, _)| *id == user)
            .map(|(_, score)| *score)
            .unwrap_or(0.)
    };
    let mut list: Vec<User> = friends.to_vec();
    for (id, _) in affinity {
        let kind = relationships.get(id).copied();
        if Some(*id) == me
            || kind == Some(RELATIONSHIP_BLOCKED)
            || kind == Some(RELATIONSHIP_FRIEND)
        {
            continue;
        }
        if let Some(user) = users.get(id)
            && !list.iter().any(|known| known.id == *id)
        {
            list.push(user.clone());
        }
    }
    let query = query.trim().to_lowercase();
    list.retain(|user| {
        query.is_empty()
            || user.display_name().to_lowercase().contains(&query)
            || user.username.to_lowercase().contains(&query)
    });
    list.sort_by(|a, b| {
        probability(b.id)
            .partial_cmp(&probability(a.id))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.display_name()
                    .to_lowercase()
                    .cmp(&b.display_name().to_lowercase())
            })
    });
    list
}

impl Root {
    pub(super) fn open_group_dm_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.requests.send(Request::Fetch {
            key: AFFINITIES_KEY.to_owned(),
            route: "/users/@me/affinities/v2/users".to_owned(),
        });
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        let events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            if let Some(picker) = this.group_dm.as_mut() {
                match event {
                    EditorEvent::Edited => {
                        picker.query = editor.read(cx).text().to_owned();
                        picker.row = 0;
                    }
                    EditorEvent::Submit(_) => this.group_dm_toggle_row(cx),
                    EditorEvent::Cancel => this.group_dm = None,
                }
            }
            cx.notify();
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.group_dm = Some(GroupDm {
            search,
            _events: events,
            query: String::new(),
            selected: Vec::new(),
            row: 0,
        });
        cx.notify();
    }

    fn group_dm_rows(&self) -> Vec<User> {
        let Some(picker) = self.group_dm.as_ref() else {
            return Vec::new();
        };
        let affinity = self
            .fetched
            .get(AFFINITIES_KEY)
            .map(affinities)
            .unwrap_or_default();
        let friends: Vec<User> = self.stores.friends().into_iter().cloned().collect();
        candidates(
            &friends,
            &affinity,
            &self.stores.relationships,
            &self.stores.users,
            self.stores.current_user.as_ref().map(|user| user.id),
            &picker.query,
        )
    }

    fn group_dm_toggle(&mut self, user: Id, cx: &mut Context<Self>) {
        let Some(picker) = self.group_dm.as_mut() else {
            return;
        };
        if let Some(at) = picker.selected.iter().position(|known| *known == user) {
            picker.selected.remove(at);
        } else if picker.selected.len() + 1 < GROUP_LIMIT {
            picker.selected.push(user);
            // Picking clears the query, as `handleSelect` does.
            if !picker.query.is_empty() {
                picker.query.clear();
                picker.search.update(cx, |editor, _| editor.set_text(""));
            }
        }
        cx.notify();
    }

    fn group_dm_toggle_row(&mut self, cx: &mut Context<Self>) {
        let rows = self.group_dm_rows();
        let Some(row) = self.group_dm.as_ref().map(|picker| picker.row) else {
            return;
        };
        if let Some(user) = rows.get(row) {
            self.group_dm_toggle(user.id, cx);
        }
    }

    /// Keys while the picker is open; true when one was used.
    pub(super) fn group_dm_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let count = self.group_dm_rows().len();
        let Some(picker) = self.group_dm.as_mut() else {
            return false;
        };
        match key {
            "escape" => self.group_dm = None,
            "down" if count > 0 => picker.row = (picker.row + 1) % count,
            "up" if count > 0 => picker.row = (picker.row + count - 1) % count,
            "backspace" if picker.query.is_empty() && !picker.selected.is_empty() => {
                picker.selected.pop();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn group_dm_create(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.group_dm.as_ref() else {
            return;
        };
        let recipients: Vec<Id> = if picker.selected.is_empty() {
            // `l.length>s&&this.handleSelect(s)`: with nothing picked, Create takes the highlighted row.
            self.group_dm_rows()
                .get(picker.row)
                .map(|user| vec![user.id])
                .unwrap_or_default()
        } else {
            picker.selected.clone()
        };
        if recipients.is_empty() {
            return;
        }
        if let [user] = recipients[..]
            && let Some(channel) = self
                .stores
                .private_channels
                .iter()
                .find(|channel| channel.kind == 1 && channel.recipients == [user])
                .map(|channel| channel.id)
        {
            self.group_dm = None;
            self.select_home(Some(channel));
            cx.notify();
            return;
        }
        let ids: Vec<String> = recipients.iter().map(Id::to_string).collect();
        let _ = self.requests.send(Request::Rest {
            key: Some(super::friends::OPEN_DM_KEY.to_owned()),
            method: reqwest::Method::POST,
            route: "/users/@me/channels".to_owned(),
            body: Some(json!({ "recipients": ids })),
        });
        self.group_dm = None;
        cx.notify();
    }

    fn group_dm_tag(&self, user: &User, cx: &mut Context<Self>) -> AnyElement {
        let id = user.id;
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        div()
            .flex_none()
            .h(px(30.))
            .pl(px(8.))
            .pr(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(color("--border-muted"))
            .bg(color("--background-mod-subtle"))
            .flex()
            .items_center()
            .gap(px(4.))
            .children(avatar.map(|image| img(image).size(px(12.)).rounded_full()))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-strong"))
                    .child(user.display_name().to_owned()),
            )
            .child(
                div()
                    .id(SharedString::from(format!("group-dm-remove-{id}")))
                    .size(px(18.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, _, _, cx| this.group_dm_toggle(id, cx)))
                    .child(icon("friend_ignore", 16., "--icon-default")),
            )
            .into_any_element()
    }

    fn group_dm_row(&self, user: &User, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = self.group_dm.as_ref() else {
            return div().into_any_element();
        };
        let id = user.id;
        let checked = picker.selected.contains(&id);
        let full = !checked && picker.selected.len() + 1 >= GROUP_LIMIT;
        let highlighted = picker.row == index;
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let status = self.stores.status(user.id).to_owned();
        div()
            .id(SharedString::from(format!("group-dm-row-{id}")))
            .h(px(48.))
            .px(px(8.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(highlighted, |this| this.bg(gpui::rgba(ROW_SELECTED)))
            .when(full, |this| this.opacity(0.5))
            .when(!full, |this| this.cursor_pointer())
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if let Some(picker) = this.group_dm.as_mut()
                    && picker.row != index
                {
                    picker.row = index;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.group_dm_toggle(id, cx)))
            .child(
                div()
                    .relative()
                    .size(px(32.))
                    .flex_none()
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full()))
                    .child(status_dot(&status, "--background-surface-high")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .child(
                // `checkboxIndicator_vTUVyc`: 20px, radius 4; brand fill with a white check once picked.
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if checked {
                        color("--checkbox-border-selected-default")
                    } else {
                        color("--checkbox-border-default")
                    })
                    .bg(if checked {
                        color("--checkbox-background-selected-default")
                    } else {
                        color("--checkbox-background-default")
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |this| {
                        this.child(icon("checkbox_check", 18., "--white"))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn group_dm_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.group_dm.as_ref()?;
        let search = picker.search.clone();
        let focused = search.read(cx).focus_handle(cx).is_focused(window);
        let picked: Vec<User> = picker
            .selected
            .iter()
            .filter_map(|id| self.stores.users.get(id).cloned())
            .collect();
        let several = picker.selected.len() > 1;
        let rows = self.group_dm_rows();
        let tags: Vec<AnyElement> = picked
            .iter()
            .map(|user| self.group_dm_tag(user, cx))
            .collect();
        let row_elements: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, user)| self.group_dm_row(user, index, cx))
            .collect();
        let empty_tags = tags.is_empty();
        search.update(cx, |editor, _| {
            editor.set_placeholder(if empty_tags {
                "Search".to_owned()
            } else {
                String::new()
            })
        });
        let body = div()
            .h(px(418.))
            .flex()
            .flex_col()
            .child(
                div()
                    .parity_with_border("group-dm-search", 1.)
                    .min_h(px(42.))
                    .px(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if focused {
                        color("--input-border-active")
                    } else {
                        color("--input-border-default")
                    })
                    .bg(color("--input-background-default"))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .children(tags)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(64.))
                            .h(px(32.))
                            .pl(px(8.))
                            .pr(px(12.))
                            .flex()
                            .items_center()
                            .child(div().w_full().child(search)),
                    ),
            )
            .child(
                div()
                    .mt(px(4.))
                    .h(px(16.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child("Add friends or server members to group DMs."),
            )
            .child(
                div()
                    .id("group-dm-list")
                    .flex_1()
                    .min_h(px(0.))
                    .mx(px(-8.))
                    .pt(px(12.))
                    .overflow_y_scroll()
                    .children(row_elements),
            )
            // `bodyScrollingSpacerBottom`: the rule over the footer, the modal's full width.
            .child(div().mx(px(-24.)).h(px(1.)).bg(color("--border-subtle")))
            .into_any_element();
        Some(modal::render(
            modal::Spec {
                id: "group-dm-modal",
                title: "New Message".into(),
                subtitle: div()
                    .child(format!("Group DMs can have up to {GROUP_LIMIT} members."))
                    .into_any_element(),
                body: Some(body),
                buttons: [
                    modal::button(
                        "group-dm-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.group_dm = None,
                    ),
                    modal::button(
                        "group-dm-create",
                        if several {
                            "Create Group Message"
                        } else {
                            "Create Message"
                        },
                        modal::Tone::Primary,
                        |this, _, cx| this.group_dm_create(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.group_dm = None),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn user(id: Id, name: &str) -> User {
        User {
            id,
            username: name.to_owned(),
            global_name: None,
            avatar: None,
            discriminator: "0".to_owned(),
        }
    }

    #[test]
    fn affinity_users_join_friends_ranked_by_probability_then_name() {
        let friends = vec![user(1, "zed"), user(2, "amy")];
        let mut users = HashMap::new();
        users.insert(3, user(3, "mor"));
        users.insert(4, user(4, "blocked"));
        let relationships = HashMap::from([(1, 1), (2, 1), (4, 2)]);
        let affinity = vec![(3, 0.5), (4, 0.9), (1, 0.1)];
        let list = candidates(&friends, &affinity, &relationships, &users, Some(9), "");
        let names: Vec<&str> = list.iter().map(|user| user.username.as_str()).collect();
        assert_eq!(names, ["mor", "zed", "amy"]);
        let found = candidates(&friends, &affinity, &relationships, &users, Some(9), "AM");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn affinities_read_both_answer_shapes() {
        let objects =
            json!({"user_affinities": [{"other_user_id": "5", "communication_probability": 0.25}]});
        assert_eq!(affinities(&objects), vec![(5, 0.25)]);
        let bare = json!({"user_affinities": ["6"]});
        assert_eq!(affinities(&bare), vec![(6, 0.)]);
    }
}
