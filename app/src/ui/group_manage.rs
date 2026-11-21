//! Group DM management around a call: the DM list row and header of a group, the "Edit Group" modal (module 66442),
//! the "Leave Group" confirm (modules 605691 and 925785) and the group's context menu (module 4027 `f`).
//!
//! - The header's name is a button (module 95655): the group avatar (20px, a 24px facepile), the name in `body-md/medium`
//!   and a pencil that shows while the pointer is over it. It opens Edit Group; its context menu is the group's menu.
//! - Edit Group: a 120px icon button with a pencil badge (`iconContainer_zwiT-r`), "Remove Icon" under a set icon, a
//!   100-character name field with the default name as placeholder, Cancel and Save. Save is disabled until the name or
//!   the icon changed. `PATCH /channels/{id}` with `name` and/or `icon` (a data URI, or null to remove); the new state
//!   arrives by `CHANNEL_UPDATE`.
//! - Leave Group: "Leave '{name}'", the warning, a checkbox "Leave without notifying other members", Cancel and Leave
//!   Group. The channel goes at once (`CHANNEL_DELETE` with `silent`) and `DELETE /channels/{id}?silent={silent}` follows.
//! - A group's row in the DM list: the group avatar, the name, "{n} Members" under it, the close button on the right.
//!
//! Group DM states are not captured from the official client yet; sizes not in a module or its CSS follow the 1:1
//! DM rows and are marked in `DECISIONS.md`.

use super::tooltip::{Side, Tip};
use super::unread_dms::{avatar, group_icon};
use super::{Root, color, context_menu, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{Id, PrivateChannel};
use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, SharedString,
    Subscription, Window, div, img, prelude::*, px,
};
use serde_json::json;
use std::sync::Arc;

/// `DismissibleContent.INVITE_NONFRIENDS_TO_GDM_COACHMARK` (module 554146).
const GDM_COACHMARK: u16 = 759;

/// Capture only: the key of the one real request that creates a group holding only this account.
#[cfg(feature = "capture")]
pub(super) const SOLO_KEY: &str = "gdm-solo";
#[cfg(feature = "capture")]
static SOLO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(feature = "capture")]
pub(super) fn set_solo(channel: Id) {
    SOLO.store(channel, std::sync::atomic::Ordering::Relaxed);
}

/// `maxLength` of the name field.
const NAME_LIMIT: usize = 100;
/// A chosen icon: the data URI the API takes and the decoded picture for the preview.
pub(super) struct Picked {
    uri: String,
    image: Arc<gpui::Image>,
}

pub(super) struct Edit {
    pub input: Entity<Editor>,
    _events: Subscription,
    channel: Id,
    /// The name the channel had when the modal opened (`h`).
    original: String,
    /// `None` leaves the icon, `Some(None)` removes it, `Some(Some(_))` sets a new one (`T` of the module).
    icon: Option<Option<Picked>>,
    save_focus: FocusHandle,
}

pub(super) struct Leave {
    channel: Id,
    silent: bool,
}

/// `{n} Members`: the group's people with this account.
fn member_count_text(channel: &PrivateChannel) -> String {
    let count = channel.recipients.len() + 1;
    format!("{count} Member{}", if count == 1 { "" } else { "s" })
}

/// The body of `PATCH /channels/{id}` for the edits: `name` when it changed, `icon` when it was set or removed.
fn patch_body(
    name: &str,
    original: &str,
    icon: &Option<Option<Picked>>,
) -> Option<serde_json::Value> {
    let mut body = serde_json::Map::new();
    if name != original {
        body.insert("name".into(), json!(name));
    }
    match icon {
        Some(Some(picked)) => {
            body.insert("icon".into(), json!(picked.uri));
        }
        Some(None) => {
            body.insert("icon".into(), serde_json::Value::Null);
        }
        None => {}
    }
    (!body.is_empty()).then_some(serde_json::Value::Object(body))
}

/// Request key prefix for `PATCH /channels/{id}` group edits (module 396787 `XA` 429 rate-limit alert).
pub(super) const GROUP_EDIT_KEY: &str = "group-edit:";

/// Module 576470 (`Countdown`, `showUnits: true`): zero-padded `DDd HHh MMm SSs` omitting days when `0`.
pub(super) fn format_countdown_units(remaining_secs: u64) -> String {
    let days = remaining_secs / 86_400;
    let hours = (remaining_secs % 86_400) / 3_600;
    let minutes = (remaining_secs % 3_600) / 60;
    let seconds = remaining_secs % 60;
    let d_u = dc_intl::t("QJyuxY");
    let h_u = dc_intl::t("1LyF1h");
    let m_u = dc_intl::t("n7dksO");
    let s_u = dc_intl::t("6m/6nM");
    if days > 0 {
        format!("{days:02}{d_u} {hours:02}{h_u} {minutes:02}{m_u} {seconds:02}{s_u}")
    } else {
        format!("{hours:02}{h_u} {minutes:02}{m_u} {seconds:02}{s_u}")
    }
}

/// Builds module 396787's rate-limit alert (`Whhv4w` / `VIJkAy`) for a 429 response with `retry_after`.
pub(super) fn rate_limit_alert(retry_after_secs: f64) -> super::chat_notices::Alert {
    let wait = retry_after_secs.max(0.).ceil() as u64;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
    super::chat_notices::Alert {
        id: "group-icon-rate-limit",
        title: dc_intl::t("Whhv4w").to_owned(),
        subtitle: None,
        content: Some(super::chat_notices::Content::RateLimitCountdown { deadline }),
        buttons: vec![super::chat_notices::AlertButton {
            id: "group-icon-rate-limit-ok",
            label: dc_intl::t("BddRzS").to_owned(),
            tone: super::modal::Tone::Primary,
            then: super::chat_notices::Then::Close,
        }],
    }
}

