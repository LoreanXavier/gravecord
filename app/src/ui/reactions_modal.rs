//! View Reactions (message menu), measured in `design/dom/modals/view-reactions-darker-cozy.json`: the emoji
//! column (each emoji with its count, the chosen one on `--background-mod-normal`), a rule, and who reacted with
//! it, loaded as `TRAFFIC/view-reactions.json` records: `GET /channels/{c}/messages/{m}/reactions/{emoji}?limit=100
//! &type=0`. Your own row can take your reaction back.

use super::{Root, avatar_source, color, icon, modal};
use crate::net::Request;
use dc_state::{Id, Message, User};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, img, prelude::*, px};

pub(super) const REACTORS_PREFIX: &str = "reactors:";

pub(crate) struct ViewReactions {
    pub message: Message,
    pub selected: usize,
}

/// `name:id` for custom emoji, the character for unicode ones, as the reactions route names them.
fn route_name(reaction: &dc_state::Reaction) -> String {
    super::reaction_route_name(reaction)
}

pub(super) fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

impl Root {
    pub(super) fn open_view_reactions(&mut self, message: Message, cx: &mut Context<Self>) {
        self.view_reactions = Some(ViewReactions {
            message,
            selected: 0,
        });
        self.load_reactors(cx);
    }

    fn reactors_key(&self) -> Option<String> {
        let view = self.view_reactions.as_ref()?;
        let reaction = view.message.reactions.get(view.selected)?;
        Some(format!(
            "{REACTORS_PREFIX}{}:{}",
            view.message.id,
            route_name(reaction)
        ))
    }

    fn load_reactors(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.view_reactions.as_ref() else {
            return;
        };
        let Some(reaction) = view.message.reactions.get(view.selected) else {
            return;
        };
        let key = format!(
            "{REACTORS_PREFIX}{}:{}",
            view.message.id,
            route_name(reaction)
        );
        let route = format!(
            "/channels/{}/messages/{}/reactions/{}?limit=100&type=0",
            view.message.channel_id,
            view.message.id,
            encode(&route_name(reaction))
        );
        let _ = self.requests.send(Request::Fetch { key, route });
        cx.notify();
    }

    fn reactor_row(&self, user: &User, mine: bool, cx: &mut Context<Self>) -> AnyElement {
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let group = SharedString::from(format!("reactor-{}", user.id));
        let user_id = user.id;
        div()
            .id(SharedString::from(format!("reactor-row-{}", user.id)))
            .group(group.clone())
            .h(px(52.))
            .pl(px(16.))
            .flex()
            .items_center()
            .cursor_pointer()
            // A reactor opens their profile.
            .on_click(cx.listener(move |this, _, window, cx| {
                this.view_reactions = None;
                this.open_full_profile(user_id, window, cx);
            }))
            .child(
                div()
                    .size(px(32.))
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full())),
            )
            .child(
                div()
                    .ml(px(10.))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    // Not in the pinned client: RoleColorEverywhere's `reactorsList`
                    // (`plugin_hooks/role_color_everywhere.rs`).
                    .child(super::plugin_hooks::role_colored(
                        div().line_height(px(20.)).font_weight(FontWeight(500.)),
                        self.role_color_everywhere(
                            super::plugin_hooks::RolePlace::ReactorsList,
                            user.id,
                        ),
                        color("--text-default").into(),
                        user.display_name().to_owned(),
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(14.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .when(mine, |this| {
                this.child(
                    div()
                        .id("reactor-remove")
                        .size(px(32.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(0.)
                        .group_hover(group, |style| style.opacity(1.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.remove_own_reaction(cx)
                        }))
                        .child(icon("friend_ignore", 16., "--text-subtle")),
                )
            })
            .into_any_element()
    }

    fn remove_own_reaction(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.view_reactions.as_ref() else {
            return;
        };
        let Some(reaction) = view.message.reactions.get(view.selected) else {
            return;
        };
        let _ = self.requests.send(Request::React {
            channel: view.message.channel_id,
            message: view.message.id,
            emoji: route_name(reaction),
            add: false,
            location: "Message",
            burst: false,
        });
        self.load_reactors(cx);
    }

    pub(super) fn view_reactions_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.view_reactions.as_ref()?;
        let me: Option<Id> = self.stores.current_user.as_ref().map(|user| user.id);
        let pills: Vec<AnyElement> = view
            .message
            .reactions
            .iter()
            .enumerate()
            .map(|(index, reaction)| {
                let picked = index == view.selected;
                let image = self
                    .images
                    .get(&format!("emoji:{}", reaction.name))
                    .cloned();
                div()
                    .id(SharedString::from(format!("reaction-tab-{index}")))
                    .mb(px(8.))
                    .w(px(73.))
                    .h(px(32.))
                    .px(px(4.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .when(picked, |this| this.bg(color("--background-mod-normal")))
                    .when(!picked, |this| {
                        this.hover(|style| style.bg(color("--background-mod-subtle")))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(view) = this.view_reactions.as_mut() {
                            view.selected = index;
                        }
                        this.load_reactors(cx);
                    }))
                    .child(
                        div()
                            .pl(px(4.))
                            .pr(px(8.))
                            .py(px(4.))
                            .children(image.map(|image| img(image).size(px(24.)))),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(reaction.count.to_string()),
                    )
                    .into_any_element()
            })
            .collect();
        let reactors: Vec<User> = self
            .reactors_key()
            .and_then(|key| self.fetched.get(&key))
            .and_then(|body| body.as_array())
            .into_iter()
            .flatten()
            .filter_map(User::from_value)
            .collect();
        let rows: Vec<AnyElement> = reactors
            .iter()
            .map(|user| self.reactor_row(user, Some(user.id) == me, cx))
            .collect();
        // `container_quI2qP`: 517 tall under a full-width rule, 17px below the title, clamped on short windows.
        let body_h = 517_f32.min((self.viewport.1 - 190.).max(200.));
        let body = div()
            .mt(px(1.))
            .mx(px(-24.))
            .h(px(body_h))
            .border_t_1()
            .border_color(color("--border-subtle"))
            .pl(px(24.))
            .flex()
            .child(
                div()
                    .id("reaction-tabs")
                    .w(px(98.))
                    .pt(px(16.))
                    .border_r_1()
                    .border_color(color("--border-subtle"))
                    .overflow_y_scroll()
                    .children(pills),
            )
            .child(
                div()
                    .id("reactors")
                    .flex_1()
                    .pt(px(12.))
                    // `reactors_quI2qP`: 24px end padding beside the `auto` scroller's stable 16px gutter.
                    .pr(px(40.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element();
        Some(modal::render(
            modal::Spec {
                id: "view-reactions-modal",
                title: "Reactions".into(),
                subtitle: None,
                body: Some(body),
                buttons: Vec::new(),
                close: Box::new(|this, _, _| this.view_reactions = None),
            },
            cx,
        ))
    }
}
