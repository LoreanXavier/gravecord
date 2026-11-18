//! A plain confirmation in front of a request, for the actions whose official confirmation is a modal of this shape
//! (a title, one sentence, Cancel and a confirm button): Delete Thread (module 307623 opens module 894533, "Delete
//! Thread" with `a6Gz9J`) and the inbox's Mark All as Read (module 713024 `tI`, `h4bVZ2` and `0MQ0Bf`). What it will
//! do is built when the dialog opens, from what was clicked, so a later navigation cannot change where it goes.

use super::Root;
use crate::net::Request;
use dc_state::Id;
use gpui::{AnyElement, Context, FontWeight, IntoElement};

/// What the confirm button does.
enum Then {
    /// One request.
    Send(Box<Request>),
    /// A bulk mark as read of `(channel, message)` pairs (`mark_read.rs`).
    MarkRead(Vec<(Id, Id)>),
    /// The channel's pin is taken off (`jA(guild, channel, false)` of module 816662): FAVORITED leaves its flags.
    Unpin { guild: Id, channel: Id },
}

/// The question that is open: nothing happens until its confirm button is pressed.
pub(super) struct Pending {
    id: &'static str,
    title: String,
    before: String,
    name: String,
    after: String,
    confirm: String,
    critical: bool,
    then: Option<Then>,
}

/// `DELETE /channels/{thread}` (`deleteChannel`, module 435183) for a thread.
pub(super) fn delete_thread_request(thread: Id) -> Request {
    Request::Rest {
        key: None,
        method: reqwest::Method::DELETE,
        route: format!("/channels/{thread}"),
        body: None,
    }
}

/// The text of a message before and after its bold name (`**{name}**`), for a body drawn as before, name, after.
fn around(key: &str, name_value: &str, name: &str) -> (String, String) {
    let runs = dc_intl::format_runs(key, &[(name_value, name.into())]);
    let at = runs
        .iter()
        .position(|(_, tag)| tag.as_deref() == Some("$b"));
    let join = |runs: &[(String, Option<Box<str>>)]| -> String {
        runs.iter().map(|(text, _)| text.as_str()).collect()
    };
    match at {
        Some(at) => (join(&runs[..at]), join(&runs[at + 1..])),
        None => (join(&runs), String::new()),
    }
}

/// `FAVORITED` of a channel override's flags (module 355097 `vv`): the channel is pinned to the top.
const FAVORITED: u64 = 2048;

impl Pending {
    /// Module 933752, opened when a pinned channel is dragged (`canDrag` of module 616740, reason `pinned-channel` of
    /// 79843): "Trying to reorder a pinned channel?", `K82phZ`, Cancel and "Unpin Channel" in the brand colour.
    pub(super) fn unpin_to_reorder(guild: Id, channel: Id) -> Self {
        Self {
            id: "pinned-reorder-modal",
            title: dc_intl::t("7r7op8").to_owned(),
            before: dc_intl::t("K82phZ").to_owned(),
            name: String::new(),
            after: String::new(),
            confirm: dc_intl::t("H8ZQbo").to_owned(),
            critical: false,
            then: Some(Then::Unpin { guild, channel }),
        }
    }

    /// Module 894533 for a thread: title and button `H7vTe2` "Delete Thread", body `a6Gz9J`.
    pub(super) fn delete_thread(thread: Id, name: &str) -> Self {
        Self {
            id: "delete-thread-modal",
            title: dc_intl::tr("Delete Thread").to_owned(),
            before: around("a6Gz9J", "channelName", name).0,
            name: name.to_owned(),
            after: around("a6Gz9J", "channelName", name).1,
            confirm: dc_intl::tr("Delete Thread").to_owned(),
            critical: true,
            then: Some(Then::Send(Box::new(delete_thread_request(thread)))),
        }
    }

    /// Module 713024 `tN`: "Mark all as read?" (`h4bVZ2`), `0MQ0Bf`, confirm `e6RscS` "Mark As Read" in the brand
    /// colour.
    pub(super) fn mark_all_read(reads: Vec<(Id, Id)>) -> Self {
        Self {
            id: "mark-all-read-modal",
            title: dc_intl::tr("Mark all as read?").to_owned(),
            before: dc_intl::tr("This will mark all your unmuted channels as read. Are you sure you want to continue?")
                .to_owned(),
            name: String::new(),
            after: String::new(),
            confirm: dc_intl::tr("Mark As Read").to_owned(),
            critical: false,
            then: Some(Then::MarkRead(reads)),
        }
    }

