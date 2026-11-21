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
//! (no account at hand held a free Boost): the confirm and the success step. Their header graphic is not in the
//! snapshot, so its box is left empty: the confirm step's Rive file (module 449349, `4e02ee04….riv`, artboard "Boost
//! Server") and the success step's fallback picture and confetti (module 88671, `0f6e600f….png`, `5211bb6d….png`;
//! with media URLs from the server it plays videos instead).

use super::{Root, channel_status, color, icon, modal};
use crate::editor::{Editor, EditorEvent};
use crate::net::Request;
use dc_state::Id;
use gpui::Focusable as _;
use gpui::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Rgba, SharedString, Subscription,
    deferred, div, img, prelude::*, px, rgba,
};
use serde_json::{Value, json};

pub(super) const KEY_PREFIX: &str = "boost-apply:";
const SLOTS_KEY: &str = "boost-apply:slots";
const APPLY_KEY: &str = "boost-apply:apply";
const SLOTS_ROUTE: &str = "/users/@me/guilds/premium/subscription-slots";
/// `days: 7` of the confirm step's warning.
const TRANSFER_DAYS: u32 = 7;

/// What the stepper's field holds (module 663803's `useState`): a number, or "" or "-" while one is being typed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Typed {
    Blank(String),
    Number(i64),
}

impl Typed {
    fn text(&self) -> String {
        match self {
            Typed::Blank(text) => text.clone(),
            Typed::Number(value) => value.to_string(),
        }
    }

    /// What the dialog is told (`onChange(h(n) ? minValue ?? 0 : n)`).
    fn value(&self, min: i64) -> i64 {
        match self {
            Typed::Blank(_) => min,
            Typed::Number(value) => *value,
        }
    }
}

/// The stepper's `onChange` for a typed `text`: "" and "-" stay as they are; otherwise `parseInt`, then at or past
/// `max` gives `max` and at or under `min` gives `min`. Text `parseInt` reads as `NaN` is refused (`None`): the input
/// is controlled, so it keeps the value it had.
pub(super) fn typed(text: &str, min: i64, max: i64) -> Option<Typed> {
    if text.is_empty() || text == "-" {
        return Some(Typed::Blank(text.to_owned()));
    }
    let number = parse_int(text)?;
    Some(Typed::Number(if number >= max {
        max
    } else if number <= min {
        min
    } else {
        number
    }))
}

/// JavaScript's `parseInt(text)` in base 10: leading white space, a sign, then the digits up to the first other
/// character; no digit is `NaN`. A number past `i64` saturates (the stepper clamps it to its maximum anyway).
fn parse_int(text: &str) -> Option<i64> {
    let rest = text.trim_start();
    let (negative, rest) = match rest.as_bytes().first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let digits = &rest[..rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len())];
    if digits.is_empty() {
        return None;
    }
    let magnitude = digits.bytes().fold(0i64, |value, digit| {
        value
            .saturating_mul(10)
            .saturating_add(i64::from(digit - b'0'))
    });
    Some(if negative { -magnitude } else { magnitude })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// The slots are being read; nothing is drawn.
    Loading,
    /// `GUILD_SELECT` of module 770101 / 710802 ("Select a Server" / "Select a server to transfer to").
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    GuildSelect,
    Quantity,
    Confirm,
    Success,
}

pub(crate) struct BoostApply {
    guild: Id,
    step: Step,
    /// `770101`: whether selecting a destination server for a boost transfer vs a fresh boost.
    is_transfer: bool,
    /// `770101`: search filter text in `Step::GuildSelect`.
    select_query: String,
    /// `770101`: override empty state when demonstrating `E3tB6+` (no servers joined).
    force_empty_guilds: bool,
    /// The ids of the slots that can be used, in the order the account lists them.
    available: Vec<String>,
    /// How many of them go to this server.
    count: usize,
    /// The stepper's field and what it holds.
    field: Option<Entity<Editor>>,
    shown: Typed,
    _field_events: Option<Subscription>,
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

/// `IeFTQu` with its `$b` run: the text and the byte range drawn bold ("N time(s)").
pub(super) fn success_runs(guild: &str, count: usize) -> (String, std::ops::Range<usize>) {
    let before = format!("You just Boosted {guild} ");
    let line = success_line(guild, count);
    let bold = before.len()..line.len() - 1;
    (line, bold)
}

/// The tier gem of `w` in module 710802 (module 313375): none and level 1 share the outline.
fn tier_gem(tier: u64) -> &'static str {
    match tier {
        2 => "boost_tier_2_simple",
        3 => "boost_tier_3_simple",
        _ => "boost_tier_1_simple",
    }
}

