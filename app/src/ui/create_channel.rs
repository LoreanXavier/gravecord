//! Create Channel and Create Category (the server menu's entries), measured in
//! `design/dom/modals/create-channel-darker-cozy.json`: Channel Type radios (Text, Voice, Forum) with their
//! descriptions, Channel Name with the type's icon, and the Private Channel switch. Create posts
//! `POST /guilds/{guild}/channels {name, type, parent_id?}`; text and voice names are lower-cased with spaces as
//! dashes, as the official field normalizes them.

use super::{Root, color, editor_style, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use serde_json::json;

/// GUILD_TEXT, GUILD_VOICE, GUILD_CATEGORY, GUILD_ANNOUNCEMENT, GUILD_STAGE_VOICE and GUILD_FORUM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Text = 0,
    Voice = 2,
    Category = 4,
    Announcement = 5,
    Stage = 13,
    Forum = 15,
}

/// The two slides of the dialog (`slide` of module 410380): the channel's details, then, for a Stage, the people and
/// roles that moderate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slide {
    Info,
    Moderators,
}

pub(crate) struct CreateChannel {
    pub kind: Kind,
    pub name: Entity<Editor>,
    _events: Subscription,
    pub private: bool,
    pub parent: Option<Id>,
    /// The body's scroll position (the dialog scrolls once the Announcement and Stage rows are there).
    scroll: gpui::ScrollHandle,
    slide: Slide,
    /// Roles picked as Stage moderators.
    moderators: Vec<Id>,
}

/// The official name rule for text-like channels: lower case, whitespace runs become one dash.
pub(crate) fn channel_slug(name: &str) -> String {
    name.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// The slug while it is typed: lower case, each space a dash at once (the trim waits for the save).
pub(crate) fn channel_slug_live(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// `container_9AraPt`: a 48x24 switch, the white 24px thumb at the side it is set to.
pub(super) fn switch(on: bool) -> gpui::Div {
    div()
        .relative()
        .flex_none()
        .w(px(48.))
        .h(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(color("--border-subtle"))
        .bg(if on {
            color("--background-brand")
        } else {
            gpui::rgba(0x0000001f)
        })
        // `thumb_9AraPt`: a 24px box whose knob is an 18px circle 3px in.
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 26. } else { 2. }))
                .size(px(18.))
                .rounded_full()
                .bg(gpui::rgb(0xffffff)),
        )
}

/// A disabled switch. `switchIndicator_9AraPt` at half opacity holds a 16px knob and dims it as one group; `gpui`'s opacity
/// dims each paint apart, so the knob is drawn solid in the colour that group gives over `behind`.
pub(super) fn switch_dimmed(on: bool, behind: &str) -> gpui::Div {
    let under = color(behind);
    let knob = gpui::Rgba {
        r: (1. + under.r) / 2.,
        g: (1. + under.g) / 2.,
        b: (1. + under.b) / 2.,
        a: 1.,
    };
    div()
        .relative()
        .flex_none()
        .w(px(48.))
        .h(px(24.))
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(16.))
                .border_1()
                .border_color(color("--border-subtle"))
                .opacity(0.5)
                .bg(if on {
                    color("--background-brand")
                } else {
                    gpui::rgba(0x0000001f)
                }),
        )
        .child(
            div()
                .absolute()
                .top(px(4.))
                .left(px(if on { 28. } else { 5. }))
                .size(px(16.))
                .rounded_full()
                .bg(knob),
        )
}

/// `radioIndicator_oJuRbG`: brand with a white 8px dot when picked, a faint ring otherwise.
pub(super) fn radio(picked: bool) -> gpui::Div {
    div()
        .size(px(20.))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_color(if picked {
            color("--checkbox-border-selected-default")
        } else {
            color("--checkbox-border-default")
        })
        .bg(if picked {
            color("--checkbox-background-selected-default")
        } else {
            color("--checkbox-background-default")
        })
        .flex()
        .items_center()
        .justify_center()
        .when(picked, |this| {
            this.child(div().size(px(8.)).rounded_full().bg(gpui::rgb(0xffffff)))
        })
}

const VIEW_CHANNEL: u64 = 1 << 10;
const CONNECT: u64 = 1 << 20;