impl Root {
    /// Fetches the pictures the group rows, header and member list draw: each group's icon (or its first two people)
    /// plus every recipient's avatar and server tag badge.
    pub(super) fn request_group_images(&mut self) {
        let mut images = Vec::new();
        for channel in self.stores.private_channels.iter().filter(|c| c.kind == 3) {
            if channel.icon.is_none() && channel.recipients.len() >= 2 {
                images.extend(
                    channel
                        .recipients
                        .iter()
                        .take(2)
                        .filter_map(|id| self.stores.users.get(id))
                        .map(|user| avatar(user, 32)),
                );
            } else {
                images.push(group_icon(channel));
            }
            for recipient in &channel.recipients {
                if let Some(user) = self.stores.users.get(recipient) {
                    images.push(super::avatar_source(user));
                    if let Some(url) = user.tag.as_ref().and_then(|tag| tag.badge_url()) {
                        images.push((url.clone(), url));
                    }
                }
            }
        }
        for (key, url) in images {
            self.request_image(key, url);
        }
    }

    /// `n.eu` or the facepile of module 598104: a group with two or more people and no icon is the first two people's
    /// pictures on a diagonal; anything else is its icon (or the default one for its id).
    pub(super) fn group_art(
        &self,
        channel: &PrivateChannel,
        size: f32,
        ring: &'static str,
    ) -> AnyElement {
        if channel.icon.is_none() && channel.recipients.len() >= 2 {
            let part = size / 1.5;
            let offset = part / 2.;
            let gap = size * 3. / 32.;
            let images: Vec<_> = channel
                .recipients
                .iter()
                .take(2)
                .map(|id| {
                    self.stores
                        .users
                        .get(id)
                        .and_then(|user| self.images.get(&avatar(user, 32).0))
                        .cloned()
                })
                .collect();
            return div()
                .relative()
                .flex_none()
                .size(px(size))
                .children(images[0].clone().map(|image| {
                    img(image)
                        .absolute()
                        .left_0()
                        .top_0()
                        .size(px(part))
                        .rounded_full()
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(offset - gap))
                        .top(px(offset - gap))
                        .size(px(part + 2. * gap))
                        .rounded_full()
                        .bg(color(ring)),
                )
                .children(images[1].clone().map(|image| {
                    img(image)
                        .absolute()
                        .left(px(offset))
                        .top(px(offset))
                        .size(px(part))
                        .rounded_full()
                }))
                .into_any_element();
        }
        let image = self.images.get(&group_icon(channel).0).cloned();
        div()
            .flex_none()
            .size(px(size))
            .children(image.map(|image| img(image).size(px(size)).rounded_full()))
            .into_any_element()
    }

    /// A group's row in the DM list.
    pub(super) fn group_list_row(
        &self,
        channel: &PrivateChannel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let selected = self.channel == Some(id)
            && !self.nitro_open
            && !self.shop_active()
            && !self.quests_active()
            && !self.message_requests_active();
        let hovered = self.hovered_dm == Some(id);
        // Module 181254 `eI`: the running call's time, as in a DM's row (`home.rs`).
        let timer = self.private_call_start(id);
        let context_menu_open = self
            .context
            .open
            .as_ref()
            .is_some_and(|open| open.target == context_menu::Target::Dm(id));
        let row_selected = selected || context_menu_open;
        let look = super::home::row_look_with_menu(
            selected,
            hovered,
            context_menu_open,
            self.stores.read_states.mentions(id) > 0,
            self.stores
                .guild_settings
                .channel_muted(0, id, chrono::Utc::now()),
        );
        let tint = look.row;
        // Module 181254 `eX`: a group is typing when anyone but this account is, and online when any member is; the
        // typing pill is drawn in the online colour.
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        let typing = self
            .stores
            .typing_users(id, std::time::Instant::now())
            .iter()
            .any(|user| Some(*user) != me);
        let online = channel
            .recipients
            .iter()
            .any(|user| self.stores.status(*user) == "online");
        let group = SharedString::from(format!("dm-row-{id}"));
        div()
            .id(SharedString::from(format!("dm-{id}")))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.open_context_menu(context_menu::Target::Dm(id), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .group(group.clone())
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if *inside {
                    this.hovered_dm = Some(id);
                } else if this.hovered_dm == Some(id) {
                    this.hovered_dm = None;
                }
                cx.notify();
            }))
            .flex_none()
            .relative()
            .h(px(44.))
            .pt(px(1.))
            .pl(px(9.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_home(Some(id));
                cx.notify();
            }))
            .children(look.pill.then(|| super::home::unread_pill(look.dimmed)))
            .child(
                div()
                    .id(SharedString::from(format!("dm-inner-{id}")))
                    .h(px(42.))
                    .w(px(self.sidebar_row_width()))
                    .pl(px(8.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .text_color(color(tint))
                    .when(row_selected, |this| {
                        this.bg(color("--interactive-background-selected"))
                    })
                    .when(!row_selected, |this| {
                        this.hover(|style| style.bg(color("--interactive-background-hover")))
                            .active(|style| style.bg(color("--interactive-background-selected")))
                    })
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .size(px(32.))
                            .when(look.dimmed, |avatar| avatar.opacity(0.3))
                            .child(self.group_art(channel, 32., "--background-base-lowest"))
                            .when(typing || online, |avatar| {
                                avatar.child(super::typing_avatar::status_corner(
                                    "online",
                                    typing,
                                    "--background-base-lowest",
                                    32.,
                                    super::motion::key(&("dm-row-avatar", id)),
                                    super::typing_avatar::css_dots(&self.stores.experiments),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            // `subtext_zln_tS` takes the row's (or the highlighted layout's) colour.
                            .text_color(color(look.text))
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .text_ellipsis()
                                    .overflow_hidden()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(look.weight())
                                    .child(self.private_channel_name(channel)),
                            )
                            // Not in the pinned client: MessagePeek's last message in place of the member count
                            // (`plugin_hooks/message_peek.rs`).
                            .children(match self.message_peek(channel, false) {
                                super::plugin_hooks::Peek::Keep => Some(
                                    div()
                                        .mt(px(-2.))
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight(500.))
                                        .child(member_count_text(channel))
                                        .into_any_element(),
                                ),
                                super::plugin_hooks::Peek::Empty => None,
                                super::plugin_hooks::Peek::Line(line) => {
                                    Some(div().mt(px(-2.)).child(line).into_any_element())
                                }
                            }),
                    )
                    .children(timer.filter(|_| !hovered).map(|start| {
                        div()
                            .flex_none()
                            .pl(px(4.))
                            .pr(px(8.))
                            .child(self.elapsed_timer(start, cx).parity("dm-call-timer"))
                    }))
                    .when(hovered || timer.is_none(), |row| {
                        row.child(
                            div()
                                .id(SharedString::from(format!("dm-close-{id}")))
                                .mr(px(8.))
                                .size(px(18.))
                                .flex_none()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .opacity(0.)
                                .group_hover(group, |style| style.opacity(0.7))
                                .hover(|style| style.opacity(1.))
                                .on_click(cx.listener(
                                    move |this, event: &gpui::ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        // `closePrivateChannel(id, _, shift)`: holding shift leaves without telling the group.
                                        this.close_dm_with(id, event.modifiers().shift, cx);
                                    },
                                ))
                                .child(icon("friend_ignore", 16., tint)),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The channel header's name of a group (module 95655, `hoverableContainer_9yrrgG`): 32px high, 8px padding and gap,
    /// the avatar with 4px after it, the name in `body-md/medium` (16/20) and a pencil that is laid out only while the
    /// pointer is over it. The whole button opens Edit Group (tooltip "Edit Group"); its context menu is the group's.
    pub(super) fn group_header_name(
        &self,
        channel: &PrivateChannel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let name = self.private_channel_name(channel);
        let size = if channel.icon.is_none() && channel.recipients.len() >= 2 {
            24.
        } else {
            20.
        };
        let hovered = self.group_header_hover;
        let button = div()
            .id("group-header-name")
            .parity("group-header-name")
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if this.group_header_hover != *inside {
                    this.group_header_hover = *inside;
                    cx.notify();
                }
            }))
            .h(px(32.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .whitespace_nowrap()
            .hover(|style| style.bg(color("--background-mod-subtle")))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.tip = None;
                this.open_group_edit(id, window, cx);
            }))
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                    this.open_context_menu(context_menu::Target::Dm(id), event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .mr(px(4.))
                    .size(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.group_art(channel, size, "--background-base-lower")),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(name),
            )
            // Laid out only while hovered (`display: none` otherwise), so the pill grows by 8 + 16px.
            .when(hovered, |this| {
                this.child(
                    div()
                        .size(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("pm_edit", 16., "--text-strong")),
                )
            });
        self.with_tip(
            "group-header-name-tip",
            // The pencil is laid out when the pointer arrives, and the official tooltip is placed against the widened
            // pill; one frame's wait lets the anchor be re-read after that layout.
            Tip::text("Edit Group", Side::Bottom).after(16),
            button,
            cx,
        )
        // The pill starts 7px left of the text column; the offset sits outside the tooltip's anchor.
        .ml(px(-7.))
        .into_any_element()
    }

    /// Whether the member list shows beside a group DM (on by default, as in the official client).
    pub(super) fn group_members_shown(&self) -> bool {
        self.group_members
            && self.current_dm().is_some_and(|channel| channel.kind == 3)
            && self.new_thread.is_none()
    }

    /// The start of a group (`container_w6h4R2` with `editableGdmIcon_5yEg41`): the 80px icon (opens Edit Group), the
    /// name at 32/40 bold, "Welcome to the beginning of the **name** group." and two 40px buttons.
    pub(super) fn group_welcome(
        &self,
        channel: &PrivateChannel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = channel.id;
        let name = self.private_channel_name(channel);
        let prefix = "Welcome to the beginning of the ";
        let text = gpui::StyledText::new(format!("{prefix}{name} group.")).with_highlights([(
            prefix.len()..prefix.len() + name.len(),
            gpui::HighlightStyle {
                font_weight: Some(FontWeight(600.)),
                ..Default::default()
            },
        )]);
        let button =
            |key: &'static str, glyph: &'static str, label: &'static str, primary: bool| {
                let tint = if primary {
                    "--white"
                } else {
                    "--control-secondary-text-default"
                };
                div()
                    .id(key)
                    .h(px(40.))
                    .px(px(16.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .bg(color(if primary {
                        "--control-primary-background-default"
                    } else {
                        "--control-secondary-background-default"
                    }))
                    .hover(move |style| {
                        style.bg(color(if primary {
                            "--control-primary-background-hover"
                        } else {
                            "--control-secondary-background-hover"
                        }))
                    })
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color(tint))
                    .child(icon(glyph, 20., tint))
                    .child(label)
            };
        div()
            .pt(px(16.))
            .child(
                div()
                    .parity("group-welcome")
                    .ml(px(16.))
                    .mr(px(32.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("group-welcome-icon")
                            .size(px(80.))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_group_edit(id, window, cx);
                            }))
                            .child(self.group_art(channel, 80., "--background-base-lower")),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(32.))
                            .line_height(px(40.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(name),
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(text),
                    )
                    .child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .gap(px(8.))
                            .child(
                                button(
                                    "group-welcome-invite",
                                    "dm_add",
                                    "Invite to Group DM",
                                    true,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.open_add_to_dm(window, cx);
                                    },
                                )),
                            )
                            .child(
                                button("group-welcome-edit", "pm_edit", "Edit Group", false)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_group_edit(id, window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// `INVITE_NONFRIENDS_TO_GDM_COACHMARK` (content 759, module 753190 `tl`/`e9`): a popout left of the members
    /// list's Invite button with the NEW badge, "Invite server members to Group DMs", its sentence and Got It, shown
    /// until it is dismissed. Measured on `group-dm/view-coachmark` (card 238 wide, 16px padding); the arrow that
    /// points at the button is not drawn.
    fn group_coachmark(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.stores.content_dismissed(GDM_COACHMARK) || !self.stores.settings_loaded() {
            return None;
        }
        let dismiss = |this: &mut Root, cx: &mut Context<Root>| {
            this.dismiss_group_coachmark();
            cx.notify();
        };
        Some(
            div()
                .id("group-coachmark")
                .parity("group-coachmark")
                .absolute()
                .left(px(-236.))
                .top(px(-3.))
                .w(px(238.))
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-higher"))
                .shadow(vec![gpui::BoxShadow {
                    color: gpui::hsla(0., 0., 0., 0.24),
                    offset: gpui::point(px(0.), px(8.)),
                    blur_radius: px(16.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .px(px(16.))
                .pt(px(35.))
                .pb(px(15.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .id("group-coachmark-close")
                        .absolute()
                        .top(px(10.))
                        .right(px(10.))
                        .size(px(16.))
                        .cursor_pointer()
                        .child(icon("close", 16., "--interactive-icon-default"))
                        .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx))),
                )
                .child(
                    div()
                        .h(px(16.))
                        .px(px(8.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--white"))
                        .child("NEW"),
                )
                .child(
                    div()
                        .mt(px(12.))
                        .text_center()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(700.))
                        .text_color(color("--text-strong"))
                        .child("Invite server members to Group DMs"),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-muted"))
                        .child(
                            "Search to find specific server members. They\u{2019}ll need to accept your invite before they join the group DM.",
                        ),
                )
                .child(
                    div()
                        .id("group-coachmark-got-it")
                        .mt(px(20.))
                        .w_full()
                        .h(px(32.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .bg(color("--control-primary-background-default"))
                        .hover(|style| style.bg(color("--control-primary-background-hover")))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--white"))
                        .child("Got It")
                        .on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx))),
                )
                .into_any_element(),
        )
    }

    /// Marks the coachmark dismissed in the account's settings (the `USER_DISMISS` write of `markAsDismissed`).
    fn dismiss_group_coachmark(&mut self) {
        if self.stores.content_dismissed(GDM_COACHMARK) {
            return;
        }
        let patch = self.stores.dismiss_content(GDM_COACHMARK);
        if patch.is_empty() || crate::storage::settings_writes_blocked() {
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::PATCH,
            route: "/users/@me/settings-proto/1".into(),
            body: Some(json!({"settings": patch})),
        });
    }

    /// The member list beside a group (`membersWrap_86es0b`): "Members\u{2014}n", a row per person (the owner with a
    /// crown) and the "Invite to Group DM" button under them while there is room for more.
    pub(super) fn group_members_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.group_members_shown() {
            return None;
        }
        let channel = self.current_dm()?;
        let mut people: Vec<Id> = self.stores.current_user.iter().map(|me| me.id).collect();
        people.extend(channel.recipients.iter().copied());
        let owner = channel.owner_id;
        let count = people.len();
        let mut rows = Vec::with_capacity(people.len());
        for user in people.iter().filter_map(|id| {
            self.stores
                .users
                .get(id)
                .or_else(|| self.stores.current_user.as_ref().filter(|me| me.id == *id))
        }) {
            let status = self.stores.status(user.id).to_owned();
            let (avatar_key, avatar_url) = super::avatar_source(user);
            let picture = self.images.get(&avatar_key).cloned();
            if picture.is_none() {
                self.want_image(avatar_key, avatar_url, cx);
            }
            let loading_avatar = picture.is_none();
            rows.push(
                div()
                    .flex_none()
                    .h(px(42.))
                    .pt(px(1.))
                    .ml(px(8.))
                    .child(
                        div()
                            .h(px(40.))
                            .w(px(248.))
                            .pl(px(8.))
                            .rounded(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .relative()
                                    .flex_none()
                                    .size(px(32.))
                                    .when(loading_avatar, |this| {
                                        this.rounded_full().bg(color("--background-mod-subtle"))
                                    })
                                    .children(
                                        picture.map(|picture| {
                                            img(picture).size(px(32.)).rounded_full()
                                        }),
                                    )
                                    .child(super::status_dot(&status, "--background-base-lower")),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(500.))
                                    .text_color(color("--channels-default"))
                                    .child(self.dm_display_name(user)),
                            )
                            .when(owner == Some(user.id), |this| {
                                this.child(div().ml(px(-8.)).child(icon(
                                    "owner_crown",
                                    14.,
                                    "--status-warning",
                                )))
                            }),
                    )
                    .into_any_element(),
            );
        }
        let room = count < super::group_dm::GROUP_LIMIT;
        let coachmark = room.then(|| self.group_coachmark(cx)).flatten();
        Some(
            div()
                .relative()
                .flex_none()
                .flex()
                .flex_row()
                .h_full()
                .children(coachmark)
                .child(
                    div()
                        .flex_none()
                        .w(px(1.))
                        .h_full()
                        .bg(color("--background-mod-subtle")),
                )
                .child(
                    div()
                        .parity("group-members")
                        .flex_none()
                        .w(px(self.member_list_width()))
                        .h_full()
                        .bg(color("--background-base-lower"))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex_none()
                                .h(px(42.))
                                .pl(px(16.))
                                .pt(px(20.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--channels-default"))
                                .child(format!("Members\u{2014}{count}")),
                        )
                        .children(rows)
                        .when(room, |this| {
                            this.child(
                                div().flex_none().h(px(56.)).pl(px(16.)).pt(px(16.)).child(
                                    div()
                                        .id("group-members-invite")
                                        .w(px(232.))
                                        .h(px(40.))
                                        .rounded(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(4.))
                                        .cursor_pointer()
                                        .bg(color("--control-secondary-background-default"))
                                        .hover(|style| {
                                            style.bg(color("--control-secondary-background-hover"))
                                        })
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--control-secondary-text-default"))
                                        .child(icon(
                                            "dm_add",
                                            20.,
                                            "--control-secondary-text-default",
                                        ))
                                        .child("Invite to Group DM")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_add_to_dm(window, cx);
                                        })),
                                ),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    /// Edit Group (header button, menu row): the name field starts with the group's own name.
    pub(super) fn open_group_edit(
        &mut self,
        channel: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.start_group_edit(channel, cx)
            && let Some(open) = self.group_edit.as_ref()
        {
            // `autoFocus` on the field.
            window.focus(&open.input.read(cx).focus_handle(cx), cx);
        }
    }

    fn start_group_edit(&mut self, channel: Id, cx: &mut Context<Self>) -> bool {
        let Some(known) = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == channel && known.kind == 3)
        else {
            return false;
        };
        let original = known.name.clone().unwrap_or_default();
        let placeholder = self.private_channel_name(known);
        let input = cx.new(|cx| {
            let mut style = editor_style();
            style.line_height = 20.;
            let mut input = Editor::new(style, cx);
            input.set_keeps_text(true);
            input.set_reports_all_edits(true);
            input.set_max_utf16(NAME_LIMIT);
            input.set_single_line(true);
            input.set_placeholder(placeholder);
            input.set_text(&original);
            input
        });
        let events = cx.subscribe(&input, |this, editor, event: &EditorEvent, cx| {
            if !this
                .group_edit
                .as_ref()
                .is_some_and(|open| open.input.entity_id() == editor.entity_id())
            {
                return;
            }
            match event {
                EditorEvent::Submit(_) => this.save_group_edit(cx),
                EditorEvent::Cancel => this.close_group_edit(),
                _ => {}
            }
            cx.notify();
        });
        self.group_edit = Some(Edit {
            input,
            _events: events,
            channel,
            original,
            icon: None,
            save_focus: cx.focus_handle(),
        });
        cx.notify();
        true
    }

    /// `F5` of module 396787: the select image modal titled "Change Icon" (8 MiB, no recent avatars), then the
    /// cropper with `AVATAR` (module 142630).
    fn pick_group_icon(&mut self, cx: &mut Context<Self>) {
        self.open_image_select(super::image_crop::Purpose::GroupIcon, cx);
    }

    /// `onComplete`: the cropped picture is the pending icon (`setIcon` on Save).
    pub(super) fn group_icon_cropped(
        &mut self,
        uri: String,
        image: Arc<gpui::Image>,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.group_edit.as_mut() {
            open.icon = Some(Some(Picked { uri, image }));
        }
        cx.notify();
    }

    /// Cancel, the close button, Escape and the backdrop (`closeOrShowDiscardChangesAlert`, module 73028): with a
    /// changed name or icon, "Discard unsaved changes?" asks first; else the modal closes.
    pub(super) fn request_close_group_edit(&mut self, cx: &mut Context<Self>) {
        if self.group_edit_changes(cx).is_some() {
            self.chat_extra.alert = Some(super::chat_notices::discard_group_edit_alert());
        } else {
            self.group_edit = None;
        }
    }

    fn close_group_edit(&mut self) {
        self.group_edit = None;
    }

    fn group_edit_changes(&self, cx: &Context<Self>) -> Option<serde_json::Value> {
        let open = self.group_edit.as_ref()?;
        patch_body(open.input.read(cx).text(), &open.original, &open.icon)
    }

    fn save_group_edit(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.group_edit.as_ref() else {
            return;
        };
        if let Some(body) = self.group_edit_changes(cx) {
            let _ = self.requests.send(Request::Rest {
                key: Some(format!("{GROUP_EDIT_KEY}{}", open.channel)),
                method: reqwest::Method::PATCH,
                route: format!("/channels/{}", open.channel),
                body: Some(body),
            });
        }
        self.group_edit = None;
        cx.notify();
    }

    /// Handles `PATCH /channels/{id}` responses for group edits (module 396787 `XA`): shows live ticking rate-limit
    /// alert when HTTP 429 returns `retry_after`.
    pub(super) fn group_edit_answered(&mut self, body: &serde_json::Value, cx: &mut Context<Self>) {
        if body["ok"].as_bool() == Some(true) {
            return;
        }
        let retry_after = body["body"]["retry_after"].as_f64();
        let Some(secs) = retry_after.filter(|s| s.is_finite() && *s > 0.) else {
            return;
        };
        self.chat_extra.alert = Some(rate_limit_alert(secs));
        let ticks = (secs.ceil() as u64).min(3600);
        cx.spawn(async move |this, cx| {
            for _ in 0..ticks {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        let open = this
                            .chat_extra
                            .alert
                            .as_ref()
                            .is_some_and(|a| a.id == "group-icon-rate-limit");
                        if open {
                            cx.notify();
                        }
                        open
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn group_edit_layer(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.group_edit.as_ref()?;
        let channel = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == open.channel)?;
        let focused = open.input.read(cx).focus_handle(cx).is_focused(window);
        let input = open.input.clone();
        let has_icon = match &open.icon {
            Some(choice) => choice.is_some(),
            None => channel.icon.is_some(),
        };
        // The preview: the picked picture, the group's own icon, or the placeholder glyph.
        let picture: Option<AnyElement> = match &open.icon {
            Some(Some(picked)) => Some(
                img(picked.image.clone())
                    .size_full()
                    .rounded_full()
                    .object_fit(gpui::ObjectFit::Cover)
                    .into_any_element(),
            ),
            Some(None) => None,
            None => channel
                .icon
                .as_ref()
                .and_then(|_| self.images.get(&group_icon(channel).0).cloned())
                .map(|image| {
                    img(image)
                        .size_full()
                        .rounded_full()
                        .object_fit(gpui::ObjectFit::Cover)
                        .into_any_element()
                }),
        };
        let button = div()
            .id("group-edit-icon")
            .parity("group-edit-icon")
            .group("group-edit-icon")
            .relative()
            .size(px(120.))
            .mx_auto()
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, _, cx| this.pick_group_icon(cx)))
            .child(match picture {
                Some(picture) => div().size_full().rounded_full().child(picture),
                None => div()
                    .size_full()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(color("--background-mod-subtle"))
                    .hover(|style| style.bg(color("--background-mod-normal")))
                    .child(icon("home_friends", 64., "--background-mod-normal")),
            })
            .child(
                div()
                    .absolute()
                    .top(px(-4.))
                    .right(px(-4.))
                    .size(px(32.))
                    .rounded_full()
                    .border_4()
                    .border_color(color("--background-surface-high"))
                    .bg(color("--background-mod-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("pm_edit", 16., "--interactive-text-default")),
            );
        let remove = has_icon.then(|| {
            div()
                .id("group-edit-remove-icon")
                .px(px(8.))
                .h(px(24.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .line_height(px(16.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-feedback-critical"))
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .child("Remove Icon")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(open) = this.group_edit.as_mut() {
                        open.icon = Some(None);
                    }
                    cx.notify();
                }))
        });
        let field = div()
            .parity_with_border("group-edit-name", if focused { 2. } else { 1. })
            .id("group-edit-name")
            .h(px(40.))
            .w_full()
            .rounded(px(8.))
            .bg(color("--input-background-default"))
            .when(focused, |d| d.border_2())
            .when(!focused, |d| d.border_1())
            .border_color(color(if focused {
                "--input-border-active"
            } else {
                "--input-border-default"
            }))
            .px(px(if focused { 11. } else { 12. }))
            .flex()
            .items_center()
            .on_click(cx.listener(move |_, _, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx);
                cx.notify();
            }))
            .child(div().flex_1().min_w(px(0.)).child(open.input.clone()));
        let body = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(button)
                    .children(remove),
            )
            .child(field);
        let changed = self.group_edit_changes(cx).is_some();
        let mut save = modal::button(
            "group-edit-save",
            "Save",
            if changed {
                modal::Tone::Primary
            } else {
                modal::Tone::PrimaryDisabled
            },
            |this, _, cx| this.save_group_edit(cx),
        );
        save.focus = Some(open.save_focus.clone());
        Some(modal::render(
            modal::Spec {
                id: "group-edit-modal",
                title: "Edit Group".into(),
                subtitle: None,
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "group-edit-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, cx| this.request_close_group_edit(cx),
                    ),
                    save,
                ],
                close: Box::new(|this, _, cx| this.request_close_group_edit(cx)),
            },
            cx,
        ))
    }

    /// The Leave Group confirm (module 605691 builds the strings, 925785 the dialog).
    pub(super) fn open_group_leave(&mut self, channel: Id, silent: bool, cx: &mut Context<Self>) {
        if self
            .stores
            .private_channels
            .iter()
            .any(|known| known.id == channel && known.kind == 3)
        {
            self.group_leave = Some(Leave { channel, silent });
            cx.notify();
        }
    }

    pub(super) fn group_leave_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.group_leave.as_ref()?;
        let channel = self
            .stores
            .private_channels
            .iter()
            .find(|known| known.id == open.channel)?;
        let name = self.private_channel_name(channel);
        let silent = open.silent;
        let id = open.channel;
        let prefix = "Are you sure you want to leave ";
        let text = gpui::StyledText::new(format!(
            "{prefix}{name}? You won\u{2019}t be able to re-join this group unless you are re-invited."
        ))
        .with_highlights(Some((
            prefix.len()..prefix.len() + name.len(),
            gpui::HighlightStyle {
                font_weight: Some(FontWeight(600.)),
                ..Default::default()
            },
        )));
        let check = div()
            .id("group-leave-silent")
            .mt(px(16.))
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .child(
                div()
                    .size(px(20.))
                    .rounded(px(4.))
                    .border_2()
                    .border_color(if silent {
                        color("--control-primary-background-default")
                    } else {
                        color("--interactive-icon-default")
                    })
                    .when(silent, |this| {
                        this.bg(color("--control-primary-background-default"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("checkbox_check", 16., "--white"))
                    }),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child("Leave without notifying other members"),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(open) = this.group_leave.as_mut() {
                    open.silent = !open.silent;
                }
                cx.notify();
            }));
        let body = div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(text)
            .child(check);
        Some(modal::render_small(
            modal::Spec {
                id: "group-leave-modal",
                title: format!("Leave '{name}'").into(),
                subtitle: None,
                body: Some(body.into_any_element()),
                buttons: vec![
                    modal::button(
                        "group-leave-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.group_leave = None,
                    ),
                    modal::button(
                        "group-leave-confirm",
                        "Leave Group",
                        modal::Tone::Critical,
                        move |this, _, cx| {
                            let silent = this.group_leave.take().is_some_and(|open| open.silent);
                            this.close_dm_with(id, silent, cx);
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.group_leave = None),
            },
            cx,
        ))
    }

    /// Harness: `group-manage edit|leave|state` open or report the dialogs of the local group (`dmcall group`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_group_manage(&mut self, args: &[&str], cx: &mut Context<Self>) -> String {
        let group = super::dm_call::GROUP_DM;
        match args {
            ["edit"] => {
                self.start_group_edit(group, cx);
                "Edit Group open".to_owned()
            }
            ["edit", "text", text] => {
                if let Some(open) = self.group_edit.as_ref() {
                    open.input.update(cx, |input, _| input.set_text(text));
                }
                cx.notify();
                format!("name field set to {text}")
            }
            ["edit", "icon"] => {
                // The synthetic picture stands for a chosen file.
                let uri = "data:image/png;base64,AA==".to_owned();
                if let Some(open) = self.group_edit.as_mut() {
                    open.icon = Some(Some(Picked {
                        uri,
                        image: Arc::new(gpui::Image::from_bytes(
                            gpui::ImageFormat::Png,
                            synthetic_png(),
                        )),
                    }));
                }
                cx.notify();
                "a synthetic icon was chosen".to_owned()
            }
            ["leave"] => {
                self.open_group_leave(group, false, cx);
                "Leave Group open".to_owned()
            }
            ["leave", "silent"] => {
                self.open_group_leave(group, true, cx);
                "Leave Group open, silent ticked".to_owned()
            }
            // `system add|remove|leave|name|noname|icon`: the group's system lines, as other people's actions would
            // arrive (a local `MESSAGE_CREATE`; nothing is sent).
            ["system", kind] => {
                let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
                    return "no current user".to_owned();
                };
                let one = json!({"id": "1570000000000000201", "username": "sample-one"});
                let two = json!({"id": "1570000000000000202", "username": "sample-two"});
                let mine = json!({"id": me.to_string(), "username": "me"});
                let (kind_id, author, content, mentions) = match *kind {
                    "add" => (1, mine.clone(), "", vec![one.clone()]),
                    "remove" => (2, mine.clone(), "", vec![two.clone()]),
                    "leave" => (2, one.clone(), "", vec![one.clone()]),
                    "name" => (4, one.clone(), "Weekend crew", vec![]),
                    "noname" => (4, one.clone(), "", vec![]),
                    "icon" => (5, two.clone(), "", vec![]),
                    _ => return "system add|remove|leave|name|noname|icon".to_owned(),
                };
                let id = 1_570_000_000_000_010_000_u64 + self.stores.messages.len() as u64;
                self.stores.dispatch(
                    "MESSAGE_CREATE",
                    &json!({
                        "id": id.to_string(), "channel_id": group.to_string(),
                        "type": kind_id, "content": content, "author": author,
                        "mentions": mentions, "timestamp": chrono::Utc::now().to_rfc3339(),
                        "attachments": [], "embeds": [], "pinned": false, "tts": false,
                        "mention_everyone": false, "mention_roles": [], "edited_timestamp": null,
                    }),
                );
                cx.notify();
                format!("a {kind} system line was added locally")
            }
            // `solo` made the one real group of the capture work and is closed for good: group DMs in the harness are
            // injected channels only.
            // `real`: an injected group shaped like the one created for the goldens (named, with an icon, nobody else in
            // it, owned by the test account, its two system lines from 2026-11-12 01:39 local); nothing is sent.
            ["real"] | ["real", "coachmark"] => {
                // The goldens other than `view-coachmark` were taken with the coachmark already dismissed.
                if args.len() == 1 {
                    let _ = self.stores.dismiss_content(GDM_COACHMARK);
                }
                let Some(me) = self.stores.current_user.as_ref().map(|me| me.id) else {
                    return "no current user".to_owned();
                };
                self.stores.dispatch(
                    "CHANNEL_CREATE",
                    &json!({
                        "id": group.to_string(), "type": 3, "name": "gravecord group", "icon": "f00dcafe",
                        "owner_id": me.to_string(), "recipients": [], "last_message_id": null,
                    }),
                );
                if let Some(channel) = self.stores.private_channels.iter().find(|c| c.id == group) {
                    let key = group_icon(channel).0;
                    self.images.insert(
                        key,
                        Arc::new(gpui::Image::from_bytes(
                            gpui::ImageFormat::Png,
                            synthetic_png(),
                        )),
                    );
                }
                let stamp = "2026-11-11T22:39:00.000000+00:00";
                let name = self
                    .stores
                    .current_user
                    .as_ref()
                    .map(|me| me.username.clone())
                    .unwrap_or_default();
                let mine = json!({"id": me.to_string(), "username": name});
                // The page the service returns, newest first, reaching the start of the group.
                let page: Vec<_> = [(5, ""), (4, "gravecord group")]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (kind, content))| {
                        json!({
                            "id": (1_570_206_400_000_000_100_u64 - index as u64).to_string(),
                            "channel_id": group.to_string(), "type": kind, "content": content,
                            "author": mine, "mentions": [], "timestamp": stamp,
                            "attachments": [], "embeds": [], "pinned": false, "tts": false,
                            "mention_everyone": false, "mention_roles": [], "edited_timestamp": null,
                        })
                    })
                    .collect();
                let spec = dc_state::message_window::FetchSpec {
                    channel: group,
                    before: None,
                    after: None,
                    limit: 50,
                    jump: None,
                    truncate: false,
                    last_message_id: None,
                    started_ms: 0,
                };
                self.group_members = true;
                self.select_home(Some(group));
                self.window_loaded(spec, json!(page), cx);
                cx.notify();
                "injected the real-shaped group; nothing was sent".to_owned()
            }
            ["solo"] => "refused: the harness never creates a real group DM".to_owned(),
            // `leave-solo`: the one real `DELETE /channels/{id}` that removes that group again, through the Leave Group
            // path; it accepts that channel id only.
            ["leave-solo"] => {
                const SOLO_GROUP: Id = 1_570_206_323_914_838_126;
                self.close_dm_with(SOLO_GROUP, false, cx);
                format!("left the solo group {SOLO_GROUP}")
            }
            ["solo-id"] => format!(
                "solo group channel id {}",
                SOLO.load(std::sync::atomic::Ordering::Relaxed)
            ),
            // `solo-edit`: renames the solo group to "gravecord group" and sets a generated icon through the real Edit
            // Group path (one PATCH with both, so the service writes both system lines).
            ["solo-edit"] => {
                let channel = SOLO.load(std::sync::atomic::Ordering::Relaxed);
                if channel == 0 || !self.start_group_edit(channel, cx) {
                    return "no solo group".to_owned();
                }
                use base64::Engine as _;
                let png = synthetic_png();
                let uri = format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&png)
                );
                if let Some(open) = self.group_edit.as_mut() {
                    open.icon = Some(Some(Picked {
                        uri,
                        image: Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)),
                    }));
                    open.input
                        .update(cx, |input, _| input.set_text("gravecord group"));
                }
                self.save_group_edit(cx);
                format!("rename and icon sent for {channel}")
            }
            ["state"] => format!(
                "stored messages {}; edit open {}; changes {:?}; leave open {}",
                self.stores.messages.get(&group).map_or(0, Vec::len),
                self.group_edit.is_some(),
                self.group_edit_changes(cx),
                self.group_leave.is_some()
            ),
            _ => "group-manage edit [text <t> | icon] | leave [silent] | state".to_owned(),
        }
    }
}

