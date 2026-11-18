//! The event prompts over a voice channel's call controls (module 394412 with the card of module 47868, in
//! `eventPromptsContainer_9bvEB0` of module 105225): each upcoming event of the channel that is not active, starts
//! within the next fifteen minutes (or is overdue) and was not dismissed. A manager of the event reads
//! `Start Event: "name"` and opens the Start Event dialog; anyone else reads `Upcoming Event: "name"` and opens the
//! event. "Scheduled for {when}" and "• N Interested" follow; the close button dismisses the prompt for good
//! (`EVENT_BANNER_DISMISS`, kept by EventBannerStore under `EventBanner`). Nothing shows while the channel has an
//! active event (`Qs` of module 508654). Sizes from `container__B7Y3X` (400 wide, 16px padding, 12px corners,
//! `--opacity-black-52`, darker on hover); no capture shows this prompt.
//!
//! A Stage channel shows them too: the waiting view (module 895903) lists them first among its cards with
//! `showDismiss: false` (a chevron in place of the close button, as module 47868 draws a card without `onDismiss`)
//! while the Stage is not live.
//!
//! GAP: the Stage controls' `eventPrompts_Z_HbQM` holds them for a speaker or a member who can mute while the Stage
//! is joined but not live and the start screen is left; this client shows the waiting view until the Stage is live.

use super::super::{Root, color, icon};
use super::list::can_manage;
use super::menu::Confirm;
use chrono::{DateTime, Datelike, Local, Utc};
use dc_state::{Id, scheduled_events::ScheduledEvent};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use serde_json::{Value, json};

/// EventBannerStore's storage key.
const STORE_KEY: &str = "EventBanner";
/// `f` of module 508654 and `withinStartWindow` of 974930: fifteen minutes.
const WINDOW_MINUTES: i64 = 15;
/// `GuildScheduledEventStatus.ACTIVE`.
const ACTIVE: u64 = 2;

/// `isEventDismissed`.
fn dismissed(event: Id) -> bool {
    crate::storage::get(STORE_KEY)
        .is_some_and(|value| !value["dismissedEventIds"][event.to_string()].is_null())
}

/// `EVENT_BANNER_DISMISS`.
fn dismiss(event: Id) {
    let mut value = crate::storage::get(STORE_KEY).unwrap_or_else(|| json!({}));
    if !value["dismissedEventIds"].is_object() {
        value["dismissedEventIds"] = json!({});
    }
    value["dismissedEventIds"][event.to_string()] = Value::Bool(true);
    crate::storage::set(STORE_KEY, value);
}

/// `RO` of module 508654: an event that is not active, whose start is at most fifteen minutes away.
pub(super) fn wants_prompt(event: &ScheduledEvent, now: DateTime<Utc>) -> bool {
    let Some(start) = event.start() else {
        return false;
    };
    let minutes = (start - now).num_minutes();
    event.status != ACTIVE
        && start <= now + chrono::Duration::minutes(WINDOW_MINUTES)
        && minutes < WINDOW_MINUTES
}

/// `startDateTimeString` of `CC` (module 974930): today or tomorrow by the calendar, else the weekday and date.
pub(super) fn start_line(start: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (start.date_naive() - now.date_naive()).num_days();
    let time = start.format("%-I:%M %p");
    if !(0..=1).contains(&days) {
        let day = start.day();
        let suffix = match (day % 10, day % 100) {
            (1, n) if n != 11 => "st",
            (2, n) if n != 12 => "nd",
            (3, n) if n != 13 => "rd",
            _ => "th",
        };
        if start.year() == now.year() {
            format!("{} {day}{suffix} \u{b7} {time}", start.format("%a %b"))
        } else {
            format!(
                "{} {day}{suffix}, {} \u{b7} {time}",
                start.format("%a %b"),
                start.year()
            )
        }
    } else if days < 1 {
        format!("Today at {time}")
    } else {
        format!("Tomorrow at {time}")
    }
}

impl Root {
    /// The events whose prompt shows over this channel's controls, soonest first.
    fn prompt_events(&self, guild: Id, channel: Id) -> Vec<ScheduledEvent> {
        let events = &self.stores.scheduled_events;
        let in_channel = |event: &&ScheduledEvent| event.channel_id == Some(channel);
        if events.active(guild).iter().any(in_channel) {
            return Vec::new();
        }
        let now = Utc::now();
        let mut shown: Vec<ScheduledEvent> = events
            .upcoming(guild)
            .into_iter()
            .filter(in_channel)
            .filter(|event| wants_prompt(event, now) && !dismissed(event.id))
            .cloned()
            .collect();
        shown.sort_by_key(|event| event.start());
        shown
    }

    /// The prompts, 8px apart, their bottom 104px over the tray's (`eventPromptsContainer_9bvEB0`).
    pub(in crate::ui) fn event_prompts(
        &self,
        guild: Id,
        channel: Id,
        opacity: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let cards = self.event_prompt_cards(guild, channel, true, cx);
        if cards.is_empty() {
            return None;
        }
        // `eventPrompt_m5zWGJ`: 8px over each card.
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(16. + 104.))
                .opacity(opacity)
                .pt(px(8.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .children(cards)
                .into_any_element(),
        )
    }