/// `CG(guildId, type, [], true)` of module 147036 plus `n3(userId, type)`: `@everyone` (the guild's own id) loses View
/// Channel, and Connect for voice and categories; the creator gets it back as a member overwrite unless they own the
/// server.
pub(crate) fn private_overwrites(guild: Id, kind: Kind, creator: Option<Id>) -> serde_json::Value {
    let bits = match kind {
        Kind::Voice | Kind::Category | Kind::Stage => VIEW_CHANNEL | CONNECT,
        Kind::Text | Kind::Forum | Kind::Announcement => VIEW_CHANNEL,
    };
    let mut overwrites =
        vec![json!({"id": guild.to_string(), "type": 0, "allow": "0", "deny": bits.to_string()})];
    if let Some(creator) = creator {
        overwrites.push(
            json!({"id": creator.to_string(), "type": 1, "allow": bits.to_string(), "deny": "0"}),
        );
    }
    serde_json::Value::Array(overwrites)
}

/// `IP(guildId)` of module 147036: an Announcement channel is created with `@everyone` denied Send Messages.
pub(crate) fn announcement_overwrites(guild: Id) -> serde_json::Value {
    json!([{
        "id": guild.to_string(),
        "type": 0,
        "allow": "0",
        "deny": dc_state::permissions::SEND_MESSAGES.to_string(),
    }])
}

/// `j(id, type)` of module 841811 with `QY` of module 233993 (Manage Channels, Mute Members, Move Members): the
/// overwrite each picked role gives a Stage's moderators (role type 0).
pub(crate) fn stage_moderator_overwrites(roles: &[Id]) -> serde_json::Value {
    use dc_state::permissions::{MANAGE_CHANNELS, MOVE_MEMBERS, MUTE_MEMBERS};
    let allow = (MANAGE_CHANNELS | MUTE_MEMBERS | MOVE_MEMBERS).to_string();
    serde_json::Value::Array(
        roles
            .iter()
            .map(|role| json!({"id": role.to_string(), "type": 0, "allow": allow, "deny": "0"}))
            .collect(),
    )
}

/// The body of `POST /guilds/{guild}/channels` (`createChannel` of module 410380): `parentId` is null for a category.
fn create_body(
    kind: Kind,
    name: &str,
    overwrites: serde_json::Value,
    parent: Option<Id>,
) -> serde_json::Value {
    let mut body = json!({"type": kind as u64, "name": name, "permission_overwrites": overwrites});
    if let Some(parent) = parent.filter(|_| kind != Kind::Category) {
        body["parent_id"] = json!(parent.to_string());
    }
    body
}

/// The Announcement row shows in a server with the `NEWS` feature (`canCreateAnnouncementChannel` of module 410380).
fn can_create_announcement(features: &[String]) -> bool {
    features.iter().any(|feature| feature == "NEWS")
}

/// The Stage row: a Community server and both Manage Roles and Manage Channels (`m` of `td` in module 410380).
fn can_create_stage(features: &[String], manage_roles: bool, manage_channels: bool) -> bool {
    features.iter().any(|feature| feature == "COMMUNITY") && manage_roles && manage_channels
}

impl Root {
    /// The current user may create channels in the open server.
    pub(super) fn can_manage_channels(&self) -> bool {
        let (Some(guild), Some(me)) = (self.current_guild(), self.stores.me()) else {
            return false;
        };
        dc_state::permissions::can_in_guild(guild, me, dc_state::permissions::MANAGE_CHANNELS)
    }