/// A small PNG for the harness's chosen icon: a colour ramp.
#[cfg(feature = "capture")]
fn synthetic_png() -> Vec<u8> {
    let image = image::RgbImage::from_fn(64, 64, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 4) as u8, 160])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = image.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked() -> Picked {
        Picked {
            uri: "data:image/png;base64,AA==".to_owned(),
            image: Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![0])),
        }
    }

    #[test]
    fn save_stays_off_until_the_name_or_the_icon_changes() {
        assert_eq!(patch_body("Team", "Team", &None), None);
        assert_eq!(patch_body("", "", &None), None);
    }

    #[test]
    fn the_patch_carries_only_what_changed() {
        assert_eq!(
            patch_body("New", "Old", &None),
            Some(json!({"name": "New"}))
        );
        assert_eq!(
            patch_body("Old", "Old", &Some(None)),
            Some(json!({"icon": null}))
        );
        assert_eq!(
            patch_body("New", "Old", &Some(Some(picked()))),
            Some(json!({"name": "New", "icon": "data:image/png;base64,AA=="}))
        );
        // Clearing the name sends an empty one, which the service reads as "default".
        assert_eq!(patch_body("", "Old", &None), Some(json!({"name": ""})));
    }

    #[test]
    fn a_group_counts_its_people_with_this_account() {
        let mut channel = PrivateChannel {
            id: 1,
            kind: 3,
            recipients: vec![2, 3],
            last_message_id: 0,
            name: None,
            icon: None,
            application_id: None,
            owner_id: None,
            flags: 0,
            message_request: false,
            spam: false,
            request_timestamp: None,
            blocked_user_warning_dismissed: false,
        };
        assert_eq!(member_count_text(&channel), "3 Members");
        channel.recipients.clear();
        assert_eq!(member_count_text(&channel), "1 Member");
    }

    #[test]
    fn rate_limit_countdown_units_and_alert_match_396787_and_576470() {
        assert_eq!(format_countdown_units(90), "00h 01m 30s");
        assert_eq!(format_countdown_units(86_400 + 7_384), "01d 02h 03m 04s");
        let alert = rate_limit_alert(45.2);
        assert_eq!(alert.title, "You are being rate limited.");
        assert!(!dc_intl::t("VIJkAy").is_empty());
    }
}