/// The header of the confirm and success dialogs (module 20742 with a graphic, centred): the graphic counts as
/// animated (a Rive file, or a `.png`: `Oy` of module 978495), so `headerGraphicAnimated_dvN-MP` drops its top
/// padding and keeps 4px under it; the graphic is 16/9 in a box at most 352 wide and, by
/// `.headerGraphicContainer_dvN-MP>*{max-height:15vh}`, at most 15% of the window tall. Returns the graphic's
/// height, the title's top and the header's height.
pub(super) fn header_geometry(window_height: f32) -> (f32, f32, f32) {
    // The section's 16px top, the graphic, its 4px, the stack's 8px gap; the 32px title and the 16px bottom.
    let graphic = (352.0_f32 * 9. / 16.).min(window_height * 0.15);
    let title = 16. + graphic + 4. + 8.;
    (graphic, title, title + 32. + 16.)
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
            is_transfer: false,
            select_query: String::new(),
            force_empty_guilds: false,
            available: Vec::new(),
            count: 1,
            field: None,
            shown: Typed::Number(1),
            _field_events: None,
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
                        open.shown = Typed::Number(1);
                        open.step = Step::Quantity;
                        self.boost_apply_make_field(cx);
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

    /// The stepper's `<input>` (module 95477's text input inside `value_BGm607`).
    fn boost_apply_make_field(&mut self, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut style = super::editor_style();
            style.size = 16.;
            style.line_height = 22.;
            let mut editor = Editor::new(style, cx);
            editor.set_single_line(true);
            editor.set_reports_all_edits(true);
            editor.set_text("1");
            editor
        });
        let events = cx.subscribe(&field, |this, field, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited => {
                    let text = field.read(cx).text().to_owned();
                    this.boost_apply_typed(&text, cx);
                }
                EditorEvent::Cancel => {
                    this.close_boost_apply();
                }
                EditorEvent::Submit(_) => {}
            }
            cx.notify();
        });
        if let Some(open) = self.boost_apply.as_mut() {
            open.field = Some(field);
            open._field_events = Some(events);
        }
    }

    /// `minValue: 1, maxValue:` the free Boosts, as module 710802 passes them.
    fn boost_apply_bounds(open: &BoostApply) -> (i64, i64) {
        (1, open.available.len().max(1) as i64)
    }

    /// Sets what the stepper holds, its field's text and the dialog's count.
    fn boost_apply_show(&mut self, shown: Typed, cx: &mut Context<Self>) {
        let Some(open) = self.boost_apply.as_mut() else {
            return;
        };
        let (min, _) = Self::boost_apply_bounds(open);
        open.count = shown.value(min).max(min) as usize;
        let text = shown.text();
        open.shown = shown;
        if let Some(field) = open.field.clone() {
            field.update(cx, |editor, cx| {
                if editor.text() != text {
                    editor.set_text(&text);
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    fn boost_apply_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(open) = self.boost_apply.as_ref() else {
            return;
        };
        let (min, max) = Self::boost_apply_bounds(open);
        let shown = typed(text, min, max).unwrap_or_else(|| open.shown.clone());
        self.boost_apply_show(shown, cx);
    }

    /// The minus and plus buttons: `j(b - 1)` and `j(b + 1)`, when the value is a number inside the bounds.
    fn boost_apply_step(&mut self, by: i64, cx: &mut Context<Self>) {
        let Some(open) = self.boost_apply.as_ref() else {
            return;
        };
        let (min, max) = Self::boost_apply_bounds(open);
        if let Typed::Number(value) = open.shown
            && ((by < 0 && value > min) || (by > 0 && value < max))
        {
            self.boost_apply_show(Typed::Number(value + by), cx);
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
        // `tierPill__zrNpV`; `tierPillStar__zrNpV` is module 152578's flower star (16px, 5px before the label, in
        // the pill's colour) with the tier gem (`tierPillGem__zrNpV`, muted) centred over it. The gem keeps its
        // default 24px: its 10px holder (`redesignIconChildContainer_d3vyhc`) does not shrink an `<svg>` sized by
        // attribute.
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
                .text_size(px(12.))
                .line_height(px(14.))
                .text_color(color("--text-default"))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .size(px(16.))
                        .mr(px(5.))
                        .child(icon("discover_flower", 16., "--text-default"))
                        .child(div().absolute().top(px(-4.)).left(px(-4.)).child(icon(
                            tier_gem(tier),
                            24.,
                            "--text-muted",
                        ))),
                )
                .child(tier_label(tier))
        };
        // `guildIcon__zrNpV` on a 50px guild icon (`Sizes.LARGE` of module 682163).
        let chip = div()
            .size(px(50.))
            .flex_none()
            .rounded(px(14.))
            .border_1()
            .border_color(gpui::hsla(0., 0., 1., 0.16))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.14),
                offset: gpui::point(px(0.), px(1.6)),
                blur_radius: px(6.4),
                spread_radius: px(0.),
                inset: false,
            }])
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
                    // `tierInfo__zrNpV`; `k` adds module 61780's double chevron (`levelUpIcon__zrNpV`, 16px with 4px
                    // each side, or `levelDownIcon__zrNpV`) and the new level when it differs.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(pill(now))
                            .when(then != now, |row| {
                                row.child(div().mx(px(4.)).child(icon(
                                    "boost_level_chevrons",
                                    16.,
                                    if then > now {
                                        "--green-360"
                                    } else {
                                        "--icon-feedback-critical"
                                    },
                                )))
                                .child(pill(then))
                            }),
                    ),
            )
            .into_any_element()
    }

    /// The dialog of the step that is open.
    pub(super) fn boost_apply_layer(
        &self,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.boost_apply.as_ref()?;
        match open.step {
            Step::Loading => None,
            Step::GuildSelect => Some(self.boost_apply_guild_select(open, cx)),
            Step::Quantity => Some(self.boost_apply_quantity(open, window, cx)),
            Step::Confirm | Step::Success => Some(self.boost_apply_graphic(open, cx)),
        }
    }

    /// Boost Slot Server Picker (`770101`: `cQYceV`, `IB13DV`, `vf3ZTa`, `E3tB6+`, `w3GLl9`,
    /// `Uj0md3`, `dUgaF8`).
    fn boost_apply_guild_select(&self, open: &BoostApply, cx: &mut Context<Self>) -> AnyElement {
        let is_transfer = open.is_transfer;
        let title = if is_transfer {
            dc_intl::t("IB13DV")
        } else {
            dc_intl::t("cQYceV")
        };
        let cta_label = if is_transfer {
            dc_intl::t("dUgaF8")
        } else {
            dc_intl::t("Uj0md3")
        };
        let q = open.select_query.trim().to_lowercase();
        let all_guilds: Vec<(Id, String, u64)> = if open.force_empty_guilds {
            Vec::new()
        } else {
            self.stores
                .guilds
                .iter()
                .filter(|g| !g.features.iter().any(|f| f == "HUB"))
                .map(|g| (g.id, g.name.clone(), g.premium_tier))
                .collect()
        };
        let filtered: Vec<(Id, String, u64)> = all_guilds
            .iter()
            .filter(|(_, name, _)| q.is_empty() || name.to_lowercase().contains(&q))
            .cloned()
            .collect();

        let body: AnyElement = if all_guilds.is_empty() {
            div()
                .px(px(20.))
                .py(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(icon("boost_gem", 32., "--brand-500"))
                .child(
                    div()
                        .id("boost-select-explore-public")
                        .cursor_pointer()
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(dc_intl::format("E3tB6+", &[]))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.boost_apply = None;
                            this.discover.open = true;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else if filtered.is_empty() {
            div()
                .px(px(20.))
                .py(px(24.))
                .text_center()
                .text_size(px(14.))
                .text_color(color("--text-muted"))
                .child(dc_intl::t("w3GLl9"))
                .into_any_element()
        } else {
            div()
                .id("boost-select-guild-list")
                .max_h(px(340.))
                .overflow_y_scroll()
                .px(px(16.))
                .pb(px(16.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .children(filtered.into_iter().map(|(gid, name, tier)| {
                    div()
                        .id(("boost-select-guild", gid))
                        .cursor_pointer()
                        .px(px(8.))
                        .py(px(8.))
                        .rounded(px(6.))
                        .hover(|s| s.bg(color("--background-mod-subtle")))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(32.))
                                        .rounded(px(10.))
                                        .bg(color("--brand-500"))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--white"))
                                        .child(
                                            name.chars()
                                                .next()
                                                .unwrap_or('S')
                                                .to_uppercase()
                                                .to_string(),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_baseline()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .truncate()
                                                .child(name),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_size(px(12.))
                                                .text_color(color("--text-muted"))
                                                .child(tier_label(tier)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .px(px(16.))
                                .py(px(6.))
                                .rounded(px(4.))
                                .bg(color("--brand-500"))
                                .text_size(px(13.))
                                .font_weight(FontWeight(500.))
                                .text_color(color("--white"))
                                .child(cta_label),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(m) = this.boost_apply.as_mut() {
                                m.guild = gid;
                                m.step = Step::Confirm;
                            }
                            cx.notify();
                        }))
                }))
                .into_any_element()
        };

        deferred(
            div()
                .id("boost-select-guild-backdrop")
                .occlude()
                .absolute()
                .inset_0()
                .bg(rgba(0x000000b3))
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_boost_apply();
                    cx.notify();
                }))
                .child(
                    div()
                        .id("boost-select-guild-modal")
                        .w(px(480.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(color("--border-subtle"))
                        .bg(color("--background-surface-high"))
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .px(px(20.))
                                .pt(px(20.))
                                .pb(px(12.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .text_size(px(20.))
                                        .font_weight(FontWeight(700.))
                                        .text_color(color("--text-strong"))
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .id("boost-select-guild-close")
                                        .size(px(30.))
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(icon("modal_close", 18., "--icon-subtle"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_boost_apply();
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(!all_guilds.is_empty(), |col| {
                            col.child(
                                div().px(px(20.)).pb(px(12.)).child(
                                    div()
                                        .h(px(38.))
                                        .px(px(12.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-base-lowest"))
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(icon("search", 16., "--icon-subtle"))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(14.))
                                                .text_color(if open.select_query.is_empty() {
                                                    color("--text-muted")
                                                } else {
                                                    color("--text-strong")
                                                })
                                                .child(if open.select_query.is_empty() {
                                                    dc_intl::t("vf3ZTa").to_owned()
                                                } else {
                                                    open.select_query.clone()
                                                }),
                                        ),
                                ),
                            )
                        })
                        .child(body),
                ),
        )
        .with_priority(96)
        .into_any_element()
    }

    fn boost_apply_quantity(
        &self,
        open: &BoostApply,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (min, max) = Self::boost_apply_bounds(open);
        let focused = open
            .field
            .as_ref()
            .is_some_and(|field| field.focus_handle(cx).is_focused(window));
        // `v` and `g` of module 663803: a blank field disables both.
        let (can_lower, can_raise) = match open.shown {
            Typed::Number(value) => (value > min, value < max),
            Typed::Blank(_) => (false, false),
        };
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
        let minus = step_button("boost-apply-minus", "automod_minus", can_lower, -1, cx);
        let plus = step_button("boost-apply-plus", "automod_plus", can_raise, 1, cx);
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
                                    .id("boost-apply-value")
                                    .relative()
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
                                    // `wrapper_UcGqzv`: the border is `--input-border-hover` under the pointer and,
                                    // with the input focused, `--input-border-active` with a 2px outline inside.
                                    .when(!focused, |this| {
                                        this.hover(|style| {
                                            style.border_color(color("--input-border-hover"))
                                        })
                                    })
                                    .when(focused, |this| {
                                        this.border_color(color("--input-border-active")).child(
                                            div()
                                                .absolute()
                                                .top(px(-1.))
                                                .left(px(-1.))
                                                .right(px(-1.))
                                                .bottom(px(-1.))
                                                .rounded(px(8.))
                                                .border_2()
                                                .border_color(color("--input-border-active")),
                                        )
                                    })
                                    .children(open.field.clone()),
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
        // The graphic's box stays empty: its Rive file and pictures are not in the snapshot.
        let (_, title_top, header_height) = header_geometry(self.viewport.1);
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
                    // `container_pmhtZL:before` spans the header; its gradient and mask are in percent, so the
                    // 478 by 244 picture is stretched to this header.
                    .h(px(header_height)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(title_top))
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
            // `confirmationText_9Zk4Mz`: centred, "N time(s)" bold (`$b`, `strong` is semibold).
            let (line, bold) = success_runs(&guild_name, open.count);
            div()
                .flex_none()
                .px(px(32.))
                .flex()
                .justify_center()
                .text_center()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(20.))
                        .text_color(color("--text-default"))
                        .child(gpui::StyledText::new(line).with_highlights([(
                            bold,
                            gpui::HighlightStyle {
                                font_weight: Some(FontWeight(600.)),
                                ..Default::default()
                            },
                        )])),
                )
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
                    "boost-apply: step={:?} free={} count={} field={:?} sending={} error={:?}",
                    open.step,
                    open.available.len(),
                    open.count,
                    open.field
                        .as_ref()
                        .map(|field| field.read(cx).text().to_owned()),
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
            ["select-guild"] | ["transfer-guild"] | ["select-empty"] => {
                let is_transfer = matches!(words, ["transfer-guild"]);
                let force_empty = matches!(words, ["select-empty"]);
                let gid = self
                    .guild
                    .or_else(|| self.stores.guilds.first().map(|g| g.id))
                    .unwrap_or(1);
                self.boost_apply = Some(BoostApply {
                    guild: gid,
                    step: Step::GuildSelect,
                    is_transfer,
                    select_query: String::new(),
                    force_empty_guilds: force_empty,
                    available: vec!["9000".to_owned()],
                    count: 1,
                    field: None,
                    shown: Typed::Number(1),
                    _field_events: None,
                    sending: false,
                    error: None,
                    from_settings: true,
                });
                cx.notify();
                format!("boost-apply: {}", words[0])
            }
            [free] => {
                let (Ok(free), Some(guild)) = (free.parse::<usize>(), self.guild) else {
                    return "usage: boost-apply <free slots>|next|success|select-guild|transfer-guild|select-empty|state".to_owned();
                };
                self.boost_apply = Some(BoostApply {
                    guild,
                    step: Step::Loading,
                    is_transfer: false,
                    select_query: String::new(),
                    force_empty_guilds: false,
                    available: Vec::new(),
                    count: 1,
                    field: None,
                    shown: Typed::Number(1),
                    _field_events: None,
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
    fn the_stepper_takes_what_parse_int_reads_and_clamps_it() {
        // Module 663803 with `minValue: 1, maxValue: 5`.
        assert_eq!(typed("3", 1, 5), Some(Typed::Number(3)));
        assert_eq!(typed("9", 1, 5), Some(Typed::Number(5)));
        assert_eq!(
            typed("12345678901234567890123", 1, 5),
            Some(Typed::Number(5))
        );
        assert_eq!(typed("0", 1, 5), Some(Typed::Number(1)));
        assert_eq!(typed("-4", 1, 5), Some(Typed::Number(1)));
        assert_eq!(typed("2a", 1, 5), Some(Typed::Number(2)));
        assert_eq!(typed(" 4", 1, 5), Some(Typed::Number(4)));
        assert_eq!(typed("+4", 1, 5), Some(Typed::Number(4)));
        assert_eq!(typed("4.9", 1, 5), Some(Typed::Number(4)));
        assert_eq!(typed("", 1, 5), Some(Typed::Blank(String::new())));
        assert_eq!(typed("-", 1, 5), Some(Typed::Blank("-".to_owned())));
        // NaN: the field keeps what it had.
        assert_eq!(typed("a", 1, 5), None);
        assert_eq!(typed(" ", 1, 5), None);
        assert_eq!(typed("--1", 1, 5), None);
        // A blank field tells the dialog the minimum.
        assert_eq!(Typed::Blank(String::new()).value(1), 1);
    }

    #[test]
    fn the_success_line_bolds_the_count() {
        let (line, bold) = success_runs("Home", 3);
        assert_eq!(line, "You just Boosted Home 3 times.");
        assert_eq!(&line[bold], "3 times");
        let (line, bold) = success_runs("Home", 1);
        assert_eq!(&line[bold], "1 time");
    }

    #[test]
    fn the_header_follows_the_window_height() {
        let near = |(a, b, c): (f32, f32, f32), (x, y, z): (f32, f32, f32)| {
            (a - x).abs() < 0.01 && (b - y).abs() < 0.01 && (c - z).abs() < 0.01
        };
        // 720 tall: 15vh is 108, under the 198 a 352 wide 16/9 box would be.
        assert!(near(header_geometry(720.), (108., 136., 184.)));
        // Tall windows stop at 198.
        assert!(near(header_geometry(2000.), (198., 226., 274.)));
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
