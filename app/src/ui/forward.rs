//! Forward (module 92961): pick up to five destinations from recent and frecent places (module 223863: this
//! session's channel history, then the guild-and-channel frecency, the origin left out, at most 15), optionally add
//! a note, and send. Each destination gets `POST /channels/{id}/messages` with an empty body and a FORWARD
//! `message_reference`, then the note as its own message; with one destination the client goes there. Measured in
//! `design/dom/modals/forward*-darker-cozy.json`; the "Message forwarded!" toast in `toasts/forwarded`.

use super::actions::FRECENCY_KEY;
use super::{Root, avatar_source, color, editor_style, icon, status_dot};
use crate::editor::{Editor, EditorEvent};
use crate::parity::Parity;
use crate::ui::motion::Posed as _;
use crate::ui::motion::Transitions as _;
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

/// Activity / Frame share payload for `SHARE_LINK` & `SHARE_CONTENT` (module 529999 / 342384).
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct ShareActivity {
    pub application_id: String,
    pub application_name: String,
    pub preview_title: Option<String>,
    pub subtitle: Option<String>,
    pub share_url: String,
    pub content: String,
    pub image_url: Option<String>,
    /// Attachment counts `(images, videos, other_files)` for MIME summary copy (`Lr0Top`/`SJ6pPX`/`h4pFfU`/`89ihS8`).
    pub media_counts: (usize, usize, usize),
    /// Extra thumbnails count beyond the first preview tile (`+{N}` overflow badge).
    pub extra_media: usize,
    /// Pending Embedded App SDK `(cmd, nonce)` to reply `{ success, didCopyLink, didSendMessage }`.
    pub sdk_cmd: Option<(String, String)>,
    pub did_copy_link: bool,
}

