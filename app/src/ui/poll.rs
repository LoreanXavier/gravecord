//! Polls in a message: the card, voting, results, the voters dialog and End Poll Now.
//!
//! Ports the poll card `Q` (module 119591), its view model `T` / `Ay` (module 951727), the vote handlers (module
//! 180662), the answer widgets (module 306417) and the voters dialog `K` (module 637806). The classes come from the
//! CSS modules 984778 (card), 259218 (answers), 588466 (image-only answers), 146899 (radio and checkbox) and 191567
//! (the per-state colour variables), with the values read from `assets/519435.8f3cb087a70781d9.css`. The strings are
//! `design/snapshot/dumps/STRINGS/en-US.json` (the key is named next to each).
//!
//! Voting: `PUT /channels/{c}/polls/{m}/answers/@me` with `{answer_ids}` (module 335759); removing a vote sends the
//! same call with no ids; `POST /channels/{c}/polls/{m}/expire` ends the poll (the author's "End Poll Now").
//! The vote shows at once and is taken back when the server refuses it (`D` and `U` in module 180662).

use super::motion::{self, Ease};
use super::tooltip::{Body, Side, Tip};
use super::{Root, avatar_source, color, modal};
use crate::net::Request;
use dc_state::message_kinds::{Poll, PollEmoji};
use dc_state::polls::LAYOUT_IMAGE_ONLY_ANSWERS;
use dc_state::{Id, Message, User};
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Rgba, SharedString, div, img, prelude::*, px,
    relative, svg,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The key of a vote request's answer (`Update::Fetched`).
pub(super) const VOTE_KEY: &str = "pollvote:";
/// The key of a voters list (`GET /channels/{c}/polls/{m}/answers/{answer}`).
pub(super) const VOTERS_PREFIX: &str = "pollvoters:";
/// `/assets/64a47ba893401c1c.svg`: the picture of an answer nobody picked (module 637806 `Q`).
const EMPTY_IMAGE: &str = "https://discordapp.com/assets/64a47ba893401c1c.svg";

/// What a poll card remembers about its message between frames (`PollStateStore`, module 862780): the answers
/// ticked but not yet sent, whether the vote is on its way, and whether the results are shown without a vote.
#[derive(Clone, Debug, Default)]
struct View {
    selected: Vec<u64>,
    submitting: bool,
    show_results: bool,
    /// This account's vote before the one on its way, to take back when the server refuses.
    before: Option<Vec<u64>>,
}

/// The voters dialog (`PollVotesModal`): the message and the answer on show.
#[derive(Clone, Debug)]
struct Voters {
    channel: Id,
    message: Id,
    answer: u64,
}

#[derive(Default)]
pub(crate) struct PollUi {
    views: HashMap<(Id, Id), View>,
    voters: Option<Voters>,
    /// "End Poll Now?": the poll awaiting the author's answer.
    end: Option<(Id, Id)>,
    /// The "Oh no! Something went wrong." alert: its body.
    error: Option<String>,
}

impl PollUi {
    pub(crate) fn modal_open(&self) -> bool {
        self.voters.is_some() || self.end.is_some() || self.error.is_some()
    }

    #[cfg(feature = "capture")]
    pub(crate) fn close_modals(&mut self) {
        self.voters = None;
        self.end = None;
        self.error = None;
    }

    /// Closes the top dialog, as Escape does.
    pub(crate) fn escape(&mut self) -> bool {
        if self.error.take().is_some() || self.end.take().is_some() || self.voters.take().is_some()
        {
            return true;
        }
        false
    }

    /// Answers ticked on a message, for tests and the harness log.
    #[cfg(test)]
    fn selected(&self, channel: Id, message: Id) -> Vec<u64> {
        self.views
            .get(&(channel, message))
            .map(|view| view.selected.clone())
            .unwrap_or_default()
    }
}

/// How the answers answer a click (`f.CQ` in module 438437).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interaction {
    List,
    Checkboxes,
    RadioButtons,
}

/// The `answer.style` of the view model: which colour variables the answer takes (module 191567).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    /// `normalStylesDefault` and the states that share it.
    Normal,
    /// `votedStyles`: this account's answer while the poll is open.
    Voted,
    /// `victorStyles`: the leading answer of a closed poll.
    Victor,
}

/// The view model `T` of module 951727 for one message.
#[derive(Clone, Debug)]
struct Model {
    expired: bool,
    time_left: Option<String>,
    has_voted: bool,
    can_tap: bool,
    can_remove: bool,
    can_submit: bool,
    show_counts: bool,
    show_results: bool,
    interaction: Interaction,
}

fn model(poll: &Poll, view: &View, now: chrono::DateTime<chrono::Utc>) -> Model {
    let expired = poll.is_expired(now);
    let time_left = poll.time_left(now);
    let has_selected = !view.selected.is_empty();
    // `hasVoteRecorded` (some reaction with `me_vote`); `hasVoted` adds `!editing`, and nothing sets editing here.
    let has_voted = poll.has_vote();
    let show_counts = has_voted || expired || view.show_results;
    // `C`: sent, the latest copy in the store, and not yet voted (or the counts are on show).
    let can_tap = !has_voted || show_counts;
    let can_submit = !view.submitting && has_selected && !has_voted;
    let can_remove = has_voted && !expired;
    let interaction = if show_counts || !can_tap {
        Interaction::List
    } else if !poll.multiselect {
        Interaction::RadioButtons
    } else {
        Interaction::Checkboxes
    };
    Model {
        expired,
        time_left,
        has_voted,
        can_tap,
        can_remove,
        can_submit,
        show_counts,
        show_results: view.show_results,
        interaction,
    }
}

/// The `match` of `Ay` in module 951727 that picks an answer's style: a closed poll's leaders are victors, an open
/// poll shows this account's own answers as voted, and the rest take the plain colours (`selected` only changes the
/// outline).
fn look(model: &Model, poll: &Poll, index: usize) -> Look {
    let did_vote = model.has_voted && poll.me_voted.get(index).copied().unwrap_or(false);
    if model.expired {
        if poll.is_leader(index) {
            Look::Victor
        } else {
            Look::Normal
        }
    } else if did_vote {
        Look::Voted
    } else {
        Look::Normal
    }
}

/// "{count} vote(s)" (`XRkuof`).
fn votes_label(count: u64) -> String {
    if count == 1 {
        "1 vote".to_owned()
    } else {
        format!("{count} votes")
    }
}

/// `Easing.inOut(Easing.ease)` of React Native's `Animated` (module 703806: `ease` is `bezier(.42, 0, 1, 1)`), the
/// easing of the percentage bar (module 119591 `M`). Sampled into stops for the motion layer.
fn bar_stops() -> &'static [(f32, f32)] {
    static STOPS: OnceLock<Vec<(f32, f32)>> = OnceLock::new();
    STOPS.get_or_init(|| {
        let ease = |x: f64| Ease::EaseIn.apply(x);
        (1..=96)
            .map(|step| {
                let t = step as f64 / 96.;
                let value = if t < 0.5 {
                    ease(2. * t) / 2.
                } else {
                    1. - ease((1. - t) * 2.) / 2.
                };
                (t as f32, value as f32)
            })
            .collect()
    })
}

/// The bar's width: `{from: {width: shouldAnimate ? 0% : N%}, to: {width: N%}}` over 1200 ms.
fn bar_width(key: (Id, Id, u64), percentage: u32, animate: bool) -> f32 {
    let target = percentage as f32;
    let key = motion::key(&("poll-bar", key));
    motion::value_from(
        key,
        if animate { 0. } else { target },
        target,
        motion::css(1200., Ease::Stops(bar_stops())),
    )
}

