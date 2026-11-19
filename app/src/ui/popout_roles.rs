//! The role list of a profile popout (module 166005 `Ql`, styles module 434367): a chip per role with a remove button,
//! and the add button that opens the Add Role picker (module 465738: a search field over the roles the account may
//! give). Giving or taking a role writes the member's whole role list, `PATCH /guilds/:g/members/:u` (module 727419
//! `updateMemberRoles`). Frames: menus2/self-popout-add-role.

use super::{Root, color, editor_style, icon, member_actions};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use dc_state::{Id, permissions};
use gpui::{
    Anchor, AnyElement, Context, Entity, Focusable, IntoElement, Subscription, Window, anchored,
    deferred, div, point, prelude::*, px, rgb,
};
use serde_json::json;

/// The open Add Role picker.
pub(super) struct Picker {
    guild: Id,
    user: Id,
    input: Entity<Editor>,
    _events: Subscription,
}

/// A role the picker or the list shows.
#[derive(Clone)]
struct Entry {
    id: Id,
    name: String,
    color: u32,
    removable: bool,
}

/// The member's role list after `role` is given (`i.includes(e)||(i=[...i,e])`).
fn with_role(mut roles: Vec<Id>, role: Id) -> Vec<Id> {
    if !roles.contains(&role) {
        roles.push(role);
    }
    roles
}

/// The roles of a member the store holds; `None` for a member it does not know.
fn known_roles(stores: &dc_state::Stores, guild: Id, user: Id) -> Option<Vec<Id>> {
    stores
        .members
        .get(guild, user)
        .map(|member| member.roles.clone())
}

/// The member's role list after `role` is taken (`filter(e=>e!==t.id)`).
fn without_role(roles: Vec<Id>, role: Id) -> Vec<Id> {
    roles.into_iter().filter(|id| *id != role).collect()
}

/// Whether a role's name matches what was typed in the picker.
fn matches_filter(name: &str, typed: &str) -> bool {
    if typed.trim().is_empty() {
        return true;
    }
    // The ordered character match of 91871, used by 465738's customMatchSorter.
    let name = name.to_lowercase();
    let mut remaining = name.chars();
    typed
        .to_lowercase()
        .chars()
        .all(|wanted| remaining.any(|ch| ch == wanted))
}

/// The picker's window: 9px of padding (8 and its border), the 40px field and the list with 4px above and below,
/// at most six rows before it scrolls.
fn picker_height(rows: usize) -> f32 {
    let list = if rows == 0 {
        0.
    } else {
        8. + 40. * rows.min(6) as f32
    };
    9. + 40. + list + 9.
}

/// 265872 / 623646: bottom-centred with 8px spacing, invert when it has more room, ceil the horizontal alignment.
fn picker_position(anchor: [f32; 4], viewport: [f32; 2], height: f32) -> [f32; 2] {
    let [left, top, width, anchor_height] = anchor;
    let x = (left + (width - 268.) / 2.)
        .clamp(12., (viewport[0] - 268. - 12.).max(12.))
        .ceil();
    let below = top + anchor_height + 8.;
    let above = top - 8. - height;
    let below_space = viewport[1] - below - height;
    let y = if below_space < 0. && above > below_space {
        above
    } else {
        below
    };
    [x, y.clamp(0., (viewport[1] - height).max(0.))]
}

impl Root {
    /// The roles of `user` in `guild`, highest first, the way the list orders them, each with whether this account
    /// may take it (`X.wO` and not managed, with Manage Roles; a linked-account role only from oneself).
    fn popout_role_entries(&self, guild: Id, user: Id) -> Vec<Entry> {
        let Some(known) = self.stores.guild(guild) else {
            return Vec::new();
        };
        let Some(member) = self.stores.members.get(guild, user) else {
            return Vec::new();
        };
        let me = self.stores.current_user.as_ref().map(|me| me.id);
        let manage = self.my_permission(guild, None, permissions::MANAGE_ROLES);
        let mut roles: Vec<Id> = member
            .roles
            .iter()
            .copied()
            .filter(|id| *id != guild && known.roles.contains_key(id))
            .collect();
        roles.sort_by_key(|id| {
            let role = known.role_info.get(id).copied().unwrap_or_default();
            (role.guild_connection, std::cmp::Reverse(role.position), *id)
        });
        roles
            .into_iter()
            .map(|id| {
                let info = known.role_info.get(&id).copied().unwrap_or_default();
                let removable = !info.guild_connection
                    && manage
                    && !info.managed
                    && me.is_some_and(|me| {
                        permissions::role_is_higher(known, me, &known.my_roles, id)
                    });
                Entry {
                    id,
                    name: known.roles.get(&id).cloned().unwrap_or_default(),
                    color: info.color,
                    removable,
                }
            })
            .collect()
    }

