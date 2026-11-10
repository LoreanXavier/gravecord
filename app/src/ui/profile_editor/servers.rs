//! Per-server profiles in the profile editor. The select at the top of the panel (`ld` of module 927071, styled by
//! module 216384) lists "Main Profile" and the account's servers; choosing one edits that server's profile with the
//! guild routes of `save.rs`. Without Nitro a server profile only shows the upsell and its groups are locked
//! (`lP`: `h = m && !p`); with Nitro it shows the dismissible notice instead (`lA`).

use super::{label, text};
use crate::net::Request;
use crate::ui::{Root, color, icon};
use dc_state::Id;
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, SharedString, deferred, div, img,
    prelude::*, px,
};

/// The local-storage key of the dismissed per-server notice (`per-server-profile-editing-notice-dismissed`).
const NOTICE_KEY: &str = "per-server-profile-editing-notice-dismissed";

/// A row of the select: `None` is "Main Profile".
pub(super) struct Choice {
    pub guild: Option<Id>,
    pub label: String,
    /// The account's nickname there (`description` of the option).
    pub nick: Option<String>,
}

/// The select's options (`f` in `ld`): Main Profile, the server being edited, then every server in the rail's order.
pub(super) fn choices(
    guilds: &[(Id, String)],
    current: Option<Id>,
    nick: impl Fn(Id) -> Option<String>,
) -> Vec<Choice> {
    let mut out = vec![Choice {
        guild: None,
        label: "Main Profile".to_owned(),
        nick: None,
    }];
    let rest = guilds.iter().filter(|(id, _)| Some(*id) != current);
    let first = guilds.iter().find(|(id, _)| Some(*id) == current);
    for (id, name) in first.into_iter().chain(rest) {
        out.push(Choice {
            guild: Some(*id),
            label: name.clone(),
            nick: nick(*id),
        });
    }
    out
}

/// A group of the panel with its locked look: the tiles take no input while it is locked.
pub(super) fn locked(content: Div, disabled: bool) -> Div {
    if disabled {
        div()
            .relative()
            .child(content)
            .child(div().absolute().inset_0().occlude())
    } else {
        div().child(content)
    }
}

impl Root {
    /// Switches the editor to another profile: the main one (`None`) or a server's. Each profile keeps its own
    /// pending changes, as the settings store keeps them per guild.
    pub(super) fn profile_editor_select_guild(
        &mut self,
        guild: Option<Id>,
        cx: &mut Context<Self>,
    ) {
        let Some(me) = self.stores.current_user.clone() else {
            return;
        };
        let nick = guild
            .and_then(|id| self.stores.members.get(id, me.id))
            .and_then(|member| member.nick.clone());
        let Some(open) = self.profile_editor.as_mut() else {
            return;
        };
        open.select_open = false;
        if open.guild == guild {
            cx.notify();
            return;
        }
        let leaving = std::mem::replace(&mut open.pending, super::save::Pending::new(guild));
        open.stashed.insert(open.guild, leaving);
        if let Some(back) = open.stashed.remove(&guild) {
            open.pending = back;
        }
        open.guild = guild;
        open.saved = super::Saved::default();
        open.avatar_image = None;
        open.banner_image = None;
        open.picker = None;
        open.banner_color = None;
        open.styles = None;
        open.error = None;
        let shown = match (&open.pending.global_name, &open.pending.nickname, guild) {
            (_, Some(Some(pending)), Some(_)) => pending.clone(),
            (Some(Some(pending)), _, None) => pending.clone(),
            (_, _, Some(_)) => nick.unwrap_or_else(|| me.display_name().to_owned()),
            _ => me.display_name().to_owned(),
        };
        let name = open.name.clone();
        name.update(cx, |editor, _| editor.set_text(&shown));
        let route = super::profile_route(me.id, guild);
        let _ = self.requests.send(Request::Fetch {
            key: format!("{}profile", super::FETCH_PREFIX),
            route,
        });
        cx.notify();
    }