/// What to fetch for the emoji of a message's poll answers.
pub(super) fn wanted(message: &Message) -> Vec<(String, String)> {
    let Some(poll) = message.extra.poll.as_ref() else {
        return Vec::new();
    };
    poll.emoji
        .iter()
        .flatten()
        .filter_map(|emoji| match emoji.id {
            Some(id) => Some((
                format!("custom:{id}"),
                format!("https://cdn.discordapp.com/emojis/{id}.webp?size=96"),
            )),
            None => {
                let file = dc_tokens::emoji_file(&emoji.name)?;
                Some((
                    format!("emoji:{}", emoji.name),
                    format!("{}/assets/{file}", dc_proto::pin::WEBAPP_ORIGIN),
                ))
            }
        })
        .collect()
}

/// A filled circle with a tick cut out (`CircleCheckIcon`, module 628284): a disc of `secondary` under the
/// `circle_check` outline in `colour`.
fn circle_check(size: f32, colour: Rgba, secondary: Rgba) -> AnyElement {
    let disc = size * 20. / 24.;
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(
            div()
                .absolute()
                .top(px(size * 2. / 24.))
                .left(px(size * 2. / 24.))
                .size(px(disc))
                .rounded_full()
                .bg(secondary),
        )
        .child(
            svg()
                .path("icons/circle_check.svg")
                .size(px(size))
                .text_color(colour),
        )
        .into_any_element()
}

/// `Vx` in module 306417: the colours of the tick on an answer.
fn vote_tick(size: f32, victor: bool, expired: bool) -> AnyElement {
    let (colour, secondary) = match (expired, victor) {
        (true, true) => (
            color("--control-connected-background-default"),
            color("--white"),
        ),
        (true, false) => (color("--icon-strong"), color("--background-mod-muted")),
        _ => (color("--brand-500"), color("--white")),
    };
    circle_check(size, colour, secondary)
}

/// A tile of the image layout: two columns share the 440px inside the card's padding with a 16px gap.
const TILE: f32 = 212.;