    /// Module 394412's cards for one channel, soonest first; `show_dismiss` false (the Stage waiting view) ends each
    /// with a chevron instead of the close button.
    pub(in crate::ui) fn event_prompt_cards(
        &self,
        guild: Id,
        channel: Id,
        show_dismiss: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if guild == 0 {
            return Vec::new();
        }
        let manage = |event: &ScheduledEvent| {
            self.stores
                .guild(guild)
                .zip(self.stores.me())
                .is_some_and(|(known, me)| can_manage(known, me, event))
        };
        self.prompt_events(guild, channel)
            .iter()
            .map(|event| self.event_prompt(event, manage(event), show_dismiss, cx))
            .collect()
    }

    fn event_prompt(
        &self,
        event: &ScheduledEvent,
        manage: bool,
        show_dismiss: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = event.id;
        let title = dc_intl::format(
            if manage { "1vGXqM" } else { "xMJyla" },
            &[("eventName", event.name.as_str().into())],
        );
        let when = event
            .start()
            .map(|start| start_line(start.with_timezone(&Local), Local::now()))
            .unwrap_or_default();
        let description = dc_intl::format("PTebCR", &[("startTime", when.into())]);
        let interested = self.stores.scheduled_events.get_user_count(id, None);
        let line = |text: String| {
            div()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(color("--text-strong"))
                .child(text)
        };
        div()
            .id(SharedString::from(format!("event-prompt-{id}")))
            .w(px(400.))
            .p(px(16.))
            .rounded(px(12.))
            .bg(color("--opacity-black-52"))
            .hover(|style| style.bg(color("--opacity-black-64")))
            .active(|style| style.bg(color("--opacity-black-48")))
            .cursor_pointer()
            .flex()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| this.event_prompt_clicked(id, manage, cx)))
            .child(div().p(px(6.)).rounded_full().flex().child(icon(
                "field_calendar",
                20.,
                "--text-strong",
            )))
            .child(
                div()
                    .ml(px(16.))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex()
                            .child(line(description))
                            .when(interested > 0, |this| {
                                this.child(div().mx(px(8.)).child(line("\u{2022}".to_owned())))
                                    .child(line(dc_intl::format(
                                        "NywdIj",
                                        &[("count", interested.into())],
                                    )))
                            }),
                    ),
            )
            .map(|this| {
                if show_dismiss {
                    this.child(
                        div()
                            .id(SharedString::from(format!("event-prompt-dismiss-{id}")))
                            .ml_auto()
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                dismiss(id);
                                cx.notify();
                            }))
                            .child(icon("modal_close", 16., "--interactive-text-default")),
                    )
                } else {
                    // `ChevronSmallRightIcon` (xs) at the end, as the waiting view's other cards end.
                    this.child(div().ml_auto().child(icon(
                        "inventory_chevron_right",
                        16.,
                        "--white",
                    )))
                }
            })
            .into_any_element()
    }

    /// A manager starts the event (the Start Event dialog); anyone else opens it in the Events modal.
    fn event_prompt_clicked(&mut self, event: Id, manage: bool, cx: &mut Context<Self>) {
        self.open_guild_events(cx);
        if manage {
            if let Some(open) = self.guild_events.as_mut() {
                open.confirm = Some(Confirm::Start(event));
            }
        } else {
            self.open_event_detail(event, cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn event(start: DateTime<Utc>, status: u64) -> ScheduledEvent {
        ScheduledEvent::from_value(&json!({
            "id": "1", "guild_id": "2", "channel_id": "3", "name": "Game night",
            "scheduled_start_time": start.to_rfc3339(), "status": status, "entity_type": 2,
            "privacy_level": 2,
        }))
        .unwrap()
    }

    #[test]
    fn only_events_starting_within_fifteen_minutes_or_overdue_and_not_active_prompt() {
        let now = Utc.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).unwrap();
        let minutes = |m: i64| now + chrono::Duration::minutes(m);
        assert!(wants_prompt(&event(minutes(10), 1), now));
        assert!(wants_prompt(&event(minutes(14), 1), now));
        // In the window, but `diffMinutes < 15` leaves the fifteenth minute out.
        assert!(!wants_prompt(&event(minutes(15), 1), now));
        assert!(wants_prompt(&event(minutes(-30), 1), now));
        assert!(!wants_prompt(&event(minutes(16), 1), now));
        assert!(!wants_prompt(&event(minutes(5), ACTIVE), now));
    }

    #[test]
    fn the_start_reads_like_moment_calendar() {
        let at = |day: u32, hour: u32| Local.with_ymd_and_hms(2026, 11, day, hour, 5, 0).unwrap();
        let now = at(18, 9);
        assert_eq!(start_line(at(18, 15), now), "Today at 3:05 PM");
        assert_eq!(start_line(at(19, 9), now), "Tomorrow at 9:05 AM");
        assert_eq!(start_line(at(21, 9), now), "Sat Nov 21st \u{b7} 9:05 AM");
    }
}