    /// Whether the account may give roles to `user` here (`canAddRoles`: Manage Roles).
    fn popout_can_add_roles(&self, guild: Id) -> bool {
        self.my_permission(guild, None, permissions::MANAGE_ROLES)
    }

    /// The roles the picker offers (`roleFilter` `A`): not @everyone, not managed, below the account's top role, not
    /// held yet, and matching what was typed; highest first.
    fn picker_entries(&self, guild: Id, user: Id, typed: &str) -> Vec<Entry> {
        let (Some(known), Some(member), Some(me)) = (
            self.stores.guild(guild),
            self.stores.members.get(guild, user),
            self.stores.current_user.as_ref().map(|me| me.id),
        ) else {
            return Vec::new();
        };
        let mut roles: Vec<(i64, Id)> = known
            .roles
            .keys()
            .copied()
            .filter(|id| *id != guild && !member.roles.contains(id))
            .filter(|id| {
                let info = known.role_info.get(id).copied().unwrap_or_default();
                !info.managed
                    && !info.guild_connection
                    && permissions::role_is_higher(known, me, &known.my_roles, *id)
            })
            .map(|id| (known.role_info.get(&id).map_or(0, |info| info.position), id))
            .collect();
        roles.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        roles
            .into_iter()
            .filter_map(|(_, id)| {
                let name = known.roles.get(&id)?.clone();
                matches_filter(&name, typed).then(|| Entry {
                    id,
                    color: known.role_info.get(&id).map_or(0, |info| info.color),
                    name,
                    removable: false,
                })
            })
            .collect()
    }