impl Root {
    /// The card under a "poll has closed" line (`eB` in module 117096, CSS `container_D5ZFqQ`): the winning answer
    /// with a green tick and "Winning answer * N%", or "The results were tied" / "There was no winner", and a
    /// "View Poll" button that jumps to the poll (`messageReference`).
    pub(super) fn poll_result_card(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let embed = message.embeds.first()?;
        let field = |name: &str| {
            embed
                .fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| field.value.clone())
        };
        field("poll_question_text")?;
        let total: u64 = field("total_votes")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let victor_votes: u64 = field("victor_answer_votes")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let percentage = if total > 0 {
            ((victor_votes as f64 / total as f64) * 100.0 + 0.5).floor() as u64
        } else {
            0
        };
        let text = |content: String, size: f32, line: f32, weight: f32, tint: &'static str| {
            div()
                .text_size(px(size))
                .line_height(px(line))
                .font_weight(FontWeight(weight))
                .text_color(color(tint))
                .child(content)
        };
        let description = if total == 0 {
            // a+lSZ/
            div().flex().flex_col().child(text(
                dc_intl::t!("a+lSZ/").into(),
                14.,
                18.,
                600.,
                "--text-default",
            ))
        } else if field("victor_answer_id").is_some() {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .child(text(
                            field("victor_answer_text").unwrap_or_default(),
                            14.,
                            18.,
                            600.,
                            "--text-default",
                        ))
                        .child(vote_tick(16., true, true)),
                )
                // ufIDIx
                .child(text(
                    format!("{} \u{2022} {percentage}%", dc_intl::t!("ufIDIx")),
                    12.,
                    16.,
                    500.,
                    "--text-muted",
                ))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                // kPN9si
                .child(text(
                    dc_intl::t!("kPN9si").into(),
                    14.,
                    18.,
                    600.,
                    "--text-default",
                ))
                .child(text(
                    format!("{percentage}%"),
                    12.,
                    16.,
                    500.,
                    "--text-muted",
                ))
        };
        let target = message.reference_id.map(|poll| {
            (
                message.reference_channel_id.unwrap_or(message.channel_id),
                poll,
            )
        });
        let id = message.id;
        Some(
            div()
                .mt(px(8.))
                // Golden `d-closed`: the row ends 4px under the result card.
                .mb(px(2.))
                .w_full()
                .max_w(px(410.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .py(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .rounded(px(8.))
                .bg(color("--background-surface-high"))
                .child(div().flex_1().overflow_hidden().child(description))
                .children(target.map(|(channel, poll)| {
                    // Jw7Vbf
                    div()
                        .id(SharedString::from(format!("poll-view-{id}")))
                        .flex_none()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.jump_to_message_id(channel, poll, true, Some(id));
                            cx.notify();
                        }))
                        .child(
                            super::sticker_popout::small_button("poll-view", "View Poll", false)
                                .w_auto(),
                        )
                }))
                .into_any_element(),
        )
    }

    /// `$m` in module 969632: up to three voters' names (not blocked or ignored people), then "and N others".
    fn poll_voters_text(&self, poll: &Poll, index: usize) -> String {
        let Some(answer) = poll.answer_ids.get(index) else {
            return String::new();
        };
        let names: Vec<String> = poll
            .voters
            .get(answer)
            .into_iter()
            .flatten()
            .filter(|user| !self.is_blocked(**user) && !self.stores.is_ignored(**user))
            .filter_map(|user| self.stores.users.get(user))
            .take(3)
            .map(|user| self.guild_display_name(user))
            .collect();
        Poll::voters_tooltip(&names, poll.counts.get(index).copied().unwrap_or(0))
    }

    fn poll_view(&self, message: &Message) -> View {
        self.polls
            .views
            .get(&(message.channel_id, message.id))
            .cloned()
            .unwrap_or_default()
    }

    fn poll_emoji(&self, emoji: &PollEmoji, size: f32) -> Option<AnyElement> {
        let key = match emoji.id {
            Some(id) => format!("custom:{id}"),
            None => format!("emoji:{}", emoji.name),
        };
        let picture = self.images.get(&key).cloned();
        Some(match picture {
            Some(picture) => img(picture).size(px(size)).flex_none().into_any_element(),
            None => div().size(px(size)).flex_none().into_any_element(),
        })
    }

    /// The poll card of a message, or nothing when the message has no poll.
    pub(super) fn poll_card(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut poll = message.extra.poll.clone()?;
        poll.normalize();
        if poll.answers.is_empty() {
            return None;
        }
        let view = self.poll_view(message);
        let now = chrono::Utc::now();
        let model = model(&poll, &view, now);
        let (channel, id) = (message.channel_id, message.id);
        let animate = view.submitting && !motion::reduced();
        let answers: Vec<AnyElement> = if poll.layout == LAYOUT_IMAGE_ONLY_ANSWERS {
            self.poll_image_answers(message, &poll, &model, &view, animate, cx)
        } else {
            (0..poll.answers.len())
                .map(|index| self.poll_answer(message, &poll, &model, &view, index, animate, cx))
                .collect()
        };
        let image_only = poll.layout == LAYOUT_IMAGE_ONLY_ANSWERS;
        // `.answersContainer`: one column with 8px gaps and `8px 0 16px` margin; the image layout is two columns
        // with 16px gaps (`answersContainer_hdMKHE`).
        let list = div()
            .flex()
            .flex_col()
            .gap(px(if image_only { 16. } else { 8. }))
            .mt(px(8.))
            .mb(px(16.))
            .children(answers);

        // Prompt: "Select one answer" / "Select one or more answers" while the poll is open.
        let prompt = (!model.expired).then_some(if poll.multiselect {
            "Select one or more answers" // yCXvxa
        } else {
            "Select one answer" // 9Y2wKO
        });
        let header = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(20.))
                    .font_weight(FontWeight(500.))
                    .text_color(color("--text-strong"))
                    .child(poll.question.clone()),
            )
            .children(prompt.map(|prompt| {
                div()
                    .mt(px(4.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(prompt)
            }));

        // The bottom bar: details on the left, then the link, then the button (`bottomBar_HOJNvw`).
        let details = self.poll_details(message, &poll, &model, cx);
        let show_link = !model.expired && !model.has_voted && !model.show_results;
        let link = show_link.then(|| {
            text_button(
                SharedString::from(format!("poll-show-results-{id}")),
                "Show results", // /KHAUF
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.poll_toggle_results(channel, id);
                cx.notify();
            }))
        });
        let primary = self.poll_primary(message, &model, cx);
        let bar = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(16.))
            .min_h(px(38.))
            .child(div().mr_auto().child(details))
            .children(link)
            .children(primary);

        Some(
            div()
                .id(SharedString::from(format!("poll-{id}")))
                .flex()
                .flex_col()
                .w_full()
                .min_w(px(302.))
                .max_w(px(472.))
                // Golden `d-closed` and `c-multi-voted`: 16px between two poll cards of one author, 25px from the
                // card to the next group's row.
                .mt(px(4.))
                .mb(px(4.))
                .p(px(16.))
                .overflow_hidden()
                .rounded(px(8.))
                .border_1()
                .border_color(color("--border-subtle"))
                .bg(color("--background-surface-high"))
                .child(header)
                .child(list)
                .child(bar)
                .into_any_element(),
        )
    }

    /// The footer's left side: "N votes" (a link to the voters) and the time left or "Poll closed".
    fn poll_details(
        &self,
        message: &Message,
        poll: &Poll,
        model: &Model,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (channel, id) = (message.channel_id, message.id);
        let first = poll.answer_ids.first().copied().unwrap_or(1);
        let votes = text_button(
            SharedString::from(format!("poll-voters-{id}")),
            votes_label(poll.total_votes()),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.poll_open_voters(channel, id, first, cx);
        }));
        let label = model
            .time_left
            .clone()
            .unwrap_or_else(|| "Poll closed".to_owned()); // e+J3JZ
        div()
            .flex()
            .flex_row()
            .whitespace_nowrap()
            .child(votes)
            // `.detailsText > :not(:first-child):before { content: "\2219"; margin: 0 .5rem }`
            .child(
                div()
                    .mx(px(8.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child("\u{2219}"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-muted"))
                    .child(label),
            )
            .into_any_element()
    }

    /// The button at the right of the bar: Vote, Remove Vote, or Go back to vote.
    fn poll_primary(
        &self,
        message: &Message,
        model: &Model,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if model.expired {
            return None;
        }
        let (channel, id) = (message.channel_id, message.id);
        let (label, primary, enabled, action) = if model.can_remove {
            ("Remove Vote", false, true, Action::Remove) // XhQEh8
        } else if !model.has_voted && model.show_results {
            ("Go back to vote", false, true, Action::ToggleResults) // gNj6In
        } else {
            ("Vote", true, model.can_submit, Action::Submit) // JwkNU4
        };
        let button_id: &'static str = match action {
            Action::Remove => "poll-remove-vote",
            Action::ToggleResults => "poll-go-back",
            Action::Submit => "poll-vote",
        };
        let button = super::sticker_popout::small_button(button_id, label, primary)
            .w_auto()
            .min_w(px(60.))
            .when(!enabled, |this| this.opacity(0.5).cursor_default())
            .when(enabled, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| {
                    match action {
                        Action::Submit => this.poll_submit(channel, id, cx),
                        Action::Remove => this.poll_remove_vote(channel, id, cx),
                        Action::ToggleResults => this.poll_toggle_results(channel, id),
                    }
                    cx.notify();
                }))
            });
        Some(button.into_any_element())
    }

    /// One answer of the default layout (`F` in module 119591 inside `answerInner_D4-WfD`).
    #[allow(clippy::too_many_arguments)]
    fn poll_answer(
        &self,
        message: &Message,
        poll: &Poll,
        model: &Model,
        view: &View,
        index: usize,
        animate: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (channel, id) = (message.channel_id, message.id);
        let answer_id = poll.answer_ids[index];
        let selected = view.selected.contains(&answer_id);
        let look = look(model, poll, index);
        let did_vote = model.has_voted && poll.me_voted[index];
        let victor = model.expired && poll.is_leader(index);
        let currently_voting = model.interaction != Interaction::List;
        let (fill, outline) = match look {
            Look::Victor => (
                color("--polls-victor-fill"),
                Some(color("--status-positive")),
            ),
            Look::Voted => (
                color("--polls-voted-fill"),
                Some(color("--control-brand-foreground")),
            ),
            Look::Normal => (color("--interactive-background-active"), None),
        };
        let group = SharedString::from(format!("poll-answer-{id}-{answer_id}"));
        let percentage = poll.percentage(index);

        let votes = poll.counts[index];
        let votes_data = model.show_counts.then(|| {
            let label = votes_label(votes);
            let label = if model.can_tap {
                let button = text_button_small(
                    SharedString::from(format!("poll-answer-votes-{id}-{answer_id}")),
                    label,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.poll_open_voters(channel, id, answer_id, cx);
                }));
                // The names of the first voters over the count, when this session knows them (`L`, module 119591).
                let who = self.poll_voters_text(poll, index);
                if who.is_empty() {
                    button.into_any_element()
                } else {
                    self.with_tip(
                        format!("poll-voters-tip-{id}-{answer_id}"),
                        Tip::body(Body::Muted(who.into()), Side::Top),
                        button,
                        cx,
                    )
                    .into_any_element()
                }
            } else {
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .font_weight(FontWeight(600.))
                    .child(label)
                    .into_any_element()
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .whitespace_nowrap()
                .child(label)
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .font_weight(FontWeight(600.))
                        .child(format!("{percentage}%")),
                )
        });

        // The icon at the right end (`R`): radio, checkbox, or the tick of the account's own answer.
        let marker: Option<AnyElement> = match model.interaction {
            Interaction::RadioButtons => Some(radio(selected)),
            Interaction::Checkboxes => Some(checkbox(selected)),
            Interaction::List if did_vote => Some(vote_tick(24., victor, model.expired)),
            Interaction::List => None,
        };

        let bar = model.show_counts.then(|| {
            let width = bar_width((channel, id, answer_id), percentage, animate);
            // The bar is clipped by the row's 8px corners: the right ones show only near the full width.
            let right = (8. - (100. - width) * 3.).clamp(0., 8.);
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(relative(width / 100.))
                .rounded_tl(px(8.))
                .rounded_bl(px(8.))
                .rounded_tr(px(right))
                .rounded_br(px(right))
                .bg(fill)
        });

        let border = match (selected && currently_voting, outline) {
            (true, _) => color("--control-brand-foreground"),
            (false, Some(outline)) => outline,
            (false, None) => gpui::rgba(0x0000_0000),
        };
        let hover_border = if selected || outline.is_some() {
            border
        } else if currently_voting {
            color("--border-strong")
        } else {
            border
        };
        let row = div()
            .id(SharedString::from(format!("poll-answer-{id}-{answer_id}")))
            .group(group.clone())
            .relative()
            .w_full()
            .min_h(px(50.))
            .px(px(16.))
            .py(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .bg(color("--background-mod-muted"))
            .text_color(color("--text-default"))
            .when(currently_voting, |this| this.cursor_pointer())
            .children(bar)
            .children(
                poll.emoji[index]
                    .as_ref()
                    .and_then(|emoji| self.poll_emoji(emoji, 24.))
                    .map(|emoji| div().mr(px(2.)).child(emoji)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .font_weight(FontWeight(600.))
                    .child(poll.answers[index].clone()),
            )
            .children(votes_data)
            .children(marker.map(|marker| div().flex_none().child(marker)))
            // `outline: 1px solid var(--custom-poll-style-border)`: it takes no room and is drawn outside the
            // 50px box (golden `ab-rest`: the outline rows sit 1px outside the fill), so it is an overlay at -1px.
            .child(
                div()
                    .absolute()
                    .top(px(-1.))
                    .left(px(-1.))
                    .right(px(-1.))
                    .bottom(px(-1.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(border)
                    .group_hover(group, move |style| style.border_color(hover_border)),
            );
        if currently_voting {
            row.on_click(cx.listener(move |this, _, _, cx| {
                this.poll_tap_answer(channel, id, answer_id);
                cx.notify();
            }))
            .into_any_element()
        } else {
            row.into_any_element()
        }
    }

    /// The image-only layout: two columns of square tiles (`answersContainer_hdMKHE`).
    fn poll_image_answers(
        &self,
        message: &Message,
        poll: &Poll,
        model: &Model,
        view: &View,
        animate: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (channel, id) = (message.channel_id, message.id);
        let tiles: Vec<AnyElement> = (0..poll.answers.len())
            .map(|index| {
                let answer_id = poll.answer_ids[index];
                let selected = view.selected.contains(&answer_id);
                let look = look(model, poll, index);
                let did_vote = model.has_voted && poll.me_voted[index];
                let victor = model.expired && poll.is_leader(index);
                let currently_voting = model.interaction != Interaction::List;
                let (background, fill) = match look {
                    Look::Victor => (
                        color("--background-feedback-mod-warning"),
                        color("--polls-victor-fill"),
                    ),
                    Look::Voted => (
                        color("--background-feedback-mod-positive"),
                        color("--polls-voted-fill"),
                    ),
                    Look::Normal => (
                        color("--polls-normal-image-background"),
                        color("--interactive-background-active"),
                    ),
                };
                // `attachment_ids[0]` names one of the message's attachments.
                let picture = poll.attachments[index]
                    .first()
                    .and_then(|attachment| self.images.get(&format!("attachment:{attachment}")))
                    .cloned();
                let percentage = poll.percentage(index);
                let votes = poll.counts[index];
                let _ = bar_width((channel, id, answer_id), percentage, animate);
                let tile = div()
                    .id(SharedString::from(format!("poll-tile-{id}-{answer_id}")))
                    .relative()
                    .flex_none()
                    .w(px(TILE))
                    .h(px(TILE))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .bg(background)
                    .when(currently_voting, |this| {
                        this.cursor_pointer().hover(|style| {
                            style.shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.2),
                                offset: gpui::point(px(0.), px(1.)),
                                blur_radius: px(2.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                        })
                    })
                    .children(picture.map(|picture| {
                        div().absolute().size_full().child(
                            img(picture)
                                .size_full()
                                .rounded(px(4.))
                                .object_fit(gpui::ObjectFit::Cover),
                        )
                    }))
                    .children(poll.emoji[index].as_ref().and_then(|emoji| {
                        // `.emoji_hdMKHE`: 30% of the tile, centred.
                        self.poll_emoji(emoji, TILE * 0.3).map(|emoji| {
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(emoji)
                        })
                    }))
                    .children(selected.then(|| vote_tick(40., victor, model.expired)))
                    .children(model.show_counts.then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .right_0()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .px(px(6.))
                            .py(px(4.))
                            .rounded_tl(px(4.))
                            .rounded_br(px(4.))
                            .bg(fill)
                            .text_color(color("--text-default"))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight(600.))
                                    .child(format!("{percentage}%")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .font_weight(FontWeight(600.))
                                    .child(format!("({votes})")),
                            )
                    }))
                    .children(
                        (model.show_counts && ((!model.expired && did_vote) || victor)).then(
                            || {
                                div()
                                    .absolute()
                                    .top(px(-9.))
                                    .right(px(-9.))
                                    .child(vote_tick(28., victor, model.expired))
                            },
                        ),
                    );
                if currently_voting {
                    tile.on_click(cx.listener(move |this, _, _, cx| {
                        this.poll_tap_answer(channel, id, answer_id);
                        cx.notify();
                    }))
                    .into_any_element()
                } else {
                    tile.into_any_element()
                }
            })
            .collect();
        let mut rows = Vec::new();
        let mut tiles = tiles.into_iter();
        loop {
            let pair: Vec<AnyElement> = tiles.by_ref().take(2).collect();
            if pair.is_empty() {
                break;
            }
            let missing = 2 - pair.len();
            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(16.))
                    .children(pair)
                    .children((0..missing).map(|_| div().w(px(TILE)).flex_none()))
                    .into_any_element(),
            );
        }
        rows
    }

    // --------------------------------------------------------------------------------------------------------
    // Actions (module 180662)
    // --------------------------------------------------------------------------------------------------------

    /// `handlePollAnswerTapped`: a radio replaces the choice, a checkbox toggles one.
    fn poll_tap_answer(&mut self, channel: Id, message: Id, answer: u64) {
        let multiselect = self
            .stores
            .messages
            .get(&channel)
            .and_then(|list| list.iter().find(|known| known.id == message))
            .and_then(|known| known.extra.poll.as_ref())
            .is_some_and(|poll| poll.multiselect);
        let view = self.polls.views.entry((channel, message)).or_default();
        view.show_results = false;
        if let Some(position) = view.selected.iter().position(|known| *known == answer) {
            view.selected.remove(position);
        } else {
            if !multiselect {
                view.selected.clear();
            }
            view.selected.push(answer);
        }
    }

    /// `showVotes`: the results without a vote, and back.
    fn poll_toggle_results(&mut self, channel: Id, message: Id) {
        let view = self.polls.views.entry((channel, message)).or_default();
        view.selected.clear();
        view.show_results = !view.show_results;
    }

    /// "Vote": the ticked answers go to the server and show at once.
    fn poll_submit(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        let answers = self
            .polls
            .views
            .get(&(channel, message))
            .map(|view| view.selected.clone())
            .unwrap_or_default();
        self.poll_send_votes(channel, message, answers, cx);
    }

    /// "Remove Vote": the same call with no answers.
    fn poll_remove_vote(&mut self, channel: Id, message: Id, cx: &mut Context<Self>) {
        self.poll_send_votes(channel, message, Vec::new(), cx);
    }

    fn poll_send_votes(
        &mut self,
        channel: Id,
        message: Id,
        answers: Vec<u64>,
        cx: &mut Context<Self>,
    ) {
        let before = self.stores.poll_set_my_votes(channel, message, &answers);
        let view = self.polls.views.entry((channel, message)).or_default();
        view.selected.clear();
        view.submitting = true;
        view.before = before;
        // `answer_ids` are the strings the selection set holds (`String(answer_id)`).
        let _ = self.requests.send(Request::Rest {
            key: Some(format!("{VOTE_KEY}{channel}:{message}")),
            method: reqwest::Method::PUT,
            route: format!("/channels/{channel}/polls/{message}/answers/@me"),
            body: Some(
                json!({"answer_ids": answers.iter().map(u64::to_string).collect::<Vec<_>>()}),
            ),
        });
        cx.notify();
    }

    /// The server answered a vote: forget the card's state, or take the vote back and say so.
    pub(super) fn poll_vote_answered(
        &mut self,
        key: &str,
        body: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let Some((channel, message)) = key
            .strip_prefix(VOTE_KEY)
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(channel, message)| {
                Some((channel.parse::<Id>().ok()?, message.parse::<Id>().ok()?))
            })
        else {
            return;
        };
        let refused = body["refused"].as_bool().unwrap_or(false);
        if body["ok"].as_bool().unwrap_or(false) {
            // `(0, L.A2)(t, n, () => void 0)`: the card's state is dropped.
            self.polls.views.remove(&(channel, message));
        } else {
            if let Some(before) = self
                .polls
                .views
                .get(&(channel, message))
                .and_then(|view| view.before.clone())
            {
                self.stores.poll_set_my_votes(channel, message, &before);
            }
            if let Some(view) = self.polls.views.get_mut(&(channel, message)) {
                view.submitting = false;
                view.before = None;
            }
            // `getAnyErrorMessage() ?? message ?? "Seems like something's gone wrong. Sorry about that."` (eAn6z2)
            let text = body["body"]["message"]
                .as_str()
                .filter(|_| !refused)
                .unwrap_or("Seems like something's gone wrong. Sorry about that.")
                .to_owned();
            self.polls.error = Some(text);
        }
        cx.notify();
    }

    /// `P` in module 180662: the voters dialog opens on an answer.
    pub(super) fn poll_open_voters(
        &mut self,
        channel: Id,
        message: Id,
        answer: u64,
        cx: &mut Context<Self>,
    ) {
        self.polls.voters = Some(Voters {
            channel,
            message,
            answer,
        });
        self.request_image("poll-empty".to_owned(), EMPTY_IMAGE.to_owned());
        self.poll_load_voters(cx);
    }

    fn poll_voters_key(channel: Id, message: Id, answer: u64) -> String {
        let _ = channel;
        format!("{VOTERS_PREFIX}{message}:{answer}")
    }

    fn poll_load_voters(&mut self, cx: &mut Context<Self>) {
        let Some(voters) = self.polls.voters.clone() else {
            return;
        };
        let count = self
            .stores
            .messages
            .get(&voters.channel)
            .and_then(|list| list.iter().find(|known| known.id == voters.message))
            .and_then(|known| known.extra.poll.as_ref())
            .and_then(|poll| poll.counts.get(poll.index_of(voters.answer)?).copied())
            .unwrap_or(0);
        if count > 0 {
            let _ = self.requests.send(Request::Fetch {
                key: Self::poll_voters_key(voters.channel, voters.message, voters.answer),
                // `GET /channels/:c/polls/:m/answers/:id` with `limit` 100 and `type` 2 (module 649963 `ao`).
                route: format!(
                    "/channels/{}/polls/{}/answers/{}?limit=100&type=2",
                    voters.channel, voters.message, voters.answer
                ),
            });
        }
        cx.notify();
    }

    /// The people of a voters list: their avatars.
    pub(super) fn poll_voters_fetched(&mut self, key: &str) {
        let users: Vec<User> = self.fetched[key]["users"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(User::from_value)
            .collect();
        // The list also feeds the names over an answer's vote count (`ReactionStore`'s VOTE reactions).
        let target = key
            .strip_prefix(VOTERS_PREFIX)
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(message, answer)| {
                Some((message.parse::<Id>().ok()?, answer.parse::<u64>().ok()?))
            });
        let channel = self.polls.voters.as_ref().map(|voters| voters.channel);
        if let (Some((message, answer)), Some(channel)) = (target, channel) {
            let ids: Vec<Id> = users.iter().map(|user| user.id).collect();
            self.stores
                .poll_voters_loaded(channel, message, answer, &ids);
        }
        for user in &users {
            self.stores
                .users
                .entry(user.id)
                .or_insert_with(|| user.clone());
        }
        for user in users {
            let (image, url) = avatar_source(&user);
            self.request_image(image, url);
        }
    }

    /// "End Poll Now" in the message menu of the author: ask first, then `POST .../expire`.
    pub(super) fn poll_end_ask(&mut self, channel: Id, message: Id) {
        self.polls.end = Some((channel, message));
    }

    fn poll_end_confirmed(&mut self, channel: Id, message: Id) {
        let _ = self.requests.send(Request::Rest {
            key: None,
            method: reqwest::Method::POST,
            route: format!("/channels/{channel}/polls/{message}/expire"),
            body: None,
        });
    }

    /// The dialogs of the poll: the voters list, "End Poll Now?", and the error alert.
    pub(super) fn poll_modals(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some(modal) = self.poll_voters_modal(cx) {
            out.push(modal);
        }
        if let Some((channel, message)) = self.polls.end {
            out.push(modal::render_small(
                modal::Spec {
                    id: "poll-end-modal",
                    title: "End Poll Now?".into(), // +rfkTK
                    subtitle: Some(
                        div()
                            .child("This will close the poll immediately and reveal the results.") // H2I1gL
                            .into_any_element(),
                    ),
                    body: None,
                    buttons: vec![
                        modal::button(
                            "poll-end-cancel",
                            "Cancel",
                            modal::Tone::Secondary,
                            |this, _, _| {
                                this.polls.end = None;
                            },
                        ),
                        modal::button(
                            "poll-end-okay",
                            "Okay",
                            modal::Tone::Primary,
                            move |this, _, _| {
                                this.polls.end = None;
                                this.poll_end_confirmed(channel, message);
                            },
                        ),
                    ],
                    close: Box::new(|this, _, _| this.polls.end = None),
                },
                cx,
            ));
        }
        if let Some(text) = self.polls.error.clone() {
            out.push(modal::render_small(
                modal::Spec {
                    id: "poll-error-modal",
                    title: "Oh no! Something went wrong.".into(), // iufib1
                    subtitle: Some(div().child(text).into_any_element()),
                    body: None,
                    buttons: vec![modal::button(
                        "poll-error-okay",
                        "Okay",
                        modal::Tone::Primary,
                        |this, _, _| this.polls.error = None,
                    )],
                    close: Box::new(|this, _, _| this.polls.error = None),
                },
                cx,
            ));
        }
        out
    }

    /// `PollVotesModal` (module 637806): the answers down the left, who picked the one on show on the right.
    fn poll_voters_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let voters = self.polls.voters.clone()?;
        let message = self
            .stores
            .messages
            .get(&voters.channel)?
            .iter()
            .find(|known| known.id == voters.message)?;
        let poll = message.extra.poll.as_ref()?;
        let total = poll.total_votes();
        let me: Option<Id> = self.stores.current_user.as_ref().map(|user| user.id);
        let _ = me;
        let (channel, message_id) = (voters.channel, voters.message);

        let rows: Vec<AnyElement> = (0..poll.answers.len())
            .filter_map(|index| {
                let answer = *poll.answer_ids.get(index)?;
                let picked = answer == voters.answer;
                let count = poll.counts.get(index).copied().unwrap_or(0);
                Some(
                    div()
                        .id(SharedString::from(format!("poll-voter-tab-{answer}")))
                        // `voteDefault_S9aZ6q`: 187 wide, 40 high, 8px in, 4px apart (golden `b-voters-modal`).
                        .w(px(187.))
                        .h(px(40.))
                        .flex_none()
                        .px(px(8.))
                        .rounded(px(4.))
                        .overflow_hidden()
                        .flex()
                        .flex_row()
                        .items_center()
                        .when(picked, |this| this.bg(color("--background-base-lowest")))
                        .when(!picked, |this| {
                            this.cursor_pointer()
                                .hover(|style| style.bg(color("--interactive-background-hover")))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.polls.voters = Some(Voters {
                                        channel,
                                        message: message_id,
                                        answer,
                                    });
                                    this.poll_load_voters(cx);
                                }))
                        })
                        .children(poll.emoji.get(index).and_then(Option::as_ref).and_then(
                            |emoji| {
                                // `padding-inline: 4px 8px; padding-block: 4px`, 24 square.
                                self.poll_emoji(emoji, 16.)
                                    .map(|emoji| div().p(px(4.)).pr(px(8.)).child(emoji))
                            },
                        ))
                        .child(
                            // The count follows the name directly (`lineClamp1` then a 2px margin).
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(poll.answers[index].clone()),
                        )
                        .child(
                            div()
                                .ml(px(2.))
                                .text_size(px(16.))
                                .line_height(px(20.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-default"))
                                .child(format!("({count})")),
                        )
                        .into_any_element(),
                )
            })
            .collect();

        let answer_count = poll
            .index_of(voters.answer)
            .and_then(|index| poll.counts.get(index).copied())
            .unwrap_or(0);
        let users: Vec<User> = self
            .fetched
            .get(&Self::poll_voters_key(channel, message_id, voters.answer))
            .and_then(|body| body["users"].as_array())
            .into_iter()
            .flatten()
            .filter_map(User::from_value)
            .collect();
        let panel: AnyElement = if answer_count > 0 && !users.is_empty() {
            div()
                .id("poll-voters-list")
                .flex_1()
                .px(px(24.))
                .pt(px(12.))
                .overflow_y_scroll()
                .children(users.iter().map(|user| self.poll_voter_row(user, cx)))
                .into_any_element()
        } else if answer_count > 0 {
            div().flex_1().into_any_element()
        } else {
            let picture = self.images.get("poll-empty").cloned();
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(106.))
                        .h(px(100.))
                        .mb(px(10.))
                        .children(picture.map(|picture| img(picture).w(px(106.)).h(px(100.)))),
                )
                .child(
                    div()
                        .text_size(px(20.))
                        .line_height(px(24.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-strong"))
                        .child("Nothing to see here"), // vhQK3o
                )
                .child(
                    div()
                        .mt(px(5.))
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-muted"))
                        .child("There are no votes for this answer"), // bwytdh
                )
                .into_any_element()
        };

        // `.spacer` rule under the title, then `.body`: a row at most 640 tall, 24px in from the left; the answers
        // column is 220 wide with a rule on its right.
        let body = div()
            .size_full()
            .flex()
            .flex_col()
            // `spacer_S9aZ6q`: 17px with the rule at its bottom edge.
            .child(
                div()
                    .flex_none()
                    .h(px(17.))
                    .border_b_1()
                    .border_color(color("--border-subtle")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .pl(px(24.))
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .id("poll-voter-answers")
                            .mt(px(16.))
                            .w(px(220.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .border_r_1()
                            .border_color(color("--border-subtle"))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .child(panel),
            )
            .into_any_element();
        // `outerContainer_dvN-MP` pads 56px at the top; the 680 wide container fills 720 - 2 * 56.
        Some(modal::render_pinned(
            modal::Spec {
                id: "poll-voters-modal",
                title: poll.question.clone().into(),
                subtitle: Some(div().child(votes_label(total)).into_any_element()),
                body: Some(body),
                buttons: Vec::new(),
                close: Box::new(|this, _, _| this.polls.voters = None),
            },
            680.,
            56.,
            608.,
            cx,
        ))
    }

    /// One voter: avatar, name and username in a 52px row (`.voter_S9aZ6q`); a click opens the profile.
    fn poll_voter_row(&self, user: &User, cx: &mut Context<Self>) -> AnyElement {
        let avatar = self.images.get(&avatar_source(user).0).cloned();
        let user_id = user.id;
        div()
            .id(SharedString::from(format!("poll-voter-{}", user.id)))
            .h(px(52.))
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.polls.voters = None;
                this.open_full_profile(user_id, window, cx);
            }))
            .child(
                div()
                    .mr(px(8.))
                    .size(px(32.))
                    .flex_none()
                    .children(avatar.map(|image| img(image).size(px(32.)).rounded_full())),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    // Not in the pinned client: RoleColorEverywhere's `pollResults`
                    // (`plugin_hooks/role_color_everywhere.rs`).
                    .child(super::plugin_hooks::role_colored(
                        div().truncate().text_size(px(16.)).line_height(px(20.)),
                        self.role_color_everywhere(
                            super::plugin_hooks::RolePlace::PollResults,
                            user.id,
                        ),
                        color("--text-default").into(),
                        user.display_name().to_owned(),
                    ))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .line_height(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-muted"))
                            .child(user.username.clone()),
                    ),
            )
            .into_any_element()
    }
}

