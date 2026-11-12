//! "Boost This Server" for an account that holds Boosts not in use (module 987144 `g` decides, module 710802 is the
//! dialog). The account's Boost slots are read (`USER_GUILD_BOOST_SLOTS`); the ones with no server and no cooldown
//! (`isAvailable`, module 545934) can be used here:
//!
//! - several: "Unused Boosts" asks how many (`UNUSED_QUANTITY_SELECT`), then the confirm step;
//! - one: the confirm step at once;
//! - none: the official client opens the purchase flow, which is not drawn; the account's Server Boost page opens.
//!
//! The confirm step sends `PUT /guilds/<id>/premium/subscriptions` with the slot ids (`VA` of module 559201) and
//! "Boost Activated" follows (module 636441).
//!
//! Measured: the first step, from the owner's capture of the official dialog (480 wide, the stepper of module
//! 663803, the styles `quantitySelector*_T4asAA`). Not measured, built from the modules and the style sheets only
//! (no account at hand held a free Boost): the confirm and the success step. Their header graphic (a Rive file and
//! a video) is not drawn, the tier pill has no star, and the stepper's number cannot be typed.

use super::{Root, channel_status, color, icon, modal};
use crate::net::Request;
use dc_state::Id;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Rgba, SharedString, deferred, div, img,
    prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(super) const KEY_PREFIX: &str = "boost-apply:";
const SLOTS_KEY: &str = "boost-apply:slots";
const APPLY_KEY: &str = "boost-apply:apply";
const SLOTS_ROUTE: &str = "/users/@me/guilds/premium/subscription-slots";
/// `days: 7` of the confirm step's warning.
const TRANSFER_DAYS: u32 = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// The slots are being read; nothing is drawn.
    Loading,
    Quantity,
    Confirm,
    Success,
}

pub(crate) struct BoostApply {
    guild: Id,
    step: Step,
    /// The ids of the slots that can be used, in the order the account lists them.
    available: Vec<String>,
    /// How many of them go to this server.
    count: usize,
    sending: bool,
    error: Option<String>,
    /// Pressed on the account's Server Boost page: with no free Boost, the server's own Boost page opens.
    from_settings: bool,
}

/// The slots of the answer that are free: on no server and not cooling down (`isAvailable`).
pub(super) fn available_slots(slots: &Value, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    slots
        .as_array()
        .into_iter()
        .flatten()
        .filter(|slot| slot["premium_guild_subscription"].is_null())
        .filter(|slot| {
            slot["cooldown_ends_at"]
                .as_str()
                .and_then(|ends| chrono::DateTime::parse_from_rfc3339(ends).ok())
                .is_none_or(|ends| ends.with_timezone(&chrono::Utc) < now)
        })
        .filter_map(|slot| slot["id"].as_str().map(str::to_owned))
        .collect()
}

/// The body of the request that puts the slots on a server.
pub(super) fn apply_body(slots: &[String]) -> Value {
    json!({"user_premium_guild_subscription_slot_ids": slots, "disable_powerup_auto_apply": false})
}

/// `ZU5x5w`: the confirm button.
pub(super) fn boost_label(count: usize) -> String {
    if count == 1 {
        "Boost".to_owned()
    } else {
        format!("Boost {count} times")
    }
}

/// `KPnDlu`: the warning of the confirm step.
pub(super) fn transfer_warning(count: usize) -> String {
    let which = if count == 1 {
        "this Boost"
    } else {
        "these Boosts"
    };
    format!(
        "Boosting this server will prevent transferring {which} to another server for {TRANSFER_DAYS} days."
    )
}

/// `IeFTQu`: the line of "Boost Activated".
pub(super) fn success_line(guild: &str, count: usize) -> String {
    let times = if count == 1 {
        "1 time".to_owned()
    } else {
        format!("{count} times")
    };
    format!("You just Boosted {guild} {times}.")
}

/// The Boost level a number of Boosts gives (`kN` of module 473145: 2, 7 and 14 Boosts).
pub(super) fn tier_for(boosts: u64) -> u64 {
    match boosts {
        14.. => 3,
        7.. => 2,
        2.. => 1,
        _ => 0,
    }
}