    /// `renderSuppressConfirmModal` of module 679636: "Are you sure?" (`VL1KOk`), `vXZ+Fo` and the shift hint
    /// `5j2by3`, confirm "Remove All Embeds" (`YEHppG`) in red (module 640238's default).
    pub(super) fn suppress_embeds(request: Request) -> Self {
        Self {
            id: "suppress-embeds-modal",
            title: dc_intl::tr("Are you sure?").to_owned(),
            before: format!(
                "{}
{}",
                dc_intl::tr("This will remove all embeds on this message for everyone."),
                dc_intl::tr("Hold shift when clearing embeds to skip this modal.")
            ),
            name: String::new(),
            after: String::new(),
            confirm: dc_intl::tr("Remove All Embeds").to_owned(),
            critical: true,
            then: Some(Then::Send(Box::new(request))),
        }
    }

    #[cfg(test)]
    fn request(&self) -> Option<&Request> {
        match self.then.as_ref()? {
            Then::Send(request) => Some(request),
            Then::MarkRead(_) | Then::Unpin { .. } => None,
        }
    }
}

impl Root {
    /// Opens the question; what it will do waits in it.
    pub(super) fn ask_destructive(&mut self, pending: Pending, cx: &mut Context<Self>) {
        self.destructive = Some(pending);
        cx.notify();
    }

    /// Escape and Cancel: nothing happens.
    pub(super) fn close_destructive(&mut self) {
        self.destructive = None;
    }

    pub(super) fn destructive_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.destructive.as_ref()?;
        // The name in bold inside one sentence, as the official body (`$b` in `a6Gz9J`) draws it.
        let bold = pending.before.len()..pending.before.len() + pending.name.len();
        let subtitle = gpui::StyledText::new(format!(
            "{}{}{}",
            pending.before, pending.name, pending.after
        ))
        .with_highlights([(
            bold,
            gpui::HighlightStyle {
                font_weight: Some(FontWeight(600.)),
                ..Default::default()
            },
        )]);
        let tone = if pending.critical {
            super::modal::Tone::Critical
        } else {
            super::modal::Tone::Primary
        };
        Some(super::modal::render_small_over_popouts(
            super::modal::Spec {
                id: pending.id,
                title: pending.title.clone().into(),
                subtitle: Some(subtitle.into_any_element()),
                body: None,
                buttons: vec![
                    super::modal::button(
                        "destructive-cancel",
                        dc_intl::tr("Cancel"),
                        super::modal::Tone::Secondary,
                        |this, _, _| this.close_destructive(),
                    ),
                    super::modal::button(
                        "destructive-confirm",
                        pending.confirm.clone(),
                        tone,
                        |this, _, cx| {
                            let Some(then) = this
                                .destructive
                                .take()
                                .and_then(|mut pending| pending.then.take())
                            else {
                                return;
                            };
                            match then {
                                Then::Send(request) => {
                                    let _ = this.requests.send(*request);
                                }
                                Then::MarkRead(reads) => this.mark_read_bulk(&reads, cx),
                                Then::Unpin { guild, channel } => {
                                    let flags =
                                        this.stores.guild_settings.channel(guild, channel).flags;
                                    this.patch_channel_override(
                                        guild,
                                        channel,
                                        serde_json::json!({ "flags": flags & !FAVORITED }),
                                    );
                                }
                            }
                        },
                    ),
                ],
                close: Box::new(|this, _, _| this.close_destructive()),
            },
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_delete_waits_in_the_question_with_its_own_id() {
        let pending = Pending::delete_thread(77, "plans");
        assert_eq!(pending.title, "Delete Thread");
        assert_eq!(pending.confirm, "Delete Thread");
        let Some(Request::Rest { method, route, .. }) = pending.request() else {
            panic!("no request");
        };
        assert_eq!(*method, reqwest::Method::DELETE);
        assert_eq!(route, "/channels/77");
    }

    /// Module 933752: the question names the pinned channel's way out and sends nothing by itself.
    #[test]
    fn reordering_a_pinned_channel_asks_to_unpin_it() {
        let pending = Pending::unpin_to_reorder(1, 2);
        assert_eq!(pending.title, "Trying to reorder a pinned channel?");
        assert_eq!(pending.confirm, "Unpin Channel");
        assert!(!pending.critical);
        assert!(pending.request().is_none());
        assert!(matches!(
            pending.then,
            Some(Then::Unpin {
                guild: 1,
                channel: 2
            })
        ));
        assert_eq!(2048 & !FAVORITED, 0);
    }

    #[test]
    fn mark_all_read_sends_nothing_until_confirmed() {
        let pending = Pending::mark_all_read(vec![(1, 2)]);
        assert!(pending.request().is_none());
        assert!(!pending.critical);
        assert_eq!(pending.title, "Mark all as read?");
    }
}