impl Root {
    /// "End Poll Now" is offered to the author of a poll whose time has not run out (module 761304: the entry is
    /// left out when `expiry.isSameOrBefore(now)` or the author is somebody else).
    pub(super) fn poll_endable(&self, message: &Message) -> bool {
        let me = self.stores.current_user.as_ref().map(|user| user.id);
        message
            .extra
            .poll
            .as_ref()
            .is_some_and(|poll| !poll.is_expired(chrono::Utc::now()))
            && me == Some(message.author.id)
    }

    /// Capture only: `poll demo <state>`, `poll create <question>|<answer>|...`, `poll delete`,
    /// `poll gateway <add|remove> <answer> [user]`, `poll state`, `poll voters <answer>`, `poll end`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_poll(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        let Some(channel) = self.channel else {
            return "poll: no channel is open".to_owned();
        };
        let polls_here = |this: &Self| -> Vec<Message> {
            this.stores
                .messages
                .get(&channel)
                .into_iter()
                .flatten()
                .filter(|message| message.extra.poll.is_some())
                .cloned()
                .collect()
        };
        match words {
            ["demo", state] => {
                let Some(value) = demo_poll(state, channel) else {
                    return format!("poll: no demo state {state}");
                };
                let list = self.stores.messages.entry(channel).or_default();
                list.retain(|message| !(message.author.id == 900 && message.extra.poll.is_some()));
                list.extend(Message::from_value(&value));
                let id = value["id"].as_str().and_then(|id| id.parse::<Id>().ok());
                if let Some(id) = id {
                    self.polls.views.remove(&(channel, id));
                    if *state == "results" {
                        self.polls
                            .views
                            .entry((channel, id))
                            .or_default()
                            .show_results = true;
                    }
                }
                self.request_images();
                self.rebuild_rows();
                cx.notify();
                format!("poll: demo {state} in {channel}")
            }
            ["create", rest @ ..] => {
                let text = rest.join(" ");
                let mut parts: Vec<String> = text.split('|').map(str::to_owned).collect();
                let multiple = parts.iter().any(|part| part.trim() == "multi");
                parts.retain(|part| part.trim() != "multi");
                // `hours=N` sets the duration (default 24); an answer written `e:<emoji> <text>` carries that emoji
                // (`poll_media.emoji.name`).
                let hours = parts
                    .iter()
                    .position(|part| part.trim().starts_with("hours="))
                    .map(|at| parts.remove(at))
                    .and_then(|part| part.trim()["hours=".len()..].parse::<u32>().ok())
                    .unwrap_or(24);
                if parts.len() < 2 {
                    return "poll: usage `poll create <question>|<answer>|<answer>[|multi][|hours=N]`"
                        .to_owned();
                }
                let question = parts.remove(0);
                let emoji: Vec<Option<String>> = parts
                    .iter_mut()
                    .map(|part| {
                        let trimmed = part.trim().to_owned();
                        let rest = trimmed.strip_prefix("e:")?;
                        let (emoji, text) = rest.split_once(' ')?;
                        *part = text.to_owned();
                        Some(emoji.to_owned())
                    })
                    .collect();
                let Some(mut body) =
                    super::composer_plus::poll_body(&question, &parts, multiple, hours)
                else {
                    return "poll: empty question or answers".to_owned();
                };
                for (at, emoji) in emoji.iter().enumerate() {
                    if let Some(emoji) = emoji {
                        body["poll"]["answers"][at]["poll_media"]["emoji"] = json!({"name": emoji});
                    }
                }
                let _ = self.requests.send(Request::Rest {
                    key: None,
                    method: reqwest::Method::POST,
                    route: format!("/channels/{channel}/messages"),
                    body: Some(body),
                });
                format!("poll: create sent to {channel}")
            }
            ["delete", target] => {
                let Ok(target) = target.parse::<Id>() else {
                    return "poll: delete needs a message id".to_owned();
                };
                let _ = self.requests.send(Request::Delete {
                    channel,
                    message: target,
                });
                format!("poll: delete sent for {target}")
            }
            ["delete"] => {
                let me = self.stores.current_user.as_ref().map(|user| user.id);
                let mine: Vec<Id> = polls_here(self)
                    .iter()
                    .filter(|message| Some(message.author.id) == me)
                    .map(|message| message.id)
                    .collect();
                for message in &mine {
                    let _ = self.requests.send(Request::Delete {
                        channel,
                        message: *message,
                    });
                }
                format!("poll: delete sent for {}", mine.len())
            }
            ["gateway", kind, answer, rest @ ..] => {
                let Some(message) = polls_here(self).last().cloned() else {
                    return "poll: no poll in the channel".to_owned();
                };
                let user = rest.first().copied().unwrap_or("901");
                let name = match *kind {
                    "add" => "MESSAGE_POLL_VOTE_ADD",
                    _ => "MESSAGE_POLL_VOTE_REMOVE",
                };
                self.stores.dispatch(
                    name,
                    &json!({"channel_id": channel.to_string(), "message_id": message.id.to_string(),
                        "user_id": user, "answer_id": answer.parse::<u64>().unwrap_or(1)}),
                );
                cx.notify();
                format!("poll: {name} answer {answer} user {user}")
            }
            ["jump", target] => {
                let Ok(target) = target.parse::<Id>() else {
                    return "poll: jump needs a message id".to_owned();
                };
                self.jump_to_message_id(channel, target, false, None);
                cx.notify();
                format!("poll: jump to {target}")
            }
            ["results"] => {
                let lines: Vec<String> = self
                    .stores
                    .messages
                    .get(&channel)
                    .into_iter()
                    .flatten()
                    .filter(|message| message.kind == 46)
                    .map(|message| {
                        let fields: Vec<String> = message
                            .embeds
                            .iter()
                            .flat_map(|embed| embed.fields.iter())
                            .map(|field| format!("{}={}", field.name, field.value))
                            .collect();
                        format!(
                            "{} embeds={} [{}] mentions={} ids={:?} flags={} everyone={}",
                            message.id,
                            message.embeds.len(),
                            fields.join(", "),
                            message.mentions,
                            message.mention_user_ids,
                            message.flags,
                            message.mention_everyone
                        )
                    })
                    .collect();
                format!("poll: results {}", lines.join(" | "))
            }
            ["state"] => {
                let lines: Vec<String> = polls_here(self)
                    .iter()
                    .filter_map(|message| {
                        let poll = message.extra.poll.as_ref()?;
                        let view = self.poll_view(message);
                        Some(format!(
                            "{} counts={:?} me={:?} expired={} selected={:?} submitting={} results={}",
                            message.id,
                            poll.counts,
                            poll.my_votes(),
                            poll.is_expired(chrono::Utc::now()),
                            view.selected,
                            view.submitting,
                            view.show_results
                        ))
                    })
                    .collect();
                format!("poll: {}", lines.join(" | "))
            }
            ["voters", answer] => {
                let Some(message) = polls_here(self).last().cloned() else {
                    return "poll: no poll in the channel".to_owned();
                };
                self.poll_open_voters(channel, message.id, answer.parse().unwrap_or(1), cx);
                format!("poll: voters {answer}")
            }
            ["end"] => {
                let Some(message) = polls_here(self).last().cloned() else {
                    return "poll: no poll in the channel".to_owned();
                };
                self.poll_end_ask(channel, message.id);
                cx.notify();
                "poll: end asked".to_owned()
            }
            _ => "poll: usage `poll demo|create|delete|gateway|state|voters|end`".to_owned(),
        }
    }
}