/// Builds the official Activity share URL (`342384.W`: `https://discord.com/activities/{app}?...`).
pub(super) fn build_activity_share_url(
    application_id: &str,
    custom_id: Option<&str>,
    link_id: Option<&str>,
    referrer_id: Option<&str>,
) -> String {
    let mut url = format!("https://discord.com/activities/{application_id}");
    let mut params = Vec::new();
    if let Some(cid) = custom_id.filter(|s| !s.is_empty()) {
        params.push(format!("custom_id={cid}"));
    }
    if let Some(lid) = link_id.filter(|s| !s.is_empty()) {
        params.push(format!("link_id={lid}"));
    }
    if let Some(rid) = referrer_id.filter(|s| !s.is_empty()) {
        params.push(format!("referrer_id={rid}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

/// Formats MIME attachment count summary for `SHARE_CONTENT` (module 529999: `Lr0Top`, `SJ6pPX`, `h4pFfU`, `89ihS8`).
pub(super) fn activity_share_media_summary(
    images: usize,
    videos: usize,
    files: usize,
) -> Option<String> {
    let total = images + videos + files;
    if total == 0 {
        return None;
    }
    let mut parts = Vec::new();
    if images > 0 {
        parts.push(dc_intl::format(
            "Lr0Top",
            &[("count", (images as f64).into())],
        ));
    }
    if videos > 0 {
        parts.push(dc_intl::format(
            "SJ6pPX",
            &[("count", (videos as f64).into())],
        ));
    }
    if files > 0 {
        parts.push(dc_intl::format(
            "h4pFfU",
            &[("count", (files as f64).into())],
        ));
    }
    if parts.len() > 1 {
        Some(dc_intl::format(
            "89ihS8",
            &[("count", (total as f64).into())],
        ))
    } else {
        parts.into_iter().next()
    }
}

pub(crate) struct Forward {
    /// The forwarded message; none when a profile is shared.
    pub message: Option<Message>,
    /// Share Profile (module 627664): the same modal, sending the profile link instead.
    pub(super) profile: Option<dc_state::User>,
    /// Share Clip (module 243258): the same modal, sending the clip's trimmed video.
    pub(super) clip: Option<super::clips::ShareClip>,
    /// Share Activity / Frame (`SHARE_LINK` & `SHARE_CONTENT`, module 529999).
    pub(super) activity: Option<ShareActivity>,
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
        let (key, url) = avatar_source(&user);
        self.request_image(key, url);
        self.open_forward_with(None, Some(user), window, cx);
    }

    /// Share Clip: the forward modal titled "Share Clip" with the clip as preview.
    pub(super) fn open_share_clip(
        &mut self,
        clip: super::clips::ShareClip,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_forward_with(None, None, window, cx);
        if let Some(forward) = self.forward.as_mut() {
            forward.clip = Some(clip);
        }
    }

    /// Share Activity / Frame (`SHARE_LINK` & `SHARE_CONTENT`, module 529999).
    pub(super) fn open_share_activity(
        &mut self,
        share: ShareActivity,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if let Some(url) = share.image_url.as_ref().filter(|u| !u.is_empty()) {
            self.request_image(
                format!("activity-share:{}", share.application_id),
                url.clone(),
            );
        }
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
                    EditorEvent::Cancel => this.close_forward(cx),
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
                EditorEvent::Cancel => this.close_forward(cx),
                EditorEvent::Edited => {}
            }
            cx.notify();
        });
        if let Some(window) = window {
            window.focus(&search.read(cx).focus_handle(cx), cx);
        }
        self.forward = Some(Forward {
            message: None,
            profile: None,
            clip: None,
            activity: Some(share),
            search,
            note,
            _events: [search_events, note_events],
            query: String::new(),
            selected: Vec::new(),
        });
        cx.notify();
    }

    /// Closes the forward modal, resolving any pending Activity SDK share promise as cancelled.
    pub(super) fn close_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(forward) = self.forward.take()
            && let Some(act) = forward.activity
            && let Some((cmd, nonce)) = act.sdk_cmd
        {
            self.activity_reply(
                &cmd,
                &nonce,
                Ok(serde_json::json!({
                    "success": act.did_copy_link,
                    "didCopyLink": act.did_copy_link,
                    "didSendMessage": false,
                })),
                cx,
            );
        }
        cx.notify();
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
        // Not in the pinned client: BetterForwards starts each dialog forwarding everything
        // (`plugin_hooks/better_forwards.rs`).
        self.better_forwards_opened(message.as_ref());
        self.forward = Some(Forward {
            message,
            profile,
            clip: None,
            activity: None,
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
        // Not in the pinned client: BetterForwards' `selfForward` keeps the current channel in the list.
        let origin = if super::plugin_hooks::better_forwards_self_forward() {
            0
        } else {
            forward
                .message
                .as_ref()
                .map_or(0, |message| message.channel_id)
        };
        let query = forward.query.trim().to_lowercase();
        let mut out: Vec<Destination> = Vec::new();
        let push = |destination: Destination, out: &mut Vec<Destination>| {
            if destination.channel() != origin && !out.contains(&destination) {
                out.push(destination);
            }
        };
        if !query.is_empty() {
            for dm in self.stores.pickable_private_channels() {
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
        if forward.clip.is_some() {
            // `customSendHandler` of module 243258: the video to each destination, then the note.
            let channels: Vec<Id> = forward.selected.iter().map(|d| d.channel()).collect();
            self.go_to_destination(&forward.selected);
            if let Some(share) = forward.clip {
                self.clips_send_share(share, channels, note, cx);
            }
            return;
        }
        if let Some(act) = forward.activity {
            // Module 529999 send handler: posts [imageUrl, content, note] to each destination,
            // toasts `kwmYkt` / `iufib1`, and resolves `{ success, didCopyLink, didSendMessage }`.
            let mut lines: Vec<String> = Vec::new();
            if let Some(img_url) = act.image_url.as_ref().filter(|s| !s.is_empty()) {
                lines.push(img_url.clone());
            }
            if !act.content.trim().is_empty() {
                lines.push(act.content.clone());
            } else if !act.share_url.is_empty() {
                lines.push(act.share_url.clone());
            }
            if !note.is_empty() {
                lines.push(note);
            }
            let body = lines.join("\n");
            for destination in &forward.selected {
                self.start_local_text(destination.channel(), body.clone());
            }
            if let Some((cmd, nonce)) = act.sdk_cmd {
                self.activity_reply(
                    &cmd,
                    &nonce,
                    Ok(serde_json::json!({
                        "success": true,
                        "didCopyLink": act.did_copy_link,
                        "didSendMessage": true,
                    })),
                    cx,
                );
            }
            self.go_to_destination(&forward.selected);
            let toast = if forward.selected.len() > 1 {
                dc_intl::t("iufib1")
            } else {
                dc_intl::t("kwmYkt")
            };
            self.show_success_toast(toast.to_owned(), cx);
            return;
        }
        if let Some(user) = forward.profile.as_ref() {
            // The link, then the note after a blank line, as one message per destination.
            let link = profile_link(user.id);
            let content = if note.is_empty() {
                link
            } else {
                format!("{link}\n\n{note}")
            };
            for destination in &forward.selected {
                self.start_local_text(destination.channel(), content.clone());
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
        let mut pending = std::collections::HashMap::new();
        // A note that starts with `@silent` sends the forward and the note without notifications (module 92961).
        let silent = dc_state::split_silent_prefix(&note).0;
        for destination in &forward.selected {
            // The forward first, then the note as a message of its own: the queue keeps that order, and a failed
            // forward fails the note that waits behind it.
            let nonce = self.start_local_forward(
                destination.channel(),
                (guild, source.channel_id, source.id),
                silent,
            );
            // Not in the pinned client: BetterForwards' `forward_only` and the note for a resend.
            self.better_forwards_sending(&nonce, &note);
            pending.insert(nonce, *destination);
            if !note.is_empty() {
                self.start_local_text(destination.channel(), note.clone());
            }
        }
        // Module 92961 awaits every send: "Message forwarded!" when all went, else Forward Failed (181622) lists the
        // destinations that did not.
        self.chat_extra.forward_batch = Some(super::chat_notices::ForwardBatch {
            message: source.clone(),
            pending,
            failed: Vec::new(),
        });
        self.go_to_destination(&forward.selected);
        cx.notify();
    }

    /// One forward of the batch ended.
    pub(super) fn forward_send_ended(&mut self, nonce: &str, sent: bool, cx: &mut Context<Self>) {
        let Some(batch) = self.chat_extra.forward_batch.as_mut() else {
            return;
        };
        let Some(destination) = batch.pending.remove(nonce) else {
            return;
        };
        if !sent {
            batch.failed.push(destination);
        }
        if !batch.pending.is_empty() {
            return;
        }
        let Some(batch) = self.chat_extra.forward_batch.take() else {
            return;
        };
        if batch.failed.is_empty() {
            self.show_app_toast("Message forwarded!", cx);
            return;
        }
        // The rows keep the order the destinations were chosen in.
        let failed: Vec<Destination> = batch.failed;
        // Not in the pinned client: BetterForwards' `resendOnFail` sends the message again as text instead.
        let channels: Vec<Id> = failed
            .iter()
            .map(|destination| destination.channel())
            .collect();
        if self.better_forwards_failed(&batch.message, &channels, cx) {
            return;
        }
        self.chat_extra.alert = Some(super::chat_notices::forward_failed_alert(
            batch.message,
            failed,
        ));
        cx.notify();
    }

    /// Retry (module 181622 `fO`): the forward modal again for the message, the failed destinations chosen.
    pub(super) fn retry_forward(
        &mut self,
        message: Message,
        destinations: Vec<Destination>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_forward(message, window, cx);
        if let Some(forward) = self.forward.as_mut() {
            forward.selected = destinations;
        }
        cx.notify();
    }

    /// `transitionToDestination` with a single destination.
    fn go_to_destination(&mut self, selected: &[Destination]) {
        // Not in the pinned client: BetterForwards' `dontFollowForwards` (`shouldTransition`).
        if !super::plugin_hooks::better_forwards_follow() {
            return;
        }
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

    /// Why the forwarded message cannot go to `destination` (`getDestinationIsUnavailable`, `P` in module 969223).
    fn forward_blocked(&self, destination: Destination) -> Option<&'static str> {
        let message = self.forward.as_ref()?.message.as_ref()?;
        let channel_in = |id: Id| {
            self.stores.guilds.iter().find_map(|guild| {
                if let Some(known) = guild.channels.iter().find(|known| known.id == id) {
                    return Some((guild, known));
                }
                let thread = guild.threads.iter().find(|known| known.id == id)?;
                guild
                    .channels
                    .iter()
                    .find(|known| known.id == thread.parent_id)
                    .map(|parent| (guild, parent))
            })
        };
        let source_restricted = channel_in(message.channel_id)
            .is_some_and(|(guild, known)| age_restricted(known, guild));
        let target = match destination {
            Destination::Channel(id) => channel_in(id),
            Destination::Dm(_) => None,
        };
        let check = ForwardCheck {
            source_restricted,
            destination_restricted: target
                .is_some_and(|(guild, known)| age_restricted(known, guild)),
            guild_destination: target.is_some(),
            nsfw_disallowed: !self.stores.account.nsfw_allowed,
            restrict_experiment: self
                .stores
                .experiments
                .variant(FORWARD_AGE_RESTRICTED_EXPERIMENT)
                == Some(1),
            external_sticker: target.is_some_and(|(guild, _)| {
                message.sticker_items.iter().any(|(id, _, _)| {
                    self.stores.stickers.sticker(*id).is_some_and(|sticker| {
                        sticker.is_guild() && sticker.guild_id != Some(guild.id)
                    })
                })
            }),
        };
        let can =
            |permission| target.is_some_and(|(_, known)| self.can_in_channel(known, permission));
        destination_unavailable(message, &check, can).map(dc_intl::t)
    }

    /// A destination's picture (32px), name and second line, as its row in the modal draws them.
    pub(super) fn destination_parts(
        &self,
        destination: Destination,
        cx: &mut Context<Self>,
    ) -> (AnyElement, String, String) {
        match destination {
            Destination::Dm(channel) => {
                let dm = self
                    .stores
                    .private_channels
                    .iter()
                    .find(|known| known.id == channel);
                let user = dm.and_then(|dm| self.stores.dm_recipient(dm)).cloned();
                let picture = user.as_ref().and_then(|user| {
                    let (key, url) = avatar_source(user);
                    let image = self.images.get(&key).cloned();
                    if image.is_none() {
                        self.want_image(key, url, cx);
                    }
                    image
                });
                let loading_avatar = picture.is_none();
                let presence = user
                    .as_ref()
                    .and_then(|user| self.stores.presences.get(&user.id).cloned())
                    .unwrap_or_else(|| "offline".to_owned());
                let icon = div()
                    .relative()
                    .size(px(32.))
                    .when(loading_avatar, |this| {
                        this.rounded_full().bg(color("--background-mod-subtle"))
                    })
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
        let (icon, name, detail) = self.destination_parts(destination, cx);
        // `getDestinationIsUnavailable` (module 969223): the row is `disabled_dhbHaH`, its picture and checkbox at half
        // opacity, its name `--text-muted`, its detail line the reason, the cursor `not-allowed`.
        let blocked = self.forward_blocked(destination);
        let detail = blocked.map_or(detail, str::to_owned);
        div()
            .id(SharedString::from(format!("forward-row-{index}")))
            .h(px(48.))
            .ml(px(-8.))
            .px(px(8.))
            .rounded(px(8.))
            .when(blocked.is_none(), |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(color("--interactive-background-hover")))
            })
            .when(blocked.is_some(), |this| this.cursor_not_allowed())
            .flex()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.forward_blocked(destination).is_none() {
                    this.toggle_destination(destination);
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .when(blocked.is_some(), |this| this.opacity(0.5))
                            .child(icon),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .text_color(color(if blocked.is_some() {
                                        "--text-muted"
                                    } else {
                                        "--text-strong"
                                    }))
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
                    .when(blocked.is_some(), |this| this.opacity(0.5))
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
        // `size-md` (design/dom/modals/forward-*): at most 800 tall (`max-height: min(800px, 100%)`), the list filling
        // it, 56px of the window kept above and below; 480 wide, or the window less 24px each side when narrower.
        let height = (f32::from(window.viewport_size().height) - 112.).clamp(300., 800.);
        let width = (f32::from(window.viewport_size().width) - 48.).min(480.);
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
                .preview_text(&super::first_spans(&message.content), false)
                .into_any_element(),
            (None, Some(user)) => self.profile_preview(user, cx),
            (None, None) => match (&forward.clip, &forward.activity) {
                (Some(clip), _) => self.clips_share_preview(clip),
                (None, Some(act)) => self.activity_share_preview(act),
                (None, None) => div().into_any_element(),
            },
        };
        let attached = forward
            .message
            .as_ref()
            .map(|message| message.attachments.len())
            .filter(|count| *count > 0);
        let has_text = forward
            .message
            .as_ref()
            .is_some_and(|message| !message.content.is_empty());
        let sharing = forward.profile.as_ref().map(|user| user.id);
        let activity_share_url = forward
            .activity
            .as_ref()
            .map(|act| act.share_url.clone())
            .filter(|u| !u.is_empty());
        let send_label = if count <= 1 {
            "Send".to_owned()
        } else {
            format!("Send ({count})")
        };
        let modal = div()
            .parity_with_border("forward-modal", 1.)
            .id("forward-modal")
            .occlude()
            .w(px(width))
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
                                    .child(if forward.clip.is_some() {
                                        "Share Clip".to_owned()
                                    } else if let Some(act) = forward.activity.as_ref() {
                                        act.preview_title
                                            .clone()
                                            .filter(|t| !t.is_empty())
                                            .unwrap_or_else(|| {
                                                format!("Share {}", act.application_name)
                                            })
                                    } else if sharing.is_some() {
                                        "Share Profile".to_owned()
                                    } else {
                                        "Forward To".to_owned()
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
                                        this.close_forward(cx);
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
                            } else if forward.clip.is_some() {
                                // `Ey7mOU`.
                                "Share your clip to a channel or directly with friends!".to_owned()
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
                        .rule(crate::ui::motion::Rule::TextField)
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
                div()
                    .pt(px(16.))
                    .px(px(24.))
                    .child(
                        div()
                            .relative()
                            .pl(px(16.))
                            .when(
                                sharing.is_none()
                                    && forward.clip.is_none()
                                    && forward.activity.is_none(),
                                |this| this.py(px(4.)),
                            )
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
                            // Not in the pinned client: BetterForwards' pickers in place of the file line (`ForwardPicker`).
                            .child(
                                if let Some(picker) = forward
                                    .message
                                    .as_ref()
                                    .and_then(|message| self.better_forwards_picker(message, cx))
                                {
                                    // `contentWrapper`: at most 250 tall, scrolling (the plugin's stylesheet).
                                    div().child(
                                        div()
                                            .id("forward-preview-content")
                                            .max_h(px(250.))
                                            .overflow_y_scroll()
                                            .flex()
                                            .flex_col()
                                            .gap(px(6.))
                                            .when(has_text, |this| this.child(preview))
                                            .child(picker),
                                    )
                                } else if let Some(files) = attached {
                                    // `forwardPreview_N34jMW` (design/dom/voice-message/forward-modal-darker-cozy.json): 56px
                                    // high, 12px gap; `contentWrapper` (6px gap), `attachmentRow` (20px paperclip in
                                    // `--text-muted`, 500 weight 16/20 label) and a 56px `attachmentPreview` at the right
                                    // (`imageWrapperBackground`: rgba(0,0,0,.04), 8px corners). A file shows no picture.
                                    div()
                                        .h(px(56.))
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .flex()
                                                .flex_col()
                                                .gap(px(6.))
                                                .when(has_text, |this| this.child(preview))
                                                .child(
                                                    div()
                                                        .h(px(20.))
                                                        .flex()
                                                        .items_center()
                                                        .gap(px(6.))
                                                        .text_color(color("--text-muted"))
                                                        .child(icon(
                                                            "forward_attachment",
                                                            20.,
                                                            "--text-muted",
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(16.))
                                                                .line_height(px(20.))
                                                                .font_weight(FontWeight(500.))
                                                                .child(if files == 1 {
                                                                    "1 file".to_owned()
                                                                } else {
                                                                    format!("{files} files")
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .size(px(56.))
                                                .flex_none()
                                                .rounded(px(8.))
                                                .bg(gpui::rgba(0x0000000a)),
                                        )
                                } else if sharing.is_some()
                                    || forward.clip.is_some()
                                    || forward.activity.is_some()
                                {
                                    div().child(preview)
                                } else {
                                    div().h(px(22.)).pt(px(2.)).child(preview)
                                },
                            ),
                    )
                    // Not in the pinned client: BetterForwards' "Forward everything" (`renderWrapper`).
                    .children(
                        forward
                            .message
                            .as_ref()
                            .and_then(|message| self.better_forwards_everything(message, cx)),
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
                    .when_some(activity_share_url, |this, url| {
                        // Module 529999 Copy Link secondary icon button (`WqhZss` / `L/PwZf`).
                        this.child(
                            self.with_tip(
                                "activity-share-copy-tip",
                                super::tooltip::Tip::text(
                                    dc_intl::t("WqhZss"),
                                    super::tooltip::Side::Top,
                                ),
                                div()
                                    .id("activity-share-copy-link")
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
                                            url.clone(),
                                        ));
                                        if let Some(act) =
                                            this.forward.as_mut().and_then(|f| f.activity.as_mut())
                                        {
                                            act.did_copy_link = true;
                                        }
                                        this.show_success_toast(
                                            dc_intl::t("L/PwZf").to_owned(),
                                            cx,
                                        );
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                    })
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
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                                // Not in the pinned client: BetterForwards reads Shift on Send (`setShift`).
                                super::plugin_hooks::better_forwards_send_pressed(
                                    event.modifiers().shift,
                                );
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
        let pose = crate::ui::motion::modal_pose("forward");
        Some(
            deferred(
                div()
                    .id("forward-layer")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(pose.dim(rgba(0x000000b8)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_forward(cx);
                    }))
                    .child(modal.posed(pose)),
            )
            // Above the media viewer (priority 2): Forward stacks over it, as the official does.
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Module 529999 preview card for `SHARE_LINK` / `SHARE_CONTENT`:
    /// title (or app name), subtitle (or MIME media count summary `Lr0Top`/`SJ6pPX`/`h4pFfU`/`89ihS8`),
    /// and a 56x56 preview thumbnail with optional `+{N}` overflow badge.
    fn activity_share_preview(&self, act: &ShareActivity) -> AnyElement {
        let title = act
            .preview_title
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&act.application_name)
            .to_owned();
        let subtitle = act
            .subtitle
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                activity_share_media_summary(
                    act.media_counts.0,
                    act.media_counts.1,
                    act.media_counts.2,
                )
            })
            .unwrap_or_else(|| act.share_url.clone());
        let thumb = self
            .images
            .get(&format!("activity-share:{}", act.application_id))
            .cloned()
            .or_else(|| {
                act.image_url
                    .as_deref()
                    .and_then(|u| self.images.get(u).cloned())
            });
        div()
            .min_h(px(56.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .truncate()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(56.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(color("--background-base-lowest"))
                    .border_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match thumb {
                        Some(pic) => img(pic)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .into_any_element(),
                        None => icon("activity_rocket", 22., "--icon-subtle").into_any_element(),
                    })
                    .when(act.extra_media > 0, |box_el| {
                        box_el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(rgba(0x00000088))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(FontWeight(700.))
                                .text_color(color("--white"))
                                .child(format!("+{}", act.extra_media)),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Capture helper: `activity-share` opens the Activity Share modal (`529999`).
    #[cfg(feature = "capture")]
    pub(crate) fn script_activity_share(&mut self, cx: &mut Context<Self>) -> String {
        let share_url =
            build_activity_share_url("880218394199220334", Some("match-42"), Some("lnk-7"), None);
        self.open_share_activity(
            ShareActivity {
                application_id: "880218394199220334".to_owned(),
                application_name: "Watch Together".to_owned(),
                preview_title: Some("Share Watch Together Highlight".to_owned()),
                subtitle: None,
                share_url: share_url.clone(),
                content: share_url,
                image_url: None,
                media_counts: (2, 1, 0),
                extra_media: 2,
                sdk_cmd: None,
                did_copy_link: false,
            },
            None,
            cx,
        );
        "activity-share: opened".to_owned()
    }

    /// `preview_0XXhWj`: the shared user, avatar 40 with status, display name and tag.
    fn profile_preview(&self, user: &dc_state::User, cx: &mut Context<Self>) -> AnyElement {
        let (avatar_key, avatar_url) = avatar_source(user);
        let picture = self.images.get(&avatar_key).cloned();
        if picture.is_none() {
            self.want_image(avatar_key, avatar_url, cx);
        }
        let loading_avatar = picture.is_none();
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
                    .when(loading_avatar, |this| {
                        this.bg(color("--background-mod-subtle"))
                    })
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

/// `2026-08-forward-age-restricted-destinations` (module 969223): variation 1 sets
/// `disableAgeRestrictedDestinations`.
const FORWARD_AGE_RESTRICTED_EXPERIMENT: &str = "2026-08-forward-age-restricted-destinations";

/// `Gc` of module 874850: an NSFW channel, or one in a server whose `nsfw_level` is EXPLICIT (1) or AGE_RESTRICTED
/// (3).
fn age_restricted(channel: &dc_state::Channel, guild: &dc_state::Guild) -> bool {
    channel.nsfw || matches!(guild.nsfw_level, 1 | 3)
}

/// What `getDestinationIsUnavailable` reads besides the message and the destination's permissions.
struct ForwardCheck {
    source_restricted: bool,
    destination_restricted: bool,
    /// A server channel (DMs skip the permission checks).
    guild_destination: bool,
    /// `nsfwAllowed === false` on the current user.
    nsfw_disallowed: bool,
    restrict_experiment: bool,
    /// A sticker of another server is attached (`USE_EXTERNAL_STICKERS` decides).
    external_sticker: bool,
}

/// `P` of module 969223, in its order: an age-restricted message only goes to another age-restricted channel
/// (`KgPx1D`); an account not allowed NSFW cannot forward into an age-restricted channel under the experiment
/// (`QHrFo6`); then, in a server channel, files without Attach Files (`P7yvbm`), embeds without Embed Links unless the
/// message has text or a forwarded copy with text or files (`Wr4RIX`), another server's sticker without Use External
/// Stickers (`0Yyrua`) and a voice message without Send Voice Messages (`quj4DY`). Returns the string key.
/// GAP: the age-assurance branch of `QHrFo6` (`u5()` with `AGE_GATED_SPACES`) is not run (owner excluded age
/// verification) and checkpoint-card components (`Wr4RIX` without Embed Links) are not parsed.
fn destination_unavailable(
    message: &Message,
    check: &ForwardCheck,
    can: impl Fn(u64) -> bool,
) -> Option<&'static str> {
    if check.source_restricted && !check.destination_restricted {
        return Some("KgPx1D");
    }
    if check.guild_destination
        && check.destination_restricted
        && check.nsfw_disallowed
        && check.restrict_experiment
    {
        return Some("QHrFo6");
    }
    if !check.guild_destination {
        return None;
    }
    let copy = message.forwarded.as_deref();
    let files =
        !message.attachments.is_empty() || copy.is_some_and(|copy| !copy.attachments.is_empty());
    if files && !can(dc_state::permissions::ATTACH_FILES) {
        return Some("P7yvbm");
    }
    let embeds = !message.embeds.is_empty() || copy.is_some_and(|copy| !copy.embeds.is_empty());
    let has_text = !message.content.is_empty()
        || copy.is_some_and(|copy| !copy.content.is_empty() || !copy.attachments.is_empty());
    if embeds && !can(dc_state::permissions::EMBED_LINKS) && !has_text {
        return Some("Wr4RIX");
    }
    if check.external_sticker && !can(dc_state::stickers::USE_EXTERNAL_STICKERS) {
        return Some("0Yyrua");
    }
    let voice = message.is_voice_message()
        || copy.is_some_and(|copy| copy.flags & dc_state::VOICE_MESSAGE_FLAG != 0);
    if voice && !can(dc_state::permissions::SEND_VOICE_MESSAGES) {
        return Some("quj4DY");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(extra: serde_json::Value) -> Message {
        let mut value = json!({"id": "5", "channel_id": "23", "content": "", "timestamp": "2026-11-07T19:00:00+00:00",
            "author": {"id": "7", "username": "a", "discriminator": "0"}});
        for (key, field) in extra.as_object().unwrap() {
            value[key] = field.clone();
        }
        Message::from_value(&value).unwrap()
    }

    fn check() -> ForwardCheck {
        ForwardCheck {
            source_restricted: false,
            destination_restricted: false,
            guild_destination: true,
            nsfw_disallowed: false,
            restrict_experiment: false,
            external_sticker: false,
        }
    }

    #[test]
    fn age_restricted_messages_only_go_to_age_restricted_channels() {
        let plain = message(json!({"content": "hi"}));
        let from_nsfw = ForwardCheck {
            source_restricted: true,
            ..check()
        };
        assert_eq!(
            destination_unavailable(&plain, &from_nsfw, |_| true),
            Some("KgPx1D")
        );
        let dm = ForwardCheck {
            guild_destination: false,
            ..from_nsfw
        };
        assert_eq!(
            destination_unavailable(&plain, &dm, |_| true),
            Some("KgPx1D")
        );
        let both = ForwardCheck {
            source_restricted: true,
            destination_restricted: true,
            ..check()
        };
        assert_eq!(destination_unavailable(&plain, &both, |_| true), None);
        assert_eq!(
            dc_intl::t("KgPx1D"),
            "Messages cannot be forwarded from age-restricted to unrestricted channels."
        );
    }

    #[test]
    fn an_account_without_nsfw_cannot_forward_into_an_age_restricted_channel_under_the_experiment()
    {
        let plain = message(json!({"content": "hi"}));
        let into = ForwardCheck {
            destination_restricted: true,
            nsfw_disallowed: true,
            ..check()
        };
        assert_eq!(destination_unavailable(&plain, &into, |_| true), None);
        let on = ForwardCheck {
            restrict_experiment: true,
            ..into
        };
        assert_eq!(
            destination_unavailable(&plain, &on, |_| true),
            Some("QHrFo6")
        );
        let allowed = ForwardCheck {
            nsfw_disallowed: false,
            ..on
        };
        assert_eq!(destination_unavailable(&plain, &allowed, |_| true), None);
    }

    #[test]
    fn permissions_are_checked_in_order() {
        let embed_only = message(json!({"embeds": [{"type": "rich", "title": "t"}]}));
        let no_embed_links = |permission| permission != dc_state::permissions::EMBED_LINKS;
        assert_eq!(
            destination_unavailable(&embed_only, &check(), no_embed_links),
            Some("Wr4RIX")
        );
        // Text alongside the embed lets it through.
        let with_text =
            message(json!({"content": "see", "embeds": [{"type": "rich", "title": "t"}]}));
        assert_eq!(
            destination_unavailable(&with_text, &check(), no_embed_links),
            None
        );
        let sticker = ForwardCheck {
            external_sticker: true,
            ..check()
        };
        assert_eq!(
            destination_unavailable(&with_text, &sticker, |permission| permission
                != dc_state::stickers::USE_EXTERNAL_STICKERS),
            Some("0Yyrua")
        );
        let file = message(
            json!({"attachments": [{"id": "1", "filename": "b.txt", "url": "u", "size": 1}]}),
        );
        assert_eq!(
            destination_unavailable(&file, &check(), |_| false),
            Some("P7yvbm")
        );
        // DMs skip the permission checks.
        let dm = ForwardCheck {
            guild_destination: false,
            ..check()
        };
        assert_eq!(destination_unavailable(&file, &dm, |_| false), None);
    }
}
