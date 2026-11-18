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

impl Root {
    /// Opens the detail view of an event on its Event Info tab and asks for its count and who is interested.
    pub(super) fn open_event_detail(&mut self, event: Id, cx: &mut Context<Self>) {
        let Some(guild) = self.guild_events.as_ref().map(|open| open.guild) else {
            return;
        };
        if let Some(open) = self.guild_events.as_mut() {
            open.detail = Some(Detail {
                event,
                tab: Tab::Info,
            });
            open.menu = None;
        }
        self.event_fetch_counts(guild, event);
        self.event_fetch_users(guild, event);
        cx.notify();
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
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = event_status(event, now, true);
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
            .into_any_element()
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
            Tab::Info => self.event_info_panel(&event, now, count, cx),
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
            .w(px(680.))
            .pt(px(8.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .rounded(px(12.))
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
                        .child(div().parity("event-detail-panel").child(panel)),
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