/// What a demo poll is made of: its answers, their votes, which one this account picked, whether several may be
/// picked, the hours to its expiry (negative: closed) and the layout.
#[cfg(feature = "capture")]
type DemoPoll = (Vec<serde_json::Value>, Vec<u64>, Vec<bool>, bool, i64, u64);

/// A poll message in a state the fixture server cannot always hold: `radio`, `multi`, `voted`, `results`,
/// `closed`, `tie`, `none`, `long`, `emoji`, `image`.
#[cfg(feature = "capture")]
fn demo_poll(state: &str, channel: Id) -> Option<serde_json::Value> {
    let now = chrono::Utc::now();
    let id = (((now.timestamp_millis() - 1_420_070_400_000) as u64) << 22) + 7;
    let days = || plain(&["Friday", "Saturday", "Sunday", "Monday"]);
    let emoji = |id: u64, name: &str, text: Option<&str>| {
        let mut media = json!({"emoji": {"name": name}});
        if let Some(text) = text {
            media["text"] = json!(text);
        }
        json!({"answer_id": id, "poll_media": media})
    };
    let demo: DemoPoll = match state {
        "radio" => (days(), vec![3, 5, 1, 0], vec![false; 4], false, 26, 1),
        "multi" => (
            plain(&["Pizza", "Tacos", "Sushi"]),
            vec![2, 4, 1],
            vec![false; 3],
            true,
            3,
            1,
        ),
        "voted" => (
            days(),
            vec![3, 5, 1, 0],
            vec![false, true, false, false],
            false,
            26,
            1,
        ),
        "results" => (days(), vec![3, 5, 1, 0], vec![false; 4], false, 26, 1),
        "closed" => (
            days(),
            vec![3, 5, 1, 0],
            vec![false, false, true, false],
            false,
            -2,
            1,
        ),
        "tie" => (
            plain(&["Friday", "Saturday", "Sunday"]),
            vec![4, 4, 1],
            vec![true, false, false],
            false,
            -2,
            1,
        ),
        "none" => (
            plain(&["Yes", "No"]),
            vec![0, 0],
            vec![false; 2],
            false,
            1,
            1,
        ),
        "long" => (
            plain(&[
                "A very long answer that has to wrap onto a second line because it does not fit in the card at all",
                "Short",
            ]),
            vec![12, 7],
            vec![false, false],
            false,
            50,
            1,
        ),
        "emoji" => (
            vec![
                emoji(1, "\u{1F389}", Some("Party")),
                emoji(2, "\u{1F634}", Some("Quiet")),
            ],
            vec![6, 2],
            vec![false, true],
            false,
            30,
            1,
        ),
        "image" => (
            vec![
                emoji(1, "\u{1F436}", None),
                emoji(2, "\u{1F431}", None),
                emoji(3, "\u{1F430}", None),
                emoji(4, "\u{1F43B}", None),
            ],
            vec![3, 2, 0, 1],
            vec![false; 4],
            false,
            20,
            2,
        ),
        _ => return None,
    };
    let (answers, counts, me, multi, hours, layout) = demo;
    let counted: Vec<serde_json::Value> = counts
        .iter()
        .zip(&me)
        .enumerate()
        .map(|(index, (count, me))| json!({"id": index + 1, "count": count, "me_voted": me}))
        .collect();
    let question = match state {
        "long" => "Which of these long options would you rather pick for the weekend plan?",
        "image" => "Pick a pet",
        "emoji" => "Party or quiet?",
        _ => "Which day for the stream?",
    };
    Some(json!({
        "id": id.to_string(),
        "channel_id": channel.to_string(),
        "type": 0,
        "content": "",
        "timestamp": (now - chrono::Duration::seconds(120)).to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
        "author": {"id": "900", "username": "ada"},
        "poll": {
            "question": {"text": question},
            "answers": answers,
            "allow_multiselect": multi,
            "layout_type": layout,
            "expiry": (now + chrono::Duration::hours(hours)).to_rfc3339_opts(chrono::SecondsFormat::Micros, false),
            "results": {"is_finalized": hours < 0, "answer_counts": counted},
        },
    }))
}