    /// The chips and the add button (`roleListContainer_xL50-d`): none for a person without roles when the account
    /// cannot give any.
    pub(super) fn popout_roles(
        &self,
        guild: Id,
        user: Id,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let entries = self.popout_role_entries(guild, user);
        let can_add = self.popout_can_add_roles(guild);
        if entries.is_empty() && !can_add {
            return None;
        }
        let none = entries.is_empty();
        let mut row = div()
            .id("popout-roles")
            .relative()
            .mt(px(2.))
            .min_h(px(24.))
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .child(self.anchor_probe("popout-roles"));
        for entry in entries {
            let tint = if entry.color == 0 {
                color("--primary-300")
            } else {
                rgb(entry.color)
            };
            let role = entry.id;
            row = row.child(
                div()
                    .h(px(24.))
                    .max_w_full()
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    // Module 166005 `K`: the chip's `dev-context` menu (Copy Role ID) in developer mode.
                    .on_mouse_up(
                        gpui::MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                            if this.open_role_chip_menu(role, event.position, cx) {
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(
                        match self
                            .stores
                            .guild(guild)
                            .and_then(|known| known.role_colors_of(entry.id))
                        {
                            // `roleCircle`: the gradient on the circle's own box (module 545442 `R`).
                            Some(colors) => {
                                div().ml(px(6.)).mr(px(4.)).size(px(12.)).flex_none().child(
                                    gpui::img(super::role_gradient::circle(&colors, 12, 1.))
                                        .size(px(12.)),
                                )
                            }
                            None => div()
                                .ml(px(6.))
                                .mr(px(4.))
                                .size(px(12.))
                                .flex_none()
                                .rounded_full()
                                .bg(tint),
                        },
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(color("--text-default"))
                            .child(entry.name.clone()),
                    )
                    // `.roleIcon_xL50-d`: 12px, 4px after the name.
                    .children(self.stores.guild(guild).and_then(|known| {
                        self.role_icon_art(known, entry.id, 12.)
                            .map(|art| div().flex_none().ml(px(4.)).child(art))
                    }))
                    .when(entry.removable, |chip| {
                        chip.child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "popout-role-remove-{role}"
                                )))
                                .flex_none()
                                .w(px(23.))
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(icon("sb_close", 12., "--interactive-icon-default"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_member_role(guild, user, role, cx);
                                })),
                        )
                    })
                    .when(!entry.removable, |chip| chip.child(div().w(px(8.)))),
            );
        }
        if can_add {
            row = row.child(self.profile_role_add_button(guild, user, none, cx));
        }
        Some(row.into_any_element())
    }

    /// `addButton_xL50-d` / `showLabel_xL50-d` of 166005: an empty list shows the label without a chip border.
    pub(super) fn profile_role_add_button(
        &self,
        guild: Id,
        user: Id,
        none: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = self.profile_palette(user, Some(guild), false);
        let tint = palette.token("--interactive-text-default");
        let hover = palette.token("--interactive-text-hover");
        let focus = self.stop("popout-role-add", cx).tab_index(0).tab_stop(true);
        let ringed = self.keyboard_mode() && self.keys.stops.has_focus(&focus);
        let key_focus = focus.clone();
        div()
            .parity("profile-role-add")
            .id("popout-role-add")
            .track_focus(&focus)
            .tab_index(0)
            .relative()
            .h(px(24.))
            .min_w(px(24.))
            .rounded(px(8.))
            .when(none, |button| button.pr(px(4.)))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .cursor_pointer()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(gpui::FontWeight(400.))
            .text_color(tint)
            .hover(move |style| style.text_color(hover))
            .child(
                gpui::svg()
                    .path("icons/plus_small.svg")
                    .size(px(18.))
                    .flex_none(),
            )
            .when(none, |button| button.child(dc_intl::t!("e3Wlyw")))
            .child(self.anchor_probe("popout-role-add"))
            .when(ringed, |button| {
                button.child(super::keyboard::hollow_ring(8.))
            })
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if key_focus.is_focused(window)
                        && matches!(event.keystroke.key.as_str(), "enter" | "space")
                    {
                        cx.stop_propagation();
                        this.open_role_picker(guild, user, window, cx);
                    }
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_role_picker(guild, user, window, cx);
            }))
            .into_any_element()
    }

    /// The add button: the picker opens with its field focused.
    pub(super) fn open_role_picker(
        &mut self,
        guild: Id,
        user: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            let mut input = Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_reports_all_edits(true);
            input.set_single_line(true);
            input.set_placeholder("Role".to_owned());
            input
        });
        let events = cx.subscribe(&input, move |this, editor, event: &EditorEvent, cx| {
            if !this
                .role_picker
                .as_ref()
                .is_some_and(|open| open.input.entity_id() == editor.entity_id())
            {
                return;
            }
            match event {
                EditorEvent::Edited => {}
                // Enter takes the first role of the list.
                EditorEvent::Submit(_) => {
                    let typed = editor.read(cx).text().to_owned();
                    if let Some(first) = this.picker_entries(guild, user, &typed).into_iter().next()
                    {
                        this.add_member_role(guild, user, first.id, cx);
                    }
                }
                EditorEvent::Cancel => this.role_picker = None,
            }
            cx.notify();
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        self.role_picker = Some(Picker {
            guild,
            user,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// A role of the picker chosen: the list goes back with it added, and the picker closes (`onSelect` `y`).
    pub(super) fn add_member_role(
        &mut self,
        guild: Id,
        user: Id,
        role: Id,
        cx: &mut Context<Self>,
    ) {
        self.role_picker = None;
        // The request carries the whole list: without the member's known roles it would replace them all.
        if let Some(roles) = self.known_member_roles(guild, user) {
            self.send_member_roles(guild, user, with_role(roles, role));
        }
        cx.notify();
    }

    /// The chip's X: the list goes back without the role (`F` of module 166005).
    fn remove_member_role(&mut self, guild: Id, user: Id, role: Id, cx: &mut Context<Self>) {
        if let Some(roles) = self.known_member_roles(guild, user) {
            self.send_member_roles(guild, user, without_role(roles, role));
        }
        cx.notify();
    }

    /// The roles the member has now, or `None` when the member is not known (nothing may be written then).
    fn known_member_roles(&self, guild: Id, user: Id) -> Option<Vec<Id>> {
        known_roles(&self.stores, guild, user)
    }

    fn send_member_roles(&mut self, guild: Id, user: Id, roles: Vec<Id>) {
        let list: Vec<String> = roles.iter().map(ToString::to_string).collect();
        let _ = self.requests.send(member_actions::member_patch(
            guild,
            user,
            "roles",
            json!(list),
        ));
    }

    /// The picker over the popout (`container_7in7_8`, 268 wide): the search field and the roles, above the list's
    /// top edge with 8px between.
    pub(super) fn role_picker_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.role_picker.as_ref()?;
        let (guild, user) = (open.guild, open.user);
        let typed = open.input.read(cx).text().to_owned();
        let entries = self.picker_entries(guild, user, &typed);
        let bounds = self.tip_anchors.borrow().get("popout-role-add").copied()?;
        let height = picker_height(entries.len());
        let [left, top] = picker_position(
            [
                bounds.left().into(),
                bounds.top().into(),
                bounds.size.width.into(),
                bounds.size.height.into(),
            ],
            [
                window.viewport_size().width.into(),
                window.viewport_size().height.into(),
            ],
            height,
        );
        let rows = entries.into_iter().enumerate().map(|(index, entry)| {
            let role = entry.id;
            div()
                .id(gpui::SharedString::from(format!("role-picker-{role}")))
                .h(px(40.))
                .flex_none()
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.add_member_role(guild, user, role, cx);
                }))
                .child(
                    div()
                        .size(px(32.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .pl(px(8.))
                        .child(
                            div()
                                .when(index == 0, |dot| dot.parity("profile-role-picker-dot"))
                                .size(px(12.))
                                .rounded_full()
                                .bg(if entry.color == 0 {
                                    // DEFAULT_ROLE_COLOR (`652215.TpD`) in the picker differs from the profile chip fallback.
                                    rgb(0x99aab5)
                                } else {
                                    rgb(entry.color)
                                }),
                        ),
                )
                .child(
                    div()
                        .ml(px(4.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(entry.name),
                )
        });
        let card = div()
            .parity_with_border("profile-role-picker", 1.)
            .id("role-picker")
            .occlude()
            .w(px(268.))
            .h(px(height))
            .p(px(8.))
            .border_1()
            .border_color(color("--border-subtle"))
            .rounded(px(8.))
            .bg(color("--modal-background"))
            .child(
                // `selectField_vWC4bk` focused: a 40px box with the 2px brand border.
                div()
                    .parity_with_border("profile-role-picker-field", 2.)
                    .h(px(40.))
                    .w_full()
                    .rounded(px(8.))
                    .border_2()
                    .border_color(color("--input-border-active"))
                    .bg(color("--input-background-default"))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .child(open.input.clone()),
            )
            .child(
                div()
                    .id("role-picker-list")
                    .mt(px(0.))
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(rows),
            );
        Some(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .position(point(px(left), px(top)))
                    .snap_to_window_with_margin(px(8.))
                    .child(super::keyboard::outside(
                        card,
                        super::keyboard::Layer::RolePicker,
                        cx,
                    )),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_member_has_no_list_to_write_back() {
        assert_eq!(known_roles(&dc_state::Stores::default(), 1, 2), None);
    }

    #[test]
    fn giving_a_role_keeps_the_list_and_adds_it_once() {
        assert_eq!(with_role(vec![1, 2], 3), vec![1, 2, 3]);
        assert_eq!(with_role(vec![1, 2], 2), vec![1, 2]);
    }

    #[test]
    fn taking_a_role_leaves_the_others() {
        assert_eq!(without_role(vec![1, 2, 3], 2), vec![1, 3]);
        assert_eq!(without_role(vec![1], 5), vec![1]);
    }

    #[test]
    fn the_picker_filters_by_what_was_typed_ignoring_case() {
        assert!(matches_filter("gravecord plain", ""));
        assert!(matches_filter("gravecord plain", "PLA"));
        assert!(matches_filter("gravecord plain", "gpl"));
        assert!(matches_filter("gravecord plain", "   "));
        assert!(!matches_filter("gravecord plain", "plg"));
        // The source's subsequence matcher finds r...e...d in "gravecord".
        assert!(matches_filter("gravecord plain", "red"));
        assert!(!matches_filter("gravecord plain", "plain!"));
    }

    #[test]
    fn the_picker_is_106_high_with_one_role_as_in_the_frame() {
        assert_eq!(picker_height(1), 106.);
        assert_eq!(picker_height(0), 58.);
        assert_eq!(picker_height(20), picker_height(6));
    }

    #[test]
    fn role_picker_centres_on_the_add_button_and_flips_at_the_window_edge() {
        assert_eq!(
            picker_position([722., 606., 66.9375, 24.], [1280., 720.], 146.),
            [622., 452.]
        );
        assert_eq!(
            picker_position([722., 100., 66.9375, 24.], [1280., 720.], 146.),
            [622., 132.]
        );
        assert_eq!(
            picker_position([0., 100., 24., 24.], [1280., 720.], 146.),
            [12., 132.]
        );
        assert_eq!(
            picker_position([1240., 100., 24., 24.], [1280., 720.], 146.),
            [1000., 132.]
        );
    }
}
