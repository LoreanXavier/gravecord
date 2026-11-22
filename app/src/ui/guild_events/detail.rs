//! The detail view of an event (module 163765's neighbour in the Events modal): a 680 wide dialog with the tabs Event
//! Info and "N Interested", and the "...", Copy Link and Interested buttons in its action bar. Measured in
//! `event-detail-darker-cozy.json` and `event-detail-interested-darker-cozy.json`.

use super::super::{Root, color, icon, status_dot_sized};
use super::layer;
use super::list::{Detail, Size, Tab, event_status, interested_sentence, status_row};
use crate::parity::Parity;
use chrono::NaiveDateTime;
use dc_state::{Id, scheduled_events::ScheduledEvent};
use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px};

/// `er(4, ...)` of module 893110: occurrences come four at a time.
const SERIES_PAGE: usize = 4;

impl Root {
    /// Opens the detail view of an event on its Event Info tab and asks for its count and who is interested.
    pub(super) fn open_event_detail(&mut self, event: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) else {
            return;
        };
        let (series, series_more) = self.event_series_page(event, SERIES_PAGE);
        if let Some(open) = self.guild_events.as_mut() {
            open.detail = Some(Detail {
                event,
                tab: Tab::Info,
                recurrence: None,
                series,
                series_more,
            });
            open.menu = None;
        }
        self.event_fetch_counts(guild, event);
        self.event_fetch_users(guild, event);
        cx.notify();
    }

    /// The first `count` occurrences of a recurring event from its start, and whether another follows within four
    /// years (`er` and `canViewMoreRecurrences` of module 893110).
    fn event_series_page(
        &self,
        event: Id,
        count: usize,
    ) -> (Vec<chrono::DateTime<chrono::Utc>>, bool) {
        let Some(known) = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(event)
        else {
            return (Vec::new(), false);
        };
        let Some(first) = known.start().filter(|_| !known.recurrence_rule.is_null()) else {
            return (Vec::new(), false);
        };
        let rule = &known.recurrence_rule;
        let times = super::recurrence::occurrences(rule, first, false, count);
        let more = times
            .last()
            .is_some_and(|last| super::recurrence::has_more(rule, *last, chrono::Utc::now()));
        (times, more)
    }

    fn set_event_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let Some(open) = self.guild_events.as_mut() else {
            return;
        };
        let guild = open.guild;
        let Some(detail) = open.detail.as_mut() else {
            return;
        };
        detail.tab = tab;
        let event = detail.event;
        if tab == Tab::Interested {
            self.event_fetch_users(guild, event);
        }
        cx.notify();
    }

    /// The tabs: 16/20 medium labels 32px apart, a 2px rail under all of them and the brand bar under the current one.
    fn event_tabs(&self, current: Tab, count: i64, cx: &mut Context<Self>) -> AnyElement {
        let tab = |which: Tab, label: String, cx: &mut Context<Self>| {
            let selected = which == current;
            div()
                .id(match which {
                    Tab::Info => "event-tab-info",
                    Tab::Interested => "event-tab-interested",
                })
                .relative()
                .pb(px(16.))
                .cursor_pointer()
                .text_size(px(16.))
                .line_height(px(20.))
                .font_weight(FontWeight(500.))
                .text_color(color(if selected {
                    "--text-strong"
                } else {
                    "--interactive-text-default"
                }))
                .child(label)
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(2.))
                            .bg(color("--tabs-indicator-default")),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_event_tab(which, cx)))
        };
        div()
            .parity("event-detail-strip")
            .relative()
            .h(px(36.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(2.))
                    .bg(color("--border-subtle")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(32.))
                    .child(tab(Tab::Info, "Event Info".to_owned(), cx))
                    .child(tab(Tab::Interested, format!("{count} Interested"), cx)),
            )
            .into_any_element()
    }

    /// A row of the Event Info tab: a 20px icon, 8px, then the text.
    /// `centered`: the count and creator lines sit 1px down in their row, the server and place lines at its top.
    fn event_info_row(&self, leading: AnyElement, text: AnyElement, centered: bool) -> gpui::Div {
        div()
            .mt(px(8.))
            .h(px(20.))
            .flex()
            .items_start()
            .gap(px(8.))
            .child(leading)
            .child(
                div()
                    .when(centered, |this| this.mt(px(1.)))
                    .text_size(px(14.))
                    .line_height(px(18.))
                    .text_color(color("--text-default"))
                    .child(text),
            )
    }

    /// The location line of the Event Info tab (module 167630): `locationText` in `--text-default`; a voice or Stage
    /// event's channel is `channelLocation` (a pointer, underlined on hover) and a click joins it (`GI` of module
    /// 427080: `selectVoiceChannel`, or the Stage), then the detail closes. An outside location stays plain text.
    fn event_location_text(
        &self,
        event: &ScheduledEvent,
        place: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let channel = (event.entity_type != dc_state::scheduled_events::ENTITY_EXTERNAL)
            .then_some(event.channel_id)
            .flatten();
        let guild = event.guild_id;
        div()
            .id("event-detail-location")
            .when_some(channel, |this, channel| {
                this.cursor_pointer()
                    .hover(|style| style.underline())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Some(open) = this.guild_events.as_mut() {
                            open.detail = None;
                        }
                        this.request_join_voice(guild, channel, cx);
                    }))
            })
            .child(place)
            .into_any_element()
    }

    fn event_info_panel(
        &self,
        event: &ScheduledEvent,
        now: NaiveDateTime,
        count: i64,
        detail: &Detail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // `J.Ay(event, recurrenceId)`: a picked occurrence shows its own start, the event's length after it.
        let picked = detail.recurrence.and_then(|ms| {
            let start = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)?;
            let mut shown = event.clone();
            let length = event.start().zip(
                event
                    .scheduled_end_time
                    .as_deref()
                    .and_then(|end| chrono::DateTime::parse_from_rfc3339(end).ok()),
            );
            shown.scheduled_start_time = start.to_rfc3339();
            shown.scheduled_end_time = length
                .map(|(from, to)| (start + (to.with_timezone(&chrono::Utc) - from)).to_rfc3339());
            if event.start() != Some(start) {
                shown.status = dc_state::scheduled_events::STATUS_SCHEDULED;
            }
            Some(shown)
        });
        let status = event_status(picked.as_ref().unwrap_or(event), now, true);
        let creator = event
            .creator_id
            .and_then(|creator| self.event_user(creator));
        let guild = self.stores.guild(event.guild_id);
        let acronym: String = guild
            .map(|guild| {
                guild
                    .name
                    .split_whitespace()
                    .filter_map(|word| word.chars().next())
                    .collect()
            })
            .unwrap_or_default();
        let guild_image = self
            .images
            .get(&format!("guild:{}", event.guild_id))
            .cloned();
        let server_icon: AnyElement = match guild_image {
            Some(image) => gpui::img(image)
                .size(px(20.))
                .rounded(px(6.))
                .into_any_element(),
            None => div()
                .size(px(20.))
                .rounded(px(6.))
                .bg(color("--background-brand"))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.))
                .text_color(color("--white"))
                .child(acronym)
                .into_any_element(),
        };
        let (place_icon, place) = self.event_place(event);
        let guild_name = guild.map(|guild| guild.name.clone()).unwrap_or_default();
        let creator_name = creator
            .as_ref()
            .map(|user| user.display_name().to_owned())
            .unwrap_or_default();
        div()
            .pt(px(16.))
            .child(
                div()
                    .parity("event-detail-info-header")
                    .pt(px(12.))
                    .flex()
                    .flex_col()
                    .children(status.as_ref().map(status_row))
                    .child(
                        div()
                            .mt(px(16.))
                            .h(px(24.))
                            .text_size(px(20.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(event.name.clone()),
                    ),
            )
            .child(
                div()
                    .parity("event-detail-info-body")
                    .pt(px(8.))
                    .pb(px(24.))
                    .child(self.event_info_row(server_icon, guild_name.into_any_element(), false))
                    .child(self.event_info_row(
                        icon(place_icon, 20., "--icon-muted").into_any_element(),
                        self.event_location_text(event, place, cx),
                        false,
                    ))
                    .child(self.event_info_row(
                        icon("event_rsvp", 20., "--icon-muted").into_any_element(),
                        interested_sentence(count).into_any_element(),
                        true,
                    ))
                    .child(self.event_info_row(
                        self.event_avatar(creator.as_ref(), 20.),
                        format!("Created by {creator_name}").into_any_element(),
                        true,
                    ))
                    .children(
                        event
                            .description
                            .clone()
                            .filter(|text| !text.is_empty())
                            .map(|text| {
                                div()
                                    .mt(px(16.))
                                    .text_size(px(14.))
                                    .line_height(px(18.))
                                    .text_color(color("--text-default"))
                                    .child(text)
                            }),
                    ),
            )
            .children(self.event_series(event, now, detail, cx))
            .into_any_element()
    }

    /// Module 893110 under a recurring event's info (`eu` of module 237519, `hideScroller`): the 1px `--border-subtle`
    /// rule (`divider_6_68tM`, 8px up into the body's bottom padding), then `recurrences_CtY5AU` (4px over 12px around)
    /// with "Events in series" (`heading-xs/medium`, 8px over the rows, 4px in) and a row per occurrence
    /// (`container_7O1vgc`: 2px by 4px, `--radius-xs`, `--background-mod-subtle` on hover and `--background-mod-normal`
    /// for the one picked): its status line, and "Canceled" (`body-sm/semibold`, `text-feedback-critical`, 8px before
    /// the end) for a canceled one, which takes no click. "View future events" (the text button of module 123292: `body-md/medium`, `--text-brand`, underlined on hover;
    /// 8px above, centred)
    /// while another occurrence follows within four years.
    /// GAP: each row's "More options" button (module 924037's recurrence menu) and the per-occurrence interested
    /// counts (`getGuildEventUserCounts` with recurrence ids) are not built.
    fn event_series(
        &self,
        event: &ScheduledEvent,
        now: NaiveDateTime,
        detail: &Detail,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if event.recurrence_rule.is_null() {
            return None;
        }
        let first = event.start()?;
        let times = detail.series.clone();
        let more = detail.series_more;
        let exceptions = self.stores.scheduled_events.exceptions_of(event.id);
        let rows = times.into_iter().enumerate().map(|(index, at)| {
            let ms = at.timestamp_millis();
            let recurrence_id = ((ms - 1_420_070_400_000) << 22).to_string();
            let exception = exceptions
                .iter()
                .find(|exception| exception["event_exception_id"].as_str() == Some(&recurrence_id));
            let canceled = exception.is_some_and(|exception| exception["is_canceled"] == true);
            let start = exception
                .and_then(|exception| exception["scheduled_start_time"].as_str())
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map_or(at, |time| time.with_timezone(&chrono::Utc));
            let mut shown = event.clone();
            shown.scheduled_start_time = start.to_rfc3339();
            shown.scheduled_end_time = None;
            shown.status = if canceled {
                dc_state::scheduled_events::STATUS_CANCELED
            } else if start == first {
                event.status
            } else {
                dc_state::scheduled_events::STATUS_SCHEDULED
            };
            let active =
                detail.recurrence == Some(ms) || (detail.recurrence.is_none() && index == 0);
            div()
                .id(("event-series-row", index))
                .flex()
                .items_center()
                .justify_between()
                .py(px(2.))
                .px(px(4.))
                .rounded(px(4.))
                .when(active, |row| row.bg(color("--background-mod-normal")))
                .when(!active && !canceled, |row| {
                    row.hover(|style| style.bg(color("--background-mod-subtle")))
                        .cursor_pointer()
                })
                .when(!canceled, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(detail) = this
                            .guild_events
                            .as_mut()
                            .and_then(|open| open.detail.as_mut())
                        {
                            detail.recurrence = Some(ms);
                        }
                        cx.notify();
                    }))
                })
                .child(
                    div()
                        .w_full()
                        .children(event_status(&shown, now, false).as_ref().map(status_row)),
                )
                .when(canceled, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .mr(px(8.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-feedback-critical"))
                            .child(dc_intl::t("fyBVRm")),
                    )
                })
        });
        Some(
            div()
                .child(div().mt(px(-8.)).h(px(1.)).bg(color("--border-subtle")))
                .child(
                    div()
                        .pt(px(4.))
                        .px(px(12.))
                        .pb(px(12.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .mb(px(8.))
                                .ml(px(4.))
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("D/jjoa")),
                        )
                        .child(div().flex().flex_col().children(rows))
                        .when(more, |this| {
                            this.child(
                                div().pt(px(8.)).flex().justify_center().child(
                                    div()
                                        .id("event-series-more")
                                        .cursor_pointer()
                                        .whitespace_nowrap()
                                        .text_size(px(16.))
                                        .line_height(px(20.))
                                        .font_weight(FontWeight(500.))
                                        .text_color(color("--text-brand"))
                                        .hover(|style| style.underline())
                                        .child(dc_intl::t("8O7Hpy"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let Some((event, shown)) = this
                                                .guild_events
                                                .as_ref()
                                                .and_then(|open| open.detail.as_ref())
                                                .map(|detail| (detail.event, detail.series.len()))
                                            else {
                                                return;
                                            };
                                            let (series, more) =
                                                this.event_series_page(event, shown + SERIES_PAGE);
                                            if let Some(detail) = this
                                                .guild_events
                                                .as_mut()
                                                .and_then(|open| open.detail.as_mut())
                                            {
                                                detail.series = series;
                                                detail.series_more = more;
                                            }
                                            cx.notify();
                                        })),
                                ),
                            )
                        }),
                )
                .into_any_element(),
        )
    }

    fn event_interested_panel(&self, event: Id) -> AnyElement {
        let users = self
            .stores
            .scheduled_events
            .get_users_for_guild_event(event, None);
        let rows: Vec<AnyElement> = users
            .iter()
            .filter(|answer| answer.response == dc_state::scheduled_events::RESPONSE_INTERESTED)
            .map(|answer| {
                let user = self.event_user(answer.user_id);
                let name = answer
                    .nick
                    .clone()
                    .or_else(|| user.as_ref().map(|user| user.display_name().to_owned()))
                    .unwrap_or_default();
                let status = if self
                    .stores
                    .current_user
                    .as_ref()
                    .is_some_and(|me| me.id == answer.user_id)
                {
                    self.stores.status.clone()
                } else {
                    self.stores.status(answer.user_id).to_owned()
                };
                div()
                    .h(px(40.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .relative()
                            .size(px(24.))
                            .flex_none()
                            .child(self.event_avatar(user.as_ref(), 24.))
                            .child(status_dot_sized(
                                &status,
                                "--background-surface-high",
                                8.,
                                3.,
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--text-default"))
                            .child(name),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        div()
            .pt(px(16.))
            .child(
                div()
                    .id("event-interested-list")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .pt(px(8.))
                    .pb(px(9.))
                    .pr(px(8.))
                    .flex()
                    .flex_col()
                    .children(rows)
                    .when(empty, |this| {
                        this.child(
                            div()
                                .h(px(40.))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .line_height(px(18.))
                                .text_color(color("--text-muted"))
                                .child("No one's interested in this event yet."),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The detail dialog over the Events modal.
    pub(super) fn event_detail_layer(
        &self,
        detail: &Detail,
        now: NaiveDateTime,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let event = self
            .stores
            .scheduled_events
            .get_guild_scheduled_event(detail.event)?
            .clone();
        let count = self.stores.scheduled_events.get_user_count(event.id, None);
        let panel = match detail.tab {
            Tab::Info => self.event_info_panel(&event, now, count, detail, cx),
            Tab::Interested => self.event_interested_panel(event.id),
        };
        let header = div()
            .parity("event-detail-header")
            .pt(px(16.))
            .px(px(24.))
            .pb(px(8.))
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(24.))
                    .truncate()
                    .text_size(px(20.))
                    .line_height(px(24.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(event.name.clone()),
            )
            .child(
                div()
                    .id("event-detail-close")
                    .mt(px(-16.))
                    .mr(px(-16.))
                    .size(px(40.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 20., "--control-icon-only-icon-default"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(open) = this.guild_events.as_mut() {
                            open.detail = None;
                        }
                        cx.notify();
                    })),
            );
        let footer = div()
            .parity("event-detail-footer")
            .pt(px(16.))
            .px(px(24.))
            .pb(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .child(self.event_more_button("event-detail-more", event.id, Size::Md, cx)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(self.event_copy_link_button("event-detail-copy", event.id, Size::Md, cx))
                    .child(self.interested_button("event-detail-interested", &event, Size::Md, cx)),
            );
        let modal = div()
            .parity_with_border("event-detail-modal", 1.)
            .id("event-detail-modal")
            .occlude()
            .w_full()
            .max_w(px(680.))
            .pt(px(8.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(modal_shadow_large())
            .child(header)
            .child(
                div().mt(px(16.)).mb(px(8.)).px(px(24.)).child(
                    div()
                        .parity("event-detail-tabs")
                        .child(self.event_tabs(detail.tab, count, cx))
                        // A long series scrolls inside the dialog rather than pushing it past the window.
                        .child(
                            div()
                                .parity("event-detail-panel")
                                .id("event-detail-panel")
                                .max_h(px((self.viewport.1 - 300.).max(200.)))
                                .overflow_y_scroll()
                                .child(panel),
                        ),
                ),
            )
            .child(footer);
        Some(layer(
            "event-detail-layer",
            0.,
            modal,
            |this, _, _| {
                if let Some(open) = this.guild_events.as_mut() {
                    open.detail = None;
                }
            },
            cx,
        ))
    }
}

/// The dialog's shadow: `0 12px 24px rgba(0,0,0,.24)`.
fn modal_shadow_large() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., 0.24),
        offset: gpui::point(px(0.), px(12.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}