#[cfg(feature = "capture")]
fn plain(texts: &[&str]) -> Vec<serde_json::Value> {
    texts
        .iter()
        .enumerate()
        .map(|(index, text)| json!({"answer_id": index + 1, "poll_media": {"text": text}}))
        .collect()
}

#[derive(Clone, Copy)]
enum Action {
    Submit,
    Remove,
    ToggleResults,
}

/// The design system text button, `secondary` variant: `--text-strong`, 14px medium, underlined on hover.
fn text_button(id: SharedString, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_size(px(14.))
        .line_height(px(18.))
        .font_weight(FontWeight(500.))
        .text_color(color("--text-strong"))
        .hover(|style| style.underline())
        .child(label.into())
}

/// The same at 12px semibold, for the vote count inside an answer.
fn text_button_small(
    id: SharedString,
    label: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_size(px(12.))
        .line_height(px(16.))
        .font_weight(FontWeight(600.))
        .text_color(color("--text-strong"))
        .hover(|style| style.underline())
        .child(label.into())
}

/// The radio of an answer (`k$` and `A` in module 306417): the 24px ring, with a `--control-brand-foreground` dot
/// when chosen.
fn radio(selected: bool) -> AnyElement {
    div()
        .relative()
        .size(px(24.))
        .child(
            svg()
                .path("icons/safety_radio_off.svg")
                .size(px(24.))
                .text_color(color("--text-default")),
        )
        .children(selected.then(|| {
            // The `r="5"` circle of the 24-unit icon.
            div()
                .absolute()
                .top(px(7.))
                .left(px(7.))
                .size(px(10.))
                .rounded_full()
                .bg(color("--control-brand-foreground"))
        }))
        .into_any_element()
}