    pub(super) fn open_create_channel(
        &mut self,
        category: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder(
                if category {
                    "New Category"
                } else {
                    "new-channel"
                }
                .to_owned(),
            );
            editor
        });
        let events = cx.subscribe(&name, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    editor.update(cx, |editor, _| editor.set_text(text));
                    this.submit_create_channel(cx);
                }
                EditorEvent::Cancel => this.create_channel = None,
                // `handleNameChange` through `LIMITED_CHANNEL_NAME` (modules 965805, 478437): text-like names are
                // slugged as they are typed.
                EditorEvent::Edited => {
                    let limited = this.create_channel.as_ref().is_some_and(|form| {
                        matches!(form.kind, Kind::Text | Kind::Forum | Kind::Announcement)
                    });
                    if limited {
                        let text = editor.read(cx).text().to_owned();
                        let slug = channel_slug_live(&text);
                        if slug != text {
                            editor.update(cx, |editor, _| editor.set_text(&slug));
                        }
                    }
                }
            }
            cx.notify();
        });
        window.focus(&gpui::Focusable::focus_handle(name.read(cx), cx), cx);
        self.create_channel = Some(CreateChannel {
            kind: if category { Kind::Category } else { Kind::Text },
            name,
            _events: events,
            private: false,
            parent: None,
            scroll: gpui::ScrollHandle::new(),
            slide: Slide::Info,
            moderators: Vec::new(),
        });
        cx.notify();
    }

    fn submit_create_channel(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Some(guild)) = (self.create_channel.as_ref(), self.guild) else {
            return;
        };
        let typed = form.name.read(cx).text().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let name = match form.kind {
            Kind::Text | Kind::Forum | Kind::Announcement => channel_slug(&typed),
            Kind::Voice | Kind::Category | Kind::Stage => typed,
        };
        let owner = self
            .current_guild()
            .and_then(|guild| guild.owner_id)
            .is_some_and(|owner| {
                Some(owner) == self.stores.current_user.as_ref().map(|user| user.id)
            });
        // Module 410380 `handleSubmit`: an Announcement channel takes `IP`, a Stage only its moderators' overwrites
        // (never private), the rest the private set when the switch is on.
        let overwrites = match form.kind {
            Kind::Announcement => announcement_overwrites(guild),
            Kind::Stage => stage_moderator_overwrites(&form.moderators),
            _ if form.private => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                private_overwrites(guild, form.kind, me.filter(|_| !owner))
            }
            _ => json!([]),
        };
        let body = create_body(form.kind, &name, overwrites, form.parent);
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/guilds/{guild}/channels"),
            body: Some(body),
        });
        self.create_channel = None;
        cx.notify();
    }

    pub(super) fn create_channel_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.create_channel.as_ref()?;
        let category = form.kind == Kind::Category;
        let typed = !form.name.read(cx).text().trim().is_empty();
        let label = |text: &'static str| {
            div()
                .mb(px(8.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        let option = |kind: Kind, name: &'static str, title: &'static str, detail: &'static str| {
            let picked = form.kind == kind;
            // One radio group: the picked option is its tab stop (Space picks the focused one).
            super::keyboard::inset_ring_when_focused(
                div().id(SharedString::from(format!("channel-type-{kind:?}"))),
            )
            .focusable()
            .tab_index(0)
            .tab_stop(picked)
            .relative()
            .h(px(42.))
            .pl(px(56.))
            .rounded(px(8.))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(form) = this.create_channel.as_mut() {
                    form.kind = kind;
                    // `handleTypeChange`: a Stage is never private; the field's hint follows the type.
                    if kind == Kind::Stage {
                        form.private = false;
                    }
                    let hint = if kind == Kind::Forum {
                        "forum-discussion"
                    } else {
                        "new-channel"
                    };
                    form.name
                        .update(cx, |editor, _| editor.set_placeholder(hint.to_owned()));
                    if matches!(kind, Kind::Text | Kind::Forum | Kind::Announcement) {
                        let text = form.name.read(cx).text().to_owned();
                        let slug = channel_slug_live(&text);
                        if slug != text {
                            form.name.update(cx, |editor, _| editor.set_text(&slug));
                        }
                    }
                }
                cx.notify();
            }))
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(0.))
                    .child(radio(picked)),
            )
            .child(div().absolute().left(px(32.)).top(px(0.)).child(icon(
                name,
                20.,
                "--text-strong",
            )))
            .child(
                div()
                    .line_height(px(20.))
                    .text_color(color("--text-default"))
                    .child(title),
            )
            .child(
                div()
                    .mt(px(4.))
                    .ml(px(-24.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-subtle"))
                    .child(detail),
            )
        };
        let name_icon = match form.kind {
            Kind::Voice => "channel_type_voice",
            Kind::Forum => "channel_type_forum",
            Kind::Category => "",
            Kind::Text => "channel_type_text",
            Kind::Announcement => "announcement_channel",
            Kind::Stage => "stage_channel",
        };
        let (news, stage) = self.current_guild().map_or((false, false), |guild| {
            let me = self.stores.me();
            let may = |permission| {
                me.is_some_and(|me| dc_state::permissions::can_in_guild(guild, me, permission))
            };
            (
                can_create_announcement(&guild.features),
                can_create_stage(
                    &guild.features,
                    may(dc_state::permissions::MANAGE_ROLES),
                    may(dc_state::permissions::MANAGE_CHANNELS),
                ),
            )
        });
        let private = form.private;
        let body = div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .when(!category, |this| {
                this.child(
                    div()
                        .child(label("Channel Type"))
                        .child(
                            div()
                                .pt(px(8.))
                                .pb(px(4.))
                                .flex()
                                .flex_col()
                                .gap(px(16.))
                                .child(option(
                                    Kind::Text,
                                    "channel_type_text",
                                    "Text",
                                    "Send messages, images, GIFs, emoji, opinions, and puns",
                                ))
                                .child(option(
                                    Kind::Voice,
                                    "channel_type_voice",
                                    "Voice",
                                    "Hang out together with voice, video, and screen share",
                                ))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(option(
                                            Kind::Forum,
                                            "channel_type_forum",
                                            "Forum",
                                            "Create a space for organized discussions",
                                        ))
                                        .child(
                                            div()
                                                .id("create-channel-forum-examples")
                                                .ml(px(48.))
                                                .mt(px(-4.))
                                                .mb(px(4.))
                                                .cursor_pointer()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-link"))
                                                .hover(|s| s.underline())
                                                .child("See examples")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let guild = this.guild;
                                                    this.open_forum_demo(guild, false, cx);
                                                })),
                                        ),
                                )
                                .children(news.then(|| {
                                    option(
                                        Kind::Announcement,
                                        "announcement_channel",
                                        "Announcement",
                                        "Important updates for people in and out of the server",
                                    )
                                }))
                                .children(stage.then(|| {
                                    option(
                                        Kind::Stage,
                                        "stage_channel",
                                        "Stage",
                                        "Host events, panels, and Q&As for an audience",
                                    )
                                })),
                        ),
                )
            })
            .child(
                div()
                    .child(label(if category { "Category Name" } else { "Channel Name" }))
                    .child(
                        div()
                            .h(px(40.))
                            .px(px(4.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--input-border-active"))
                            .bg(color("--input-background-default"))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .when(!category, |this| {
                                this.child(
                                    div()
                                        .size(px(30.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon(name_icon, 16., "--icon-strong")),
                                )
                            })
                            .when(category, |this| this.pl(px(12.)))
                            .child(div().flex_1().min_w(px(0.)).child(form.name.clone()))
                            // The emoji button, grey until hovered (`spriteGreyscale_SjG8Ug`).
                            .children(self.images.get("emoji_button").cloned().map(|image| {
                                let field = form.name.clone();
                                super::keyboard::inset_ring_when_focused(
                                    div().id("create-channel-emoji"),
                                )
                                .focusable()
                                .tab_index(0)
                                    .w(px(32.))
                                    .h(px(30.))
                                    .flex()
                                    .items_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(
                                        move |this, event: &gpui::ClickEvent, window, cx| {
                                            let at = event.position();
                                            this.picker_field = Some(field.clone());
                                            this.open_picker(
                                                super::picker::Target::Field,
                                                gpui::point(at.x, at.y - px(24.)),
                                                window,
                                                cx,
                                            );
                                        },
                                    ))
                                    .child(gpui::img(image).size(px(18.)).grayscale(true))
                            })),
                    ),
            )
            // `e9` of module 410380: under the name of an Announcement channel, what following it means.
            .children((form.kind == Kind::Announcement).then(|| {
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(
                        "Users can Follow this channel in their own servers. Posts you \"Publish\" from this channel will appear in those servers. Learn more.",
                    )
            }))
            // Neither an Announcement channel nor a Stage has the private switch (`renderPrivacyOptions`).
            .children((!matches!(form.kind, Kind::Announcement | Kind::Stage)).then(|| {
                super::keyboard::inset_ring_when_focused(div().id("create-channel-private"))
                    .focusable()
                    .tab_index(0)
                    .cursor_pointer()
                    .flex()
                    .gap(px(24.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = this.create_channel.as_mut() {
                            form.private = !form.private;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(div().pr(px(4.)).child(icon("channel_private", 16., "--interactive-icon-default")))
                                    .child(
                                        div()
                                            .line_height(px(20.))
                                            .font_weight(FontWeight(500.))
                                            .text_color(color("--text-strong"))
                                            .child(if category { "Private Category" } else { "Private Channel" }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-subtle"))
                                    .child(if category {
                                        "By making a category private, only selected members and roles will be able to view this category. Synced channels in this category will automatically match to this setting."
                                    } else {
                                        "Only selected members and roles will be able to view this channel."
                                    }),
                            ),
                    )
                    .child(switch(private))
            }))
            .into_any_element();
        let create_label = if category {
            "Create Category"
        } else {
            "Create Channel"
        };
        if form.slide == Slide::Moderators {
            return Some(self.stage_moderators_modal(form, cx));
        }
        // Four or more types no longer fit: `bodyInnerShouldScroll_dvN-MP` holds the body to 416px, between two 1px
        // rules (`bodySpacerTopBorder`, `bodyScrollingSpacerBottom`) across the whole dialog.
        let scrolling = !category && (news || stage);
        let body = if scrolling {
            div()
                .relative()
                .mx(px(-24.))
                .h(px(418.))
                .border_t_1()
                .border_b_1()
                .border_color(color("--border-subtle"))
                .child(
                    div()
                        .id("create-channel-scroll")
                        .track_scroll(&form.scroll)
                        .size_full()
                        .overflow_y_scroll()
                        .py(px(12.))
                        .px(px(24.))
                        .child(body),
                )
                .child(super::menu::menu_scrollbar(form.scroll.clone()))
                .into_any_element()
        } else {
            body.into_any_element()
        };
        // `L1zJgb`: "in <category>" under the title when the dialog was opened from a category's plus.
        let subtitle = form
            .parent
            .and_then(|parent| {
                self.current_guild()?
                    .channels
                    .iter()
                    .find(|known| known.id == parent)
                    .map(|known| known.name.clone())
            })
            .map(|name| div().child(format!("in {name}")).into_any_element());
        // A Stage goes on to pick its moderators (`g` in module 410380: "Next").
        let stage_next = form.kind == Kind::Stage;
        Some(modal::render(
            modal::Spec {
                id: "create-channel-modal",
                title: create_label.into(),
                subtitle,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "create-channel-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, _| this.create_channel = None,
                    ),
                    modal::button(
                        "create-channel-submit",
                        if stage_next { "Next" } else { create_label },
                        if typed {
                            modal::Tone::Primary
                        } else {
                            modal::Tone::PrimaryDisabled
                        },
                        move |this, _, cx| {
                            if stage_next {
                                if let Some(form) = this.create_channel.as_mut()
                                    && !form.name.read(cx).text().trim().is_empty()
                                {
                                    form.slide = Slide::Moderators;
                                }
                            } else {
                                this.submit_create_channel(cx);
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.create_channel = None),
            },
            cx,
        ))
    }

    /// The Stage's second slide (`tr` of module 410380, "Add Stage moderators"): who moderates it, then Create, or
    /// Skip while nobody is picked. Roles only: the member search and the typed filter are not built.
    fn stage_moderators_modal(&self, form: &CreateChannel, cx: &mut Context<Self>) -> AnyElement {
        let name = form.name.read(cx).text().trim().to_owned();
        let roles: Vec<(Id, String)> = self.current_guild().map_or_else(Vec::new, |guild| {
            let mut roles: Vec<(Id, String)> = guild
                .roles
                .iter()
                .filter(|(id, _)| **id != guild.id)
                .map(|(id, name)| (*id, name.clone()))
                .collect();
            roles.sort_by_key(|role| role.1.to_lowercase());
            roles
        });
        let rows = roles.into_iter().map(|(role, label)| {
            let picked = form.moderators.contains(&role);
            div()
                .id(("stage-moderator", role))
                .h(px(40.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--background-mod-subtle")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = this.create_channel.as_mut() {
                        if let Some(at) = form.moderators.iter().position(|known| *known == role) {
                            form.moderators.remove(at);
                        } else {
                            form.moderators.push(role);
                        }
                    }
                    cx.notify();
                }))
                .child(super::server_games::checkbox(picked))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(color("--text-default"))
                        .child(label),
                )
        });
        let body = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .line_height(px(24.))
                    .text_size(px(16.))
                    .text_color(color("--text-default"))
                    .child(icon("stage_channel", 16., "--text-default"))
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(
                        "Stage moderators are Speakers who can add and remove other Speakers. They can also start the Stage event. Stage moderators don't have to be mods of the server.",
                    ),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(color("--text-subtle"))
                    .child("Add individual members by starting with @ or type a role name"),
            )
            .child(div().max_h(px(240.)).flex().flex_col().children(rows))
            .into_any_element();
        let label = if form.moderators.is_empty() {
            "Skip"
        } else {
            "Create Channel"
        };
        modal::render(
            modal::Spec {
                id: "create-channel-modal",
                title: "Add Stage moderators".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "create-channel-back",
                        "Back",
                        modal::Tone::Secondary,
                        |this, _, _| {
                            if let Some(form) = this.create_channel.as_mut() {
                                form.slide = Slide::Info;
                            }
                        },
                    ),
                    modal::button(
                        "create-channel-submit",
                        label,
                        modal::Tone::Primary,
                        |this, _, cx| this.submit_create_channel(cx),
                    ),
                ],
                close: Box::new(|this, _, _| this.create_channel = None),
            },
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Kind, announcement_overwrites, can_create_announcement, can_create_stage, channel_slug,
        create_body, private_overwrites, stage_moderator_overwrites,
    };

    #[test]
    fn an_announcement_channel_denies_everyone_sending() {
        let body = create_body(
            Kind::Announcement,
            "news",
            announcement_overwrites(10),
            Some(5),
        );
        assert_eq!(
            body,
            serde_json::json!({
                "type": 5,
                "name": "news",
                "permission_overwrites": [{"id": "10", "type": 0, "allow": "0", "deny": "2048"}],
                "parent_id": "5",
            })
        );
    }

    #[test]
    fn a_stage_gives_its_picked_roles_the_moderator_set() {
        // Manage Channels (16) + Mute Members (4194304) + Move Members (16777216).
        let body = create_body(
            Kind::Stage,
            "Town Hall",
            stage_moderator_overwrites(&[7, 8]),
            None,
        );
        assert_eq!(
            body,
            serde_json::json!({
                "type": 13,
                "name": "Town Hall",
                "permission_overwrites": [
                    {"id": "7", "type": 0, "allow": "20971536", "deny": "0"},
                    {"id": "8", "type": 0, "allow": "20971536", "deny": "0"},
                ],
            })
        );
        assert_eq!(
            create_body(Kind::Stage, "x", stage_moderator_overwrites(&[]), None)["permission_overwrites"],
            serde_json::json!([])
        );
    }

    #[test]
    fn the_extra_rows_need_their_server_features() {
        let community = vec!["COMMUNITY".to_owned()];
        let news = vec!["NEWS".to_owned()];
        assert!(can_create_announcement(&news));
        assert!(!can_create_announcement(&community));
        assert!(can_create_stage(&community, true, true));
        assert!(!can_create_stage(&community, true, false));
        assert!(!can_create_stage(&community, false, true));
        assert!(!can_create_stage(&news, true, true));
    }

    #[test]
    fn names_take_the_channel_form() {
        assert_eq!(channel_slug("My New  Channel"), "my-new-channel");
    }

    #[test]
    fn a_private_text_channel_hides_from_everyone_and_shows_to_its_creator() {
        let overwrites = private_overwrites(10, Kind::Text, Some(7));
        assert_eq!(
            overwrites,
            serde_json::json!([
                {"id": "10", "type": 0, "allow": "0", "deny": "1024"},
                {"id": "7", "type": 1, "allow": "1024", "deny": "0"},
            ])
        );
    }

    #[test]
    fn a_private_voice_channel_also_denies_connect_and_an_owner_needs_no_overwrite() {
        let overwrites = private_overwrites(10, Kind::Voice, None);
        assert_eq!(
            overwrites,
            serde_json::json!([{"id": "10", "type": 0, "allow": "0", "deny": "1049600"}])
        );
    }
}