fn tier_label(tier: u64) -> String {
    if tier == 0 {
        "No Level".to_owned()
    } else {
        format!("Level {tier}")
    }
}

impl Root {
    /// "Boost This Server" was pressed for `guild`.
    pub(super) fn boost_this_server(&mut self, guild: Id, cx: &mut Context<Self>) {
        self.boost_server_from(guild, false, cx);
    }

    pub(super) fn boost_server_from(
        &mut self,
        guild: Id,
        from_settings: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .boost_apply
            .as_ref()
            .is_some_and(|open| open.step == Step::Loading)
        {
            return;
        }
        self.boost_apply = Some(BoostApply {
            guild,
            step: Step::Loading,
            available: Vec::new(),
            count: 1,
            sending: false,
            error: None,
            from_settings,
        });
        let _ = self.requests.send(Request::Rest {
            key: Some(SLOTS_KEY.to_owned()),
            method: reqwest::Method::GET,
            route: SLOTS_ROUTE.to_owned(),
            body: None,
        });
        cx.notify();
    }

    pub(super) fn boost_apply_open(&self) -> bool {
        self.boost_apply
            .as_ref()
            .is_some_and(|open| open.step != Step::Loading)
    }

    pub(super) fn close_boost_apply(&mut self) -> bool {
        let open = self.boost_apply_open();
        self.boost_apply = None;
        open
    }