    /// The select's list under its button (`listbox_VrghH6`: 264 wide, on `--background-surface-higher`).
    pub(super) fn profile_editor_server_list(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.profile_editor.as_ref()?;
        if !open.select_open {
            return None;
        }
        let me = self.stores.current_user.as_ref()?.id;
        let current = open.guild;
        let guilds: Vec<(Id, String)> = self
            .stores
            .guilds
            .iter()
            .map(|guild| (guild.id, guild.name.clone()))
            .collect();
        let options = choices(&guilds, current, |guild| {
            self.stores
                .members
                .get(guild, me)
                .and_then(|member| member.nick.clone())
        });
        let rows: Vec<_> = options
            .into_iter()
            .map(|choice| {
                let guild = choice.guild;
                let leading = match guild {
                    None => icon("sb_discord", 20., "--icon-default").into_any_element(),
                    Some(id) => {
                        let image = self.images.get(&format!("guild:{id}")).cloned();
                        let acronym = self
                            .stores
                            .guilds
                            .iter()
                            .find(|known| known.id == id)
                            .map(dc_state::Guild::acronym)
                            .unwrap_or_default();
                        let tile = div()
                            .size(px(24.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(color("--background-surface-high"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(10.))
                            .text_color(color("--text-default"));
                        match image {
                            Some(image) => tile.child(img(image).size(px(24.))),
                            None => tile.child(acronym),
                        }
                        .into_any_element()
                    }
                };
                let name = div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(choice.label);
                let nick = choice.nick.filter(|nick| !nick.is_empty()).map(|nick| {
                    text(nick, 12., 16., 400., "--text-subtle")
                        .overflow_hidden()
                        .whitespace_nowrap()
                });
                div()
                    .id(SharedString::from(format!(
                        "profile-editor-server-{}",
                        guild.map_or("main".to_owned(), |id| id.to_string())
                    )))
                    .flex_none()
                    .min_h(px(40.))
                    .p(px(4.))
                    .pl(px(8.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(color("--text-strong"))
                    .hover(|style| style.bg(color("--interactive-background-hover")))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .size(px(32.))
                            .ml(px(-8.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(leading),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(name)
                            .children(nick),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile_editor_select_guild(guild, cx);
                    }))
            })
            .collect();
        Some(
            deferred(
                div()
                    .id("profile-editor-server-list")
                    .occlude()
                    .absolute()
                    .left(px(9.))
                    .top(px(76.))
                    .w(px(264.))
                    .max_h(px(5. * 40. + 10.))
                    .overflow_y_scroll()
                    .p(px(4.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.profile_editor.as_mut() {
                            open.select_open = false;
                        }
                        cx.notify();
                    }))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// The top of a server profile's panel: the dismissible notice with Nitro (`lA`), the upsell without
    /// (`lI`, with the lock divider below it).
    pub(super) fn profile_editor_server_intro(
        &self,
        premium: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if premium {
            if crate::storage::get(NOTICE_KEY).and_then(|value| value.as_bool()) == Some(true) {
                return div().into_any_element();
            }
            return div()
                .flex()
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--icon-feedback-info"))
                .bg(color("--background-feedback-mod-info"))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-default"))
                        .child(
                            "You can customize styles, display name, pronouns, and bio for each server.",
                        ),
                )
                .child(
                    div()
                        .id("profile-editor-notice-dismiss")
                        .size(px(24.))
                        .mt(px(-2.))
                        .mr(px(-2.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(icon("modal_close", 16., "--icon-subtle"))
                        .on_click(cx.listener(|_, _, _, cx| {
                            crate::storage::set(NOTICE_KEY, serde_json::json!(true));
                            cx.notify();
                        })),
                )
                .into_any_element();
        }
        let pitch = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(text(
                "Customize more with Nitro",
                16.,
                20.,
                500.,
                "--text-default",
            ))
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-default"))
                    .child(
                        "Go beyond name and pronouns. Give each server unique banners, themes, decorations, and more with ",
                    )
                    .child(
                        div()
                            .id("profile-editor-server-nitro-link")
                            .cursor_pointer()
                            .text_color(color("--text-link"))
                            .child("Nitro")
                            .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx))),
                    ),
            );
        let unlock = div()
            .id("profile-editor-server-unlock")
            .h(px(32.))
            .rounded(px(8.))
            .bg(color("--control-primary-background-default"))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.))
            .cursor_pointer()
            .child(icon(
                "settings_nitro",
                16.,
                "--control-primary-text-default",
            ))
            .child(text(
                "Unlock with Nitro",
                14.,
                18.,
                500.,
                "--control-primary-text-default",
            ))
            .on_click(cx.listener(|this, _, _, cx| this.open_nitro(cx)));
        // `lockDivider`: a rule across the panel with the lock on it.
        let divider = div()
            .relative()
            .mt(px(12.))
            .mx(px(-16.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(14.))
                    .h(px(1.))
                    .bg(color("--border-normal")),
            )
            .child(
                div()
                    .size(px(28.))
                    .rounded_full()
                    .border_1()
                    .border_color(color("--border-normal"))
                    .bg(color("--background-surface-highest"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("channel_lock", 16., "--icon-subtle")),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(pitch)
            .child(unlock)
            .child(divider)
            .into_any_element()
    }
}

/// The group label with the Nitro mark (`showNitroIcon`), muted while the group is locked.
pub(super) fn nitro_label(title: &'static str, disabled: bool) -> Div {
    let (tint, mark) = if disabled {
        ("--text-muted", "--icon-muted")
    } else {
        ("--text-strong", "--icon-default")
    };
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(text(title, 14., 18., 500., tint))
        .child(icon("settings_nitro", 16., mark))
}

/// The plain group label, muted while the group is locked.
pub(super) fn group_label(title: &'static str, disabled: bool) -> Div {
    if disabled {
        text(title, 14., 18., 500., "--text-muted")
    } else {
        label(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(id: Id, name: &str) -> (Id, String) {
        (id, name.to_owned())
    }

    #[test]
    fn the_select_lists_main_first_then_the_open_server_then_the_rest() {
        let guilds = vec![guild(1, "One"), guild(2, "Two"), guild(3, "Three")];
        let list = choices(&guilds, Some(2), |id| (id == 3).then(|| "Nick".to_owned()));
        let names: Vec<_> = list.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(names, ["Main Profile", "Two", "One", "Three"]);
        assert_eq!(list[0].guild, None);
        assert_eq!(list[3].nick.as_deref(), Some("Nick"));
    }
}