/// The checkbox (`e5`): 20px, a 2px `--text-default` border and 3px corners; chosen it is `--brand-500` with a white
/// tick at 85% of its size.
fn checkbox(selected: bool) -> AnyElement {
    let base = div()
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.));
    if selected {
        base.bg(color("--brand-500"))
            .child(
                svg()
                    .path("icons/checkmark_large.svg")
                    .size(px(17.))
                    .text_color(color("--white")),
            )
            .into_any_element()
    } else {
        base.border_2()
            .border_color(color("--text-default"))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn poll(multiselect: bool) -> Poll {
        let mut poll = Poll {
            question: "Q".into(),
            answers: vec!["a".into(), "b".into(), "c".into()],
            multiselect,
            expiry: Some(Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap()),
            counts: vec![2, 1, 0],
            me_voted: vec![false, false, false],
            ..Poll::default()
        };
        poll.normalize();
        poll
    }

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 11, 0, 0, 0).unwrap()
    }

    #[test]
    fn an_open_poll_before_voting_takes_clicks() {
        let poll = poll(false);
        let view = View::default();
        let model = model(&poll, &view, now());
        assert_eq!(model.interaction, Interaction::RadioButtons);
        assert!(!model.show_counts && !model.can_submit && !model.expired);
        let multi = self::model(&self::poll(true), &view, now());
        assert_eq!(multi.interaction, Interaction::Checkboxes);
        // Something ticked: Vote is on.
        let ticked = View {
            selected: vec![2],
            ..View::default()
        };
        assert!(self::model(&poll, &ticked, now()).can_submit);
        // A vote on its way: Vote is off.
        let sending = View {
            selected: vec![2],
            submitting: true,
            ..View::default()
        };
        assert!(!self::model(&poll, &sending, now()).can_submit);
    }

    #[test]
    fn show_results_and_voting_turn_answers_into_a_list() {
        let mut poll = poll(false);
        let results = View {
            show_results: true,
            ..View::default()
        };
        let model_results = model(&poll, &results, now());
        assert!(model_results.show_counts && !model_results.has_voted);
        assert_eq!(model_results.interaction, Interaction::List);
        poll.set_my_votes(&[1]);
        let voted = model(&poll, &View::default(), now());
        assert!(voted.has_voted && voted.can_remove && voted.show_counts);
        assert_eq!(voted.interaction, Interaction::List);
        assert_eq!(look(&voted, &poll, 0), Look::Voted);
        assert_eq!(look(&voted, &poll, 1), Look::Normal);
    }

    #[test]
    fn a_closed_poll_marks_its_victors_and_hides_the_buttons() {
        let poll = poll(false);
        let later = Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap();
        let closed = model(&poll, &View::default(), later);
        assert!(closed.expired && closed.show_counts && !closed.can_remove);
        assert_eq!(look(&closed, &poll, 0), Look::Victor);
        assert_eq!(look(&closed, &poll, 1), Look::Normal);
        assert_eq!(closed.time_left, None);
    }

    #[test]
    fn the_bar_easing_runs_from_zero_to_one_and_is_symmetric() {
        let stops = bar_stops();
        assert!((stops.last().unwrap().1 - 1.).abs() < 1e-6);
        let half = stops.iter().find(|(at, _)| *at >= 0.5).unwrap().1;
        assert!((half - 0.5).abs() < 0.01);
        // Slow start: at a quarter of the time half of ease(0.5) = 0.315 has been covered, about 0.158.
        let quarter = stops.iter().find(|(at, _)| *at >= 0.25).unwrap().1;
        assert!(quarter > 0.1 && quarter < 0.2, "{quarter}");
    }

    #[test]
    fn vote_labels_pluralise() {
        assert_eq!(votes_label(0), "0 votes");
        assert_eq!(votes_label(1), "1 vote");
        assert_eq!(votes_label(12), "12 votes");
    }

    #[test]
    fn ticks_follow_radio_and_checkbox_rules() {
        let mut ui = PollUi::default();
        // Radio: choosing a second answer replaces the first; checkbox keeps both and toggles.
        let tap = |ui: &mut PollUi, multi: bool, answer: u64| {
            let view = ui.views.entry((1, 2)).or_default();
            if let Some(position) = view.selected.iter().position(|known| *known == answer) {
                view.selected.remove(position);
            } else {
                if !multi {
                    view.selected.clear();
                }
                view.selected.push(answer);
            }
        };
        tap(&mut ui, false, 1);
        tap(&mut ui, false, 2);
        assert_eq!(ui.selected(1, 2), [2]);
        tap(&mut ui, true, 3);
        assert_eq!(ui.selected(1, 2), [2, 3]);
        tap(&mut ui, true, 2);
        assert_eq!(ui.selected(1, 2), [3]);
    }
}