    /// An answer to one of this module's requests.
    pub(super) fn boost_apply_fetched(&mut self, key: &str, body: &Value, cx: &mut Context<Self>) {
        let Some(open) = self.boost_apply.as_mut() else {
            return;
        };
        match key {
            SLOTS_KEY => {
                let available = if body["ok"] == true {
                    available_slots(&body["body"], chrono::Utc::now())
                } else {
                    Vec::new()
                };
                match available.len() {
                    0 => {
                        // No free Boost: buying one is not drawn. From the server, the account's Server Boost
                        // page shows what it has; from that page, the server's own Boost page opens.
                        let (guild, from_settings) = (open.guild, open.from_settings);
                        self.boost_apply = None;
                        if from_settings {
                            self.close_settings(cx);
                            if self.guild != Some(guild) {
                                self.select(guild, None);
                            }
                            self.open_boost_page(cx);
                        } else {
                            self.boost_page = None;
                            self.server_settings = None;
                            self.open_settings_section("Server Boost", cx);
                        }
                    }
                    1 => {
                        open.available = available;
                        open.count = 1;
                        open.step = Step::Confirm;
                    }
                    _ => {
                        open.available = available;
                        open.count = 1;
                        open.step = Step::Quantity;
                    }
                }
            }
            APPLY_KEY => {
                open.sending = false;
                if body["ok"] == true {
                    open.error = None;
                    open.step = Step::Success;
                } else {
                    // `applyBoostError.message`: the server's own words.
                    open.error = body["body"]["message"].as_str().map(str::to_owned);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn boost_apply_step(&mut self, by: i64, cx: &mut Context<Self>) {
        if let Some(open) = self.boost_apply.as_mut() {
            let most = open.available.len().max(1) as i64;
            open.count = (open.count as i64 + by).clamp(1, most) as usize;
            cx.notify();
        }
    }

    fn boost_apply_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.boost_apply.as_mut() else {
            return;
        };
        if open.sending || open.available.is_empty() {
            return;
        }
        open.sending = true;
        open.error = None;
        let slots: Vec<String> = open.available.iter().take(open.count).cloned().collect();
        let _ = self.requests.send(Request::Rest {
            key: Some(APPLY_KEY.to_owned()),
            method: reqwest::Method::PUT,
            route: format!("/guilds/{}/premium/subscriptions", open.guild),
            body: Some(apply_body(&slots)),
        });
        cx.notify();
    }

    fn body_text(text: impl Into<SharedString>) -> gpui::Div {
        div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .child(text.into())
    }

    /// `serverCard__zrNpV`: the server, its level now and the level the Boosts take it to.
    fn boost_apply_card(&self, guild: Id, adding: usize) -> AnyElement {
        let Some(guild) = self.stores.guild(guild) else {
            return div().into_any_element();
        };
        let picture = self.images.get(&format!("guild:{}", guild.id)).cloned();
        let now = guild.premium_tier;
        let then = tier_for(guild.premium_subscription_count + adding as u64);
        let pill = |tier: u64| {
            div()
                .flex_none()
                .rounded(px(11.))
                .bg(color("--background-mod-normal"))
                .py(px(4.))
                .pl(px(4.))
                .pr(px(8.))
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(color("--text-default"))
                .child(icon("boost_gem", 14., "--text-muted"))
                .child(tier_label(tier))
        };
        let chip = div()
            .size(px(50.))
            .flex_none()
            .rounded(px(14.))
            .border_1()
            .border_color(gpui::hsla(0., 0., 1., 0.16))
            .bg(color("--background-base-low"))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .font_weight(FontWeight(600.))
            .text_color(color("--text-strong"));
        let chip = match picture {
            Some(picture) => chip.child(img(picture).size(px(48.)).rounded(px(13.))),
            None => chip.child(guild.acronym()),
        };
        div()
            .my(px(16.))
            .p(px(16.))
            .rounded(px(8.))
            .bg(color("--background-base-lower"))
            .flex()
            .items_center()
            .child(chip)
            .child(
                div()
                    .ml(px(16.))
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mb(px(8.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(guild.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(pill(now))
                            .when(then != now, |row| {
                                row.child(icon("discover_arrow_right", 16., "--green-360"))
                                    .child(pill(then))
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The dialog of the step that is open.
    pub(super) fn boost_apply_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.boost_apply.as_ref()?;
        match open.step {
            Step::Loading => None,
            Step::Quantity => Some(self.boost_apply_quantity(open, cx)),
            Step::Confirm | Step::Success => Some(self.boost_apply_graphic(open, cx)),
        }
    }

    fn boost_apply_quantity(&self, open: &BoostApply, cx: &mut Context<Self>) -> AnyElement {
        let most = open.available.len();
        let step_button = |id: &'static str,
                           glyph: &'static str,
                           enabled: bool,
                           by: i64,
                           cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(32.))
                .flex_none()
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .when(!enabled, |this| this.opacity(0.5))
                .when(enabled, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(color("--control-icon-only-background-hover")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.boost_apply_step(by, cx);
                        }))
                })
                .child(icon(glyph, 16., "--control-icon-only-icon-default"))
        };
        let minus = step_button("boost-apply-minus", "automod_minus", open.count > 1, -1, cx);
        let plus = step_button("boost-apply-plus", "automod_plus", open.count < most, 1, cx);
        // `quantitySelectorBody` (margin-top 20), `quantitySelectorWrapper` (margin 32px auto), the stepper's
        // `actions_BGm607` (gap 4) around `value_BGm607` (48 wide), the label 16px after it.
        let body = div()
            .mt(px(20.))
            .flex()
            .flex_col()
            .child(Self::body_text(
                "You have some unused Boosts. How many do you want to use for this server?",
            ))
            .child(
                div()
                    .my(px(32.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(minus)
                            .child(
                                div()
                                    .w(px(48.))
                                    .h(px(40.))
                                    .px(px(12.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(color("--input-border-default"))
                                    .bg(color("--input-background-default"))
                                    .flex()
                                    .items_center()
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .text_color(color("--text-default"))
                                    .child(open.count.to_string()),
                            )
                            .child(plus),
                    )
                    .child(
                        div()
                            .ml(px(16.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child("Server Boost(s)"),
                    ),
            )
            .into_any_element();
        modal::render_over_layers(
            modal::Spec {
                id: "boost-apply-quantity",
                title: "Unused Boosts".into(),
                subtitle: None,
                body: Some(body),
                buttons: vec![
                    modal::button(
                        "boost-apply-cancel",
                        "Cancel",
                        modal::Tone::Secondary,
                        |this, _, cx| {
                            this.close_boost_apply();
                            cx.notify();
                        },
                    ),
                    modal::button(
                        "boost-apply-next",
                        "Next",
                        modal::Tone::Primary,
                        |this, _, cx| {
                            if let Some(open) = this.boost_apply.as_mut() {
                                open.step = Step::Confirm;
                            }
                            cx.notify();
                        },
                    ),
                ],
                close: Box::new(|this, _, cx| {
                    this.close_boost_apply();
                    cx.notify();
                }),
            },
            cx,
        )
    }

    /// The confirm and the success step: the dialog with a header (module 772707), as the voice status dialog's.
    fn boost_apply_graphic(&self, open: &BoostApply, cx: &mut Context<Self>) -> AnyElement {
        let success = open.step == Step::Success;
        let gradient = channel_status::header_gradient(
            color("--expressive-gradient-purple-start"),
            color("--expressive-gradient-purple-end"),
        );
        let pose = super::motion::modal_pose("boost-apply-modal");
        let header_width = channel_status::HEADER_WIDTH as f32;
        // The header without a subtitle: the title's 32px line ends 16px above the header's edge.
        let header_height = 216.;
        let header = div()
            .relative()
            .flex_none()
            .w(px(header_width))
            .h(px(header_height))
            .overflow_hidden()
            .child(
                img(gradient)
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .w(px(header_width))
                    .h(px(channel_status::HEADER_HEIGHT as f32)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(168.))
                    .left(px(32.))
                    .w(px(414.))
                    .h(px(32.))
                    .flex()
                    .justify_center()
                    .text_size(px(24.))
                    .line_height(px(32.))
                    .font_weight(FontWeight(600.))
                    .text_color(color("--text-strong"))
                    .child(if success {
                        "Boost Activated"
                    } else {
                        "Boost this server"
                    }),
            );
        let guild_name = self
            .stores
            .guild(open.guild)
            .map(|guild| guild.name.clone())
            .unwrap_or_default();
        let body = if success {
            div()
                .flex_none()
                .px(px(32.))
                .child(Self::body_text(success_line(&guild_name, open.count)))
        } else {
            div()
                .flex_none()
                .px(px(32.))
                .flex()
                .flex_col()
                .child(Self::body_text("Ready to Boost this server?"))
                .child(self.boost_apply_card(open.guild, open.count))
                .child(Self::body_text(transfer_warning(open.count)).mt(px(4.)))
                .children(open.error.clone().map(|message| {
                    div()
                        .mt(px(8.))
                        .text_size(px(14.))
                        .line_height(px(18.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--text-feedback-critical"))
                        .child(message)
                }))
        };
        let action = if success {
            modal::button(
                "boost-apply-done",
                "Nice!",
                modal::Tone::Primary,
                |this, _, cx| {
                    this.close_boost_apply();
                    cx.notify();
                },
            )
        } else {
            modal::button(
                "boost-apply-confirm",
                boost_label(open.count),
                if open.sending {
                    modal::Tone::PrimaryLoading
                } else {
                    modal::Tone::Primary
                },
                |this, _, cx| this.boost_apply_confirm(cx),
            )
        };
        let footer = div()
            .flex_none()
            .px(px(32.))
            .py(px(16.))
            .flex()
            .gap(px(8.))
            .child(modal::footer_button(action, cx));
        let panel = div()
            .id("boost-apply-modal")
            .opacity(pose.opacity)
            .paint_scale(pose.scale, gpui::point(0.5, 0.5))
            .occlude()
            .relative()
            .w(px(channel_status::MODAL_WIDTH))
            .pb(px(16.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .overflow_hidden()
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.24),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(header)
            .child(div().flex_none().h(px(16.)))
            .child(body)
            .child(div().flex_none().h(px(16.)))
            .child(footer)
            .child(
                super::keyboard::inset_ring_when_focused(div().id("boost-apply-close"))
                    .focusable()
                    .tab_index(0)
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(38.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("modal_close", 20., "--text-strong"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_boost_apply();
                        cx.notify();
                    })),
            );
        deferred(
            div()
                .id("boost-apply-modal-layer")
                .occlude()
                .absolute()
                .inset_0()
                .bg(Rgba {
                    a: f32::from(0xb8u8) / 255. * pose.scrim,
                    ..rgba(0x000000ff)
                })
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_boost_apply();
                    cx.notify();
                }))
                .child(panel),
        )
        .with_priority(4)
        .into_any_element()
    }

    /// Capture only: `boost-apply <free slots>` opens the dialog as an account with that many free Boosts would see
    /// it, `boost-apply next`, `boost-apply success` and `boost-apply state`.
    #[cfg(feature = "capture")]
    pub(crate) fn script_boost_apply(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["state"] => match self.boost_apply.as_ref() {
                Some(open) => format!(
                    "boost-apply: step={:?} free={} count={} sending={} error={:?}",
                    open.step,
                    open.available.len(),
                    open.count,
                    open.sending,
                    open.error
                ),
                None => "boost-apply: closed".to_owned(),
            },
            ["next"] => {
                if let Some(open) = self.boost_apply.as_mut() {
                    open.step = Step::Confirm;
                }
                cx.notify();
                "boost-apply: confirm".to_owned()
            }
            ["success"] => {
                if let Some(open) = self.boost_apply.as_mut() {
                    open.step = Step::Success;
                }
                cx.notify();
                "boost-apply: success".to_owned()
            }
            [free] => {
                let (Ok(free), Some(guild)) = (free.parse::<usize>(), self.guild) else {
                    return "usage: boost-apply <free slots>|next|success|state".to_owned();
                };
                self.boost_apply = Some(BoostApply {
                    guild,
                    step: Step::Loading,
                    available: Vec::new(),
                    count: 1,
                    sending: false,
                    error: None,
                    from_settings: false,
                });
                let slots: Vec<Value> = (0..free)
                    .map(|index| json!({"id": format!("{}", 9000 + index), "premium_guild_subscription": null, "cooldown_ends_at": null}))
                    .collect();
                self.boost_apply_fetched(SLOTS_KEY, &json!({"ok": true, "body": slots}), cx);
                format!("boost-apply: {free} free")
            }
            _ => "usage: boost-apply <free slots>|next|success|state".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_slots_with_no_server_and_no_cooldown_are_free() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-12T10:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let slots = json!([
            {"id": "1", "premium_guild_subscription": null, "cooldown_ends_at": null},
            {"id": "2", "premium_guild_subscription": {"id": "9", "guild_id": "5"}, "cooldown_ends_at": null},
            {"id": "3", "premium_guild_subscription": null, "cooldown_ends_at": "2026-11-15T10:00:00+00:00"},
            {"id": "4", "premium_guild_subscription": null, "cooldown_ends_at": "2026-11-01T10:00:00+00:00"},
        ]);
        assert_eq!(
            available_slots(&slots, now),
            vec!["1".to_owned(), "4".to_owned()]
        );
        assert!(available_slots(&json!({"message": "401"}), now).is_empty());
    }

    #[test]
    fn the_request_carries_the_slot_ids() {
        assert_eq!(
            apply_body(&["11".to_owned(), "12".to_owned()]),
            json!({"user_premium_guild_subscription_slot_ids": ["11", "12"], "disable_powerup_auto_apply": false})
        );
    }

    #[test]
    fn the_texts_follow_the_count() {
        assert_eq!(boost_label(1), "Boost");
        assert_eq!(boost_label(3), "Boost 3 times");
        assert_eq!(
            transfer_warning(1),
            "Boosting this server will prevent transferring this Boost to another server for 7 days."
        );
        assert_eq!(
            transfer_warning(2),
            "Boosting this server will prevent transferring these Boosts to another server for 7 days."
        );
        assert_eq!(success_line("Home", 1), "You just Boosted Home 1 time.");
        assert_eq!(success_line("Home", 2), "You just Boosted Home 2 times.");
    }

    #[test]
    fn the_level_follows_the_boost_count() {
        assert_eq!(tier_for(0), 0);
        assert_eq!(tier_for(1), 0);
        assert_eq!(tier_for(2), 1);
        assert_eq!(tier_for(6), 1);
        assert_eq!(tier_for(7), 2);
        assert_eq!(tier_for(14), 3);
    }
}
