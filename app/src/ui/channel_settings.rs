//! Channel settings (module 336541 on the `standardSidebarView_jnk11m` shell): a 372px sidebar (the channel and its
//! category over Overview, Permissions, Invites, Integrations and Delete Channel) beside a 740px column with the ESC
//! button. Overview edits the name, topic, slowmode, spoiler flag and thread inactivity; any change raises
//! "Careful — you have unsaved changes!" with Reset and Save Changes (`PATCH /channels/{id}`). Measured in
//! `design/dom/settings/channel-*-darker-cozy.json`.

use super::{Root, color, icon, window_controls};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::{ChannelKind, Id};
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, SharedString, Subscription, Window,
    deferred, div, img, prelude::*, px,
};

/// `IS_SPOILER_CHANNEL` in the channel flags (module 375389).
pub(super) const IS_SPOILER_CHANNEL: u64 = 2_097_152;
/// `s_7`: slowmode choices in seconds, up to `WA1` (six hours).
const SLOWMODE: [u64; 14] = [
    0, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 21600,
];
/// Module 268761 `Gk`: thread inactivity in minutes.
const ARCHIVE: [(u64, &str); 4] = [
    (60, "1 Hour"),
    (1440, "24 Hours"),
    (4320, "3 Days"),
    (10080, "1 Week"),
];
/// The Overview's illustration (`/assets/b00bbf36ff21d401.svg`), 280 by 165.
const OVERVIEW_ART: &str = "https://discordapp.com/assets/b00bbf36ff21d401.svg";
pub(super) const SAVE_KEY: &str = "channel-settings-save";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Overview,
    Permissions,
    Invites,
    Integrations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Select {
    Slowmode,
    Archive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Values {
    name: String,
    topic: String,
    slowmode: u64,
    spoiler: bool,
    archive: u64,
}

pub(crate) struct ChannelSettings {
    pub channel: Id,
    pub section: Section,
    pub(super) name: Entity<Editor>,
    pub(super) topic: Entity<Editor>,
    slowmode: u64,
    spoiler: bool,
    archive: u64,
    saved: Values,
    open_select: Option<Select>,
    confirm_delete: bool,
    /// Advanced permissions: the role or member shown, and overwrites changed but not saved (allow, deny, type).
    pub(super) perm_target: Option<Id>,
    pub(super) perm_edits: std::collections::HashMap<Id, (u64, u64, u64)>,
    /// "Sync permissions?" is open.
    pub(super) confirm_sync: bool,
    /// The "+" popout's search field while it is open (`createOverwritePopout_4T6RB7`).
    pub(super) add_target: Option<(Entity<Editor>, Subscription)>,
    _events: [Subscription; 2],
}

/// Module 879631 `$(seconds, false)`: "Off", "N seconds", "N minutes", "N hours", "N days".
pub(super) fn slowmode_label(seconds: u64) -> String {
    let plural = |count: u64, unit: &str| {
        if count == 1 {
            format!("1 {unit}")
        } else {
            format!("{count} {unit}s")
        }
    };
    if seconds == 0 {
        "Off".to_owned()
    } else if seconds >= 86_400 {
        plural(seconds / 86_400, "day")
    } else if seconds >= 3_600 {
        plural(seconds / 3_600, "hour")
    } else if seconds >= 60 {
        plural(seconds / 60, "minute")
    } else {
        plural(seconds, "second")
    }
}

impl ChannelSettings {
    pub(super) fn confirm_delete_open(&self) -> bool {
        self.confirm_delete
    }

    pub(super) fn close_delete(&mut self) {
        self.confirm_delete = false;
    }

    fn values(&self, cx: &Context<Root>) -> Values {
        Values {
            name: self.name.read(cx).text().to_owned(),
            topic: self.topic.read(cx).text().to_owned(),
            slowmode: self.slowmode,
            spoiler: self.spoiler,
            archive: self.archive,
        }
    }
}

impl Root {
    pub(super) fn open_channel_settings(
        &mut self,
        channel: Id,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(known) = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .find(|known| known.id == channel)
            .cloned()
        else {
            return;
        };
        let saved = Values {
            name: known.name.clone(),
            topic: known.topic.clone().unwrap_or_default(),
            slowmode: known.rate_limit_per_user,
            spoiler: known.flags & IS_SPOILER_CHANNEL != 0,
            archive: known.default_auto_archive_duration.unwrap_or(4320),
        };
        let name = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_text(&saved.name);
            editor
        });
        let topic = cx.new(|cx| {
            let mut editor = Editor::new(super::editor_style(), cx);
            editor.set_placeholder("Let everyone know how to use this channel!".to_owned());
            editor.set_text(&saved.topic);
            editor
        });
        let voice = known.kind == ChannelKind::Voice;
        let name_events = cx.subscribe(&name, move |this, editor, event: &EditorEvent, cx| {
            match event {
                // Text channel names are slugged as they are typed: lower case, dashes for spaces.
                EditorEvent::Edited if !voice => {
                    let text = editor.read(cx).text().to_owned();
                    let slug = super::create_channel::channel_slug_live(&text);
                    if slug != text {
                        editor.update(cx, |editor, _| editor.set_text(&slug));
                    }
                }
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => this.channel_settings = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        let topic_events = cx.subscribe(&topic, |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    editor.update(cx, |editor, _| editor.set_text(&text));
                }
                EditorEvent::Cancel => this.channel_settings = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        self.channel_settings = Some(ChannelSettings {
            channel,
            section: Section::Overview,
            name,
            topic,
            slowmode: saved.slowmode,
            spoiler: saved.spoiler,
            archive: saved.archive,
            saved,
            open_select: None,
            confirm_delete: false,
            perm_target: None,
            perm_edits: std::collections::HashMap::new(),
            confirm_sync: false,
            add_target: None,
            _events: [name_events, topic_events],
        });
        self.request_image(OVERVIEW_ART.to_owned(), OVERVIEW_ART.to_owned());
        let plus = super::channel_settings_pages::ADD_TARGET_ART;
        self.request_image(plus.to_owned(), plus.to_owned());
        let _ = window;
        cx.notify();
    }

    fn reset_channel_settings(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.channel_settings.as_mut() else {
            return;
        };
        let saved = open.saved.clone();
        open.name
            .update(cx, |editor, _| editor.set_text(&saved.name));
        open.topic
            .update(cx, |editor, _| editor.set_text(&saved.topic));
        open.slowmode = saved.slowmode;
        open.spoiler = saved.spoiler;
        open.archive = saved.archive;
        open.perm_edits.clear();
        cx.notify();
    }

    /// `updateChannel`: the edited fields in one PATCH; the flags keep every other bit.
    fn save_channel_settings(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.channel_settings.as_ref() else {
            return;
        };
        let values = open.values(cx);
        let channel = open.channel;
        // Advanced permissions: one PUT per changed overwrite.
        for (target, (allow, deny, kind)) in &open.perm_edits {
            let _ = self.requests.send(Request::Rest {
                key: None,
                method: reqwest::Method::PUT,
                route: format!("/channels/{channel}/permissions/{target}"),
                body: Some(serde_json::json!({
                    "id": target.to_string(),
                    "type": kind,
                    "allow": allow.to_string(),
                    "deny": deny.to_string(),
                })),
            });
        }
        let overview_dirty = values != open.saved;
        let flags = self
            .stores
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .find(|known| known.id == channel)
            .map_or(0, |known| known.flags);
        let flags = if values.spoiler {
            flags | IS_SPOILER_CHANNEL
        } else {
            flags & !IS_SPOILER_CHANNEL
        };
        let name = super::create_channel::channel_slug(&values.name);
        if let Some(open) = self.channel_settings.as_mut() {
            open.perm_edits.clear();
        }
        if !overview_dirty {
            cx.notify();
            return;
        }
        let _ = self.requests.send(Request::Rest {
            key: Some(SAVE_KEY.to_owned()),
            method: reqwest::Method::PATCH,
            route: format!("/channels/{channel}"),
            body: Some(serde_json::json!({
                "name": name,
                "topic": values.topic,
                "rate_limit_per_user": values.slowmode,
                "flags": flags,
                "default_auto_archive_duration": values.archive,
            })),
        });
        if let Some(open) = self.channel_settings.as_mut() {
            open.saved = Values { name, ..values };
        }
        cx.notify();
    }

    /// The PATCH answered: the channel as the server keeps it.
    pub(super) fn channel_settings_saved(
        &mut self,
        report: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        if report["ok"] == true {
            self.stores.dispatch("CHANNEL_UPDATE", &report["body"]);
            if let Some(open) = self.channel_settings.as_mut()
                && let Some(name) = report["body"]["name"].as_str()
            {
                let name = name.to_owned();
                open.saved.name = name.clone();
                open.name.update(cx, |editor, _| editor.set_text(&name));
            }
        }
        cx.notify();
    }

    pub(super) fn sidebar_item(
        &self,
        id: &'static str,
        label: &'static str,
        selected: bool,
        danger: bool,
        cx: &mut Context<Self>,
        run: impl Fn(&mut Root, &mut Context<Root>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .h(px(32.))
            .mb(px(2.))
            .px(px(10.))
            .rounded(px(4.))
            .cursor_pointer()
            .flex()
            .items_center()
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(if danger {
                color("--text-feedback-critical")
            } else if selected {
                color("--text-strong")
            } else {
                color("--text-subtle")
            })
            .when(selected, |this| this.bg(color("--background-mod-strong")))
            .when(!selected, |this| {
                this.hover(|style| style.bg(color("--background-mod-subtle")))
            })
            .child(div().flex_1().child(label))
            .when(danger, |this| {
                this.child(icon(
                    "channel_settings_trash",
                    16.,
                    "--text-feedback-critical",
                ))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                run(this, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    pub(super) fn field_label(text: &'static str) -> gpui::Div {
        div()
            .mb(px(8.))
            .h(px(20.))
            .text_size(px(16.))
            .line_height(px(20.))
            .font_weight(FontWeight(500.))
            .text_color(color("--text-strong"))
            .child(text)
    }

    fn helper(text: &'static str) -> gpui::Div {
        div()
            .mt(px(4.))
            .pr(px(12.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(color("--text-subtle"))
            .child(text)
    }

    /// `selectField_vWC4bk`: the chosen label and the chevron; a click opens the list.
    fn select_field(
        &self,
        which: Select,
        label: String,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = match which {
            Select::Slowmode => "channel-settings-slowmode",
            Select::Archive => "channel-settings-archive",
        };
        div()
            .id(key)
            .relative()
            .child(self.anchor_probe(key))
            .h(px(40.))
            .rounded(px(8.))
            .border_1()
            .border_color(if open {
                color("--input-border-active")
            } else {
                color("--input-border-default")
            })
            .bg(color("--input-background-default"))
            .cursor_pointer()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .pl(px(12.))
                    .text_size(px(16.))
                    .text_color(color("--text-default"))
                    .child(label),
            )
            .child(
                div().w(px(36.)).pr(px(4.)).flex().justify_center().child(
                    gpui::svg()
                        .path("icons/combo_chevron.svg")
                        .size(px(20.))
                        .text_color(color("--text-strong"))
                        .when(open, |this| {
                            this.with_transformation(gpui::Transformation::rotate(gpui::radians(
                                std::f32::consts::PI,
                            )))
                        }),
                ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(settings) = this.channel_settings.as_mut() {
                    settings.open_select = if settings.open_select == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn overview(&self, open: &ChannelSettings, cx: &mut Context<Self>) -> AnyElement {
        let topic_length = open.topic.read(cx).text().chars().count();
        let toolbar_button = |glyph: &'static str| {
            div()
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .child(icon(glyph, 20., "--icon-subtle"))
        };
        let emoji = |size: f32| {
            self.images
                .get("emoji_button")
                .cloned()
                .map(move |picture| img(picture).size(px(size)).grayscale(true))
        };
        let name_field = div()
            .flex()
            .flex_col()
            .child(Self::field_label("Channel Name"))
            .child(
                div()
                    .id("channel-settings-name-box")
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(settings) = this.channel_settings.as_ref() {
                            let handle = gpui::Focusable::focus_handle(settings.name.read(cx), cx);
                            window.focus(&handle, cx);
                        }
                    }))
                    .h(px(40.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--input-border-default"))
                    .bg(color("--input-background-default"))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(px(12.))
                            .child(open.name.clone()),
                    )
                    .child(div().w(px(32.)).pr(px(12.)).children(emoji(18.))),
            );
        let topic_field = div()
            .flex()
            .flex_col()
            .child(Self::field_label("Channel Topic"))
            .child(
                // `staticToolbar_hPmUPu` keeps -4px under it, which the control's 4px gap cancels.
                div()
                    .h(px(32.))
                    .rounded_t(px(4.))
                    .bg(gpui::rgba(0x97979f29))
                    .flex()
                    .items_center()
                    .child(toolbar_button("topic_bold"))
                    .child(toolbar_button("topic_italic"))
                    .child(toolbar_button("topic_underline"))
                    .child(toolbar_button("topic_strike"))
                    .child(
                        div()
                            .mx(px(6.))
                            .w(px(1.))
                            .h(px(20.))
                            .bg(color("--border-subtle")),
                    )
                    .child(toolbar_button("topic_spoiler"))
                    .child(div().flex_1())
                    .child(div().pr(px(8.)).children(emoji(18.))),
            )
            .child(
                div()
                    .id("channel-settings-topic-box")
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(settings) = this.channel_settings.as_ref() {
                            let handle = gpui::Focusable::focus_handle(settings.topic.read(cx), cx);
                            window.focus(&handle, cx);
                        }
                    }))
                    .relative()
                    .h(px(202.))
                    .rounded_b(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--input-background-default"))
                    .child(
                        div()
                            .pl(px(15.))
                            .pr(px(12.))
                            .pt(px(17.))
                            .child(open.topic.clone()),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(15.))
                            .bottom(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-default"))
                            .child(1024usize.saturating_sub(topic_length).to_string()),
                    ),
            );
        let slowmode = div()
            .mb(px(8.))
            .flex()
            .flex_col()
            .child(Self::field_label("Slowmode"))
            .child(self.select_field(
                Select::Slowmode,
                slowmode_label(open.slowmode),
                open.open_select == Some(Select::Slowmode),
                cx,
            ))
            .child(Self::helper(
                "Members will be restricted to sending one message and creating one thread per this interval, unless they have the Bypass Slowmode permission.",
            ));
        let radio = |id: &'static str,
                     title: &'static str,
                     detail: &'static str,
                     chosen: bool,
                     spoiler: bool| {
            div()
                .id(id)
                .relative()
                .pl(px(32.))
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .child(super::settings::radio_dot(chosen)),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(4.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .text_color(color("--text-subtle"))
                        .child(detail),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(settings) = this.channel_settings.as_mut() {
                        settings.spoiler = spoiler;
                    }
                    cx.notify();
                }))
        };
        let visibility = div()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(Self::field_label("Content Visibility"))
            .child(
                div()
                    .pt(px(8.))
                    .pb(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(radio(
                        "channel-settings-visibility-default",
                        "Default",
                        "Channel content is always visible.",
                        !open.spoiler,
                        false,
                    ))
                    .child(radio(
                        "channel-settings-visibility-spoiler",
                        "Spoiler Channel",
                        "Mark this channel as containing spoilers, so that plot twists, heavy topics, or other sensitive discussions are hidden until users choose to view.",
                        open.spoiler,
                        true,
                    )),
            );
        let archive_label = ARCHIVE
            .iter()
            .find(|(minutes, _)| *minutes == open.archive)
            .map_or_else(|| "3 Days".to_owned(), |(_, label)| (*label).to_owned());
        let archive = div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .child(Self::field_label("Hide After Inactivity"))
            .child(self.select_field(Select::Archive, archive_label, open.open_select == Some(Select::Archive), cx))
            .child(Self::helper(
                "New threads will not show in the channel list after being inactive for the specified duration.",
            ));
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .h(px(24.))
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child("Overview"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(24.))
                    .child(name_field)
                    .child(topic_field)
                    .child(slowmode)
                    .child(visibility)
                    .child(archive),
            )
            .child(
                div().mt(px(60.)).flex().justify_center().children(
                    self.images
                        .get(OVERVIEW_ART)
                        .cloned()
                        .map(|art| img(art).w(px(280.)).h(px(165.))),
                ),
            )
            .into_any_element()
    }

    /// The open select's list (`selectDropdown_vWC4bk`): 40px rows, the choice checked, above the field when it does
    /// not fit below, 4px off it.
    pub(super) fn channel_settings_select(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.channel_settings.as_ref()?;
        let which = open.open_select?;
        let key = match which {
            Select::Slowmode => "channel-settings-slowmode",
            Select::Archive => "channel-settings-archive",
        };
        let field = self.tip_anchors.borrow().get(key).copied()?;
        let options: Vec<(u64, String)> = match which {
            Select::Slowmode => SLOWMODE
                .iter()
                .map(|seconds| (*seconds, slowmode_label(*seconds)))
                .collect(),
            Select::Archive => ARCHIVE
                .iter()
                .map(|(minutes, label)| (*minutes, (*label).to_owned()))
                .collect(),
        };
        let current = match which {
            Select::Slowmode => open.slowmode,
            Select::Archive => open.archive,
        };
        let height = (options.len() as f32 * 40. + 8.).min(208.) + 2.;
        let below =
            f32::from(field.bottom()) + 4. + height <= f32::from(window.viewport_size().height);
        let top = if below {
            f32::from(field.bottom()) + 4.
        } else {
            f32::from(field.top()) - 4. - height
        };
        let rows = options.into_iter().map(|(value, label)| {
            let chosen = value == current;
            div()
                .id(SharedString::from(format!("{key}-{value}")))
                .flex_none()
                .h(px(40.))
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|style| style.bg(color("--interactive-background-hover")))
                .flex()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .pl(px(8.))
                        .text_size(px(16.))
                        .text_color(color("--text-strong"))
                        .child(label),
                )
                .when(chosen, |this| {
                    this.child(
                        div()
                            .px(px(4.))
                            .child(icon("select_check", 20., "--text-strong")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(settings) = this.channel_settings.as_mut() {
                        match which {
                            Select::Slowmode => settings.slowmode = value,
                            Select::Archive => settings.archive = value,
                        }
                        settings.open_select = None;
                    }
                    cx.notify();
                }))
        });
        Some(
            deferred(
                div()
                    .id("channel-settings-select")
                    .occlude()
                    .absolute()
                    .left(field.left())
                    .top(px(top))
                    .w(field.size.width)
                    .h(px(height))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-surface-higher"))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0., 0., 0., 0.16),
                        offset: gpui::point(px(0.), px(2.)),
                        blur_radius: px(8.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        if let Some(settings) = this.channel_settings.as_mut() {
                            settings.open_select = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        // `auto_tqapBM`: a 16px gutter beside the rows for the scrollbar.
                        div()
                            .id("channel-settings-select-list")
                            .size_full()
                            .p(px(4.))
                            .pr(px(20.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .children(rows),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    pub(super) fn channel_settings_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.channel_settings.as_ref()?;
        let channel_id = open.channel;
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| guild.channels.iter().any(|known| known.id == channel_id))?;
        let channel = guild
            .channels
            .iter()
            .find(|known| known.id == channel_id)?
            .clone();
        let category = channel
            .parent_id
            .and_then(|parent| guild.channels.iter().find(|known| known.id == parent))
            .map(|parent| parent.name.clone());
        let dirty = open.values(cx) != open.saved || !open.perm_edits.is_empty();
        let section = open.section;
        let header = div()
            .h(px(24.))
            .px(px(10.))
            .pb(px(6.))
            .flex()
            .items_center()
            .gap(px(2.))
            .child(icon(
                if channel.kind == ChannelKind::Voice {
                    "voice_channel"
                } else {
                    "text_channel"
                },
                12.,
                "--channels-default",
            ))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--channels-default"))
                    .child(open.name.read(cx).text().to_owned()),
            )
            .children(category.map(|category| {
                div()
                    .ml(px(4.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-default"))
                    .child(category)
            }));
        let go = |section: Section| {
            move |this: &mut Root, _: &mut Context<Root>| {
                if let Some(settings) = this.channel_settings.as_mut() {
                    settings.section = section;
                    settings.open_select = None;
                    let channel = settings.channel;
                    match section {
                        Section::Invites => this.request_channel_settings_page(channel, true),
                        Section::Integrations => this.request_channel_settings_page(channel, false),
                        Section::Overview | Section::Permissions => {}
                    }
                }
            }
        };
        let sidebar = div()
            .w(px(372.))
            .h_full()
            .flex_none()
            .pr(px(8.))
            .flex()
            .justify_end()
            .child(
                div()
                    .w(px(264.))
                    .pt(px(60.))
                    .pl(px(20.))
                    .pr(px(6.))
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(self.sidebar_item(
                        "channel-settings-overview",
                        "Overview",
                        section == Section::Overview,
                        false,
                        cx,
                        go(Section::Overview),
                    ))
                    .child(self.sidebar_item(
                        "channel-settings-permissions",
                        "Permissions",
                        section == Section::Permissions,
                        false,
                        cx,
                        go(Section::Permissions),
                    ))
                    .child(self.sidebar_item(
                        "channel-settings-invites",
                        "Invites",
                        section == Section::Invites,
                        false,
                        cx,
                        go(Section::Invites),
                    ))
                    .child(self.sidebar_item(
                        "channel-settings-integrations",
                        "Integrations",
                        section == Section::Integrations,
                        false,
                        cx,
                        go(Section::Integrations),
                    ))
                    .child(
                        div()
                            .mx(px(10.))
                            .my(px(8.))
                            .h(px(1.))
                            .bg(color("--border-subtle")),
                    )
                    .child(self.sidebar_item(
                        "channel-settings-delete",
                        "Delete Channel",
                        false,
                        true,
                        cx,
                        |this, _| {
                            if let Some(settings) = this.channel_settings.as_mut() {
                                settings.confirm_delete = true;
                            }
                        },
                    )),
            );
        let page: AnyElement = match section {
            Section::Overview => self.overview(open, cx),
            Section::Permissions => self.channel_permissions_page(&channel, cx),
            Section::Invites => self.channel_invites_page(channel_id, cx),
            Section::Integrations => self.channel_integrations_page(channel_id, cx),
        };
        let close = div()
            .pt(px(60.))
            .w(px(36.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .id("channel-settings-close")
                    .size(px(36.))
                    .rounded_full()
                    .border_2()
                    .border_color(color("--icon-subtle"))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("settings_esc_close", 18., "--icon-subtle"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.channel_settings = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .mt(px(6.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--icon-subtle"))
                    .child("ESC"),
            );
        let notice = dirty.then(|| {
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .w(px(740.))
                .px(px(20.))
                .pb(px(20.))
                .child(
                    div()
                        .h(px(54.))
                        .pl(px(16.))
                        .pr(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-highest"))
                        .shadow(vec![gpui::BoxShadow {
                            color: gpui::hsla(0., 0., 0., 0.2),
                            offset: gpui::point(px(0.), px(2.)),
                            blur_radius: px(10.),
                            spread_radius: px(0.),
                            inset: false,
                        }])
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .child("Careful \u{2014} you have unsaved changes!"),
                        )
                        .child(
                            div()
                                .id("channel-settings-reset")
                                .w(px(60.))
                                .h(px(32.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-brand"))
                                .hover(|style| style.underline())
                                .child("Reset")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.reset_channel_settings(cx)),
                                ),
                        )
                        .child(
                            div()
                                .id("channel-settings-save")
                                .ml(px(10.))
                                .h(px(32.))
                                .px(px(11.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(gpui::rgba(0xffffff14))
                                .bg(color("--control-connected-background-default"))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(gpui::white())
                                .child("Save Changes")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_channel_settings(cx)),
                                ),
                        ),
                )
        });
        let content = div()
            .relative()
            .flex_1()
            .h_full()
            .bg(color("--background-base-low"))
            .child(
                div()
                    .id("channel-settings-scroller")
                    .mt(px(32.))
                    .h_full()
                    .overflow_y_scroll()
                    .flex()
                    // Stretched, the page would be exactly as tall as the scroller and never scroll.
                    .items_start()
                    .child(
                        div()
                            .w(px(740.))
                            .pt(px(60.))
                            .px(px(40.))
                            .pb(px(80.))
                            .child(page),
                    ),
            )
            // ESC holds its place while the page scrolls.
            .child(div().absolute().left(px(740.)).top(px(32.)).child(close))
            .children(notice);
        Some(
            deferred(
                div()
                    .parity("channel-settings")
                    .id("channel-settings")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(color("--background-base-lowest"))
                    .flex()
                    .child(sidebar)
                    .child(content)
                    .child(div().absolute().top_0().right_0().child(window_controls())),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// "Delete Channel" (`hCHzAr`): `DELETE /channels/{id}`, then the server's first channel.
    pub(super) fn delete_channel_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.channel_settings.as_ref()?;
        if !open.confirm_delete {
            return None;
        }
        let channel = open.channel;
        let name = open.saved.name.clone();
        Some(super::modal::render_small(
            super::modal::Spec {
                id: "delete-channel-modal",
                title: "Delete Channel".into(),
                subtitle: Some(
                    div()
                        .flex()
                        .flex_wrap()
                        .child("Are you sure you want to delete\u{a0}")
                        .child(
                            div()
                                .font_weight(FontWeight(600.))
                                .child(format!("#{name}")),
                        )
                        .child("? This cannot be undone.")
                        .into_any_element(),
                ),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "delete-channel-cancel",
                        "Cancel",
                        super::modal::Tone::Secondary,
                        |this, _, _| {
                            if let Some(settings) = this.channel_settings.as_mut() {
                                settings.confirm_delete = false;
                            }
                        },
                    ),
                    super::modal::button(
                        "delete-channel-confirm",
                        "Delete Channel",
                        super::modal::Tone::Critical,
                        move |this, _, _| {
                            let _ = this.requests.send(Request::Rest {
                                key: None,
                                method: reqwest::Method::DELETE,
                                route: format!("/channels/{channel}"),
                                body: None,
                            });
                            this.channel_settings = None;
                            if let Some(guild) = this.guild {
                                this.select(guild, None);
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| {
                    if let Some(settings) = this.channel_settings.as_mut() {
                        settings.confirm_delete = false;
                    }
                }),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::slowmode_label;

    #[test]
    fn slowmode_labels_read_like_the_list() {
        assert_eq!(slowmode_label(0), "Off");
        assert_eq!(slowmode_label(5), "5 seconds");
        assert_eq!(slowmode_label(60), "1 minute");
        assert_eq!(slowmode_label(120), "2 minutes");
        assert_eq!(slowmode_label(3600), "1 hour");
        assert_eq!(slowmode_label(21600), "6 hours");
    }
}
