//! Forward (module 92961): pick up to five destinations from recent and frecent places (module 223863: this
//! session's channel history, then the guild-and-channel frecency, the origin left out, at most 15), optionally add
//! a note, and send. Each destination gets `POST /channels/{id}/messages` with an empty body and a FORWARD
//! `message_reference`, then the note as its own message; with one destination the client goes there. Measured in
//! `design/dom/modals/forward*-darker-cozy.json`; the "Message forwarded!" toast in `toasts/forwarded`.

use super::actions::FRECENCY_KEY;
use super::{Root, avatar_source, color, editor_style, status_dot};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use crate::parity::Parity;
use dc_state::frecency::{Frecency, Kind, Usage};
use dc_state::{ChannelKind, Id, Message};
use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, IntoElement, SharedString, Subscription,
    Window, deferred, div, img, prelude::*, px, rgba, svg,
};

const MAX_DESTINATIONS: usize = 5;
const MAX_RESULTS: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    /// A guild channel, by id.
    Channel(Id),
    /// A DM, by its channel id.
    Dm(Id),
}

impl Destination {
    fn channel(self) -> Id {
        match self {
            Destination::Channel(id) | Destination::Dm(id) => id,
        }
    }
}

pub(crate) struct Forward {
    /// The forwarded message; none when a profile is shared.
    pub message: Option<Message>,
    /// Share Profile (module 627664): the same modal, sending the profile link instead.
    pub(super) profile: Option<dc_state::User>,
    pub(super) search: Entity<Editor>,
    pub(super) note: Entity<Editor>,
    _events: [Subscription; 2],
    query: String,
    selected: Vec<Destination>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

impl Forward {
    pub fn note(&self) -> Entity<Editor> {
        self.note.clone()
    }
}

impl Root {
    pub(super) fn open_forward(
        &mut self,
        message: Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_forward_with(Some(message), None, window, cx);
    }

    /// Share Profile: the forward modal titled "Share Profile" with the user as preview.
    pub(super) fn open_share_profile(
        &mut self,
        user: dc_state::User,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_forward_with(None, Some(user), window, cx);
    }

    fn open_forward_with(
        &mut self,
        message: Option<Message>,
        profile: Option<dc_state::User>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        let note = cx.new(|cx| {
            let mut editor = Editor::new(editor_style(), cx);
            editor.set_placeholder("Add an optional message...".to_owned());
            editor
        });
        let search_events = cx.subscribe(&search, |this, editor, event: &EditorEvent, cx| {
            if let Some(forward) = this.forward.as_mut() {
                match event {
                    EditorEvent::Edited => forward.query = editor.read(cx).text().to_owned(),
                    EditorEvent::Cancel => this.forward = None,
                    EditorEvent::Submit(_) => {}
                }
            }
            cx.notify();
        });
        let note_events = cx.subscribe(&note, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Submit(text) => {
                    let text = text.clone();
                    this.send_forward(Some(text), cx);
                }
                EditorEvent::Cancel => this.forward = None,
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        window.focus(&search.read(cx).focus_handle(cx), cx);
        self.forward = Some(Forward {
            message,
            profile,
            search,
            note,
            _events: [search_events, note_events],
            query: String::new(),
            selected: Vec::new(),
        });
        cx.notify();
    }

    /// Where the open channel came from and where the user went, newest first (`getChannelHistory`).
    pub(super) fn remember_channel(&mut self, channel: Id) {
        self.channel_history.retain(|known| *known != channel);
        self.channel_history.insert(0, channel);
        self.channel_history.truncate(50);
    }

    fn destination_of(&self, channel: Id) -> Option<Destination> {
        if self
            .stores
            .private_channels
            .iter()
            .any(|known| known.id == channel)
        {
            return Some(Destination::Dm(channel));
        }
        self.stores
            .guilds
            .iter()
            .flat_map(|guild| &guild.channels)
            .find(|known| known.id == channel)
            .filter(|known| matches!(known.kind, ChannelKind::Text | ChannelKind::Voice))
            .map(|_| Destination::Channel(channel))
    }

    /// The rows: selected first while not searching, then history and frecency; or the search matches.
    fn forward_results(&self, forward: &Forward) -> Vec<Destination> {
        let origin = forward
            .message
            .as_ref()
            .map_or(0, |message| message.channel_id);
        let query = forward.query.trim().to_lowercase();
        let mut out: Vec<Destination> = Vec::new();
        let push = |destination: Destination, out: &mut Vec<Destination>| {
            if destination.channel() != origin && !out.contains(&destination) {
                out.push(destination);
            }
        };
        if !query.is_empty() {
            for dm in &self.stores.private_channels {
                let matches = self.stores.dm_recipient(dm).is_some_and(|user| {
                    user.display_name().to_lowercase().contains(&query)
                        || user.username.to_lowercase().contains(&query)
                });
                if matches {
                    push(Destination::Dm(dm.id), &mut out);
                }
            }
            for guild in &self.stores.guilds {
                for channel in &guild.channels {
                    if matches!(channel.kind, ChannelKind::Text | ChannelKind::Voice)
                        && channel.name.to_lowercase().contains(&query)
                    {
                        push(Destination::Channel(channel.id), &mut out);
                    }
                }
            }
            return out;
        }
        for selected in &forward.selected {
            push(*selected, &mut out);
        }
        for channel in &self.channel_history {
            if let Some(destination) = self.destination_of(*channel) {
                push(destination, &mut out);
            }
        }
        let mut frecency = Frecency::new(Kind::Channel);
        let stored = self
            .fetched
            .get(FRECENCY_KEY)
            .and_then(|body| body["settings"].as_str())
            .map(|settings| {
                dc_proto::frecency::id_map(settings, dc_proto::frecency::GUILD_AND_CHANNEL_FRECENCY)
            })
            .unwrap_or_default();
        frecency.overwrite(
            stored
                .into_iter()
                .map(|(key, item)| {
                    (
                        key.to_string(),
                        Usage {
                            total_uses: item.total_uses,
                            recent_uses: item.recent_uses,
                        },
                    )
                })
                .collect(),
        );
        for key in frecency.frequently(now_ms(), |key| {
            key.parse::<Id>()
                .ok()
                .and_then(|id| self.destination_of(id))
                .is_some()
        }) {
            if let Some(destination) = key
                .parse::<Id>()
                .ok()
                .and_then(|id| self.destination_of(id))
            {
                push(destination, &mut out);
            }
        }
        out.truncate(MAX_RESULTS);
        out
    }

    fn toggle_destination(&mut self, destination: Destination) {
        let Some(forward) = self.forward.as_mut() else {
            return;
        };
        if let Some(index) = forward
            .selected
            .iter()
            .position(|known| *known == destination)
        {
            forward.selected.remove(index);
        } else if forward.selected.len() < MAX_DESTINATIONS {
            forward.selected.insert(0, destination);
        }
    }

    pub(super) fn send_forward(&mut self, note: Option<String>, cx: &mut Context<Self>) {
        let Some(forward) = self.forward.take() else {
            return;
        };
        if forward.selected.is_empty() {
            self.forward = Some(forward);
            return;
        }
        let note = note.unwrap_or_else(|| forward.note.read(cx).text().trim().to_owned());
        if let Some(user) = forward.profile.as_ref() {
            // The link, then the note after a blank line, as one message per destination.
            let link = profile_link(user.id);
            let content = if note.is_empty() {
                link
            } else {
                format!("{link}\n\n{note}")
            };
            for destination in &forward.selected {
                let _ = self.requests.send(Request::Send {
                    channel: destination.channel(),
                    content: content.clone(),
                    files: Vec::new(),
                    reply: None,
                });
            }
        }
        let Some(source) = forward.message.as_ref() else {
            self.go_to_destination(&forward.selected);
            self.show_success_toast("Message forwarded!".to_owned(), cx);
            return;
        };
        let guild = self
            .stores
            .guilds
            .iter()
            .find(|guild| {
                guild
                    .channels
                    .iter()
                    .any(|channel| channel.id == source.channel_id)
            })
            .map(|guild| guild.id);
        for destination in &forward.selected {
            let _ = self.requests.send(Request::Forward {
                destination: destination.channel(),
                guild,
                channel: source.channel_id,
                message: source.id,
                note: (!note.is_empty()).then(|| note.clone()),
            });
        }
        self.go_to_destination(&forward.selected);
        self.show_app_toast("Message forwarded!", cx);
    }

    /// `transitionToDestination` with a single destination.
    fn go_to_destination(&mut self, selected: &[Destination]) {
        if let [only] = selected {
            match only {
                Destination::Dm(channel) => self.select_home(Some(*channel)),
                Destination::Channel(channel) => {
                    let guild = self
                        .stores
                        .guilds
                        .iter()
                        .find(|guild| guild.channels.iter().any(|known| known.id == *channel))
                        .map(|guild| guild.id);
                    if let Some(guild) = guild {
                        self.select(guild, Some(*channel));
                    }
                }
            }
        }
    }

    fn destination_row(
        &self,
        destination: Destination,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .forward
            .as_ref()
            .is_some_and(|forward| forward.selected.contains(&destination));
        let (icon, name, detail) = match destination {
            Destination::Dm(channel) => {
                let dm = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel);
                let user = dm.and_then(|dm| self.stores.dm_recipient(dm)).cloned();
                let picture = user
                    .as_ref()
                    .and_then(|user| self.images.get(&avatar_source(user).0).cloned());
                let presence = user
                    .as_ref()
                    .and_then(|user| self.stores.presences.get(&user.id).cloned())
                    .unwrap_or_else(|| "offline".to_owned());
                let icon = div()
                    .relative()
                    .size(px(32.))
                    .children(picture.map(|picture| img(picture).size(px(32.)).rounded_full()))
                    .child(status_dot(&presence, "--background-surface-high"))
                    .into_any_element();
                (
                    icon,
                    user.as_ref()
                        .map(|user| user.display_name().to_owned())
                        .unwrap_or_default(),
                    user.map(|user| user.username).unwrap_or_default(),
                )
            }
            Destination::Channel(channel) => {
                let guild = self
                    .stores
                    .guilds
                    .iter()
                    .find(|guild| guild.channels.iter().any(|known| known.id == channel));
                let known =
                    guild.and_then(|guild| guild.channels.iter().find(|known| known.id == channel));
                let voice = known.is_some_and(|known| known.kind == ChannelKind::Voice);
                let guild_picture = guild
                    .and_then(|guild| self.images.get(&format!("guild:{}", guild.id)).cloned());
                let acronym = guild.map(dc_state::Guild::acronym).unwrap_or_default();
                // The guild icon at 24px with the channel's type badged at its lower right.
                let icon = div()
                    .relative()
                    .size(px(32.))
                    .child(
                        div()
                            .size(px(24.))
                            .rounded_full()
                            .overflow_hidden()
                            .bg(color("--background-base-lower"))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(8.))
                            .text_color(color("--text-default"))
                            .when_some(guild_picture, |this, picture| {
                                this.child(img(picture).size(px(24.)))
                            })
                            .child(acronym),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(12.))
                            .top(px(12.))
                            .size(px(20.))
                            .p(px(4.))
                            .rounded_full()
                            .bg(color("--background-mod-strong"))
                            .child(
                                svg()
                                    .path(if voice {
                                        "icons/voice_channel.svg"
                                    } else {
                                        "icons/text_channel.svg"
                                    })
                                    .size(px(12.))
                                    .text_color(color("--interactive-text-default")),
                            ),
                    )
                    .into_any_element();
                (
                    icon,
                    known.map(|known| known.name.clone()).unwrap_or_default(),
                    guild.map(|guild| guild.name.clone()).unwrap_or_default(),
                )
            }
        };
        div()
            .id(SharedString::from(format!("forward-row-{index}")))
            .h(px(48.))
            .ml(px(-8.))
            .px(px(8.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|style| style.bg(color("--interactive-background-hover")))
            .flex()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_destination(destination);
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(icon)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(color("--text-muted"))
                                    .child(detail),
                            ),
                    ),
            )
            .child(
                div()
                    .ml(px(16.))
                    .size(px(20.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if selected {
                        color("--background-mod-subtle")
                    } else {
                        color("--checkbox-border-default")
                    })
                    .bg(if selected {
                        color("--control-primary-background-default")
                    } else {
                        color("--checkbox-background-default")
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |this| {
                        this.child(
                            svg()
                                .path("icons/theme_check.svg")
                                .size(px(18.))
                                .text_color(color("--white")),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn forward_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let forward = self.forward.as_ref()?;
        let results = self.forward_results(forward);
        let count = forward.selected.len();
        let full = count >= MAX_DESTINATIONS;
        let search_focused = forward.search.read(cx).focus_handle(cx).is_focused(window);
        let height = (f32::from(window.viewport_size().height) - 112.).max(300.);
        let rows: Vec<AnyElement> = if results.is_empty() {
            vec![
                div()
                    .h(px(72.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .text_color(color("--text-muted"))
                    .child("We searched far and wide. Unfortunately, no results were found.")
                    .into_any_element(),
            ]
        } else {
            results
                .iter()
                .enumerate()
                .map(|(index, destination)| self.destination_row(*destination, index, cx))
                .collect()
        };
        let preview = match (&forward.message, &forward.profile) {
            (Some(message), _) => self
                .preview_text(&super::first_spans(&message.content))
                .into_any_element(),
            (None, Some(user)) => self.profile_preview(user),
            (None, None) => div().into_any_element(),
        };
        let sharing = forward.profile.as_ref().map(|user| user.id);
        let send_label = if count <= 1 {
            "Send".to_owned()
        } else {
            format!("Send ({count})")
        };
        let modal = div()
            .parity_with_border("forward-modal", 1.)
            .id("forward-modal")
            .occlude()
            .w(px(480.))
            .h(px(height))
            .pt(px(8.))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .pt(px(16.))
                    .px(px(24.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(24.))
                            .flex()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(20.))
                                    .line_height(px(24.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color("--text-strong"))
                                    .child(if sharing.is_some() {
                                        "Share Profile"
                                    } else {
                                        "Forward To"
                                    }),
                            )
                            .child(
                                div()
                                    .id("forward-close")
                                    .mt(px(-16.))
                                    .mr(px(-16.))
                                    .size(px(40.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(super::icon(
                                        "modal_close",
                                        20.,
                                        "--control-icon-only-icon-default",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.forward = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-muted"))
                            .child(if full {
                                format!("Maximum {MAX_DESTINATIONS} places at once.")
                            } else {
                                "Select where you want to share this message.".to_owned()
                            }),
                    ),
            )
            .child(
                div().pt(px(16.)).px(px(24.)).child(
                    div()
                        .h(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(if search_focused {
                            color("--input-border-active")
                        } else {
                            color("--input-border-default")
                        })
                        .bg(color("--input-background-default"))
                        .pl(px(4.))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            div()
                                .size(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    svg()
                                        .path("icons/picker_search.svg")
                                        .size(px(16.))
                                        .text_color(color("--input-text-default")),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .pr(px(12.))
                                .child(forward.search.clone()),
                        ),
                ),
            )
            .child(
                div()
                    .id("forward-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(px(12.))
                    .pb(px(12.))
                    .pl(px(24.))
                    .pr(px(8.))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .child(
                div()
                    .h(px(8.))
                    .border_b_1()
                    .border_color(color("--border-subtle")),
            )
            .child(
                div().pt(px(16.)).px(px(24.)).child(
                    div()
                        .relative()
                        .pl(px(16.))
                        .when(sharing.is_none(), |this| this.py(px(4.)))
                        .child(
                            div()
                                .absolute()
                                .left(px(0.))
                                .top(px(0.))
                                .bottom(px(0.))
                                .w(px(4.))
                                .rounded(px(2.))
                                .bg(color("--border-subtle")),
                        )
                        .child(if sharing.is_some() {
                            div().child(preview)
                        } else {
                            div().h(px(22.)).pt(px(2.)).child(preview)
                        }),
                ),
            )
            .child(
                div()
                    .pt(px(16.))
                    .pb(px(8.))
                    .px(px(24.))
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--border-normal"))
                            .bg(color("--input-background-default"))
                            .pl(px(16.))
                            .pr(px(12.))
                            .flex()
                            .items_center()
                            .child(div().flex_1().min_w(px(0.)).child(forward.note.clone()))
                            .child(
                                div()
                                    .id("forward-emoji")
                                    .size(px(32.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .children(
                                        self.images
                                            .get("emoji_button")
                                            .cloned()
                                            .map(|picture| img(picture).size(px(18.))),
                                    )
                                    .on_click(cx.listener(
                                        |this, event: &gpui::ClickEvent, window, cx| {
                                            let at = event.position();
                                            this.open_picker(
                                                super::picker::Target::ForwardNote,
                                                gpui::point(at.x, at.y - px(24.)),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                            ),
                    )
                    .when_some(sharing, |this, user| {
                        // `additionalActions`: Copy Link, a secondary icon button.
                        this.child(
                            div()
                                .id("share-copy-link")
                                .size(px(40.))
                                .rounded(px(8.))
                                .bg(color("--control-secondary-background-default"))
                                .hover(|style| {
                                    style.bg(color("--control-secondary-background-hover"))
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    svg()
                                        .path("icons/share_link.svg")
                                        .size(px(20.))
                                        .text_color(color("--control-secondary-text-default")),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        profile_link(user),
                                    ));
                                    this.show_success_toast("Link Copied!".to_owned(), cx);
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("forward-send")
                            .w(px(100.))
                            .h(px(40.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color("--control-primary-border-default"))
                            .bg(color("--control-primary-background-default"))
                            .when(count == 0, |this| this.opacity(0.5))
                            .when(count > 0, |this| this.cursor_pointer())
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--white"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.send_forward(None, cx);
                                cx.notify();
                            }))
                            .child(send_label)
                            .child(
                                svg()
                                    .path("icons/forward_send.svg")
                                    .size(px(20.))
                                    .text_color(color("--white")),
                            ),
                    ),
            );
        Some(
            deferred(
                div()
                    .id("forward-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.forward = None;
                        cx.notify();
                    }))
                    .child(modal),
            )
            // Above the media viewer (priority 2): Forward stacks over it, as the official does.
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// `preview_0XXhWj`: the shared user, avatar 40 with status, display name and tag.
    fn profile_preview(&self, user: &dc_state::User) -> AnyElement {
        let picture = self.images.get(&avatar_source(user).0).cloned();
        let presence = self
            .stores
            .presences
            .get(&user.id)
            .cloned()
            .unwrap_or_else(|| "offline".to_owned());
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .relative()
                    .size(px(40.))
                    .rounded_full()
                    .children(picture.map(|picture| img(picture).size(px(40.)).rounded_full()))
                    .child(super::status_dot_40(&presence, "--background-surface-high")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(user.display_name().to_owned()),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .into_any_element()
    }
}

/// Module 640794: `${location.protocol}//${location.host}/users/{id}` on the desktop client's host.
pub(super) fn profile_link(user: Id) -> String {
    format!("https://discord.com/users/{user}")
}
